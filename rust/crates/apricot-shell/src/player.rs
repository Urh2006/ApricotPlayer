//! The player page and its playback session (Windows `start_player_at`,
//! `refresh_player`, `poll_playback_runtime` and the player actions of
//! `activate_action`).

use std::path::Path;

use apricot_app::{
    ActivationRequest, PlayAction, PlaybackPhase, PlayerNavigationOrigin, PlayerNavigationOutcome,
    SessionToggle,
};
use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
use apricot_playback::{
    InitialPlaybackState, MpvCacheConfig, MpvLaunchOptions, PitchMode, PlaybackCommand,
    PlaybackEvent, PlaybackRuntime, RepeatMode, SpeedAudioMode, audio_filter_chain,
    is_default_rate, mpv_pitch_property, pitch_filter_active, tagged_equalizer_filter,
};

use crate::{PlayerFocus, Shell, ShellUi, View};

/// Windows `QueueStartMode`: how a started queue item leaves the queue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QueueStartMode {
    Front,
}

pub(crate) struct PendingQueuedStart {
    item: MediaItem,
    mode: QueueStartMode,
}

/// The playback state the shell keeps beside the application's session.
#[derive(Default)]
pub(crate) struct PlayerRuntime {
    pub(crate) playback: Option<PlaybackRuntime>,
    /// Python `restart_current_playback` announces only the restart.
    quiet_next_resume: bool,
    pending_queued_start: Option<PendingQueuedStart>,
    /// Windows `clip_preview`: the generation of a running clip preview.
    pub(crate) clip_preview: Option<u64>,
}

/// Python `format_rate_for_speech`.
fn format_rate_for_speech(value: f64) -> String {
    format!("{value:.2}")
}

/// Python `clamp_rate`: clamp and round to two decimals.
fn clamp_rate(value: f64, minimum: f64, maximum: f64) -> f64 {
    (value.clamp(minimum, maximum) * 100.0).round() / 100.0
}

/// Windows `format_duration`.
pub fn format_duration(seconds: f64) -> String {
    let seconds = std::time::Duration::try_from_secs_f64(seconds.max(0.0))
        .map_or(0, |duration| duration.as_secs());
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Windows `play_local_file`: the media item of a local file.
#[must_use]
pub fn local_file_item(path: &Path) -> MediaItem {
    let title = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map_or_else(|| path.display().to_string(), ToOwned::to_owned);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let kind = if matches!(
        extension.as_str(),
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "wmv"
    ) {
        MediaKind::Video
    } else {
        MediaKind::Audio
    };
    let path_text = path.to_string_lossy().into_owned();
    MediaItem {
        id: MediaId(path_text.clone()),
        source: MediaSource::Local,
        kind,
        title,
        url: None,
        stream_url: None,
        external_audio_url: None,
        local_path: Some(path_text),
        channel: String::new(),
        duration_seconds: None,
        metadata: std::collections::BTreeMap::new(),
    }
}

/// Python announcement keys for session toggles.
const fn session_toggle_key(toggle: SessionToggle, enabled: bool) -> &'static str {
    match (toggle, enabled) {
        (SessionToggle::AutoplayNext, true) => "autoplay_next_on",
        (SessionToggle::AutoplayNext, false) => "autoplay_next_off",
        (SessionToggle::BassBoost, true) => "bass_boost_on",
        (SessionToggle::BassBoost, false) => "bass_boost_off",
        (SessionToggle::VolumeBoost, true) => "volume_boost_on",
        (SessionToggle::VolumeBoost, false) => "volume_boost_off",
        (SessionToggle::Repeat, true) => "repeat_on",
        (SessionToggle::Repeat, false) => "repeat_off",
        (SessionToggle::Shuffle, true) => "shuffle_on",
        (SessionToggle::Shuffle, false) => "shuffle_off",
        (SessionToggle::Fullscreen, true) => "fullscreen_on",
        (SessionToggle::Fullscreen, false) => "fullscreen_off",
    }
}

impl Shell {
    /// Windows `process_pending_activations`.
    pub fn process_activations(&mut self, ui: &mut dyn ShellUi) {
        while let Some(request) = self.application.take_activation() {
            match request {
                ActivationRequest::Show => ui.restore(),
                ActivationRequest::OpenSettings => {
                    ui.restore();
                    self.activate_action(ui, "open_settings");
                }
                ActivationRequest::OpenFile(path) => {
                    ui.restore();
                    self.play_local_file(ui, &path);
                }
            }
        }
    }

