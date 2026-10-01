//! Spotify session runtime: browser login, account switching, logout and
//! removal, off the UI thread. Results come back as [`SpotifyEvent`]s on a
//! channel; `notify` wakes the UI after every event.
//!
//! One account is locally active at a time (plan 6.2). Every result carries
//! the login ID or [`SpotifyStamp`] of its request so the UI can drop late
//! completions of an older request or account.

use std::{
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::AtomicBool,
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

use apricot_core::SpotifyStamp;
use librespot_core::{SessionConfig, authentication::Credentials, session::Session};
use librespot_metadata::Metadata;
use librespot_protocol::authentication::AuthenticationType;

use crate::{
    accounts::{
        AccountStore, SpotifyAccount, SpotifyAccounts, account_key, protect_credentials,
        unprotect_credentials,
    },
    oauth::{CallbackPage, OAuthError, PkceLogin},
    playback::{PlaybackNotice, Shared, SpotifyPlayback},
};

const LOGIN_DEADLINE: Duration = Duration::from_secs(300);
const ATTRIBUTE_WAIT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpotifyError {
    Cancelled,
    TimedOut,
    Denied,
    BadCallback,
    Network(String),
    LoginFailed(String),
    Storage(String),
    /// The account has no saved login (logged out or unreadable blob).
    NoCredentials,
    /// Spotify refused the credentials (revoked or invalid).
    Rejected,
    /// Spotify answered, but not as expected (changed interface, rights).
    Service(String),
}

impl SpotifyError {
    /// Text key of the localized message.
    pub const fn text_key(&self) -> &'static str {
        match self {
            Self::Cancelled => "spotify_login_cancelled",
            Self::TimedOut => "spotify_login_timed_out",
            Self::Denied => "spotify_login_denied",
            Self::BadCallback => "spotify_login_bad_callback",
            Self::Network(_) => "spotify_error_network",
            Self::LoginFailed(_) | Self::Rejected => "spotify_error_login_failed",
            Self::Storage(_) => "spotify_error_storage",
            Self::NoCredentials => "spotify_error_logged_out",
            Self::Service(_) => "spotify_error_service",
        }
    }

    /// Short technical detail for `{error}` (no tokens, no URLs).
    pub fn detail(&self) -> &str {
        match self {
            Self::Network(detail)
            | Self::LoginFailed(detail)
            | Self::Storage(detail)
            | Self::Service(detail) => detail,
            _ => "",
        }
    }
}

