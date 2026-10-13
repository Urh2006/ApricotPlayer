//! Smaller player actions of Windows `activate_action`: replay gain
//! (`cycle_player_replaygain`), copying the location, timestamp link and
//! stream URL, chapters (`seek_relative_player_chapter`,
//! `show_player_chapters` and the external chapter download) and the BPM
//! analysis (`announce_bpm`, `poll_bpm_analyses`).

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

use apricot_app::chapters::{self, Chapter};
use apricot_core::MediaItem;
use apricot_playback::PlaybackCommand;
use serde_json::Value;

use crate::{ChoiceDialog, PlayerFocus, Shell, ShellUi, View, format_duration};

/// Windows `PendingChapters`: chapters downloaded from `chapters_url`.
struct PendingChapters {
    generation: u64,
    /// `Some(next)` for previous or next chapter, `None` for the list.
    direction: Option<bool>,
    receiver: Receiver<Vec<Value>>,
}

/// Windows `PendingBpmAnalysis`.
struct PendingBpmAnalysis {
    key: String,
    item_key: String,
    generation: u64,
    cancelled: Arc<AtomicBool>,
    receiver: Receiver<Option<i64>>,
}

#[derive(Default)]
pub(crate) struct PlayerTools {
    pending_chapters: Option<PendingChapters>,
    pending_bpm: Vec<PendingBpmAnalysis>,
}

impl Drop for PlayerTools {
    /// Windows `WM_DESTROY` cancels running BPM analyses.
    fn drop(&mut self) {
        for analysis in &self.pending_bpm {
            analysis.cancelled.store(true, Ordering::Release);
        }
    }
}

/// Windows `application_directory`: the folder of the executable.
pub(crate) fn application_directory() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
}

/// Windows `current_bpm_state`: the analysis key, the item key and the speed.
fn current_bpm_state(session: &apricot_app::PlayerSession) -> Option<(String, String, f64)> {
    let item_key = session
        .current_item()?
        .copy_location()
        .filter(|key| !key.trim().is_empty())?;
    let (speed, pitch) = session
        .audio()
        .map_or((1.0, 1.0), |audio| (audio.speed, audio.pitch));
    Some((
        apricot_app::bpm::bpm_analysis_state_key(&item_key, speed, pitch),
        item_key,
        speed,
    ))
}

impl Shell {
    /// Windows `cycle_player_replaygain`.
    pub(crate) fn cycle_player_replaygain(&mut self, ui: &mut dyn ShellUi) {
        let mode = match self.application.cycle_replaygain_mode() {
            Ok(mode) => mode,
            Err(error) => {
                ui.set_status(&format!("Settings were not saved: {error}"), true);
                return;
            }
        };
        let applied = !self.application.player_session().is_open()
            || self
                .execute_player_command_result(PlaybackCommand::SetReplayGain(mode.clone()))
                .is_ok();
        let mode_label = self.text(apricot_app::player_model::replaygain_mode_key(&mode));
        let message = self
            .text("audio_normalization_changed")
            .replace("{mode}", &mode_label);
        ui.set_status(&message, true);
        if !applied {
            ui.set_status(&self.text("audio_normalization_restart_needed"), true);
        }
        if self.view == View::Player {
            self.refresh_player(ui, PlayerFocus::Preserve);
        }
    }

    /// Windows `active_media_item` for the screens the shell shows.
    pub(crate) fn active_media_item(&self, ui: &dyn ShellUi) -> Option<MediaItem> {
        if ui.focused_player_control().is_some() {
            return self.application.player_session().current_item().cloned();
        }
        if crate::audiovault::is_view(self.view) {
            return self.audiovault_selected_item(ui);
        }
        match self.view {
            View::Player => self.application.player_session().current_item().cloned(),
            View::LocalFolder => ui
                .selected_row()
                .and_then(|index| self.application.local_folder_session().items().get(index))
                .cloned(),
            View::Results | View::Trending => ui
                .selected_row()
                .and_then(|index| self.application.search_session().items().get(index))
                .cloned(),
            View::Collection => ui
                .selected_row()
                .and_then(|index| {
                    self.application
                        .youtube_collection()
                        .and_then(|collection| collection.items().get(index))
                })
                .cloned(),
            View::Favorites => ui
                .selected_row()
                .and_then(|index| self.application.favorites().get(index))
                .cloned(),
            View::History => ui
                .selected_row()
                .and_then(|index| self.application.history().get(index))
                .cloned(),
            View::UserPlaylistItems => ui
                .selected_row()
                .and_then(|index| {
                    self.application
                        .user_playlists()
                        .get(self.playlists.playlist)?
                        .items
                        .get(index)
                })
                .cloned(),
            View::RssItems => self.selected_rss_episode(ui),
            View::MainMenu
            | View::Search
            | View::DirectLink
            | View::UserPlaylists
            | View::RssFeeds
            | View::PodcastSearchResults
            | View::PodcastCategories
            | View::DownloadQueue
            | View::AudiovaultMenu
            | View::AudiovaultSearch
            | View::AudiovaultResults
            | View::SpotifyHub
            | View::SpotifyAccounts => None,
            View::SpotifyBrowse => self.spotify_selected_media(ui),
            View::Subscriptions | View::NotificationCenter => self.subscription_selected_media(ui),
        }
    }

