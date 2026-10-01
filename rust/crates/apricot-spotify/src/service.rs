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
        }
    }

    /// Short technical detail for `{error}` (no tokens, no URLs).
    pub fn detail(&self) -> &str {
        match self {
            Self::Network(detail) | Self::LoginFailed(detail) | Self::Storage(detail) => detail,
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
    Resolved {
        stamp: SpotifyStamp,
        result: Result<crate::playback::SpotifyTrack, SpotifyError>,
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
                let spotify_uri = librespot_core::SpotifyUri::from_uri(&uri)
                    .map_err(|_| SpotifyError::LoginFailed("uri".to_owned()))?;
                let item = librespot_metadata::audio::AudioItem::get_file(&session, spotify_uri)
                    .await
                    .map_err(|error| SpotifyError::Network(error.kind.to_string()))?;
                Ok(crate::playback::SpotifyTrack::from_audio_item(&item))
            }
            .await;
            Self::emit(&sender, &notify, SpotifyEvent::Resolved { stamp, result });
        });
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
        active.playback.set_spirc(None);
        active.session.shutdown();
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
