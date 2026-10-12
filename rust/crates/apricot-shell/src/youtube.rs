//! `YouTube` and `SoundCloud` work of the main window: the search requests of
//! the component runtime (Windows `start_youtube_work`,
//! `poll_youtube_runtime`, `finish_youtube_list`, `finish_youtube_error`,
//! `cancel_youtube_work`) and stream resolving before playback
//! (`start_media_item_with_options` for streamed sources,
//! `start_youtube_resolve_with_options`, `finish_youtube_resolve`,
//! `show_preparing_player`, the stream URL cache).

use std::path::PathBuf;

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

impl Shell {
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
        api || self.youtube.search.is_pending() || self.youtube.pending_trending_api.is_some()
    }

    /// Windows `cancel_youtube_work`.
    pub(crate) fn cancel_youtube_work(&mut self) {
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
