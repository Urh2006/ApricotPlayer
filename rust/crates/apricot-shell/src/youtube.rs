//! `YouTube` and `SoundCloud` work of the main window: the search requests of
//! the component runtime (Windows `start_youtube_work`,
//! `poll_youtube_runtime`, `finish_youtube_list`, `finish_youtube_error`,
//! `cancel_youtube_work`) and stream resolving before playback
//! (`start_media_item_with_options` for streamed sources,
//! `start_youtube_resolve_with_options`, `finish_youtube_resolve`,
//! `show_preparing_player`, the stream URL cache).

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
};

use apricot_app::{
    SearchWork, YoutubeCollectionWork,
    stream_url_cache::{CachedStream, StreamKeySettings, StreamUrlCache},
};
use apricot_core::{MediaItem, MediaSource};
use apricot_media::{
    YoutubeBackend, YoutubeFormat, YoutubeSessionConfig, YoutubeStreamPreference,
    select_youtube_playback_formats,
};
use apricot_platform::{YoutubeSearchService, YoutubeSearchServiceUpdate};
use apricot_playback::PlaybackEvent;

use crate::player::{PendingQueuedStart, QueueStartMode};
use crate::{PlayerFocus, Shell, ShellUi, View};

#[cfg(test)]
mod parity_tests {
    use super::*;
    use crate::tests::{equalizer::Ui, keyboard::shell};
    fn video(number: u32) -> MediaItem {
        let mut item = crate::local_file_item(std::path::Path::new("/tmp/video.mp4"));
        item.id = apricot_core::MediaId(number.to_string());
        item.source = MediaSource::Youtube;
        item.local_path = None;
        item.url = Some(
            format!("https://www.youtube.com/watch?v=aaaaaaaaaa{number}")
                .parse()
                .unwrap(),
        );
        item
    }
    #[test]
    fn hydrated_metadata_updates_matching_generation_without_moving_selection() {
        let mut shell = shell();
        let mut ui = Ui::default();
        let work = shell
            .application
            .begin_youtube_search("test", apricot_media::YoutubeSearchKind::Video)
            .unwrap();
        shell
            .application
            .apply_search_results(work.generation, vec![video(1), video(2)], None);
        shell.view = View::Results;
        ui.selected = Some(1);
        let mut hydrated = video(1);
        hydrated
            .metadata
            .insert("views".to_owned(), serde_json::json!(1234));
        shell.apply_metadata_items(
            &mut ui,
            MetadataScope::Search(work.generation),
            &[hydrated.clone()],
        );
        assert_eq!(
            shell.application.search_session().items()[0]
                .metadata
                .get("views"),
            Some(&serde_json::json!(1234))
        );
        assert_eq!(ui.selected, Some(1));
        assert_eq!(ui.row_updates.len(), 1);
        assert_eq!(ui.row_updates[0].0, 0);
        hydrated
            .metadata
            .insert("views".to_owned(), serde_json::json!(9999));
        shell.apply_metadata_items(
            &mut ui,
            MetadataScope::Search(work.generation + 1),
            &[hydrated],
        );
        assert_eq!(
            shell.application.search_session().items()[0]
                .metadata
                .get("views"),
            Some(&serde_json::json!(1234))
        );
    }
    #[test]
    fn enabled_prefetch_schedules_the_actual_next_sequence_item_once() {
        let mut shell = shell();
        let mut ui = Ui::default();
        shell
            .application
            .set_boolean_setting(apricot_core::SettingId::PrefetchNextStreamUrl, true)
            .unwrap();
        shell
            .application
            .set_boolean_setting(apricot_core::SettingId::EnableStreamUrlCache, true)
            .unwrap();
        let work = shell
            .application
            .begin_youtube_search("test", apricot_media::YoutubeSearchKind::Video)
            .unwrap();
        shell
            .application
            .apply_search_results(work.generation, vec![video(1), video(2)], None);
        let first = shell.application.prepare_search_playback(0).unwrap();
        shell.application.start_player_item(first);
        shell.schedule_stream_prefetch(&mut ui);
        assert_eq!(shell.youtube.prefetches.len(), 1);
        let next_key = shell.stream_cache_key(video(2).url.as_ref().unwrap().as_str());
        assert!(shell.youtube.prefetches.contains_key(&next_key));
        shell.schedule_stream_prefetch(&mut ui);
        assert_eq!(shell.youtube.prefetches.len(), 1);
    }

