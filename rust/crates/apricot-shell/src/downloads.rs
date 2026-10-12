//! Downloads (Windows `show_download_queue`, `refresh_download_queue`,
//! `start_download_item`, `confirm_download`, `build_download_request`,
//! `start_all_queued_downloads`, the user playlist and podcast feed
//! downloads, the download queue toggles, cancellation, the progress window
//! and `apply_download_update`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

use apricot_app::context_menu::{ContextCommand, ContextMenuEntry};
use apricot_app::{
    ActiveDownload, DownloadChoice, DownloadTaskKind, DownloadTaskStatus, QueueToggleOutcome,
    QueuedDownload,
};
use apricot_core::{MediaItem, MediaKind, Route, RouteFrame, TranslationCatalog};
use apricot_platform::{
    DownloadEvent, DownloadMode, DownloadOptions, DownloadPhase, DownloadRequest,
    VideoDownloadFormat,
};

use crate::download_work::{DownloadWorkerUpdate, spawn_batch_download, spawn_download};
use crate::player_tools::application_directory;
use crate::search::queued_marker_key;
use crate::{ChoiceDialog, ListScreen, MainMenuSelection, ScreenControl, Shell, ShellUi, View};

/// Python ignores the same download shortcut for the same item within
/// 0.35 s, so a repeated or doubled key press does not start the download
/// twice.
const DOWNLOAD_SHORTCUT_REPEAT_WINDOW: Duration = Duration::from_millis(350);

/// One press of a download shortcut, as Python's `last_download_shortcut`.
#[derive(Clone, Debug, PartialEq)]
struct DownloadShortcutPress {
    choice: DownloadChoice,
    identity: String,
    at: Instant,
}

fn accept_download_shortcut(
    last: &mut Option<DownloadShortcutPress>,
    press: DownloadShortcutPress,
) -> bool {
    if let Some(previous) = last.as_ref()
        && previous.choice == press.choice
        && previous.identity == press.identity
        && press.at.saturating_duration_since(previous.at) < DOWNLOAD_SHORTCUT_REPEAT_WINDOW
    {
        return false;
    }
    *last = Some(press);
    true
}

/// The last progress a download reported to the UI.
#[derive(Clone, Debug, PartialEq)]
struct DownloadProgressReport {
    percent_bucket: Option<i64>,
    title: String,
    at: Instant,
}

/// Python's progress hook reports the whole percent, clamped to 0..100.
#[allow(clippy::cast_possible_truncation)]
fn download_percent_bucket(percent: Option<f64>) -> Option<i64> {
    percent
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 100.0) as i64)
}

/// Python's `make_download_progress_hook`: report when the whole percent or
/// the title changes, otherwise at most every 0.75 s.
fn should_report_download_progress(
    last: &mut DownloadProgressReport,
    percent: Option<f64>,
    title: &str,
    now: Instant,
) -> bool {
    let bucket = download_percent_bucket(percent);
    let report = bucket != last.percent_bucket
        || title != last.title
        || now.saturating_duration_since(last.at) >= Duration::from_millis(750);
    if report {
        last.percent_bucket = bucket;
        title.clone_into(&mut last.title);
        last.at = now;
    }
    report
}

/// The download progress window's content (Windows
/// `DownloadProgressWindow`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadProgress {
    pub title: String,
    pub message: String,
    pub percent: u32,
    pub hide: String,
    pub details: String,
}

/// The downloads' runtime state (Windows `download_sender`,
/// `download_receiver`, `download_cancellations`,
/// `download_progress_reports`, `download_progress_task_ids`,
/// `download_progress_task_id` and `last_download_shortcut`).
pub(crate) struct DownloadState {
    sender: SyncSender<DownloadWorkerUpdate>,
    receiver: Receiver<DownloadWorkerUpdate>,
    cancellations: HashMap<u64, Arc<AtomicBool>>,
    progress_reports: HashMap<u64, DownloadProgressReport>,
    progress_task_ids: HashSet<u64>,
    progress_task_id: Option<u64>,
    progress_open: bool,
    last_shortcut: Option<DownloadShortcutPress>,
    /// Result rows whose queue marker changed while the list had focus on
    /// them (Windows `deferred_youtube_metadata_rows`).
    pub(crate) deferred_rows: HashSet<usize>,
}

impl Default for DownloadState {
    fn default() -> Self {
        let (sender, receiver) = mpsc::sync_channel(512);
        Self {
            sender,
            receiver,
            cancellations: HashMap::new(),
            progress_reports: HashMap::new(),
            progress_task_ids: HashSet::new(),
            progress_task_id: None,
            progress_open: false,
            last_shortcut: None,
            deferred_rows: HashSet::new(),
        }
    }
}

/// Windows `active_download_label`.
fn active_download_label(task: &ActiveDownload, catalog: &TranslationCatalog) -> String {
    let state_key = match task.status {
        DownloadTaskStatus::Downloading => "download_state_downloading",
        DownloadTaskStatus::Processing => "download_state_processing",
        DownloadTaskStatus::CancelRequested => "download_state_cancelled",
    };
    let mut parts = vec![task.title.clone(), catalog.text(state_key).to_owned()];
    if task.total > 0 {
        parts.push(
            catalog
                .text("downloads_remaining")
                .replace("{remaining}", &task.remaining().to_string())
                .replace("{total}", &task.total.to_string()),
        );
    }
    if task.current_title != task.title || task.total > 1 {
        parts.push(task.current_title.clone());
    }
    if let Some(percent) = task.percent.filter(|percent| *percent > 0.0) {
        parts.push(
            catalog
                .text("download_percent_value")
                .replace("{percent}", &format!("{percent:.0}")),
        );
    }
    parts.join(" | ")
}

/// Windows `media_kind_label`.
fn media_kind_label<'a>(item: &MediaItem, catalog: &'a TranslationCatalog) -> &'a str {
    let key = match item.kind {
        MediaKind::Audio => "download_audio_mode",
        MediaKind::Video => "video",
        MediaKind::LiveStream => "live_stream",
        MediaKind::Playlist => "playlist",
        MediaKind::Channel => "channel",
        MediaKind::PodcastFeed => "rss_feeds",
        MediaKind::PodcastEpisode => "podcast_episode",
        MediaKind::Movie => "movie",
        MediaKind::TvShow => "tv_show",
        MediaKind::TvEpisode => "episode",
        MediaKind::Unknown => "unknown",
    };
    catalog.text(key)
}