    /// Windows `copy_text_and_announce`.
    pub(crate) fn copy_text_and_announce(
        &self,
        ui: &mut dyn ShellUi,
        text: &str,
        success_key: &str,
    ) {
        if ui.copy_text(text) {
            ui.set_status(&self.text(success_key), true);
        } else {
            let message = "Could not copy to the clipboard: the clipboard refused the text";
            ui.set_status(message, true);
            ui.show_error(message);
        }
    }

    /// Windows `copy_active_location`.
    pub(crate) fn copy_active_location(&self, ui: &mut dyn ShellUi) {
        let Some(item) = self.active_media_item(ui) else {
            ui.set_status(&self.text("no_selection"), true);
            return;
        };
        let Some(location) = item.copy_location() else {
            ui.set_status(&self.text("no_selection"), true);
            return;
        };
        let key = if item.is_local_media() {
            "path_copied"
        } else {
            "url_copied"
        };
        self.copy_text_and_announce(ui, &location, key);
    }

    /// Windows `copy_current_timestamp_link`.
    pub(crate) fn copy_current_timestamp_link(&self, ui: &mut dyn ShellUi) {
        let session = self.application.player_session();
        let Some(url) = session
            .current_item()
            .and_then(|item| item.youtube_url_at_timestamp(session.position_seconds()))
        else {
            ui.set_status(&self.text("timestamp_url_unavailable"), true);
            return;
        };
        self.copy_text_and_announce(ui, url.as_ref(), "timestamp_url_copied");
    }

    /// Windows `copy_active_stream_url`: streamed items are resolved first.
    pub(crate) fn copy_active_stream_url(&mut self, ui: &mut dyn ShellUi) {
        let Some(item) = self.active_media_item(ui) else {
            ui.set_status(&self.text("no_selection"), true);
            return;
        };
        if item.is_local_media() {
            ui.set_status(&self.text("direct_media_link_unavailable_local"), true);
            return;
        }
        if let Some(stream_url) = item.stream_url.as_ref() {
            self.copy_text_and_announce(ui, stream_url.as_str(), "stream_url_copied");
            return;
        }
        if matches!(
            item.source,
            apricot_core::MediaSource::Youtube | apricot_core::MediaSource::Soundcloud
        ) {
            self.start_youtube_resolve(
                ui,
                &item,
                crate::youtube::ResolvePurpose::CopyStreamUrl,
                None,
                false,
                None,
            );
            return;
        }
        if item.source == apricot_core::MediaSource::Direct
            && let Some(url) = item.url.as_ref()
        {
            self.copy_text_and_announce(ui, url.as_str(), "stream_url_copied");
            return;
        }
        let message = self.text("stream_url_failed").replace(
            "{error}",
            "this source has not exposed a direct media URL yet",
        );
        ui.set_status(&message, true);
    }

    /// Windows `seek_player_chapter`.
    pub(crate) fn seek_player_chapter(&mut self, ui: &mut dyn ShellUi, seconds: f64) -> bool {
        self.player.clip_preview = None;
        self.execute_player_command(
            ui,
            PlaybackCommand::SeekAbsolute {
                seconds,
                exact: true,
            },
        )
    }

    fn chapter_message(&self, chapter: &Chapter) -> String {
        let title = if chapter.title.is_empty() {
            self.text("chapters")
        } else {
            chapter.title.clone()
        };
        self.text("chapter_selected")
            .replace("{title}", &title)
            .replace("{time}", &format_duration(chapter.start_seconds))
    }