    #[test]
    fn prefetch_results_populate_the_cache_and_disabled_cache_discards_results() {
        let mut shell = shell();
        let next = video(2);
        let url = next.url.as_ref().unwrap().to_string();
        let key = shell.stream_cache_key(&url);
        let stream = CachedStream {
            item: next,
            stream_url: "https://example.test/media".to_owned(),
            external_audio_url: None,
        };
        let (sender, receiver) = mpsc::channel();
        sender.send(Some(stream.clone())).unwrap();
        shell.youtube.prefetches.insert(key.clone(), receiver);
        shell
            .application
            .set_boolean_setting(apricot_core::SettingId::EnableStreamUrlCache, false)
            .unwrap();
        assert!(!shell.poll_stream_prefetches());
        assert!(!shell.stream_cache().contains(&key, unix_now()));
        shell
            .application
            .set_boolean_setting(apricot_core::SettingId::EnableStreamUrlCache, true)
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        sender.send(Some(stream)).unwrap();
        shell.youtube.prefetches.insert(key.clone(), receiver);
        assert!(!shell.poll_stream_prefetches());
        assert_eq!(
            shell.cached_stream(&url).unwrap().stream_url,
            "https://example.test/media"
        );
    }

    #[test]
    fn metadata_updates_the_focused_row_only_after_selection_moves() {
        let mut shell = shell();
        let mut ui = Ui::default();
        let work = shell
            .application
            .begin_youtube_search("test", apricot_media::YoutubeSearchKind::Video)
            .unwrap();
        shell
            .application
            .apply_search_results(work.generation, vec![video(1), video(2)], None);
        shell.view = View::Results;
        ui.selected = Some(0);
        let mut hydrated = video(1);
        hydrated
            .metadata
            .insert("views".to_owned(), serde_json::json!(1234));
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(vec![hydrated])).unwrap();
        shell.youtube.metadata = Some(PendingMetadata {
            scope: MetadataScope::Search(work.generation),
            urls: vec![],
            api: false,
            receiver,
        });
        shell.poll_metadata(&mut ui);
        assert!(ui.row_updates.is_empty());
        assert!(shell.downloads.deferred_rows.contains(&0));
        ui.selected = Some(1);
        shell.apply_deferred_result_rows(&mut ui, 1);
        assert_eq!(ui.row_updates[0].0, 0);
        let count = apricot_app::player_information::display_count(
            &shell.application.search_session().items()[0].metadata,
            "views",
        )
        .unwrap();
        assert!(ui.row_updates[0].1.contains(&count), "{:?}", ui.row_updates);
        assert_eq!(ui.selected, Some(1));
    }
}

/// Windows `YOUTUBE_BACKEND`.
const YOUTUBE_BACKEND: YoutubeBackend = YoutubeBackend::YtDlp;

/// Windows `YoutubeResolvePurpose`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResolvePurpose {
    Playback,
    CopyStreamUrl,
}

/// Windows `PendingYoutubeResolve`.
#[derive(Clone, Debug)]
pub(crate) struct PendingResolve {
    pub(crate) token: u64,
    purpose: ResolvePurpose,
    original_item: MediaItem,
    session_shuffle: Option<bool>,
    preserve_sequence: bool,
    start_position_seconds: Option<f64>,
}

/// Windows `PendingYoutubeListWork`.
#[derive(Clone, Debug)]
pub(crate) enum PendingList {
    Search(SearchWork),
    Collection(YoutubeCollectionWork),
    /// Python `play_playlist`: every video of a playlist, then play.
    PlaylistPlayback {
        token: u64,
        shuffle: bool,
    },
    Trending(crate::trending::PendingTrending),
}

impl PendingList {
    pub(crate) const fn token(&self) -> u64 {
        match self {
            Self::Search(work) => work.generation,
            Self::Collection(work) => work.generation,
            Self::PlaylistPlayback { token, .. } => *token,
            Self::Trending(pending) => pending.work.generation,
        }
    }
}

#[derive(Default)]
pub(crate) struct YoutubeState {
    pub(crate) search: YoutubeSearchService,
    /// Windows `pending_youtube_work`.
    pub(crate) pending_list: Option<PendingList>,
    /// Windows `pending_player_navigation`: next or previous waits for more
    /// results.
    pub(crate) pending_navigation: Option<i32>,
    /// Windows `pending_youtube_trending_api`.
    pub(crate) pending_trending_api: Option<crate::trending::PendingTrendingApi>,
    pending_resolve: Option<PendingResolve>,
    next_token: u64,
    stream_cache: Option<StreamUrlCache>,
    metadata: Option<PendingMetadata>,
    metadata_attempted: HashSet<String>,
    metadata_api_disabled: HashSet<MetadataScope>,
    prefetches: HashMap<String, Receiver<Option<CachedStream>>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
enum MetadataScope {
    Search(u64),
    Collection(u64),
}
struct PendingMetadata {
    scope: MetadataScope,
    urls: Vec<String>,
    api: bool,
    receiver: Receiver<Result<Vec<MediaItem>, String>>,
}

impl YoutubeState {
    /// Whether a stream is being resolved (Windows `pending_youtube_resolve`).
    pub(crate) const fn resolving(&self) -> bool {
        self.pending_resolve.is_some()
    }