    /// Windows `open_media_file`: Play file in the main menu.
    pub fn open_media_file(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("play_file");
        let title = self.text("play_file");
        match ui.choose_media_file(&title) {
            Ok(Some(path)) => {
                self.application
                    .enqueue_activation(ActivationRequest::OpenFile(path));
                self.process_activations(ui);
            }
            Ok(None) => {}
            Err(error) => ui.show_error(&error),
        }
        ui.focus_primary_control();
    }

    /// Windows `play_local_file`.
    pub fn play_local_file(&mut self, ui: &mut dyn ShellUi, path: &Path) {
        if !path.is_file() {
            ui.show_error("The selected media file does not exist");
            return;
        }
        self.start_media_item(ui, local_file_item(path), None);
    }

    /// Windows `start_media_item_with_options` for items that play without
    /// resolving a stream first (local files and queue items of them).
    pub(crate) fn start_media_item(
        &mut self,
        ui: &mut dyn ShellUi,
        item: MediaItem,
        queue_mode: Option<QueueStartMode>,
    ) {
        let start_position_seconds = self.application.playback_resume_position(&item);
        self.application.prepare_standalone_playback();
        self.player.pending_queued_start = queue_mode.map(|mode| PendingQueuedStart {
            item: item.clone(),
            mode,
        });
        self.start_player_at(ui, item, None, start_position_seconds);
    }

    /// Windows `start_media_item_at`: a standalone item from `position`.
    pub(crate) fn start_media_item_at(
        &mut self,
        ui: &mut dyn ShellUi,
        item: MediaItem,
        start_position_seconds: f64,
    ) {
        self.application.prepare_standalone_playback();
        self.player.pending_queued_start = None;
        self.start_player_at(ui, item, None, Some(start_position_seconds.max(0.0)));
    }

    /// Windows `start_sequence_media_item` and `start_media_item_with_shuffle`:
    /// an item of a list that keeps its playback sequence.
    pub(crate) fn start_sequence_media_item(
        &mut self,
        ui: &mut dyn ShellUi,
        item: MediaItem,
        session_shuffle: Option<bool>,
    ) {
        let start_position_seconds = self.application.playback_resume_position(&item);
        self.player.pending_queued_start = None;
        self.start_player_at(ui, item, session_shuffle, start_position_seconds);
    }

    /// Windows `start_player_at`.
    pub(crate) fn start_player_at(
        &mut self,
        ui: &mut dyn ShellUi,
        item: MediaItem,
        session_shuffle: Option<bool>,
        start_position_seconds: Option<f64>,
    ) {
        if self.player.playback.is_none() {
            match PlaybackRuntime::spawn() {
                Ok(runtime) => self.player.playback = Some(runtime),
                Err(error) => {
                    self.player.pending_queued_start = None;
                    let message = self.player_failed_message(&error.to_string());
                    ui.show_error(&message);
                    return;
                }
            }
        }
        // A player opened from a screen returns there.
        self.application.forget_player_return_frame();
        self.persist_current_playback_position(ui);
        self.player.clip_preview = None;
        let podcast_speed = item
            .metadata
            .get("podcast_speed_preset")
            .and_then(serde_json::Value::as_f64)
            .filter(|speed| (0.25..=4.0).contains(speed));
        let generation = self.application.start_player_item_with_shuffle_at(
            item,
            session_shuffle,
            start_position_seconds,
        );
        if let Some(speed) = podcast_speed {
            self.application.set_player_speed(speed);
        }
        let Some(options) = self.playback_launch_options(ui, start_position_seconds) else {
            let message = self.player_failed_message("mpv was not found");
            let _ = self
                .application
                .apply_playback_event(generation, PlaybackEvent::Failed(message.clone()));
            self.player.pending_queued_start = None;
            ui.show_error(&message);
            return;
        };
        let item = self
            .application
            .player_session()
            .current_item()
            .expect("player item was started")
            .clone();
        let start_result = self
            .player
            .playback
            .as_ref()
            .expect("playback runtime was initialized")
            .start(generation, options, item);
        if let Err(error) = start_result {
            let message = self.player_failed_message(&error.to_string());
            let _ = self
                .application
                .apply_playback_event(generation, PlaybackEvent::Failed(message.clone()));
            self.player.pending_queued_start = None;
            ui.show_error(&message);
            return;
        }
        if self.view != View::Player {
            self.application.navigate_to_player();
            self.view = View::Player;
        }
        self.refresh_player(ui, PlayerFocus::Initial);
        ui.set_playback_polling(true);
    }