impl From<OAuthError> for SpotifyError {
    fn from(error: OAuthError) -> Self {
        match error {
            OAuthError::Cancelled => Self::Cancelled,
            OAuthError::TimedOut => Self::TimedOut,
            OAuthError::Denied => Self::Denied,
            OAuthError::BadCallback => Self::BadCallback,
            OAuthError::Network(detail) => Self::Network(detail),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpotifyEvent {
    /// The browser should open this URL for login `login`.
    LoginUrl { login: u64, url: String },
    LoginFinished {
        login: u64,
        result: Result<SpotifyAccount, SpotifyError>,
    },
    Connected {
        stamp: SpotifyStamp,
        result: Result<SpotifyAccount, SpotifyError>,
    },
    /// The Connect device of the active session lost its connection.
    Disconnected,
    /// Metadata of a track or episode link, for playback from Apricot.
    /// `context` is the album or playlist URI when the link was one; the
    /// track is then its first playable track.
    Resolved {
        stamp: SpotifyStamp,
        result: Result<(crate::playback::SpotifyTrack, Option<String>), SpotifyError>,
    },
    /// The confirmed queue with the titles of its tracks.
    Queue {
        stamp: SpotifyStamp,
        queue: crate::queue::SpotifyQueue,
    },
    /// A list or collection the UI asked for.
    Catalog {
        stamp: SpotifyStamp,
        result: Result<CatalogResult, SpotifyError>,
    },
    /// A change of the account's data, as Spotify confirms it.
    Edited {
        stamp: SpotifyStamp,
        result: Result<crate::library_edit::EditOutcome, SpotifyError>,
    },
    /// Spotify accepted (or refused) moving playback to another device.
    Transferred {
        stamp: SpotifyStamp,
        result: Result<(), SpotifyError>,
    },
}

/// The locally active account: its session, Connect device and PCM source.
pub struct ActiveSession {
    session: Session,
    playback: Arc<SpotifyPlayback>,
    shared: Arc<Shared>,
}

type Notify = Arc<dyn Fn() + Send + Sync>;

pub struct SpotifyService {
    store: AccountStore,
    runtime: OnceLock<tokio::runtime::Runtime>,
    sender: Sender<SpotifyEvent>,
    notify: Notify,
    login_cancel: Mutex<Option<Arc<AtomicBool>>>,
    /// The locally active session and its account key.
    session: Arc<Mutex<Option<(String, ActiveSession)>>>,
    /// Title and artists by URI, for the queue view.
    titles: Arc<Mutex<std::collections::HashMap<String, (String, String)>>>,
    /// Spotify's internal web interfaces (pathfinder, spclient).
    api: Arc<crate::api::Api>,
}

/// What the UI asks to read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogRequest {
    Search {
        query: String,
        kind: crate::catalog::SearchKind,
        offset: u64,
    },
    Library {
        filter: crate::catalog::LibraryFilter,
        folder: Option<String>,
        offset: u64,
    },
    LikedSongs {
        offset: u64,
    },
    Album(String),
    Playlist {
        uri: String,
        offset: u64,
    },
    Artist(String),
    Show {
        uri: String,
        offset: u64,
    },
    /// Playlists the account may add to, for "Add to playlist".
    EditablePlaylists,
    /// Songs the account hid.
    HiddenSongs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogResult {
    Page(crate::catalog::CatalogPage),
    Collection(crate::catalog::Collection),
    Playlists(Vec<crate::catalog::CatalogItem>),
    HiddenSongs(Vec<String>),
}

/// Liked state of the tracks and episodes Spotify did not mark, so rows
/// can say "liked".
async fn fill_saved(
    api: &crate::api::Api,
    session: &Session,
    items: &mut [crate::catalog::CatalogItem],
) {
    let unknown: Vec<String> = items
        .iter()
        .filter(|item| item.saved.is_none() && item.kind.is_playable_item())
        .map(|item| item.uri.clone())
        .collect();
    if unknown.is_empty() {
        return;
    }
    let Ok(states) = crate::catalog::saved(api, session, &unknown).await else {
        return;
    };
    let known: std::collections::HashMap<&str, bool> = unknown
        .iter()
        .map(String::as_str)
        .zip(states.iter().copied())
        .collect();
    for item in items {
        if item.saved.is_none() {
            item.saved = known.get(item.uri.as_str()).copied();
        }
    }
}

impl From<crate::api::ApiError> for SpotifyError {
    fn from(error: crate::api::ApiError) -> Self {
        match error {
            crate::api::ApiError::Network(detail) => Self::Network(detail),
            other => Self::Service(other.to_string()),
        }
    }
}

impl SpotifyService {
    pub fn new(app_data: &Path, notify: Notify) -> (Self, Receiver<SpotifyEvent>) {
        install_tls_provider();
        crate::diagnostics::install(app_data);
        let (sender, receiver) = mpsc::channel();
        (
            Self {
                store: AccountStore::new(app_data),
                runtime: OnceLock::new(),
                sender,
                notify,
                login_cancel: Mutex::new(None),
                session: Arc::default(),
                titles: Arc::default(),
                api: Arc::new(crate::api::Api::new(Some(
                    app_data.join("spotify").join("pathfinder.json"),
                ))),
            },
            receiver,
        )
    }

    pub fn accounts(&self) -> SpotifyAccounts {
        self.store.load()
    }

    /// Account key of the connected session, if any.
    pub fn connected_account(&self) -> Option<String> {
        self.session
            .lock()
            .ok()
            .and_then(|session| session.as_ref().map(|(key, _)| key.clone()))
    }

    /// Reads the title, artists and length of `uri` (track or episode) with
    /// the active session.
    pub fn resolve_track(&self, stamp: SpotifyStamp, uri: &str) {
        let session = self
            .session
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|(_, active)| active.session.clone()));
        let sender = self.sender.clone();
        let notify = self.notify.clone();
        let uri = uri.to_owned();
        self.runtime().spawn(async move {
            let result = async {
                let session = session.ok_or(SpotifyError::NoCredentials)?;
                let network =
                    |error: librespot_core::Error| SpotifyError::Network(error.kind.to_string());
                let spotify_uri = librespot_core::SpotifyUri::from_uri(&uri)
                    .map_err(|_| SpotifyError::LoginFailed("uri".to_owned()))?;
                // An album or playlist plays as its context from the first track.
                let (track_uri, context) = match &spotify_uri {
                    librespot_core::SpotifyUri::Album { .. } => {
                        let album = librespot_metadata::Album::get(&session, &spotify_uri)
                            .await
                            .map_err(network)?;
                        let first = album.tracks().next().cloned();
                        (first, Some(uri.clone()))
                    }
                    librespot_core::SpotifyUri::Playlist { .. } => {
                        let playlist = librespot_metadata::Playlist::get(&session, &spotify_uri)
                            .await
                            .map_err(network)?;
                        let first = playlist.tracks().next().cloned();
                        (first, Some(uri.clone()))
                    }
                    _ => (Some(spotify_uri.clone()), None),
                };
                let track_uri = track_uri.ok_or(SpotifyError::LoginFailed("empty".to_owned()))?;
                let item = librespot_metadata::audio::AudioItem::get_file(&session, track_uri)
                    .await
                    .map_err(network)?;
                Ok((
                    crate::playback::SpotifyTrack::from_audio_item(&item),
                    context,
                ))
            }
            .await;
            Self::emit(&sender, &notify, SpotifyEvent::Resolved { stamp, result });
        });
    }

    /// The confirmed queue now, with the titles already known; the dialog
    /// opens with it at once and [`Self::load_queue`] brings the rest.
    pub fn queue_now(&self) -> crate::queue::SpotifyQueue {
        let state = self
            .playback()
            .and_then(|playback| playback.player_state())
            .unwrap_or_default();
        let mut queue = crate::queue::snapshot(&state);
        if let Ok(known) = self.titles.lock() {
            fill_titles(&mut queue, &known);
        }
        queue
    }

    /// Reads the confirmed queue and the titles of its tracks. Titles are
    /// cached for the session, so a reload after an edit is quick.
    pub fn load_queue(&self, stamp: SpotifyStamp) {
        let active = self.session.lock().ok().and_then(|slot| {
            slot.as_ref()
                .map(|(_, active)| (active.session.clone(), active.playback.clone()))
        });
        let sender = self.sender.clone();
        let notify = self.notify.clone();
        let titles = self.titles.clone();
        self.runtime().spawn(async move {
            let state = active
                .as_ref()
                .and_then(|(_, playback)| playback.player_state())
                .unwrap_or_default();
            let mut queue = crate::queue::snapshot(&state);
            if let Some((session, _)) = active {
                let wanted: Vec<String> = queue
                    .current
                    .iter()
                    .chain(&queue.entries)
                    .map(|entry| entry.uri.clone())
                    .filter(|uri| titles.lock().is_ok_and(|known| !known.contains_key(uri)))
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                let mut tasks = tokio::task::JoinSet::new();
                for uri in wanted {
                    let session = session.clone();
                    tasks.spawn(async move {
                        let parsed = librespot_core::SpotifyUri::from_uri(&uri).ok()?;
                        let item = librespot_metadata::audio::AudioItem::get_file(&session, parsed)
                            .await
                            .ok()?;
                        let track = crate::playback::SpotifyTrack::from_audio_item(&item);
                        Some((uri, (track.title, track.artists)))
                    });
                }
                while let Some(result) = tasks.join_next().await {
                    if let Ok(Some((uri, title))) = result
                        && let Ok(mut known) = titles.lock()
                    {
                        known.insert(uri, title);
                    }
                }
                if let Ok(known) = titles.lock() {
                    fill_titles(&mut queue, &known);
                }
            }
            Self::emit(&sender, &notify, SpotifyEvent::Queue { stamp, queue });
        });
    }

    /// The Connect devices that can play and the device that plays now;
    /// `None` without a connected session.
    pub fn devices_now(&self) -> Option<(Vec<crate::devices::SpotifyDevice>, bool)> {
        let (own_id, playback) = self.session.lock().ok()?.as_ref().map(|(_, active)| {
            (
                active.session.device_id().to_owned(),
                active.playback.clone(),
            )
        })?;
        let connect = playback.connect_devices()?;
        let playing = !connect.active_device_id.is_empty();
        Some((crate::devices::devices(&connect, &own_id), playing))
    }

    /// Moves playback from the device that plays to `device_id` (Spotify
    /// Connect transfer). Playing here, it moves away; playing elsewhere, it
    /// can come here.
    pub fn transfer_to(&self, stamp: SpotifyStamp, device_id: &str) {
        let active = self.session.lock().ok().and_then(|slot| {
            slot.as_ref()
                .map(|(_, active)| (active.session.clone(), active.playback.clone()))
        });
        let sender = self.sender.clone();
        let notify = self.notify.clone();
        let target = device_id.to_owned();
        self.runtime().spawn(async move {
            let result = async {
                let (session, playback) = active.ok_or(SpotifyError::NoCredentials)?;
                let from = playback
                    .connect_devices()
                    .map(|connect| connect.active_device_id)
                    .filter(|id| !id.is_empty())
                    .ok_or_else(|| SpotifyError::Network(String::new()))?;
                session
                    .spclient()
                    .transfer(&from, &target, None)
                    .await
                    .map(|_| ())
                    .map_err(|error| SpotifyError::Network(error.kind.to_string()))
            }
            .await;
            Self::emit(
                &sender,
                &notify,
                SpotifyEvent::Transferred { stamp, result },
            );
        });
    }

    fn active_session(&self) -> Option<Session> {
        self.session
            .lock()
            .ok()?
            .as_ref()
            .map(|(_, active)| active.session.clone())
    }

    /// `spotify:user:<name>:collection`, the Liked Songs context of the
    /// connected account.
    pub fn liked_songs_context(&self) -> Option<String> {
        self.active_session()
            .map(|session| format!("spotify:user:{}:collection", session.username()))
    }

    /// Reads a list or collection; the answer is `SpotifyEvent::Catalog`.
    pub fn load_catalog(&self, stamp: SpotifyStamp, request: CatalogRequest) {
        use crate::catalog;
        let session = self.active_session();
        let session_for_saved = session.clone();
        let api = self.api.clone();
        let api_for_saved = self.api.clone();
        let sender = self.sender.clone();
        let notify = self.notify.clone();
        self.runtime().spawn(async move {
            let result = async {
                let session = session.ok_or(SpotifyError::NoCredentials)?;
                let api = api.as_ref();
                Ok(match request {
                    CatalogRequest::EditablePlaylists => {
                        CatalogResult::Playlists(catalog::editable_playlists(api, &session).await?)
                    }
                    CatalogRequest::HiddenSongs => {
                        CatalogResult::HiddenSongs(catalog::hidden_songs(api, &session).await?)
                    }
                    CatalogRequest::Search {
                        query,
                        kind,
                        offset,
                    } => CatalogResult::Page(
                        catalog::search(api, &session, &query, kind, offset).await?,
                    ),
                    CatalogRequest::Library {
                        filter,
                        folder,
                        offset,
                    } => CatalogResult::Page(
                        catalog::library(api, &session, filter, folder.as_deref(), offset).await?,
                    ),
                    CatalogRequest::LikedSongs { offset } => {
                        CatalogResult::Page(catalog::liked_songs(api, &session, offset).await?)
                    }
                    CatalogRequest::Album(uri) => {
                        CatalogResult::Collection(catalog::album(api, &session, &uri).await?)
                    }
                    CatalogRequest::Playlist { uri, offset } => CatalogResult::Collection(
                        catalog::playlist(api, &session, &uri, offset).await?,
                    ),
                    CatalogRequest::Artist(uri) => {
                        CatalogResult::Collection(catalog::artist(api, &session, &uri).await?)
                    }
                    CatalogRequest::Show { uri, offset } => {
                        CatalogResult::Collection(catalog::show(api, &session, &uri, offset).await?)
                    }
                })
            }
            .await;
            if let Err(error) = &result {
                log::warn!("catalog request failed: {error:?}");
            }
            let mut result = result;
            if let (Ok(answer), Some(session)) = (&mut result, &session_for_saved) {
                match answer {
                    CatalogResult::Page(page) => {
                        fill_saved(&api_for_saved, session, &mut page.items).await;
                    }
                    CatalogResult::Collection(collection) => {
                        fill_saved(&api_for_saved, session, &mut collection.page.items).await;
                    }
                    CatalogResult::Playlists(_) | CatalogResult::HiddenSongs(_) => {}
                }
            }
            Self::emit(&sender, &notify, SpotifyEvent::Catalog { stamp, result });
        });
    }

    /// Changes the account's data; the answer is `SpotifyEvent::Edited`.
    pub fn edit(&self, stamp: SpotifyStamp, edit: crate::library_edit::LibraryEdit) {
        let session = self.active_session();
        let api = self.api.clone();
        let sender = self.sender.clone();
        let notify = self.notify.clone();
        self.runtime().spawn(async move {
            let result = async {
                let session = session.ok_or(SpotifyError::NoCredentials)?;
                Ok(crate::library_edit::apply(&api, &session, edit).await?)
            }
            .await;
            if let Err(error) = &result {
                log::warn!("library change failed: {error:?}");
            }
            Self::emit(&sender, &notify, SpotifyEvent::Edited { stamp, result });
        });
    }

    /// The PCM source and Connect controls of the active session.
    pub fn playback(&self) -> Option<Arc<SpotifyPlayback>> {
        self.session
            .lock()
            .ok()?
            .as_ref()
            .map(|(_, active)| active.playback.clone())
    }

    fn runtime(&self) -> &tokio::runtime::Runtime {
        self.runtime.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("apricot-spotify")
                .enable_all()
                .build()
                .expect("Spotify runtime")
        })
    }

    fn emit(sender: &Sender<SpotifyEvent>, notify: &Notify, event: SpotifyEvent) {
        let _ = sender.send(event);
        notify();
    }

    /// Starts a browser login. A previous pending login is cancelled.
    pub fn begin_login(&self, login: u64, page: CallbackPage) {
        self.cancel_login();
        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut slot) = self.login_cancel.lock() {
            *slot = Some(cancel.clone());
        }
        let store = self.store.clone();
        let sender = self.sender.clone();
        let notify = self.notify.clone();
        let session_slot = self.session.clone();
        let handle = self.runtime().handle().clone();
        std::thread::Builder::new()
            .name("apricot-spotify-login".into())
            .spawn(move || {
                let attempt = std::panic::AssertUnwindSafe(|| {
                    let client_id = SessionConfig::default().client_id;
                    let pkce = PkceLogin::new(&client_id)?;
                    Self::emit(
                        &sender,
                        &notify,
                        SpotifyEvent::LoginUrl {
                            login,
                            url: pkce.auth_url().to_owned(),
                        },
                    );
                    let code =
                        pkce.wait_for_code(&cancel, Instant::now() + LOGIN_DEADLINE, &page)?;
                    let token = pkce.exchange(&client_id, &code)?;
                    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                        return Err(SpotifyError::Cancelled);
                    }
                    let device_id = store.device_id().map_err(SpotifyError::Storage)?;
                    let (account, session) = handle.block_on(open_session(
                        device_id,
                        Credentials::with_access_token(token),
                        notify.clone(),
                        sender.clone(),
                    ))?;
                    store
                        .upsert_active(account.clone())
                        .map_err(SpotifyError::Storage)?;
                    replace_session(&session_slot, Some((account.key.clone(), session)));
                    Ok(account)
                });
                // A panic must end the login with an error, never leave the
                // dialog waiting forever.
                let result = std::panic::catch_unwind(attempt).unwrap_or_else(|_| {
                    Err(SpotifyError::LoginFailed("internal error".to_owned()))
                });
                Self::emit(
                    &sender,
                    &notify,
                    SpotifyEvent::LoginFinished { login, result },
                );
            })
            .ok();
    }

    pub fn cancel_login(&self) {
        if let Ok(mut slot) = self.login_cancel.lock()
            && let Some(cancel) = slot.take()
        {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    /// Connects `key` with its saved login. The previous local session is
    /// closed first (one active account). `key` becomes the active account
    /// only when the connection succeeds. A saved login that no longer
    /// decrypts or that Spotify rejects is forgotten, so the account shows
    /// as logged out and offers a new login.
    pub fn connect(&self, stamp: SpotifyStamp, key: &str) {
        let accounts = self.store.load();
        let account = accounts.accounts.iter().find(|a| a.key == key).cloned();
        let key = key.to_owned();
        replace_session(&self.session, None);
        let store = self.store.clone();
        let sender = self.sender.clone();
        let notify = self.notify.clone();
        let session_slot = self.session.clone();
        self.runtime().spawn(async move {
            let result =
                async {
                    let account = account.ok_or(SpotifyError::NoCredentials)?;
                    let credentials = unprotect_credentials(&account.credentials)
                        .and_then(|blob| serde_json::from_slice::<Credentials>(&blob).ok());
                    let Some(credentials) = credentials else {
                        let _ = store.logout(&key);
                        return Err(SpotifyError::NoCredentials);
                    };
                    let device_id = store.device_id().map_err(SpotifyError::Storage)?;
                    let (fresh, session) =
                        match open_session(device_id, credentials, notify.clone(), sender.clone())
                            .await
                        {
                            Err(SpotifyError::Rejected) => {
                                let _ = store.logout(&key);
                                return Err(SpotifyError::NoCredentials);
                            }
                            other => other?,
                        };
                    let updated = store
                        .upsert_active(fresh.clone())
                        .map_err(SpotifyError::Storage)?;
                    drop(updated);
                    replace_session(&session_slot, Some((fresh.key.clone(), session)));
                    Ok(fresh)
                }
                .await;
            Self::emit(&sender, &notify, SpotifyEvent::Connected { stamp, result });
        });
    }

    /// Logout: closes the session of `key` and forgets its saved login.
    ///
    /// # Errors
    ///
    /// Returns the storage error.
    pub fn logout(&self, key: &str) -> Result<SpotifyAccounts, SpotifyError> {
        self.close_if(key);
        self.store.logout(key).map_err(SpotifyError::Storage)
    }

    /// Removes the account with its saved login and data folder.
    ///
    /// # Errors
    ///
    /// Returns the storage error.
    pub fn remove(&self, key: &str) -> Result<SpotifyAccounts, SpotifyError> {
        self.close_if(key);
        self.store.remove(key).map_err(SpotifyError::Storage)
    }

    fn close_if(&self, key: &str) {
        let matches = self
            .session
            .lock()
            .is_ok_and(|session| session.as_ref().is_some_and(|(k, _)| k == key));
        if matches {
            replace_session(&self.session, None);
        }
    }

    /// The next playback notice of the active session (new track, unavailable).
    pub fn take_notice(&self) -> Option<PlaybackNotice> {
        self.session
            .lock()
            .ok()?
            .as_ref()
            .and_then(|(_, active)| active.shared.take_notice())
    }

    /// Closes the local session (application exit).
    pub fn shutdown(&self) {
        self.cancel_login();
        replace_session(&self.session, None);
    }
}

/// rustls needs one process-wide crypto provider; the dependency graph
/// enables two (ring and aws-lc-rs), so rustls cannot choose by itself.
fn install_tls_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn replace_session(
    slot: &Arc<Mutex<Option<(String, ActiveSession)>>>,
    next: Option<(String, ActiveSession)>,
) {
    let installed = next.as_ref().map(|(_, active)| {
        active.playback.clone() as Arc<dyn apricot_playback::pcm_source::PcmSource>
    });
    let previous = slot
        .lock()
        .ok()
        .and_then(|mut guard| std::mem::replace(&mut *guard, next));
    apricot_playback::pcm_source::set_pcm_source(installed);
    if let Some((_, active)) = previous {
        active.shared.close_all();
        active.playback.set_spirc(None);
        active.session.shutdown();
    }
}

fn fill_titles(
    queue: &mut crate::queue::SpotifyQueue,
    known: &std::collections::HashMap<String, (String, String)>,
) {
    for entry in queue.current.iter_mut().chain(queue.entries.iter_mut()) {
        if let Some((title, artists)) = known.get(&entry.uri) {
            entry.title.clone_from(title);
            entry.artists.clone_from(artists);
        }
    }
}

/// Connects the account through its Connect device (Spirc) and builds the
/// saved account from the session: reusable credentials (DPAPI), product,
/// country and display name.
#[allow(clippy::too_many_lines)]
async fn open_session(
    device_id: String,
    credentials: Credentials,
    notify: Notify,
    sender: Sender<SpotifyEvent>,
) -> Result<(SpotifyAccount, ActiveSession), SpotifyError> {
    use librespot_connect::{ConnectConfig, Spirc};
    use librespot_core::config::DeviceType;
    use librespot_playback::{
        audio_backend::Sink,
        config::{Bitrate, PlayerConfig},
        mixer::{Mixer, NoOpVolume},
        player::Player,
    };
    let config = SessionConfig {
        device_id,
        ..SessionConfig::default()
    };
    // No LibreSpot cache: it would write credentials in plain text.
    let session = Session::new(config, None);
    let (playback, shared) = SpotifyPlayback::new(320, notify.clone());
    let sink_events = Arc::new(Mutex::new(None));
    let player_config = PlayerConfig {
        bitrate: Bitrate::Bitrate320,
        gapless: true,
        ..PlayerConfig::default()
    };
    let sink_shared = shared.clone();
    let sink_slot = sink_events.clone();
    let player = Player::new(
        player_config,
        session.clone(),
        Box::new(NoOpVolume),
        move || Box::new(SpotifyPlayback::sink(sink_shared, sink_slot)) as Box<dyn Sink>,
    );
    // Registered before any command, so the sink sees every fence event.
    if let Ok(mut slot) = sink_events.lock() {
        *slot = Some(player.get_player_event_channel());
    }
    let listener = player.get_player_event_channel();
    let mixer: Arc<dyn Mixer> = Arc::new(crate::playback::KeptVolumeMixer::default());
    let computer = std::env::var("COMPUTERNAME").unwrap_or_default();
    let connect = ConnectConfig {
        name: if computer.is_empty() {
            "ApricotPlayer".to_owned()
        } else {
            format!("ApricotPlayer ({computer})")
        },
        device_type: DeviceType::Computer,
        initial_volume: u16::MAX,
        ..ConnectConfig::default()
    };
    let (spirc, task) = Spirc::new(connect, session.clone(), credentials, player, mixer)
        .await
        .map_err(|error| {
            use librespot_core::error::ErrorKind;
            match error.kind {
                ErrorKind::PermissionDenied | ErrorKind::Unauthenticated => SpotifyError::Rejected,
                ErrorKind::Unavailable | ErrorKind::DeadlineExceeded => {
                    SpotifyError::Network(error.kind.to_string())
                }
                kind => SpotifyError::LoginFailed(kind.to_string()),
            }
        })?;
    tokio::spawn(async move {
        task.await;
        let _ = sender.send(SpotifyEvent::Disconnected);
        notify();
    });
    tokio::spawn(SpotifyPlayback::listen(shared.clone(), listener));
    tokio::spawn(SpotifyPlayback::watch_state(
        shared.clone(),
        spirc.player_state(),
    ));
    tokio::spawn(SpotifyPlayback::watch_devices(
        shared.clone(),
        spirc.devices(),
    ));
    tokio::spawn(SpotifyPlayback::watch_remote_volume(
        shared.clone(),
        spirc.remote_volume(),
    ));
    playback.set_spirc(Some(spirc));
    let username = session.username();
    let reusable = Credentials {
        username: Some(username.clone()),
        auth_type: AuthenticationType::AUTHENTICATION_STORED_SPOTIFY_CREDENTIALS,
        auth_data: session.auth_data(),
    };
    let json =
        serde_json::to_vec(&reusable).map_err(|error| SpotifyError::Storage(error.to_string()))?;
    let protected = protect_credentials(&json).map_err(SpotifyError::Storage)?;
    let started = Instant::now();
    let mut product = session.get_user_attribute("type").unwrap_or_default();
    while product.is_empty() && started.elapsed() < ATTRIBUTE_WAIT {
        tokio::time::sleep(Duration::from_millis(100)).await;
        product = session.get_user_attribute("type").unwrap_or_default();
    }
    playback.set_premium(product == "premium");
    let display_name = session
        .spclient()
        .get_user_profile(&username, Some(0), Some(0))
        .await
        .ok()
        .and_then(|body| serde_json::from_slice::<serde_json::Value>(&body).ok())
        .and_then(|json| json.get("name").and_then(|n| n.as_str()).map(str::to_owned))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| username.clone());
    Ok((
        SpotifyAccount {
            key: account_key(&username),
            display_name,
            product,
            country: session.country(),
            credentials: protected,
        },
        ActiveSession {
            session,
            playback,
            shared,
        },
    ))
}

#[cfg(test)]
mod live_tests {
    #[test]
    #[ignore = "network"]
    fn connecting_with_a_bogus_token_fails_without_panicking() {
        super::install_tls_provider();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (sender, _receiver) = std::sync::mpsc::channel();
        let result = runtime.block_on(super::open_session(
            "0123456789abcdef0123456789abcdef01234567".to_owned(),
            librespot_core::authentication::Credentials::with_access_token("bogus"),
            std::sync::Arc::new(|| {}),
            sender,
        ));
        eprintln!("open_session: {:?}", result.err());
    }
}