    /// Windows `next_youtube_operation_token`.
    pub(crate) fn next_token(&mut self) -> u64 {
        self.next_token = self.next_token.wrapping_add(1).max(1);
        self.next_token
    }
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
}

/// Windows `youtube_stream_preference`.
fn stream_preference(value: &str) -> YoutubeStreamPreference {
    match value.trim().to_ascii_lowercase().as_str() {
        "video" => YoutubeStreamPreference::PreferVideo,
        "audio" => YoutubeStreamPreference::PreferAudio,
        _ => YoutubeStreamPreference::Automatic,
    }
}

/// Windows `media_resolve_backend`.
fn media_resolve_backend(item: &MediaItem) -> YoutubeBackend {
    if item.source == MediaSource::Soundcloud
        || (item.source == MediaSource::Direct && item.youtube_url_at_timestamp(0.0).is_none())
    {
        YoutubeBackend::YtDlp
    } else {
        YOUTUBE_BACKEND
    }
}

/// Windows `resolved_playback_item`.
fn resolved_playback_item(
    mut resolved: MediaItem,
    original: &MediaItem,
    preserve_sequence: bool,
) -> MediaItem {
    if preserve_sequence || original.source == MediaSource::Direct {
        resolved.source = original.source;
        resolved.id = original.id.clone();
        resolved.url.clone_from(&original.url);
    }
    resolved
}

/// The bundled components folder next to the executable (Windows
/// `application_directory().join("components")`).
pub(crate) fn components_directory() -> Option<PathBuf> {
    crate::player_tools::application_directory().map(|path| path.join("components"))
}

/// The folders where a cookies file may have been exported to (Windows
/// `windows_documents_folders`).
pub(crate) fn documents_folders() -> Vec<PathBuf> {
    if cfg!(windows) {
        apricot_platform::browser_cookies::windows_documents_folders()
    } else {
        std::env::var_os("HOME")
            .map(|home| vec![PathBuf::from(home).join("Documents")])
            .unwrap_or_default()
    }
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn item_needs_metadata(item: &MediaItem) -> bool {
    if item.source != MediaSource::Youtube
        || item.url.is_none()
        || !matches!(
            item.kind,
            apricot_core::MediaKind::Video | apricot_core::MediaKind::LiveStream
        )
    {
        return false;
    }
    let present = |keys: &[&str]| {
        keys.iter().any(|key| {
            item.metadata
                .get(*key)
                .is_some_and(|v| !v.is_null() && v.as_str().is_none_or(|s| !s.trim().is_empty()))
        })
    };
    !present(&["views", "view_count"])
        || !present(&[
            "age",
            "timestamp",
            "release_timestamp",
            "upload_date",
            "uploaded_at",
            "publish_date",
        ])
}

impl Shell {
    /// Resolve the next item off the UI thread, using the Windows cache policy.
    pub(crate) fn schedule_stream_prefetch(&mut self, ui: &mut dyn ShellUi) {
        let settings = self.application.settings();
        if !settings.prefetch_next_stream_url || !settings.enable_stream_url_cache {
            return;
        }
        let Some(item) = self.application.prefetch_candidate() else {
            return;
        };
        if !matches!(
            item.source,
            MediaSource::Youtube | MediaSource::Soundcloud | MediaSource::Direct
        ) || apricot_app::audiovault::item_kind(&item).is_some()
        {
            return;
        }
        let Some(url) = item.url.as_ref().map(ToString::to_string) else {
            return;
        };
        let key = self.stream_cache_key(&url);
        if self.youtube.prefetches.contains_key(&key)
            || self.stream_cache().contains(&key, unix_now())
        {
            return;
        }
        let Some(components) = components_directory() else {
            return;
        };
        let executable = apricot_platform::app_update::preferred_ytdlp_executable(&components);
        let config = self.youtube_session_config();
        let preference = stream_preference(&self.application.settings().stream_format_preference);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            use apricot_media::{YoutubeCommand, YoutubeEngine, YoutubeResponsePayload};
            let resolved = apricot_platform::YtDlpYoutubeEngine::new(&executable)
                .ok()
                .and_then(|mut engine| {
                    engine.execute(YoutubeCommand::Configure { config }).ok()?;
                    let YoutubeResponsePayload::Resolved { item, formats } = engine
                        .execute(YoutubeCommand::Resolve { url, preference })
                        .ok()?
                    else {
                        return None;
                    };
                    let selected = select_youtube_playback_formats(&formats, preference)?;
                    let primary = formats.get(selected.primary_index)?;
                    Some(CachedStream {
                        item: *item,
                        stream_url: primary.url.clone(),
                        external_audio_url: selected
                            .external_audio_index
                            .and_then(|i| formats.get(i))
                            .map(|f| f.url.clone()),
                    })
                });
            let _ = sender.send(resolved);
        });
        self.youtube.prefetches.insert(key, receiver);
        ui.set_background_polling(true);
    }

    fn poll_stream_prefetches(&mut self) -> bool {
        let keys: Vec<_> = self.youtube.prefetches.keys().cloned().collect();
        for key in keys {
            let result = self.youtube.prefetches[&key].try_recv();
            if matches!(result, Err(TryRecvError::Empty)) {
                continue;
            }
            self.youtube.prefetches.remove(&key);
            if let Ok(Some(stream)) = result {
                let settings = self.application.settings();
                if settings.enable_stream_url_cache {
                    let minutes = settings.stream_url_cache_minutes;
                    self.stream_cache()
                        .insert(&key, &stream, minutes, unix_now());
                }
            }
        }
        !self.youtube.prefetches.is_empty()
    }