    /// Windows `refresh_player`.
    pub(crate) fn refresh_player(&mut self, ui: &mut dyn ShellUi, focus: PlayerFocus) {
        let Some(model) = self.application.player_screen_model() else {
            return;
        };
        ui.set_window_title(&model.window_title);
        ui.show_player(&model, focus);
        self.refresh_player_details(ui, focus);
        self.sync_window_fullscreen(ui);
    }

    pub(crate) const fn has_playback(&self) -> bool {
        self.player.playback.is_some()
    }

    /// A player command without the failure announcement.
    pub(crate) fn execute_player_command_result(
        &self,
        command: PlaybackCommand,
    ) -> Result<(), String> {
        let generation = self.application.player_session().generation();
        self.player
            .playback
            .as_ref()
            .ok_or_else(|| "player is not running".to_owned())?
            .execute(generation, command)
            .map_err(|error| error.to_string())
    }

    /// Windows `apply_global_player_equalizer`: a player that follows the
    /// global equalizer gets the current graph.
    pub(crate) fn apply_global_player_equalizer(&self, ui: &mut dyn ShellUi) {
        let follows_global = self
            .application
            .player_session()
            .audio()
            .is_some_and(|audio| audio.equalizer.is_none());
        if follows_global && self.application.player_session().is_open() && self.has_playback() {
            let graph = self.application.player_equalizer_graph(None);
            let _ = self.execute_player_command(ui, PlaybackCommand::SetEqualizerFilter(graph));
        }
    }

    /// Windows `playback_launch_options`.
    fn playback_launch_options(
        &self,
        ui: &dyn ShellUi,
        start_position_seconds: Option<f64>,
    ) -> Option<MpvLaunchOptions> {
        let (executable, library) = apricot_platform::mpv_locations()?;
        let settings = self.application.settings();
        let session = self.application.player_session();
        let audio = session.audio()?;
        let boosted = session
            .enabled_toggles()
            .contains(&SessionToggle::VolumeBoost)
            || audio.volume > 100.0;
        let mut options = MpvLaunchOptions::new(executable);
        options.library = Some(library);
        options.video_mode = ui.video_mode();
        options.initial_volume = audio.volume;
        options.volume_max = if boosted { 300 } else { 100 };
        options.initial_speed = audio.speed;
        options.initial_pitch = mpv_pitch_property(self.pitch_mode(), audio.pitch);
        options.audio_pitch_correction = self.speed_audio_mode().audio_pitch_correction();
        options.initial_playback_state = if session.phase() == PlaybackPhase::Paused {
            InitialPlaybackState::Paused
        } else {
            InitialPlaybackState::Playing
        };
        options.initial_position_seconds = start_position_seconds;
        options.repeat_mode = if session.enabled_toggles().contains(&SessionToggle::Repeat) {
            RepeatMode::One
        } else {
            RepeatMode::Off
        };
        options.gapless = settings.gapless_playback;
        options.replay_gain.clone_from(&settings.replaygain_mode);
        options.audio_device = nonempty(&audio.output_device);
        options.cache = settings.enable_stream_cache.then(|| MpvCacheConfig {
            megabytes: u32::try_from(settings.cache_size_mb.clamp(128, 4_096)).unwrap_or(512),
        });
        options.initial_audio_filter = self.player_audio_filter(None);
        // Python `start_mpv` writes the mpv output to `mpv.log` in the data folder.
        options.log_file = self
            .application
            .settings_file()
            .parent()
            .map(|folder| folder.join("mpv.log"));
        Some(options)
    }

    fn speed_audio_mode(&self) -> SpeedAudioMode {
        SpeedAudioMode::from_setting(&self.application.settings().speed_audio_mode)
    }

    fn pitch_mode(&self) -> PitchMode {
        PitchMode::from_setting(&self.application.settings().pitch_mode)
    }