    /// Windows `seek_relative_player_chapter`.
    pub(crate) fn seek_relative_player_chapter(&mut self, ui: &mut dyn ShellUi, next: bool) {
        if self.request_external_chapters(ui, Some(next)) {
            return;
        }
        let session = self.application.player_session();
        if session.current_item().is_none() {
            return;
        }
        let chapters = chapters::session_chapters(session);
        let Some(chapter) = chapters::relative_chapter(&chapters, session.position_seconds(), next)
        else {
            ui.set_status(&self.text("no_chapters_available"), true);
            return;
        };
        let message = self.chapter_message(chapter);
        if self.seek_player_chapter(ui, chapter.start_seconds) {
            ui.set_status(&message, true);
        }
    }

    /// Windows `ensure_player_for_auxiliary_view`.
    pub(crate) fn ensure_player_for_auxiliary_view(&mut self, ui: &mut dyn ShellUi) -> bool {
        let session = self.application.player_session();
        if self.view == View::Player && session.current_item().is_some() {
            return true;
        }
        if self.view != View::Player && session.is_open() {
            return self.show_current_player_screen(ui);
        }
        ui.set_status(&self.text("no_player"), true);
        false
    }

    /// Windows `show_player_chapters`: the chapter list with the current
    /// chapter selected; Play seeks to the chosen one.
    pub(crate) fn show_player_chapters(&mut self, ui: &mut dyn ShellUi) {
        self.stop_key_repeat(ui);
        if !self.ensure_player_for_auxiliary_view(ui) {
            return;
        }
        if self.request_external_chapters(ui, None) {
            return;
        }
        let session = self.application.player_session();
        if session.current_item().is_none() {
            return;
        }
        let chapters = chapters::session_chapters(session);
        if chapters.is_empty() {
            ui.set_status(&self.text("no_chapters_available"), true);
            return;
        }
        let generation = session.generation();
        let position = session.position_seconds();
        let selected = chapters
            .iter()
            .rposition(|chapter| chapter.start_seconds <= position + 0.1)
            .unwrap_or(0);
        let title = self.text("chapters");
        let choices = chapters
            .iter()
            .enumerate()
            .map(|(index, chapter)| {
                let name = if chapter.title.is_empty() {
                    title.clone()
                } else {
                    chapter.title.clone()
                };
                let time = format_duration(chapter.start_seconds);
                let range = chapter.end_seconds.map_or(time.clone(), |end| {
                    format!("{time} - {}", format_duration(end))
                });
                format!("{}. {range}. {name}", index + 1)
            })
            .collect();
        let list_name = self.text("chapter_list");
        let outcome = ui.choose(&ChoiceDialog {
            title: title.clone(),
            prompt: list_name.clone(),
            list_name,
            choices,
            initial: selected,
            ok: self.text("play"),
            cancel: self.text("back"),
        });
        if let Some(chapter) = outcome.and_then(|index| chapters.get(index))
            && self.view == View::Player
            && self.application.player_session().is_open()
            && self.application.player_session().generation() == generation
            && self.seek_player_chapter(ui, chapter.start_seconds)
        {
            let message = self.chapter_message(chapter);
            ui.set_status(&message, true);
        }
        ui.focus_primary_control();
    }