/// Windows `queued_download_label`.
fn queued_download_label(queued: &QueuedDownload, catalog: &TranslationCatalog) -> String {
    let mut parts = vec![
        queued.item.title.clone(),
        media_kind_label(&queued.item, catalog).to_owned(),
    ];
    if queued.item.kind == MediaKind::Video && !queued.item.channel.trim().is_empty() {
        parts.push(format!(
            "{}: {}",
            catalog.text("channel"),
            queued.item.channel
        ));
    }
    parts.extend([
        catalog.text(queued_marker_key(queued)).to_owned(),
        catalog.text("download_state_queued").to_owned(),
    ]);
    parts.join(" | ")
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn rounded_percent(percent: f64) -> u32 {
    percent.round().clamp(0.0, 100.0) as u32
}

/// Windows `download_progress_presentation`: the percent and the message.
fn download_progress_presentation(
    task: &ActiveDownload,
    catalog: &TranslationCatalog,
) -> (u32, String) {
    let total = task.total;
    let completed = task.completed.min(total);
    let remaining = total.saturating_sub(completed);
    let percent = completed
        .saturating_mul(100)
        .saturating_add(total / 2)
        .checked_div(total)
        .map_or_else(
            || rounded_percent(task.percent.unwrap_or_default()),
            |rounded| u32::try_from(rounded.min(100)).unwrap_or(100),
        );
    let title = if task.current_title.trim().is_empty() {
        &task.title
    } else {
        &task.current_title
    };
    let message = catalog
        .text("download_progress_message")
        .replace("{title}", title)
        .replace("{completed}", &completed.to_string())
        .replace("{total}", &total.to_string())
        .replace("{remaining}", &remaining.to_string());
    (percent, message)
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// Windows `normalized_audio_format`.
fn normalized_audio_format(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "m4a" | "opus" | "wav" | "flac" => value.trim().to_ascii_lowercase(),
        _ => "mp3".to_owned(),
    }
}

/// Windows `collection_download_url`.
fn collection_download_url(item: &MediaItem, url: &str) -> String {
    if item.kind != MediaKind::Channel {
        return url.to_owned();
    }
    let mut url = url.trim_end_matches('/').to_owned();
    if !url.ends_with("/videos") {
        url.push_str("/videos");
    }
    url
}

/// Windows `safe_path_component`.
#[must_use]
pub fn safe_path_component(value: &str) -> String {
    let result = value
        .trim()
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) || character.is_control()
            {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let mut result = result
        .chars()
        .take(120)
        .collect::<String>()
        .trim_matches([' ', '.'])
        .to_owned();
    if result.is_empty() {
        result.push_str("download");
    }
    let reserved_stem = result
        .split_once('.')
        .map_or(result.as_str(), |(stem, _)| stem)
        .to_ascii_uppercase();
    let reserved = matches!(reserved_stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || reserved_stem
            .strip_prefix("COM")
            .or_else(|| reserved_stem.strip_prefix("LPT"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'));
    if reserved {
        result.insert(0, '_');
    }
    result
}

/// The music folder under the download folder (Windows
/// `download_folder_for_item` and `user_playlist_download_folder`).
fn download_root(settings: &apricot_storage::SettingsDocument) -> Result<PathBuf, String> {
    let root = PathBuf::from(settings.download_folder.trim());
    if root.is_absolute() {
        Ok(root)
    } else {
        Err("The configured download folder must be an absolute path.".to_owned())
    }
}

fn root_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Windows `download_folder_for_item`.
fn download_folder_for_item(
    settings: &apricot_storage::SettingsDocument,
    item: &MediaItem,
    collection: bool,
) -> Result<PathBuf, String> {
    let root = download_root(settings)?;
    let name = root_name(&root);
    let mut folder = if matches!(
        item.kind,
        MediaKind::PodcastEpisode | MediaKind::PodcastFeed
    ) {
        if name == "podcasts" {
            root
        } else if name == "music" {
            root.parent()
                .unwrap_or_else(|| Path::new("."))
                .join("podcasts")
        } else {
            root.join("podcasts")
        }
    } else if name == "music" {
        root
    } else {
        root.join("music")
    };
    if item.kind == MediaKind::PodcastEpisode {
        let feed = if item.channel.trim().is_empty() {
            "Unknown podcast"
        } else {
            &item.channel
        };
        folder.push(safe_path_component(feed));
    } else if collection {
        folder.push(safe_path_component(&item.title));
    }
    Ok(folder)
}

/// Windows `user_playlist_download_folder`.
fn user_playlist_download_folder(
    settings: &apricot_storage::SettingsDocument,
    title: &str,
) -> Result<PathBuf, String> {
    let root = download_root(settings)?;
    let music = if root_name(&root) == "music" {
        root
    } else {
        root.join("music")
    };
    Ok(music.join(safe_path_component(title)))
}

/// Windows `download_options_from_settings`.
fn download_options_from_settings(
    settings: &apricot_storage::SettingsDocument,
    settings_file: &Path,
) -> DownloadOptions {
    DownloadOptions {
        audio_format: normalized_audio_format(&settings.audio_format),
        audio_quality: settings.audio_quality.trim().to_owned(),
        video_format: VideoDownloadFormat::from_setting(&settings.video_format),
        // Python `video_format_selector`: 0 (or less) means no height limit.
        max_video_height: u32::try_from(settings.max_video_height.max(0)).unwrap_or(1_080),
        quiet: settings.quiet_downloads,
        keep_playlist_order: settings.keep_playlist_order,
        filename_template: settings.filename_template.clone(),
        write_thumbnail: settings.write_thumbnail,
        write_description: settings.write_description,
        write_info_json: settings.write_info_json,
        write_subtitles: settings.write_subtitles,
        write_automatic_subtitles: settings.auto_subtitles,
        subtitle_languages: settings
            .subtitle_languages
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect(),
        embed_metadata: settings.embed_metadata,
        embed_thumbnail: settings.embed_thumbnail,
        restrict_filenames: settings.restrict_filenames,
        concurrent_fragments: u32::try_from(settings.concurrent_fragments.clamp(1, 32))
            .unwrap_or(4),
        retries: u32::try_from(settings.retries.clamp(0, 100)).unwrap_or(10),
        socket_timeout_seconds: u32::try_from(settings.socket_timeout.clamp(1, 300)).unwrap_or(20),
        rate_limit: nonempty(&settings.rate_limit),
        proxy_url: nonempty(&settings.proxy),
        cookies_file: None,
        cookie_user_agent: nonempty(&settings.cookie_user_agent),
        // Windows: the configured FFmpeg or the packaged one.
        ffmpeg_location: apricot_platform::ffmpeg_executable(
            &settings.ffmpeg_location,
            application_directory().as_deref(),
        ),
        download_archive: settings.download_archive.then(|| {
            settings_file
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("download-archive.txt")
        }),
    }
}

fn ytdlp_executable() -> Option<PathBuf> {
    crate::youtube::components_directory()
        .map(|components| apricot_platform::app_update::preferred_ytdlp_executable(&components))
}

/// Windows `UserPlaylistDownloadPlan`.
struct UserPlaylistDownloadPlan {
    playlist_index: usize,
    title: String,
    items: Vec<MediaItem>,
}

impl Shell {
    /// Windows `show_download_queue`.
    pub(crate) fn show_download_queue(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("current_downloads");
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if self.application.current_route() != Route::DownloadQueue {
            self.application.navigate_main_menu();
            self.application
                .navigate_to(RouteFrame::new(Route::DownloadQueue));
        }
        self.view = View::DownloadQueue;
        self.refresh_download_queue(ui, true, false);
    }

    /// Windows `refresh_download_queue`: the Tab order is Back, then
    /// Download selected, Download all as audio and video while something is
    /// queued, Cancel and Cancel all while something downloads, the list.
    pub(crate) fn refresh_download_queue(
        &mut self,
        ui: &mut dyn ShellUi,
        focus: bool,
        announce_status: bool,
    ) {
        let previous = (self.view == View::DownloadQueue)
            .then(|| ui.selected_row())
            .flatten();
        let catalog = self.catalog();
        let downloads = self.application.downloads();
        let mut rows: Vec<String> = downloads
            .active()
            .iter()
            .map(|task| active_download_label(task, &catalog))
            .collect();
        rows.extend(
            downloads
                .queued()
                .iter()
                .map(|queued| queued_download_label(queued, &catalog)),
        );
        let count = downloads.count();
        if count == 0 {
            rows.push(catalog.text("no_queued_downloads").to_owned());
        }
        let selected = previous
            .filter(|_| count > 0)
            .unwrap_or_default()
            .min(rows.len() - 1);
        let button = |id: &'static str, key: &str| ScreenControl::Button {
            id,
            label: catalog.text(key).to_owned(),
        };
        let mut controls = vec![button("back", "back")];
        if !downloads.queued().is_empty() {
            controls.extend([
                button("open", "download_selected_queued"),
                button("download_all_audio", "download_all_as_audio"),
                button("download_all_video", "download_all_as_video"),
            ]);
        }
        if !downloads.active().is_empty() {
            controls.extend([
                button("download_cancel", "cancel_download"),
                button("download_cancel_all", "cancel_all_downloads"),
            ]);
        }
        controls.push(ScreenControl::List);
        ui.show_list_screen(&ListScreen {
            list_name: catalog.text("current_downloads").to_owned(),
            rows,
            selected: Some(selected),
            controls,
            focus_list: focus,
        });
        if announce_status {
            let message = if count == 0 {
                catalog.text("no_queued_downloads").to_owned()
            } else {
                format!("{}: {count}", catalog.text("current_downloads"))
            };
            ui.set_status(&message, true);
        }
    }

    /// Windows `refresh_download_projection`.
    fn refresh_download_projection(&mut self, ui: &mut dyn ShellUi, update_main_menu: bool) {
        if self.view == View::DownloadQueue {
            self.refresh_download_queue(ui, false, false);
        } else if update_main_menu && self.view == View::MainMenu {
            let current = ui.selected_row();
            self.refresh_main_menu(ui, MainMenuSelection::Preserve, current, false);
        }
    }

    /// The download screen's buttons (Windows `handle_window_command`);
    /// `true` when `id` is one.
    pub(crate) fn download_command(&mut self, ui: &mut dyn ShellUi, id: &str) -> bool {
        match id {
            "download_all_audio" => self.start_all_queued_downloads(ui, DownloadChoice::Audio),
            "download_all_video" => self.start_all_queued_downloads(ui, DownloadChoice::Video),
            "download_cancel" => self.cancel_selected_download(ui),
            "download_cancel_all" => self.cancel_all_downloads(ui),
            "download_remove_queued" => self.remove_selected_queued_download(ui),
            "playlist_download" => self.download_current_user_playlist(ui),
            "rss_download_episode" => {
                if let Some(item) = self.active_rss_item(ui) {
                    self.start_download_item(ui, &item, DownloadChoice::Audio, false);
                }
            }
            "rss_queue_episode" => {
                if let Some(item) = self.active_rss_item(ui) {
                    self.toggle_download_queue_item(ui, &item, DownloadChoice::Audio);
                }
            }
            "rss_download_feed" => self.download_current_rss_feed(ui),
            _ => return false,
        }
        true
    }

    /// The download actions of the action list; `true` when `action_id` is
    /// one.
    pub(crate) fn download_action(&mut self, ui: &mut dyn ShellUi, action_id: &str) -> bool {
        match action_id {
            "open_current_downloads" => self.show_download_queue(ui),
            "download_audio" => self.start_download_shortcut(ui, DownloadChoice::Audio),
            "download_video" => self.start_download_shortcut(ui, DownloadChoice::Video),
            "queue_audio" => self.toggle_active_download_queue(ui),
            _ => return false,
        }
        true
    }

    /// Windows `start_selected_queued_download`: an active row cancels.
    pub(crate) fn start_selected_queued_download(
        &mut self,
        ui: &mut dyn ShellUi,
        requested_choice: Option<DownloadChoice>,
    ) {
        let Some(selected) = ui.selected_row() else {
            return;
        };
        let active_count = self.application.downloads().active().len();
        if selected < active_count {
            self.cancel_selected_download(ui);
            return;
        }
        let Some(queued) = self
            .application
            .downloads()
            .queued()
            .get(selected - active_count)
            .cloned()
        else {
            ui.set_status(&self.text("download_queue_empty"), true);
            return;
        };
        let choice = match requested_choice.unwrap_or(queued.choice) {
            DownloadChoice::Ask => match self.choose_download_format(ui) {
                Some(choice) => choice,
                None => return,
            },
            choice => choice,
        };
        self.start_download_item(ui, &queued.item, choice, true);
    }

    /// Windows `choose_download_format`.
    fn choose_download_format(&self, ui: &mut dyn ShellUi) -> Option<DownloadChoice> {
        let prompt = self.text("select_download_format_message");
        match ui.choose(&ChoiceDialog {
            title: self.text("select_download_format"),
            list_name: prompt.clone(),
            prompt,
            choices: vec![self.text("download_audio"), self.text("download_video")],
            initial: 0,
            ok: self.text("ok"),
            cancel: self.text("cancel"),
        })? {
            0 => Some(DownloadChoice::Audio),
            _ => Some(DownloadChoice::Video),
        }
    }

    /// Windows `start_download_shortcut`.
    fn start_download_shortcut(&mut self, ui: &mut dyn ShellUi, choice: DownloadChoice) {
        if self.view == View::MainMenu {
            return;
        }
        let identity = self
            .active_media_item(ui)
            .and_then(|item| item.stable_identity())
            .unwrap_or_default();
        let press = DownloadShortcutPress {
            choice,
            identity,
            at: Instant::now(),
        };
        if accept_download_shortcut(&mut self.downloads.last_shortcut, press) {
            self.start_active_download(ui, choice);
        }
    }

    /// Windows `start_active_download`.
    pub(crate) fn start_active_download(&mut self, ui: &mut dyn ShellUi, choice: DownloadChoice) {
        let session = self.application.player_session();
        if self.view == View::Player
            && (session.clip_start_seconds().is_some() || session.clip_end_seconds().is_some())
        {
            // Clip export arrives with the converters (unit M9b).
            let message = apricot_app::unavailable_feature_message(
                &self.catalog(),
                &self.text("export_clip"),
            );
            ui.set_status(&message, true);
            return;
        }
        if self.view == View::DownloadQueue {
            self.start_selected_queued_download(ui, Some(choice));
            return;
        }
        if self.view == View::MainMenu {
            return;
        }
        let Some(item) = self.active_media_item(ui) else {
            ui.set_status(&self.text("no_selection"), true);
            return;
        };
        self.start_download_item(ui, &item, choice, false);
    }

    /// Windows `start_download_item`.
    pub(crate) fn start_download_item(
        &mut self,
        ui: &mut dyn ShellUi,
        item: &MediaItem,
        requested_choice: DownloadChoice,
        remove_queued: bool,
    ) {
        // AudioVault downloads arrive with their unit (M12).
        if item.is_local_media() {
            ui.set_status(&self.text("no_selection"), true);
            return;
        }
        let choice = if item.kind == MediaKind::PodcastEpisode {
            DownloadChoice::Audio
        } else {
            requested_choice
        };
        let choice = match choice {
            DownloadChoice::Ask => match self.choose_download_format(ui) {
                Some(choice) => choice,
                None => return,
            },
            choice => choice,
        };
        if !self.confirm_download(ui, item, choice) {
            return;
        }
        if remove_queued {
            let _ = self.application.downloads_mut().remove_queued(item);
        }
        let request = match self.build_download_request(ui, item, choice, None) {
            Ok(Some(request)) => request,
            Ok(None) => {
                ui.set_status(&self.text("download_cancelled"), false);
                return;
            }
            Err(error) => {
                ui.show_error(&error);
                return;
            }
        };
        let Some(executable) = ytdlp_executable() else {
            ui.show_error("Application path is unavailable.");
            return;
        };
        let task_kind = match item.kind {
            MediaKind::Playlist => DownloadTaskKind::Playlist,
            MediaKind::Channel => DownloadTaskKind::Channel,
            MediaKind::PodcastFeed => DownloadTaskKind::PodcastFeed,
            _ => DownloadTaskKind::Single,
        };
        let total = usize::from(!request.allow_playlist);
        let task_id =
            self.application
                .downloads_mut()
                .begin(item.clone(), choice, task_kind, total);
        let cancellation = self.register_cancellation(task_id);
        spawn_download(
            task_id,
            executable,
            request,
            cancellation,
            self.downloads.sender.clone(),
        );
        ui.set_background_polling(true);
        let key = match task_kind {
            DownloadTaskKind::Playlist => "download_playlist_start",
            DownloadTaskKind::Channel => "download_channel_start",
            DownloadTaskKind::PodcastFeed => "download_feed_start",
            _ if choice == DownloadChoice::Audio => "download_audio_start",
            _ => "download_video_start",
        };
        ui.set_status(&self.text("download_started"), true);
        ui.set_status(&self.text(key), false);
        self.refresh_download_projection(ui, false);
        if matches!(
            task_kind,
            DownloadTaskKind::Playlist | DownloadTaskKind::Channel | DownloadTaskKind::UserPlaylist
        ) {
            self.show_download_progress_window(ui, task_id);
        }
    }

    fn register_cancellation(&mut self, task_id: u64) -> Arc<AtomicBool> {
        let cancellation = Arc::new(AtomicBool::new(false));
        self.downloads
            .cancellations
            .insert(task_id, Arc::clone(&cancellation));
        cancellation
    }

    /// Windows `confirm_download`.
    fn confirm_download(
        &self,
        ui: &mut dyn ShellUi,
        item: &MediaItem,
        choice: DownloadChoice,
    ) -> bool {
        if !self.application.settings().confirm_before_download {
            return true;
        }
        let action = self.text(if choice == DownloadChoice::Audio {
            "download_audio_mode"
        } else {
            "download_video_mode"
        });
        let message = self
            .text("download_confirm")
            .replace("{action}", &action)
            .replace("{title}", &item.title);
        if ui.ask_yes_no(&message, &self.text("yes"), &self.text("no")) {
            return true;
        }
        ui.set_status(&self.text("download_cancelled"), false);
        false
    }

    /// Windows `build_download_request`; `Ok(None)` when a save dialog was
    /// cancelled.
    fn build_download_request(
        &mut self,
        ui: &mut dyn ShellUi,
        item: &MediaItem,
        choice: DownloadChoice,
        output_override: Option<&Path>,
    ) -> Result<Option<DownloadRequest>, String> {
        let cookies_file = self.effective_cookies_file();
        let settings = self.application.settings().clone();
        let settings_file = self.application.settings_file();
        let url = item
            .url
            .as_ref()
            .map(ToString::to_string)
            .ok_or_else(|| "The selected item has no downloadable URL.".to_owned())?;
        let url = collection_download_url(item, &url);
        let mode = if choice == DownloadChoice::Audio {
            DownloadMode::Audio
        } else {
            DownloadMode::Video
        };
        let allow_playlist = matches!(
            item.kind,
            MediaKind::Playlist | MediaKind::Channel | MediaKind::PodcastFeed
        );
        let mut output_directory = download_folder_for_item(&settings, item, allow_playlist)?;
        if let Some(output_override) = output_override {
            output_override.clone_into(&mut output_directory);
        }
        std::fs::create_dir_all(&output_directory)
            .map_err(|error| format!("Could not create the download folder: {error}"))?;
        let mut target_path = None;
        if settings.ask_download_location_each_time && output_override.is_none() {
            if allow_playlist {
                let Some(selected) =
                    ui.choose_download_folder(&self.text("choose_save_folder"), &output_directory)
                else {
                    return Ok(None);
                };
                output_directory = selected;
            } else {
                let extension = if mode == DownloadMode::Audio {
                    normalized_audio_format(&settings.audio_format)
                } else {
                    "mp4".to_owned()
                };
                let default_file = format!("{}.{}", safe_path_component(&item.title), extension);
                let Some(selected) = ui.choose_save_file(
                    &self.text("choose_save_path"),
                    Some(&output_directory),
                    &default_file,
                    &[extension.as_str()],
                ) else {
                    return Ok(None);
                };
                output_directory = selected
                    .parent()
                    .map(Path::to_path_buf)
                    .ok_or_else(|| "The selected download path has no folder.".to_owned())?;
                target_path = Some(selected);
            }
        }
        let mut options = download_options_from_settings(&settings, &settings_file);
        options.cookies_file = cookies_file;
        Ok(Some(DownloadRequest {
            url,
            title: item.title.clone(),
            mode,
            output_directory,
            target_path,
            allow_playlist,
            options,
        }))
    }

    /// Windows `start_all_queued_downloads`.
    pub(crate) fn start_all_queued_downloads(
        &mut self,
        ui: &mut dyn ShellUi,
        requested_choice: DownloadChoice,
    ) {
        let queued = self.application.downloads().queued().to_vec();
        if queued.is_empty() {
            ui.set_status(&self.text("download_queue_empty"), true);
            return;
        }
        let settings = self.application.settings();
        let default_folder = PathBuf::from(settings.download_folder.trim());
        let batch_folder = if settings.ask_download_location_each_time {
            let initial = if default_folder.is_absolute() {
                default_folder.clone()
            } else {
                PathBuf::from("/")
            };
            let Some(folder) =
                ui.choose_download_folder(&self.text("choose_save_folder"), &initial)
            else {
                ui.set_status(&self.text("download_cancelled"), false);
                return;
            };
            Some(folder)
        } else {
            None
        };
        let mut requests = Vec::with_capacity(queued.len());
        for queued_item in &queued {
            let choice = if queued_item.item.kind == MediaKind::PodcastEpisode {
                DownloadChoice::Audio
            } else {
                requested_choice
            };
            match self.build_download_request(
                ui,
                &queued_item.item,
                choice,
                batch_folder.as_deref(),
            ) {
                Ok(Some(request)) => requests.push(request),
                Ok(None) => return,
                Err(error) => {
                    ui.show_error(&error);
                    return;
                }
            }
        }
        let Some(executable) = ytdlp_executable() else {
            ui.show_error("Application path is unavailable.");
            return;
        };
        let batch_title = self.text(if requested_choice == DownloadChoice::Audio {
            "download_all_as_audio"
        } else {
            "download_all_as_video"
        });
        let mut batch_item = queued[0].item.clone();
        batch_item.title = batch_title;
        let _ = self.application.downloads_mut().take_queued();
        let task_id = self.application.downloads_mut().begin(
            batch_item,
            requested_choice,
            DownloadTaskKind::Batch,
            requests.len(),
        );
        let cancellation = self.register_cancellation(task_id);
        spawn_batch_download(
            task_id,
            executable,
            requests,
            batch_folder.unwrap_or(default_folder),
            cancellation,
            self.downloads.sender.clone(),
        );
        ui.set_background_polling(true);
        let message = self
            .text("batch_download_start")
            .replace("{count}", &queued.len().to_string());
        ui.set_status(&message, true);
        self.refresh_download_projection(ui, false);
        if queued.len() > 5
            || queued
                .iter()
                .any(|queued| matches!(queued.item.kind, MediaKind::Playlist | MediaKind::Channel))
        {
            self.show_download_progress_window(ui, task_id);
        }
    }

    /// Windows `selected_user_playlist_download_plan`.
    fn selected_user_playlist_download_plan(
        &self,
        ui: &dyn ShellUi,
    ) -> Option<UserPlaylistDownloadPlan> {
        let playlist_index = match self.view {
            View::UserPlaylists => ui.selected_row()?,
            View::UserPlaylistItems => self.current_user_playlist_index(),
            _ => return None,
        };
        let playlist = self.application.user_playlists().get(playlist_index)?;
        Some(UserPlaylistDownloadPlan {
            playlist_index,
            title: playlist.title.clone(),
            items: playlist
                .items
                .iter()
                .filter(|item| !item.is_local_media() && item.url.is_some())
                .cloned()
                .collect(),
        })
    }

    /// Windows `download_current_user_playlist`.
    pub(crate) fn download_current_user_playlist(&mut self, ui: &mut dyn ShellUi) {
        let Some(plan) = self.selected_user_playlist_download_plan(ui) else {
            ui.set_status(&self.text("no_playlists"), true);
            return;
        };
        if plan.items.is_empty() {
            ui.set_status(&self.text("playlist_empty"), true);
            return;
        }
        let settings = self.application.settings().clone();
        let output_folder = match user_playlist_download_folder(&settings, &plan.title) {
            Ok(folder) if !settings.ask_download_location_each_time => folder,
            Ok(folder) => {
                let initial = folder
                    .parent()
                    .filter(|path| path.is_dir())
                    .map_or_else(|| folder.clone(), Path::to_path_buf);
                let Some(chosen) =
                    ui.choose_download_folder(&self.text("choose_save_folder"), &initial)
                else {
                    ui.set_status(&self.text("download_cancelled"), true);
                    return;
                };
                chosen
            }
            Err(error) => {
                ui.show_error(&error);
                return;
            }
        };
        let mut requests = Vec::with_capacity(plan.items.len());
        for item in &plan.items {
            let mut request = match self.build_download_request(
                ui,
                item,
                DownloadChoice::Video,
                Some(&output_folder),
            ) {
                Ok(Some(request)) => request,
                Ok(None) => return,
                Err(error) => {
                    ui.show_error(&error);
                    return;
                }
            };
            if matches!(item.kind, MediaKind::Playlist | MediaKind::Channel) {
                request.output_directory = output_folder.join(safe_path_component(&item.title));
                if let Err(error) = std::fs::create_dir_all(&request.output_directory) {
                    ui.show_error(&format!("Could not create the download folder: {error}"));
                    return;
                }
            }
            requests.push(request);
        }
        let Some(executable) = ytdlp_executable() else {
            ui.show_error("Application path is unavailable.");
            return;
        };
        let count = requests.len();
        self.set_current_user_playlist_index(plan.playlist_index);
        let mut batch_item = plan.items[0].clone();
        batch_item.title.clone_from(&plan.title);
        let task_id = self.application.downloads_mut().begin(
            batch_item,
            DownloadChoice::Video,
            DownloadTaskKind::UserPlaylist,
            count,
        );
        let cancellation = self.register_cancellation(task_id);
        spawn_batch_download(
            task_id,
            executable,
            requests,
            output_folder,
            cancellation,
            self.downloads.sender.clone(),
        );
        ui.set_background_polling(true);
        self.show_download_progress_window(ui, task_id);
        let message = self
            .text("batch_download_start")
            .replace("{count}", &count.to_string());
        ui.set_status(&message, true);
        self.refresh_download_projection(ui, false);
    }

    /// Windows `download_current_rss_feed`.
    fn download_current_rss_feed(&mut self, ui: &mut dyn ShellUi) {
        let Some((title, items, initial)) = self.current_rss_feed_download() else {
            ui.set_status(&self.text("rss_items_empty"), true);
            return;
        };
        let output_override = if self.application.settings().ask_download_location_each_time {
            let Some(folder) =
                ui.choose_download_folder(&self.text("choose_save_folder"), &initial)
            else {
                return;
            };
            Some(folder)
        } else {
            None
        };
        let mut requests = Vec::with_capacity(items.len());
        for item in &items {
            match self.build_download_request(
                ui,
                item,
                DownloadChoice::Audio,
                output_override.as_deref(),
            ) {
                Ok(Some(request)) => requests.push(request),
                Ok(None) => return,
                Err(error) => {
                    ui.show_error(&error);
                    return;
                }
            }
        }
        let Some(executable) = ytdlp_executable() else {
            return;
        };
        let mut feed_item = items[0].clone();
        feed_item.title = title;
        feed_item.kind = MediaKind::PodcastFeed;
        let task_id = self.application.downloads_mut().begin(
            feed_item,
            DownloadChoice::Audio,
            DownloadTaskKind::PodcastFeed,
            requests.len(),
        );
        let cancellation = self.register_cancellation(task_id);
        spawn_batch_download(
            task_id,
            executable,
            requests,
            output_override.unwrap_or(initial),
            cancellation,
            self.downloads.sender.clone(),
        );
        ui.set_background_polling(true);
        ui.set_status(&self.text("download_feed_start"), true);
        self.refresh_download_projection(ui, false);
    }

    /// The open feed's title, its episodes with a link and its download
    /// folder (Windows `download_current_rss_feed`).
    fn current_rss_feed_download(&self) -> Option<(String, Vec<MediaItem>, PathBuf)> {
        let feed = self
            .application
            .rss_feeds()
            .get(self.current_rss_feed_index())?;
        let items: Vec<MediaItem> = feed
            .items
            .iter()
            .filter(|item| item.url.is_some())
            .cloned()
            .collect();
        let initial =
            download_folder_for_item(self.application.settings(), items.first()?, true).ok()?;
        Some((feed.title.clone(), items, initial))
    }

    /// Windows `cancel_selected_download`.
    fn cancel_selected_download(&mut self, ui: &mut dyn ShellUi) {
        let task_id = ui.selected_row().and_then(|selected| {
            self.application
                .downloads()
                .active()
                .get(selected)
                .map(|task| task.id)
        });
        let Some(task_id) = task_id else {
            ui.set_status(&self.text("no_active_download"), true);
            return;
        };
        self.cancel_download_task(ui, task_id);
    }

    /// Windows `remove_selected_queued_download`.
    fn remove_selected_queued_download(&mut self, ui: &mut dyn ShellUi) {
        let active_count = self.application.downloads().active().len();
        let Some(item) = ui
            .selected_row()
            .and_then(|selected| selected.checked_sub(active_count))
            .and_then(|index| self.application.downloads().queued().get(index))
            .map(|queued| queued.item.clone())
        else {
            return;
        };
        let _ = self.application.downloads_mut().remove_queued(&item);
        let message = self
            .text("download_deselected")
            .replace("{title}", &item.title);
        ui.set_status(&message, true);
        self.refresh_download_projection(ui, false);
    }

    /// Windows `cancel_download_task`.
    fn cancel_download_task(&mut self, ui: &mut dyn ShellUi, task_id: u64) {
        let Some(title) = self
            .application
            .downloads()
            .active_task(task_id)
            .map(|task| task.title.clone())
        else {
            return;
        };
        if let Some(cancellation) = self.downloads.cancellations.get(&task_id) {
            cancellation.store(true, Ordering::Release);
        }
        let _ = self.application.downloads_mut().request_cancel(task_id);
        let message = self
            .text("download_cancel_requested")
            .replace("{title}", &title);
        ui.set_status(&message, true);
        self.refresh_download_projection(ui, false);
    }

    /// Windows `cancel_all_downloads`.
    fn cancel_all_downloads(&mut self, ui: &mut dyn ShellUi) {
        if self.application.downloads().active().is_empty() {
            ui.set_status(&self.text("no_active_download"), true);
            return;
        }
        for cancellation in self.downloads.cancellations.values() {
            cancellation.store(true, Ordering::Release);
        }
        let _ = self.application.downloads_mut().request_cancel_all();
        ui.set_status(&self.text("all_downloads_cancel_requested"), true);
        self.refresh_download_projection(ui, false);
    }

    /// Windows `toggle_active_download_queue`.
    fn toggle_active_download_queue(&mut self, ui: &mut dyn ShellUi) {
        let Some(item) = self.active_media_item(ui) else {
            return;
        };
        let choice = if item.kind == MediaKind::PodcastEpisode {
            DownloadChoice::Audio
        } else {
            DownloadChoice::Ask
        };
        self.toggle_download_queue_item(ui, &item, choice);
    }

    /// Windows `toggle_download_queue_item`.
    fn toggle_download_queue_item(
        &mut self,
        ui: &mut dyn ShellUi,
        item: &MediaItem,
        choice: DownloadChoice,
    ) {
        let outcome = self
            .application
            .downloads_mut()
            .queue_item(item.clone(), choice);
        let collection = matches!(item.kind, MediaKind::Playlist | MediaKind::Channel);
        let key = match outcome {
            QueueToggleOutcome::Deselected => "download_deselected",
            QueueToggleOutcome::Rejected => "no_selection",
            _ if item.kind == MediaKind::PodcastEpisode => {
                "podcast_episode_audio_selected_download"
            }
            _ => match choice {
                DownloadChoice::Ask => "selected_for_download_or_playlist",
                DownloadChoice::Audio if collection => "collection_audio_selected_download",
                DownloadChoice::Video if collection => "collection_video_selected_download",
                DownloadChoice::Audio => "audio_selected_download",
                DownloadChoice::Video => "video_selected_download",
            },
        };
        let message = self.text(key).replace("{title}", &item.title);
        ui.set_status(&message, true);
        self.refresh_queued_row(ui);
        self.refresh_download_projection(ui, false);
    }

    /// Windows `refresh_queued_row`: the result row's marker changes once
    /// the selection moves; the episode row changes at once.
    fn refresh_queued_row(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = ui.selected_row() else {
            return;
        };
        match self.view {
            View::Results | View::Trending | View::Collection => {
                self.downloads.deferred_rows.insert(index);
            }
            View::RssItems => {
                if let Some(label) = self.rss_episode_row_label(index) {
                    ui.set_list_row(index, &label);
                }
            }
            _ => {}
        }
    }

    /// Windows `apply_deferred_youtube_metadata_rows` when the selection
    /// moved.
    pub(crate) fn apply_deferred_result_rows(&mut self, ui: &mut dyn ShellUi, exclude: usize) {
        let pending = std::mem::take(&mut self.downloads.deferred_rows);
        let catalog = self.catalog();
        for index in pending {
            if index == exclude {
                self.downloads.deferred_rows.insert(index);
                continue;
            }
            let item = match self.view {
                View::Results | View::Trending => {
                    self.application.search_session().items().get(index)
                }
                View::Collection => self
                    .application
                    .youtube_collection()
                    .and_then(|collection| collection.items().get(index)),
                _ => None,
            };
            if let Some(item) = item {
                let label = crate::result_label(item, &catalog, self.application.downloads());
                ui.set_list_row(index, &label);
            }
        }
    }

    /// Windows `show_download_progress_window`.
    fn show_download_progress_window(&mut self, ui: &mut dyn ShellUi, task_id: u64) {
        let Some(task) = self.application.downloads().active_task(task_id) else {
            return;
        };
        let catalog = self.catalog();
        let (percent, message) = download_progress_presentation(task, &catalog);
        self.downloads.progress_task_ids.insert(task_id);
        self.downloads.progress_task_id = Some(task_id);
        self.downloads.progress_open = true;
        ui.show_download_progress(&DownloadProgress {
            title: catalog.text("download_progress_title").to_owned(),
            message,
            percent,
            hide: catalog.text("download_progress_hide").to_owned(),
            details: catalog.text("download_progress_details").to_owned(),
        });
    }

    /// Windows `update_download_progress_window`.
    fn update_download_progress_window(&self, ui: &mut dyn ShellUi) {
        if !self.downloads.progress_open {
            return;
        }
        let Some(task) = self
            .downloads
            .progress_task_id
            .and_then(|task_id| self.application.downloads().active_task(task_id))
        else {
            return;
        };
        let (percent, message) = download_progress_presentation(task, &self.catalog());
        ui.update_download_progress(percent, &message);
    }

    /// Windows `close_download_progress_window`.
    fn close_download_progress_window(&mut self, ui: &mut dyn ShellUi, task_id: u64) {
        self.downloads.progress_task_ids.remove(&task_id);
        if self.downloads.progress_task_id != Some(task_id) {
            return;
        }
        let next = self
            .application
            .downloads()
            .active()
            .iter()
            .map(|task| task.id)
            .find(|candidate| self.downloads.progress_task_ids.contains(candidate));
        if let Some(next) = next {
            self.downloads.progress_task_id = Some(next);
            self.update_download_progress_window(ui);
            return;
        }
        self.downloads.progress_task_id = None;
        self.downloads.progress_task_ids.clear();
        if std::mem::take(&mut self.downloads.progress_open) {
            ui.close_download_progress();
        }
    }

    /// Details in the progress window (Windows
    /// `show_download_progress_details`).
    pub fn download_progress_details(&mut self, ui: &mut dyn ShellUi) {
        self.show_download_queue(ui);
    }

    /// Windows `poll_download_updates`; `true` while downloads run.
    pub(crate) fn poll_downloads(&mut self, ui: &mut dyn ShellUi) -> bool {
        while let Ok(update) = self.downloads.receiver.try_recv() {
            self.apply_download_update(ui, update);
        }
        !self.application.downloads().active().is_empty()
    }

    fn accept_download_progress(
        &mut self,
        task_id: u64,
        phase: DownloadPhase,
        percent: Option<f64>,
        title: &str,
    ) -> bool {
        let now = Instant::now();
        let Some(last) = self.downloads.progress_reports.get_mut(&task_id) else {
            self.downloads.progress_reports.insert(
                task_id,
                DownloadProgressReport {
                    percent_bucket: download_percent_bucket(percent),
                    title: title.to_owned(),
                    at: now,
                },
            );
            return true;
        };
        phase == DownloadPhase::Processing
            || should_report_download_progress(last, percent, title, now)
    }

    /// Windows `apply_download_update` for a progress event.
    fn apply_download_event(&mut self, ui: &mut dyn ShellUi, task_id: u64, event: DownloadEvent) {
        match event {
            DownloadEvent::Progress {
                phase,
                title,
                percent,
                playlist_index,
                playlist_count,
            } => {
                if !self.accept_download_progress(task_id, phase, percent, &title) {
                    return;
                }
                let status = if phase == DownloadPhase::Processing {
                    DownloadTaskStatus::Processing
                } else {
                    DownloadTaskStatus::Downloading
                };
                let _ = self.application.downloads_mut().update_progress(
                    task_id,
                    status,
                    &title,
                    percent,
                    playlist_index.and_then(|value| usize::try_from(value).ok()),
                    playlist_count.and_then(|value| usize::try_from(value).ok()),
                );
            }
            DownloadEvent::ItemFailed { message } => {
                let _ = self
                    .application
                    .downloads_mut()
                    .add_item_failure(task_id, &message);
            }
            DownloadEvent::FileFinished { title, .. } => {
                if let Some(task) = self.application.downloads().active_task(task_id) {
                    let should_increment = !matches!(
                        task.kind,
                        DownloadTaskKind::Playlist | DownloadTaskKind::Channel
                    ) || task.total == 0;
                    let completed = task.completed.saturating_add(usize::from(should_increment));
                    let total = task.total.max(completed);
                    let _ = self
                        .application
                        .downloads_mut()
                        .set_completed(task_id, completed, total);
                }
                let _ = self.application.downloads_mut().update_progress(
                    task_id,
                    DownloadTaskStatus::Processing,
                    &title,
                    Some(100.0),
                    None,
                    None,
                );
            }
        }
        self.refresh_download_projection(ui, false);
        self.update_download_progress_window(ui);
    }

    /// Windows `apply_download_update`.
    fn apply_download_update(&mut self, ui: &mut dyn ShellUi, update: DownloadWorkerUpdate) {
        let (task_id, result, output_directory) = match update {
            DownloadWorkerUpdate::Event { task_id, event } => {
                self.apply_download_event(ui, task_id, event);
                return;
            }
            DownloadWorkerUpdate::Finished {
                task_id,
                result,
                output_directory,
            } => (task_id, result, output_directory),
        };
        self.downloads.cancellations.remove(&task_id);
        self.downloads.progress_reports.remove(&task_id);
        let Some(task) = self.application.downloads_mut().finish(task_id) else {
            return;
        };
        self.close_download_progress_window(ui, task_id);
        let cancelled = result
            .as_ref()
            .err()
            .is_some_and(|error| error.to_ascii_lowercase().contains("cancelled"));
        let succeeded = result.is_ok();
        let message = match result {
            Ok(summary) if !summary.item_failures.is_empty() => self
                .text("batch_download_done_with_errors")
                .replace("{failed}", &summary.item_failures.len().to_string()),
            Ok(_) => {
                let key = match task.kind {
                    DownloadTaskKind::Batch => "batch_download_done",
                    DownloadTaskKind::PodcastFeed => "download_feed_done",
                    DownloadTaskKind::Playlist | DownloadTaskKind::UserPlaylist => {
                        "download_playlist_done"
                    }
                    DownloadTaskKind::Channel => "download_channel_done",
                    DownloadTaskKind::Single if task.choice == DownloadChoice::Audio => {
                        "download_audio_done"
                    }
                    DownloadTaskKind::Single => "download_video_done",
                };
                self.text(key).replace("{title}", &task.title)
            }
            Err(_) if cancelled => self.text("download_cancelled"),
            Err(error) => self.text("download_failed").replace("{error}", &error),
        };
        // Python shows a cancelled download on the status line only and a
        // failed one in a message box.
        ui.set_status(&message, succeeded);
        if !succeeded && !cancelled {
            ui.show_error(&message);
        }
        if succeeded {
            let history_action = match task.kind {
                DownloadTaskKind::Single if task.choice == DownloadChoice::Audio => {
                    Some("downloaded audio")
                }
                DownloadTaskKind::Single => Some("downloaded video"),
                DownloadTaskKind::Playlist => Some("downloaded playlist"),
                DownloadTaskKind::Channel => Some("downloaded channel"),
                DownloadTaskKind::PodcastFeed
                | DownloadTaskKind::UserPlaylist
                | DownloadTaskKind::Batch => None,
            };
            if let Some(action) = history_action
                && let Err(error) = self.application.record_history(
                    task.item.clone(),
                    action,
                    crate::unix_timestamp(),
                )
            {
                ui.set_status(&format!("Download history was not saved: {error}"), true);
            }
        }
        let settings = self.application.settings();
        let app_has_focus = ui.app_is_active();
        let popup = app_has_focus && settings.popup_when_download_complete && succeeded;
        let open_folder =
            settings.open_folder_after_download && succeeded && output_directory.is_dir();
        // The system notification when the window is in the background
        // arrives with the notifications (unit M14).
        self.refresh_download_projection(ui, true);
        if popup {
            ui.show_error(&message);
        }
        if open_folder && let Err(error) = apricot_platform::shell::open_path(&output_directory) {
            ui.set_status(
                &format!("Could not open the download folder: {error}"),
                true,
            );
        }
    }

    /// Windows `download_queue_context_entries`.
    pub(crate) fn download_queue_context_entries(
        &self,
        ui: &dyn ShellUi,
    ) -> Option<Vec<ContextMenuEntry>> {
        let selected = ui.selected_row()?;
        let downloads = self.application.downloads();
        let active_count = downloads.active().len();
        let entry = |id: &'static str, key: &str| ContextMenuEntry::Command {
            label: self.text(key),
            command: ContextCommand::PlayerAction(id),
        };
        if selected < active_count {
            return Some(vec![
                entry("download_cancel", "cancel_download"),
                entry("download_cancel_all", "cancel_all_downloads"),
            ]);
        }
        downloads.queued().get(selected - active_count)?;
        Some(vec![
            entry("download_selected_queued", "download_selected_queued"),
            entry("download_selected_audio", "download_audio"),
            entry("download_selected_video", "download_video"),
            entry("download_all_audio", "download_all_as_audio"),
            entry("download_all_video", "download_all_as_video"),
            entry("download_remove_queued", "remove_from_queue"),
        ])
    }

    /// A download context command of the download screen; `true` when
    /// `id` is one.
    pub(crate) fn download_context_command(&mut self, ui: &mut dyn ShellUi, id: &str) -> bool {
        match id {
            "download_selected_queued" => self.start_selected_queued_download(ui, None),
            "download_selected_audio" => {
                self.start_active_download(ui, DownloadChoice::Audio);
            }
            "download_selected_video" => {
                self.start_active_download(ui, DownloadChoice::Video);
            }
            _ => return self.download_command(ui, id),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_shortcut_for_the_same_item_is_ignored_briefly() {
        let mut last = None;
        let at = Instant::now();
        let press = |choice, at| DownloadShortcutPress {
            choice,
            identity: "id".to_owned(),
            at,
        };
        assert!(accept_download_shortcut(
            &mut last,
            press(DownloadChoice::Audio, at)
        ));
        assert!(!accept_download_shortcut(
            &mut last,
            press(DownloadChoice::Audio, at + Duration::from_millis(100))
        ));
        assert!(accept_download_shortcut(
            &mut last,
            press(DownloadChoice::Video, at + Duration::from_millis(150))
        ));
        assert!(accept_download_shortcut(
            &mut last,
            press(DownloadChoice::Video, at + Duration::from_millis(600))
        ));
    }

    #[test]
    fn folders_follow_the_python_layout() {
        let settings = apricot_storage::SettingsDocument::with_platform_defaults(
            "/Users/me/Downloads/Apricot",
            "/tmp/cache",
            "beta",
        );
        let mut episode = crate::local_file_item(Path::new("/tmp/a.mp3"));
        episode.kind = MediaKind::PodcastEpisode;
        episode.channel = "Show: One".to_owned();
        assert_eq!(
            download_folder_for_item(&settings, &episode, false).expect("folder"),
            PathBuf::from("/Users/me/Downloads/Apricot/podcasts/Show_ One")
        );
        let mut playlist = crate::local_file_item(Path::new("/tmp/b.mp3"));
        playlist.kind = MediaKind::Playlist;
        playlist.title = "Mix".to_owned();
        assert_eq!(
            download_folder_for_item(&settings, &playlist, true).expect("folder"),
            PathBuf::from("/Users/me/Downloads/Apricot/music/Mix")
        );
        assert_eq!(safe_path_component(" con.txt "), "_con.txt");
        assert_eq!(safe_path_component("..."), "download");
    }

    #[test]
    fn progress_counts_finished_items() {
        let catalog = apricot_app::embedded_catalog("en");
        let mut controller = apricot_app::DownloadController::default();
        let item = crate::local_file_item(Path::new("/tmp/c.mp3"));
        let id = controller.begin(item, DownloadChoice::Audio, DownloadTaskKind::Batch, 4);
        controller.set_completed(id, 1, 4);
        let task = controller.active_task(id).expect("task");
        let (percent, message) = download_progress_presentation(task, &catalog);
        assert_eq!(percent, 25);
        assert!(message.contains('3'), "{message}");
    }
}