    /// Windows `player_audio_filter`.
    fn player_audio_filter(&self, pitch_override: Option<f64>) -> Option<String> {
        let pitch = pitch_override.unwrap_or_else(|| {
            self.application
                .player_session()
                .audio()
                .map_or(1.0, |audio| audio.pitch)
        });
        let equalizer = self
            .application
            .player_equalizer_graph(None)
            .map(|graph| tagged_equalizer_filter(apricot_playback::EQUALIZER_FILTER_LABEL, &graph));
        audio_filter_chain(
            self.speed_audio_mode(),
            equalizer.as_deref(),
            self.pitch_mode(),
            pitch,
        )
    }

    /// Windows `player_failed_message`.
    fn player_failed_message(&self, error: &str) -> String {
        if let Some(start) = error.find("spotify_") {
            let key: String = error[start..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            let text = self.text(&key);
            if text != key {
                return text;
            }
        }
        self.text("player_failed").replace("{error}", error)
    }

    /// Windows `playing_message`.
    fn playing_message(&self, item: &MediaItem) -> String {
        let smart = item
            .metadata
            .get("spotify_smart_shuffle")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let title = if item.source == MediaSource::Spotify && !item.channel.is_empty() {
            let key = if smart {
                "spotify_playing_by_smart"
            } else {
                "spotify_playing_by"
            };
            self.text(key)
                .replace("{title}", &item.title)
                .replace("{artist}", &item.channel)
        } else {
            item.title.clone()
        };
        self.text("playing").replace("{title}", &title)
    }

    /// Windows `announce_player_text`.
    fn announce_player_text(&self, ui: &mut dyn ShellUi, key: &str, replacements: &[(&str, &str)]) {
        let mut message = self.text(key);
        for (name, value) in replacements {
            message = message.replace(&format!("{{{name}}}"), value);
        }
        ui.set_status(&message, true);
    }

    /// Windows `poll_playback_runtime`; returns `false` when polling can stop.
    pub fn poll_playback(&mut self, ui: &mut dyn ShellUi) -> bool {
        loop {
            let Some(runtime) = self.player.playback.as_ref() else {
                return false;
            };
            let update = match runtime.poll_update() {
                Ok(Some(update)) => update,
                Ok(None) => return true,
                Err(error) => {
                    self.player.playback = None;
                    self.player.pending_queued_start = None;
                    ui.show_error(&format!("Player stopped: {error}"));
                    return false;
                }
            };
            // mpv reports its pause state while the item loads; Python
            // announces play and pause only when they change afterwards.
            let loading = self.application.player_session().phase() == PlaybackPhase::Starting;
            if !self
                .application
                .apply_playback_event(update.generation, update.event.clone())
            {
                continue;
            }
            match update.event {
                PlaybackEvent::Started => self.player_started(ui),
                PlaybackEvent::Paused(paused) => {
                    let key = if paused {
                        "playback_paused"
                    } else {
                        "playback_playing"
                    };
                    let focused_play_pause = ui.focused_player_control() == Some("play_pause");
                    // After a restart the status keeps "Playback restarted".
                    let restarted = !paused && std::mem::take(&mut self.player.quiet_next_resume);
                    if !restarted {
                        ui.set_status(
                            &self.text(key),
                            self.application.settings().announce_play_pause
                                && !focused_play_pause
                                && !loading,
                        );
                    }
                    if self.view == View::Player {
                        self.refresh_player(ui, PlayerFocus::Preserve);
                    }
                }
                PlaybackEvent::Position { .. }
                | PlaybackEvent::MediaInfo(_)
                | PlaybackEvent::AudioDevices(_) => {}
                PlaybackEvent::PreviewFinished => {
                    if self.player.clip_preview == Some(update.generation) {
                        self.player.clip_preview = None;
                        ui.set_status(&self.text("clip_preview_finished"), true);
                    }
                    if self.view == View::Player {
                        self.refresh_player(ui, PlayerFocus::Preserve);
                    }
                }
                PlaybackEvent::Ended => {
                    if self.player.clip_preview.take().is_some() {
                        let _ = self.execute_player_command(ui, PlaybackCommand::SetPaused(true));
                        ui.set_status(&self.text("clip_preview_finished"), true);
                        continue;
                    }
                    let autoplay_next = self
                        .application
                        .player_session()
                        .enabled_toggles()
                        .contains(&SessionToggle::AutoplayNext);
                    if autoplay_next {
                        self.navigate_player_relative(ui, 1, true);
                        return self.player.playback.is_some();
                    }
                    ui.set_status(
                        &self.text("playback_finished"),
                        self.application.settings().announce_playback_finished,
                    );
                    if self.view == View::Player {
                        self.refresh_player(ui, PlayerFocus::Preserve);
                    }
                }
                PlaybackEvent::Failed(error) => {
                    self.player.clip_preview = None;
                    self.player.pending_queued_start = None;
                    let message = self.player_failed_message(&error);
                    ui.set_status(&message, true);
                    ui.show_error(&message);
                }
                PlaybackEvent::CommandFailed(_) => {
                    ui.set_status(&self.text("timing_unavailable"), true);
                }
            }
        }
    }

    fn player_started(&mut self, ui: &mut dyn ShellUi) {
        if let Some(pending) = self.player.pending_queued_start.take() {
            let result = match pending.mode {
                QueueStartMode::Front => {
                    self.application.confirm_queued_item_started(&pending.item)
                }
            };
            if let Err(error) = result {
                let message = format!("Playback queue was not updated: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
        let Some(item) = self.application.player_session().current_item().cloned() else {
            return;
        };
        let collection = item
            .metadata
            .get("spotify_collection")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !collection {
            ui.set_status(&self.playing_message(&item), true);
        }
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |duration| duration.as_secs_f64());
        if let Err(error) = self.application.record_history(item, "played", timestamp) {
            ui.set_status(&format!("History was not saved: {error}"), true);
        }
    }

    /// Windows `execute_player_command`.
    pub(crate) fn execute_player_command(
        &self,
        ui: &mut dyn ShellUi,
        command: PlaybackCommand,
    ) -> bool {
        let generation = self.application.player_session().generation();
        let Some(runtime) = self.player.playback.as_ref() else {
            return false;
        };
        if runtime.execute(generation, command).is_ok() {
            return true;
        }
        // Python announces a failed player command and keeps playing.
        self.announce_player_text(ui, "timing_unavailable", &[]);
        false
    }

    /// Windows `toggle_player_pause`.
    pub(crate) fn toggle_player_pause(&mut self, ui: &mut dyn ShellUi) {
        self.player.clip_preview = None;
        if !self.application.player_session().is_open() {
            return;
        }
        match self.application.player_session().play_action() {
            PlayAction::Pause => {
                let _ = self.execute_player_command(ui, PlaybackCommand::SetPaused(true));
            }
            PlayAction::Resume => {
                let _ = self.execute_player_command(ui, PlaybackCommand::SetPaused(false));
            }
            PlayAction::RestartFromStart => {
                // Python `restart_current_playback`.
                if self.execute_player_command(
                    ui,
                    PlaybackCommand::SeekAbsolute {
                        seconds: 0.0,
                        exact: true,
                    },
                ) && self.execute_player_command(ui, PlaybackCommand::SetPaused(false))
                {
                    self.player.quiet_next_resume = true;
                    self.announce_player_text(ui, "playback_restarted", &[]);
                }
            }
        }
    }

    /// Windows `background_play_pause`.
    pub(crate) fn background_play_pause(&mut self, ui: &mut dyn ShellUi) {
        if self.application.player_session().is_open() {
            self.toggle_player_pause(ui);
        } else {
            ui.set_status(&self.text("no_player"), true);
        }
    }

    /// Windows `seek_player`.
    pub(crate) fn seek_player(&mut self, ui: &mut dyn ShellUi, seconds: f64) {
        self.player.clip_preview = None;
        let _ = self.execute_player_command(
            ui,
            PlaybackCommand::SeekRelative {
                seconds,
                exact: false,
            },
        );
    }

    /// Windows `seek_player_absolute`.
    fn seek_player_absolute(&mut self, ui: &mut dyn ShellUi, seconds: f64, key: &str) {
        self.player.clip_preview = None;
        if self.execute_player_command(
            ui,
            PlaybackCommand::SeekAbsolute {
                seconds: seconds.max(0.0),
                exact: true,
            },
        ) {
            self.announce_player_text(ui, key, &[]);
        }
    }

    pub(crate) fn seek_player_to_start(&mut self, ui: &mut dyn ShellUi) {
        self.seek_player_absolute(ui, 0.0, "seeked_to_start");
    }

    pub(crate) fn seek_player_to_end(&mut self, ui: &mut dyn ShellUi) {
        let Some(duration) = self.application.player_session().duration_seconds() else {
            self.announce_player_text(ui, "timing_unavailable", &[]);
            return;
        };
        self.seek_player_absolute(ui, (duration - 0.5).max(0.0), "seeked_to_end");
    }

    /// Windows `announce_player_time`.
    pub(crate) fn announce_player_time(&mut self, ui: &mut dyn ShellUi) {
        let session = self.application.player_session();
        let elapsed = session.position_seconds();
        let Some(duration) = session.duration_seconds() else {
            self.announce_player_text(ui, "timing_unavailable", &[]);
            return;
        };
        self.announce_player_text(
            ui,
            "time_announcement",
            &[
                ("elapsed", &format_duration(elapsed)),
                ("remaining", &format_duration((duration - elapsed).max(0.0))),
                ("total", &format_duration(duration)),
            ],
        );
    }

    /// Windows `announce_player_volume`.
    pub(crate) fn announce_player_volume(&mut self, ui: &mut dyn ShellUi) {
        let Some(audio) = self.application.player_session().audio() else {
            self.announce_player_text(ui, "timing_unavailable", &[]);
            return;
        };
        let volume = format!("{:.0}", audio.volume.round());
        self.announce_player_text(ui, "volume_announcement", &[("volume", &volume)]);
    }

    /// Windows `announce_player_format`.
    pub(crate) fn announce_player_format(&mut self, ui: &mut dyn ShellUi) {
        let message = self
            .application
            .player_format_status()
            .unwrap_or_else(|| self.text("no_player"));
        ui.set_status(&message, true);
    }

    /// Windows `adjust_player_volume`: no announcement, as Python's
    /// `change_volume_async`.
    pub(crate) fn adjust_player_volume(&mut self, ui: &mut dyn ShellUi, delta: f64) {
        let Some(audio) = self.application.player_session().audio() else {
            return;
        };
        let maximum = if self
            .application
            .player_session()
            .enabled_toggles()
            .contains(&SessionToggle::VolumeBoost)
        {
            300.0
        } else {
            100.0
        };
        let volume = (audio.volume + delta).clamp(0.0, maximum);
        if self.execute_player_command(ui, PlaybackCommand::SetVolume(volume)) {
            self.application.set_player_volume(volume);
        }
    }

    /// Windows `apply_player_speed`.
    fn apply_player_speed(&mut self, ui: &mut dyn ShellUi, speed: f64) -> bool {
        let correction = self.speed_audio_mode().audio_pitch_correction();
        if !self.execute_player_command(ui, PlaybackCommand::SetAudioPitchCorrection(correction))
            || !self.execute_player_command(ui, PlaybackCommand::SetSpeed(speed))
        {
            return false;
        }
        self.application.set_player_speed(speed);
        self.sync_player_details(ui);
        true
    }

    /// Windows `apply_player_pitch`, including linked speed changes.
    fn apply_player_pitch(
        &mut self,
        ui: &mut dyn ShellUi,
        pitch: f64,
        speed_delta: Option<f64>,
    ) -> bool {
        let mode = self.pitch_mode();
        let pitch_filter = pitch_filter_active(mode, pitch).then_some(pitch);
        if !self.execute_player_command(ui, PlaybackCommand::SetAudioPitchCorrection(true))
            || !self.execute_player_command(
                ui,
                PlaybackCommand::SetPitch(mpv_pitch_property(mode, pitch)),
            )
            || !self.execute_player_command(ui, PlaybackCommand::SetPitchFilter(pitch_filter))
        {
            return false;
        }
        self.application.set_player_pitch(pitch);
        self.sync_player_details(ui);
        if mode == PitchMode::LinkedSpeed
            && let Some(delta) = speed_delta
            && let Some(current) = self
                .application
                .player_session()
                .audio()
                .map(|audio| audio.speed)
        {
            let speed = clamp_rate(current + delta, 0.25, 4.0);
            if self.execute_player_command(ui, PlaybackCommand::SetSpeed(speed)) {
                self.application.set_player_speed(speed);
                self.sync_player_details(ui);
            }
        }
        true
    }

    /// Windows `adjust_player_speed`.
    pub(crate) fn adjust_player_speed(&mut self, ui: &mut dyn ShellUi, delta: f64) {
        let Some(audio) = self.application.player_session().audio() else {
            return;
        };
        let speed = clamp_rate(audio.speed + delta, 0.25, 4.0);
        if !self.apply_player_speed(ui, speed) {
            return;
        }
        self.announce_player_text(
            ui,
            "speed_announcement",
            &[("speed", &format_rate_for_speech(speed))],
        );
        if is_default_rate(speed) {
            ui.play_default_reached_sound();
        }
    }

    /// Windows `adjust_player_pitch`.
    pub(crate) fn adjust_player_pitch(&mut self, ui: &mut dyn ShellUi, delta: f64) {
        let Some(audio) = self.application.player_session().audio() else {
            return;
        };
        let pitch = clamp_rate(audio.pitch + delta, 0.5, 2.0);
        if !self.apply_player_pitch(ui, pitch, Some(pitch - audio.pitch)) {
            return;
        }
        self.announce_player_text(
            ui,
            "pitch_announcement",
            &[("pitch", &format_rate_for_speech(pitch))],
        );
        if is_default_rate(pitch) {
            ui.play_default_reached_sound();
        }
    }

    /// Windows `reset_player_speed_pitch`.
    pub(crate) fn reset_player_speed_pitch(&mut self, ui: &mut dyn ShellUi) {
        let speed = apricot_app::player_start_speed(&self.application.settings().player_speed);
        if !self.apply_player_speed(ui, speed) || !self.apply_player_pitch(ui, 1.0, None) {
            return;
        }
        self.announce_player_text(
            ui,
            "speed_pitch_reset",
            &[
                ("speed", &format_rate_for_speech(speed)),
                ("pitch", &format_rate_for_speech(1.0)),
            ],
        );
        ui.play_default_reached_sound();
    }

    /// Windows `toggle_player_session_setting` for the toggles of this unit.
    pub(crate) fn toggle_player_session_setting(
        &mut self,
        ui: &mut dyn ShellUi,
        toggle: SessionToggle,
    ) {
        let enabled = !self
            .application
            .player_session()
            .enabled_toggles()
            .contains(&toggle);
        let command_succeeded = match toggle {
            SessionToggle::Repeat => {
                self.execute_player_command(ui, PlaybackCommand::SetRepeat(enabled))
            }
            SessionToggle::VolumeBoost => self.execute_player_command(
                ui,
                PlaybackCommand::SetVolumeMax(if enabled { 300 } else { 100 }),
            ),
            SessionToggle::BassBoost => {
                let graph = self.application.player_equalizer_graph(Some(enabled));
                self.application.player_session().is_open()
                    && self.player.playback.is_some()
                    && self.execute_player_command(ui, PlaybackCommand::SetEqualizerFilter(graph))
            }
            SessionToggle::AutoplayNext | SessionToggle::Fullscreen | SessionToggle::Shuffle => {
                true
            }
        };
        if !command_succeeded {
            return;
        }
        let clamped_volume = if toggle == SessionToggle::VolumeBoost && !enabled {
            let volume = self
                .application
                .player_session()
                .audio()
                .map_or(100.0, |audio| audio.volume.min(100.0));
            if !self.execute_player_command(ui, PlaybackCommand::SetVolume(volume)) {
                return;
            }
            Some(volume)
        } else {
            None
        };
        self.application.set_player_toggle(toggle, enabled);
        if let Some(volume) = clamped_volume {
            self.application.set_player_volume(volume);
        }
        ui.set_status(&self.text(session_toggle_key(toggle, enabled)), true);
        if self.view == View::Player {
            self.refresh_player(ui, PlayerFocus::Preserve);
        }
    }

    /// Windows `navigate_player_relative_from`.
    pub(crate) fn navigate_player_relative(
        &mut self,
        ui: &mut dyn ShellUi,
        delta: i32,
        after_end: bool,
    ) {
        match self.application.request_relative_player_item(delta) {
            PlayerNavigationOutcome::Item { item, origin } => {
                let queue_mode =
                    (origin == PlayerNavigationOrigin::Queue).then_some(QueueStartMode::Front);
                if origin == PlayerNavigationOrigin::Sequence {
                    self.start_sequence_media_item(ui, *item, None);
                } else {
                    self.start_media_item(ui, *item, queue_mode);
                }
            }
            // Loading more search or channel results arrives with those
            // screens; a local item never needs it.
            PlayerNavigationOutcome::LoadingMore(_)
            | PlayerNavigationOutcome::LoadingMoreCollection(_)
            | PlayerNavigationOutcome::Unavailable => {
                if after_end {
                    ui.set_status(
                        &self.text("playback_finished"),
                        self.application.settings().announce_playback_finished,
                    );
                    return;
                }
                let key = if delta < 0 {
                    "no_previous_item"
                } else {
                    "no_next_item"
                };
                ui.set_status(&self.text(key), true);
            }
        }
    }

    /// Windows `persist_current_playback_position`.
    fn persist_current_playback_position(&mut self, ui: &mut dyn ShellUi) {
        if let Err(error) = self.application.save_current_playback_position() {
            ui.set_status(&format!("Playback position was not saved: {error}"), false);
        }
    }

    /// Windows `close_player_runtime`.
    pub(crate) fn close_player_runtime(&mut self, ui: &mut dyn ShellUi) {
        self.player.clip_preview = None;
        self.stop_key_repeat(ui);
        let generation = self.application.player_session().generation();
        if let Some(runtime) = self.player.playback.as_ref() {
            let _ = runtime.close(generation);
        }
        self.persist_current_playback_position(ui);
        self.application.close_player_session();
        self.player.pending_queued_start = None;
        if self.extras.details_visible {
            self.extras.details_visible = false;
            ui.hide_player_details();
        }
        ui.set_playback_polling(false);
        ui.set_window_title(apricot_platform::product_name());
    }

    /// Windows `player_back_shortcut` without details, full screen and
    /// background playback, which arrive in later units: from the player
    /// and its buttons Escape stops playback and returns to the previous
    /// screen, from the Back buttons to the main menu.
    pub(crate) fn player_back(&mut self, ui: &mut dyn ShellUi) {
        if self.handle_player_back_in_place(ui) {
            return;
        }
        if self.view != View::Player {
            self.navigate_back(ui);
            return;
        }
        let focus = ui.focused_player_control();
        let background = self.application.settings().enable_background_playback;
        let navigation = focus.is_some_and(|id| id.starts_with("back"));
        let closes_playback = !navigation || !background;
        if closes_playback {
            self.navigate_back(ui);
        } else {
            self.leave_player_to_main_menu(ui, true);
        }
    }

    /// Windows `leave_player_to_main_menu`.
    pub(crate) fn leave_player_to_main_menu(
        &mut self,
        ui: &mut dyn ShellUi,
        force_keep_playing: bool,
    ) {
        let keep_playing =
            force_keep_playing && self.application.settings().enable_background_playback;
        self.application
            .set_player_toggle(SessionToggle::Fullscreen, false);
        if keep_playing {
            self.application.keep_player_return_frame();
        } else {
            self.close_player_runtime(ui);
        }
        self.show_main_menu(ui);
    }

    /// Windows `close_current_player`.
    pub(crate) fn close_current_player(&mut self, ui: &mut dyn ShellUi) {
        let show_menu = matches!(self.view, View::Player | View::MainMenu);
        self.close_player_runtime(ui);
        let message = self.text("player_closed");
        ui.set_status(&message, true);
        if show_menu {
            self.show_main_menu(ui);
            ui.set_status(&message, false);
        }
    }

    /// The configured seek step (Windows `configured_seek_seconds`).
    pub(crate) fn configured_seek_seconds(&self) -> f64 {
        self.application.settings().seek_seconds.clamp(0.1, 600.0)
    }

    pub(crate) fn configured_volume_step(&self) -> f64 {
        f64::from(i32::try_from(self.application.settings().volume_step.clamp(1, 100)).unwrap_or(5))
    }

    pub(crate) fn configured_speed_step(&self) -> f64 {
        self.application.settings().speed_step.clamp(0.01, 1.0)
    }

    pub(crate) fn configured_pitch_step(&self) -> f64 {
        self.application.settings().pitch_step.clamp(0.01, 1.0)
    }

    /// Closes the player when the application quits, saving its position.
    pub fn shutdown(&mut self, ui: &mut dyn ShellUi) {
        if self.application.player_session().is_open() {
            self.close_player_runtime(ui);
        }
    }
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