    /// Windows `request_external_chapters`: `true` while the chapters of
    /// `chapters_url` are being downloaded; the action runs when they arrive.
    fn request_external_chapters(&mut self, ui: &mut dyn ShellUi, direction: Option<bool>) -> bool {
        let session = self.application.player_session();
        let Some(item) = session.current_item() else {
            return false;
        };
        if !chapters::item_chapters(item).is_empty()
            || item
                .metadata
                .get("_chapters_url_checked")
                .and_then(Value::as_bool)
                == Some(true)
        {
            return false;
        }
        let Some(url) = item
            .metadata
            .get("chapters_url")
            .and_then(Value::as_str)
            .filter(|url| !url.trim().is_empty())
            .map(str::to_owned)
        else {
            return false;
        };
        let generation = session.generation();
        if let Some(pending) = self.tools.pending_chapters.as_mut()
            && pending.generation == generation
        {
            pending.direction = direction;
            return true;
        }
        let proxy = self.application.settings().proxy.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let chapters = apricot_platform::RssClient::new(Some(&proxy))
                .and_then(|client| client.fetch_chapters(&url))
                .unwrap_or_default();
            let _ = sender.send(chapters);
        });
        self.tools.pending_chapters = Some(PendingChapters {
            generation,
            direction,
            receiver,
        });
        ui.set_background_polling(true);
        true
    }

    /// Windows `poll_external_chapters`; `true` while a download runs.
    pub(crate) fn poll_external_chapters(&mut self, ui: &mut dyn ShellUi) -> bool {
        let Some(pending) = self.tools.pending_chapters.as_ref() else {
            return false;
        };
        if pending.generation != self.application.player_session().generation()
            || self.view != View::Player
        {
            self.tools.pending_chapters = None;
            return false;
        }
        let chapters = match pending.receiver.try_recv() {
            Ok(chapters) => chapters,
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => Vec::new(),
        };
        let Some(pending) = self.tools.pending_chapters.take() else {
            return false;
        };
        if !self
            .application
            .cache_external_chapters(pending.generation, chapters)
        {
            return false;
        }
        match pending.direction {
            Some(next) => self.seek_relative_player_chapter(ui, next),
            None => self.show_player_chapters(ui),
        }
        self.tools.pending_chapters.is_some()
    }

    /// Windows `announce_bpm`: starts the analysis of the part around the
    /// position and announces "Analyzing"; the result is announced later.
    pub(crate) fn announce_bpm(&mut self, ui: &mut dyn ShellUi) {
        let session = self.application.player_session();
        let current = session
            .is_open()
            .then(|| current_bpm_state(session))
            .flatten();
        let Some((key, item_key, speed)) = current else {
            ui.set_status(&self.text("bpm_not_available"), true);
            return;
        };
        let analyzing = self.text("bpm_analyzing");
        if self
            .tools
            .pending_bpm
            .iter()
            .any(|analysis| analysis.key == key)
        {
            ui.set_status(&analyzing, true);
            return;
        }
        let (start_seconds, duration_seconds) = apricot_app::bpm::bpm_analysis_window(
            session.position_seconds(),
            session.duration_seconds(),
        );
        let generation = session.generation();
        // Python `current_stream_url`: what the player was given to play.
        let source = session
            .current_item()
            .and_then(|item| {
                item.local_path
                    .clone()
                    .filter(|path| !path.trim().is_empty())
                    .or_else(|| item.stream_url.as_ref().map(ToString::to_string))
                    .or_else(|| item.url.as_ref().map(ToString::to_string))
            })
            .unwrap_or_default();
        let request = apricot_platform::BpmAnalysisRequest {
            ffmpeg: apricot_platform::ffmpeg_executable(
                &self.application.settings().ffmpeg_location,
                application_directory().as_deref(),
            ),
            source,
            start_seconds,
            duration_seconds,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let bpm = apricot_platform::analyze_source_bpm(&request, &worker_cancelled)
                .map(|source_bpm| apricot_app::bpm::effective_playback_bpm(source_bpm, speed));
            let _ = sender.send(bpm);
        });
        self.tools.pending_bpm.push(PendingBpmAnalysis {
            key,
            item_key,
            generation,
            cancelled,
            receiver,
        });
        ui.set_status(&analyzing, true);
        ui.set_background_polling(true);
    }

    /// Windows `poll_bpm_analyses`: a result is dropped when the item,
    /// speed or pitch changed; `true` while analyses run.
    pub(crate) fn poll_bpm_analyses(&mut self, ui: &mut dyn ShellUi) -> bool {
        if self.tools.pending_bpm.is_empty() {
            return false;
        }
        let session = self.application.player_session();
        let generation = session.generation();
        let current = session
            .is_open()
            .then(|| current_bpm_state(session))
            .flatten();
        let mut finished = Vec::new();
        self.tools.pending_bpm.retain(|analysis| {
            let still_current = analysis.generation == generation
                && current.as_ref().is_some_and(|(key, item_key, _)| {
                    *key == analysis.key && *item_key == analysis.item_key
                });
            if !still_current {
                analysis.cancelled.store(true, Ordering::Release);
            }
            match analysis.receiver.try_recv() {
                Ok(bpm) => {
                    finished.push((still_current, bpm));
                    false
                }
                Err(TryRecvError::Empty) => true,
                Err(TryRecvError::Disconnected) => {
                    finished.push((still_current, None));
                    false
                }
            }
        });
        for (still_current, bpm) in finished {
            if !still_current {
                continue;
            }
            let message = match bpm.filter(|bpm| *bpm != 0) {
                Some(bpm) => self
                    .text("bpm_announcement")
                    .replace("{bpm}", &bpm.to_string()),
                None => self.text("bpm_not_available"),
            };
            ui.set_status(&message, true);
        }
        !self.tools.pending_bpm.is_empty()
    }
}