    fn metadata_scope(&self) -> Option<MetadataScope> {
        match self.view {
            View::Results | View::Trending => Some(MetadataScope::Search(
                self.application.search_session().generation(),
            )),
            View::Collection => self
                .application
                .youtube_collection()
                .map(|c| MetadataScope::Collection(c.generation())),
            _ => None,
        }
    }

    fn metadata_items(&self, scope: MetadataScope) -> &[MediaItem] {
        match scope {
            MetadataScope::Search(generation)
                if generation == self.application.search_session().generation() =>
            {
                self.application.search_session().items()
            }
            MetadataScope::Collection(generation) => self
                .application
                .youtube_collection()
                .filter(|c| c.generation() == generation)
                .map_or(&[], |c| c.items()),
            MetadataScope::Search(_) => &[],
        }
    }

    fn schedule_metadata(&mut self, ui: &mut dyn ShellUi) {
        if self.youtube.metadata.is_some() {
            return;
        }
        let Some(scope) = self.metadata_scope() else {
            return;
        };
        let api_key = self
            .application
            .settings()
            .youtube_data_api_key
            .trim()
            .to_owned();
        let api = !api_key.is_empty() && !self.youtube.metadata_api_disabled.contains(&scope);
        let candidates: Vec<_> = self
            .metadata_items(scope)
            .iter()
            .filter(|item| item_needs_metadata(item))
            .filter(|item| {
                item.url
                    .as_ref()
                    .is_some_and(|url| !self.youtube.metadata_attempted.contains(url.as_str()))
            })
            .take(if api { 50 } else { 5 })
            .cloned()
            .collect();
        if candidates.is_empty() {
            return;
        }
        let Some(components) = components_directory() else {
            return;
        };
        let executable = apricot_platform::app_update::preferred_ytdlp_executable(&components);
        let urls: Vec<_> = candidates
            .iter()
            .filter_map(|item| item.url.as_ref().map(ToString::to_string))
            .collect();
        let worker_urls = urls.clone();
        let config = self.youtube_session_config();
        let proxy = nonempty(&self.application.settings().proxy);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            use apricot_media::{YoutubeCommand, YoutubeEngine, YoutubeResponsePayload};
            let result = if api {
                apricot_platform::YoutubeDataApiClient::new(proxy.as_deref())
                    .and_then(|client| client.fetch_metadata(&api_key, &candidates))
                    .map_err(|e| e.to_string())
            } else {
                apricot_platform::YtDlpYoutubeEngine::new(&executable)
                    .map_err(|e| e.to_string())
                    .and_then(|mut engine| {
                        engine
                            .execute(YoutubeCommand::Configure { config })
                            .map_err(|e| e.to_string())?;
                        match engine
                            .execute(YoutubeCommand::Metadata { urls: worker_urls })
                            .map_err(|e| e.to_string())?
                        {
                            YoutubeResponsePayload::Hydrated { items } => Ok(items),
                            _ => Err("Unexpected metadata response".to_owned()),
                        }
                    })
            };
            let _ = sender.send(result);
        });
        self.youtube.metadata_attempted.extend(urls.iter().cloned());
        self.youtube.metadata = Some(PendingMetadata {
            scope,
            urls,
            api,
            receiver,
        });
        ui.set_background_polling(true);
    }

    fn poll_metadata(&mut self, ui: &mut dyn ShellUi) -> bool {
        if let Some(pending) = self.youtube.metadata.as_ref() {
            let result = pending.receiver.try_recv();
            if matches!(result, Err(TryRecvError::Empty)) {
                return true;
            }
            let pending = self.youtube.metadata.take().expect("pending metadata");
            match result {
                Ok(Ok(items)) => self.apply_metadata_items(ui, pending.scope, &items),
                _ if pending.api => {
                    self.youtube.metadata_api_disabled.insert(pending.scope);
                    for url in pending.urls {
                        self.youtube.metadata_attempted.remove(&url);
                    }
                }
                _ => {}
            }
        }
        self.schedule_metadata(ui);
        self.youtube.metadata.is_some()
    }

    fn apply_metadata_items(
        &mut self,
        ui: &mut dyn ShellUi,
        scope: MetadataScope,
        items: &[MediaItem],
    ) {
        let visible = self.metadata_scope() == Some(scope);
        let catalog = self.catalog();
        for item in items {
            let index = item.stable_identity().and_then(|identity| {
                self.metadata_items(scope)
                    .iter()
                    .position(|candidate| candidate.stable_identity().as_ref() == Some(&identity))
            });
            let changed = match scope {
                MetadataScope::Search(generation) => {
                    self.application.apply_search_metadata(generation, item)
                }
                MetadataScope::Collection(generation) => self
                    .application
                    .apply_youtube_collection_metadata(generation, item),
            };
            if changed
                && visible
                && let Some(index) = index
            {
                if ui.selected_row() == Some(index) {
                    self.downloads.deferred_rows.insert(index);
                } else if let Some(updated) = self.metadata_items(scope).get(index) {
                    ui.set_list_row(
                        index,
                        &crate::result_label(updated, &catalog, self.application.downloads()),
                    );
                }
            }
        }
    }

    /// Windows `effective_cookies_file`, as an existing file for yt-dlp.
    pub(crate) fn effective_cookies_file(&mut self) -> Option<PathBuf> {
        crate::cookies::update_repair_settings(self.application.settings());
        let path = self
            .application
            .effective_cookies_file(&documents_folders());
        nonempty(&path)
            .map(PathBuf::from)
            .filter(|path| path.is_file())
    }

    /// Windows `youtube_session_config`.
    pub(crate) fn youtube_session_config(&mut self) -> YoutubeSessionConfig {
        let cookies_file = self.effective_cookies_file();
        let settings = self.application.settings();
        YoutubeSessionConfig {
            cookies_header: None,
            cookies_have_youtube_login: cookies_file
                .as_deref()
                .is_some_and(apricot_app::cookies::cookies_file_has_youtube_login),
            cookies_file: cookies_file.map(|path| path.to_string_lossy().into_owned()),
            proxy_url: nonempty(&settings.proxy),
            cookie_user_agent: nonempty(&settings.cookie_user_agent),
        }
    }

    /// Windows `start_youtube_work` for a search.
    pub(crate) fn start_youtube_search(&mut self, ui: &mut dyn ShellUi, work: SearchWork) {
        let generation = work.generation;
        let (query, kind, limit) = (work.query.clone(), work.kind, work.limit);
        self.youtube.pending_list = Some(PendingList::Search(work));
        let Some(components) = components_directory() else {
            self.finish_youtube_error_state(ui, generation, "Application path is unavailable");
            return;
        };
        let config = self.youtube_session_config();
        match self.youtube.search.start(
            YOUTUBE_BACKEND,
            &components,
            config,
            generation,
            query,
            kind,
            limit,
        ) {
            Ok(()) => ui.set_background_polling(true),
            Err(error) => self.finish_youtube_error_state(ui, generation, &error.to_string()),
        }
    }

    /// Windows `poll_youtube_runtime`; `true` while work runs.
    pub(crate) fn poll_youtube(&mut self, ui: &mut dyn ShellUi) -> bool {
        let api = self.poll_youtube_trending_api(ui);
        loop {
            let update = match self.youtube.search.poll() {
                Ok(Some(update)) => update,
                Ok(None) => break,
                Err(error) => {
                    let token = self
                        .youtube
                        .pending_resolve
                        .as_ref()
                        .map(|pending| pending.token)
                        .or_else(|| self.youtube.pending_list.as_ref().map(PendingList::token))
                        .unwrap_or_default();
                    self.finish_youtube_error(ui, token, &error.to_string());
                    break;
                }
            };
            match update {
                YoutubeSearchServiceUpdate::Results {
                    token,
                    items,
                    continuation,
                } => self.finish_youtube_list(ui, token, items, continuation),
                YoutubeSearchServiceUpdate::Resolved {
                    token,
                    item,
                    formats,
                } => self.finish_youtube_resolve(ui, token, *item, &formats),
                YoutubeSearchServiceUpdate::Hydrated { .. } => {}
                YoutubeSearchServiceUpdate::Failed { token, message } => {
                    self.finish_youtube_error(ui, token, &message);
                }
            }
        }
        let metadata = self.poll_metadata(ui);
        let prefetch = self.poll_stream_prefetches();
        api || metadata
            || prefetch
            || self.youtube.search.is_pending()
            || self.youtube.pending_trending_api.is_some()
    }

    /// Windows `cancel_youtube_work`.
    pub(crate) fn cancel_youtube_work(&mut self) {
        self.youtube.metadata = None;
        self.youtube.metadata_attempted.clear();
        self.youtube.metadata_api_disabled.clear();
        if self.youtube.pending_list.is_none()
            && self.youtube.pending_resolve.is_none()
            && self.youtube.pending_trending_api.is_none()
            && !self.youtube.search.is_pending()
        {
            return;
        }
        match self.youtube.pending_list.take() {
            Some(PendingList::Search(_) | PendingList::Trending(_)) => {
                let _ = self.application.cancel_pending_search();
            }
            Some(PendingList::Collection(_)) => {
                let _ = self.application.cancel_pending_youtube_collection();
            }
            Some(PendingList::PlaylistPlayback { .. }) | None => {}
        }
        self.youtube.pending_resolve = None;
        self.youtube.pending_trending_api = None;
        self.youtube.pending_navigation = None;
        self.player.pending_queued_start = None;
        let _ = self.youtube.search.cancel();
    }

    /// Windows `finish_youtube_error`.
    fn finish_youtube_error(&mut self, ui: &mut dyn ShellUi, token: u64, message: &str) {
        let Some(pending) = self
            .youtube
            .pending_resolve
            .take_if(|pending| pending.token == token)
        else {
            if matches!(
                self.youtube.pending_list.as_ref(),
                Some(PendingList::Trending(pending)) if pending.work.generation == token
            ) {
                self.finish_youtube_trending_error(ui, token, message);
                return;
            }
            let return_from_collection = matches!(
                self.youtube.pending_list,
                Some(PendingList::Collection(YoutubeCollectionWork {
                    work_kind: apricot_app::YoutubeCollectionWorkKind::Initial,
                    ..
                }))
            );
            self.finish_youtube_error_state(ui, token, message);
            if return_from_collection {
                self.navigate_back(ui);
            }
            return;
        };
        let direct_fallback = pending.purpose == ResolvePurpose::Playback
            && pending.original_item.source == MediaSource::Direct
            && pending
                .original_item
                .youtube_url_at_timestamp(0.0)
                .is_none();
        if pending.purpose == ResolvePurpose::Playback && !direct_fallback {
            self.player.pending_queued_start = None;
        }
        if direct_fallback {
            // Python `resolve_stream_url` falls back to the link itself.
            ui.set_status(&self.text("direct_link_fallback"), false);
            self.start_resolved_player(
                ui,
                pending.original_item,
                pending.session_shuffle,
                pending.start_position_seconds,
            );
            return;
        }
        let visible = match pending.purpose {
            ResolvePurpose::CopyStreamUrl => {
                self.text("stream_url_failed").replace("{error}", message)
            }
            ResolvePurpose::Playback => {
                let friendly = apricot_app::comments::friendly_error(&self.catalog(), message);
                self.player_failed_message(&friendly)
            }
        };
        if pending.purpose == ResolvePurpose::Playback {
            self.fail_preparing_player(&visible);
        }
        let settings = self.application.settings();
        if pending.purpose == ResolvePurpose::Playback
            && settings.enable_age_restricted_videos
            && apricot_media::cookie_errors::is_cookie_auth_error(message)
            && !apricot_app::cookies::normalized_cookies_browser(settings).is_empty()
        {
            self.prompt_cookie_refresh_for_playback(ui, pending, &visible);
            return;
        }
        ui.set_status(&visible, true);
        if pending.purpose == ResolvePurpose::Playback {
            ui.show_error(&visible);
        }
        ui.focus_primary_control();
    }

    /// Windows `finish_youtube_error_state` for a search.
    pub(crate) fn finish_youtube_error_state(
        &mut self,
        ui: &mut dyn ShellUi,
        generation: u64,
        message: &str,
    ) {
        self.player.pending_queued_start = None;
        let (was_initial, search_focus) = match self.youtube.pending_list.take() {
            Some(PendingList::Search(work)) => {
                let _ = self.application.fail_search(generation, message);
                (work.work_kind == apricot_app::SearchWorkKind::Initial, true)
            }
            Some(PendingList::Collection(work)) => {
                let _ = self
                    .application
                    .fail_youtube_collection(generation, message);
                (
                    work.work_kind == apricot_app::YoutubeCollectionWorkKind::Initial,
                    false,
                )
            }
            Some(PendingList::Trending(_)) => {
                let _ = self.application.fail_search(generation, message);
                (true, false)
            }
            Some(PendingList::PlaylistPlayback { .. }) | None => (true, false),
        };
        self.youtube.pending_navigation = None;
        ui.set_status(message, true);
        if was_initial {
            ui.show_error(message);
            if search_focus {
                ui.focus_screen_control("search_query");
            } else {
                ui.focus_primary_control();
            }
        }
    }

    /// Windows `start_media_item_with_options`: streamed sources are
    /// resolved first, other items start at once.
    pub(crate) fn start_media_item_with_options(
        &mut self,
        ui: &mut dyn ShellUi,
        item: MediaItem,
        queue_mode: Option<QueueStartMode>,
        session_shuffle: Option<bool>,
        preserve_sequence: bool,
        start_position_seconds: Option<f64>,
    ) {
        // Python plays AudioVault movies from their resolved stream and TV
        // show episodes from the playback cache.
        if self.audiovault_intercept_start(ui, &item) {
            return;
        }
        self.cancel_youtube_work();
        let start_position_seconds =
            start_position_seconds.or_else(|| self.application.playback_resume_position(&item));
        if !preserve_sequence {
            self.application.prepare_standalone_playback();
        }
        self.player.pending_queued_start = queue_mode.map(|mode| PendingQueuedStart {
            item: item.clone(),
            mode,
        });
        if !matches!(
            item.source,
            MediaSource::Youtube | MediaSource::Soundcloud | MediaSource::Direct
        ) {
            self.start_player_at(ui, item, session_shuffle, start_position_seconds);
            return;
        }
        self.start_youtube_resolve(
            ui,
            &item,
            ResolvePurpose::Playback,
            session_shuffle,
            preserve_sequence,
            start_position_seconds,
        );
    }

    fn stream_cache(&mut self) -> &mut StreamUrlCache {
        let path = self
            .application
            .settings_file()
            .parent()
            .map(|folder| folder.join(apricot_app::stream_url_cache::STREAM_URL_CACHE_FILE));
        self.youtube.stream_cache.get_or_insert_with(|| {
            path.map_or_else(Default::default, |path| {
                StreamUrlCache::load(path, unix_now())
            })
        })
    }

    /// Windows `stream_cache_key`.
    fn stream_cache_key(&mut self, url: &str) -> String {
        let cookies = self.effective_cookies_file();
        let settings = self.application.settings();
        apricot_app::stream_url_cache::cache_key(
            url,
            &StreamKeySettings {
                stream_format_preference: settings.stream_format_preference.trim().to_lowercase(),
                video_format: settings.video_format.trim().to_lowercase(),
                max_height: settings.max_video_height.max(0),
                restricted: settings.enable_age_restricted_videos,
                cookies_file: cookies
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                cookies_signature: cookies
                    .as_deref()
                    .map(apricot_app::stream_url_cache::cookies_signature)
                    .unwrap_or_default(),
                cookies_browser: settings.cookies_from_browser.trim().to_lowercase(),
            },
        )
    }

    /// Windows `cached_stream`.
    fn cached_stream(&mut self, url: &str) -> Option<CachedStream> {
        if !self.application.settings().enable_stream_url_cache {
            return None;
        }
        let key = self.stream_cache_key(url);
        self.stream_cache().get(&key, unix_now())
    }

    /// Windows `remember_stream`.
    fn remember_stream(
        &mut self,
        original: &MediaItem,
        resolved: &MediaItem,
        stream_url: &str,
        external_audio_url: Option<String>,
    ) {
        let settings = self.application.settings();
        if !settings.enable_stream_url_cache {
            return;
        }
        let minutes = settings.stream_url_cache_minutes;
        let Some(url) = original.url.as_ref().map(ToString::to_string) else {
            return;
        };
        let key = self.stream_cache_key(&url);
        let stream = CachedStream {
            item: resolved.clone(),
            stream_url: stream_url.to_owned(),
            external_audio_url,
        };
        self.stream_cache()
            .insert(&key, &stream, minutes, unix_now());
    }

    /// Windows `start_youtube_resolve_with_options`.
    pub(crate) fn start_youtube_resolve(
        &mut self,
        ui: &mut dyn ShellUi,
        item: &MediaItem,
        purpose: ResolvePurpose,
        session_shuffle: Option<bool>,
        preserve_sequence: bool,
        start_position_seconds: Option<f64>,
    ) {
        let Some(url) = item.url.as_ref().map(ToString::to_string) else {
            self.report_resolve_start_error(ui, purpose, "The selected item has no media URL");
            return;
        };
        // Python `resolve_stream_url` first asks `cached_stream_url`.
        if let Some(cached) = self.cached_stream(&url) {
            if purpose == ResolvePurpose::CopyStreamUrl {
                self.copy_text_and_announce(ui, &cached.stream_url, "stream_url_copied");
                return;
            }
            let mut resolved = resolved_playback_item(cached.item, item, preserve_sequence);
            resolved.stream_url = cached.stream_url.parse().ok();
            resolved.external_audio_url = cached
                .external_audio_url
                .as_deref()
                .and_then(|url| url.parse().ok());
            if resolved.stream_url.is_some() {
                self.start_player_at(ui, resolved, session_shuffle, start_position_seconds);
                return;
            }
        }
        let token = self.youtube.next_token();
        let backend = media_resolve_backend(item);
        let Some(components) = components_directory() else {
            self.report_resolve_start_error(ui, purpose, "Application path is unavailable");
            return;
        };
        let preference = stream_preference(&self.application.settings().stream_format_preference);
        let config = self.youtube_session_config();
        match self.youtube.search.start_resolve(
            backend,
            &components,
            config,
            token,
            url,
            preference,
        ) {
            Ok(()) => {
                let pending = PendingResolve {
                    token,
                    purpose,
                    original_item: item.clone(),
                    session_shuffle,
                    preserve_sequence,
                    start_position_seconds,
                };
                self.show_resolve_start(ui, &pending);
                self.youtube.pending_resolve = Some(pending);
                ui.set_background_polling(true);
            }
            Err(error) => self.report_resolve_start_error(ui, purpose, &error.to_string()),
        }
    }

    /// Windows `report_youtube_resolve_start_error`.
    fn report_resolve_start_error(
        &mut self,
        ui: &mut dyn ShellUi,
        purpose: ResolvePurpose,
        message: &str,
    ) {
        if purpose == ResolvePurpose::Playback {
            self.player.pending_queued_start = None;
            self.fail_preparing_player(message);
        }
        let visible = if purpose == ResolvePurpose::CopyStreamUrl {
            self.text("stream_url_failed").replace("{error}", message)
        } else {
            message.to_owned()
        };
        ui.set_status(&visible, true);
        if purpose == ResolvePurpose::Playback {
            ui.show_error(&visible);
        }
        ui.focus_primary_control();
    }

    /// Windows `finish_youtube_resolve`.
    fn finish_youtube_resolve(
        &mut self,
        ui: &mut dyn ShellUi,
        token: u64,
        item: MediaItem,
        formats: &[YoutubeFormat],
    ) {
        let Some(pending) = self
            .youtube
            .pending_resolve
            .take_if(|pending| pending.token == token)
        else {
            return;
        };
        let preference = stream_preference(&self.application.settings().stream_format_preference);
        let Some(selection) = select_youtube_playback_formats(formats, preference) else {
            self.report_resolve_start_error(
                ui,
                pending.purpose,
                "No playable YouTube stream was returned",
            );
            return;
        };
        let Some(primary) = formats.get(selection.primary_index) else {
            self.report_resolve_start_error(
                ui,
                pending.purpose,
                "The selected YouTube stream was invalid",
            );
            return;
        };
        let external_audio_url = selection
            .external_audio_index
            .and_then(|index| formats.get(index))
            .map(|format| format.url.clone());
        self.remember_stream(
            &pending.original_item,
            &item,
            &primary.url,
            external_audio_url.clone(),
        );
        if pending.purpose == ResolvePurpose::CopyStreamUrl {
            self.copy_text_and_announce(ui, &primary.url, "stream_url_copied");
            return;
        }
        let mut item =
            resolved_playback_item(item, &pending.original_item, pending.preserve_sequence);
        let Ok(stream_url) = primary.url.parse() else {
            self.report_resolve_start_error(
                ui,
                pending.purpose,
                "The selected YouTube stream URL was invalid",
            );
            return;
        };
        item.stream_url = Some(stream_url);
        item.external_audio_url = external_audio_url.and_then(|url| url.parse().ok());
        self.start_resolved_player(
            ui,
            item,
            pending.session_shuffle,
            pending.start_position_seconds,
        );
    }

    /// Windows `handle_cookie_event` for a playback that waited for fresh
    /// cookies: the resolve starts again.
    pub(crate) fn retry_cookie_playback(&mut self, ui: &mut dyn ShellUi, pending: &PendingResolve) {
        self.start_youtube_resolve(
            ui,
            &pending.original_item,
            pending.purpose,
            pending.session_shuffle,
            pending.preserve_sequence,
            pending.start_position_seconds,
        );
    }

    /// Windows `show_resolve_start`: "Preparing playback" on the status line
    /// without speaking it, and the player page before the stream arrives.
    fn show_resolve_start(&mut self, ui: &mut dyn ShellUi, pending: &PendingResolve) {
        if pending.purpose == ResolvePurpose::CopyStreamUrl {
            ui.set_status(&self.text("resolving_stream_url"), true);
            return;
        }
        let item = &pending.original_item;
        let status = self
            .text("preparing_stream")
            .replace("{title}", &item.title);
        ui.set_status(&status, false);
        self.show_preparing_player(
            ui,
            item.clone(),
            pending.session_shuffle,
            pending.start_position_seconds,
        );
    }

    /// Windows `show_preparing_player`.
    fn show_preparing_player(
        &mut self,
        ui: &mut dyn ShellUi,
        item: MediaItem,
        session_shuffle: Option<bool>,
        start_position_seconds: Option<f64>,
    ) {
        self.persist_current_playback_position(ui);
        let session = self.application.player_session();
        if session.is_open() {
            let _ = self
                .execute_player_command_result(apricot_playback::PlaybackCommand::SetPaused(true));
        }
        self.application.forget_player_return_frame();
        let generation = self.application.start_player_item_with_shuffle_at(
            item,
            session_shuffle,
            start_position_seconds,
        );
        self.player.preparing_generation = Some(generation);
        self.application.navigate_to_player();
        self.view = View::Player;
        self.refresh_player(ui, PlayerFocus::Initial);
    }

    /// Windows `start_resolved_player` with `preparing_player_start`: the
    /// stream starts unless the waiting player page was closed or replaced.
    fn start_resolved_player(
        &mut self,
        ui: &mut dyn ShellUi,
        item: MediaItem,
        session_shuffle: Option<bool>,
        start_position_seconds: Option<f64>,
    ) {
        if let Some(generation) = self.player.preparing_generation {
            let session = self.application.player_session();
            if !session.is_open() || session.generation() != generation {
                self.player.preparing_generation = None;
                return;
            }
        }
        self.start_player_at(ui, item, session_shuffle, start_position_seconds);
    }

    /// Windows `fail_preparing_player`.
    fn fail_preparing_player(&mut self, message: &str) {
        let Some(generation) = self.player.preparing_generation.take() else {
            return;
        };
        if self.application.player_session().generation() == generation {
            let _ = self
                .application
                .apply_playback_event(generation, PlaybackEvent::Failed(message.to_owned()));
        }
    }
}
