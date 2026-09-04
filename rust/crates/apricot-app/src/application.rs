//! Top-level application coordinator consumed by platform UI adapters.

use std::collections::{BTreeSet, VecDeque};

use apricot_core::{MediaItem, SettingId, SettingsSection};
use apricot_playback::PlaybackEvent;
use apricot_storage::SettingsDocument;

use crate::{
    ActionFinderContext, ActionFinderModel, ActivationRequest, AppState, AudioSession,
    EqualizerSession, MainMenuAvailability, MainMenuModel, MenuVisibility, PlayerScreenModel,
    PlayerSession, PlayerSessionDefaults, PlayerViewState, SessionToggle, SettingsController,
    SettingsControllerError, SettingsScreenModel, embedded_catalog,
};

#[derive(Debug)]
pub struct Application {
    settings: SettingsController,
    menu_availability: MainMenuAvailability,
    activation_requests: VecDeque<ActivationRequest>,
    state: AppState,
}

impl Application {
    pub fn new(settings: SettingsController, menu_availability: MainMenuAvailability) -> Self {
        Self {
            settings,
            menu_availability,
            activation_requests: VecDeque::new(),
            state: AppState::default(),
        }
    }

    pub const fn player_session(&self) -> &PlayerSession {
        &self.state.player
    }

    pub fn player_screen_model(&self) -> Option<PlayerScreenModel> {
        let item = self.state.player.current_item()?;
        Some(PlayerScreenModel::build(
            &embedded_catalog(&self.settings.current().language),
            self.settings.current(),
            item,
            &PlayerViewState::from(&self.state.player),
        ))
    }

    pub fn start_player_item(&mut self, item: MediaItem) -> u64 {
        let settings = self.settings.current();
        let mut toggles = BTreeSet::new();
        if settings.autoplay_next {
            toggles.insert(SessionToggle::AutoplayNext);
        }
        if settings.volume_boost_by_default {
            toggles.insert(SessionToggle::VolumeBoost);
        }
        if settings.player_fullscreen {
            toggles.insert(SessionToggle::Fullscreen);
        }
        let defaults = PlayerSessionDefaults {
            audio: AudioSession {
                volume: f64::from(i32::try_from(settings.default_volume).unwrap_or(100)),
                output_device: settings.audio_output_device.clone(),
                speed: settings.player_speed.parse().unwrap_or(1.0),
                pitch: 1.0,
                equalizer: EqualizerSession {
                    enabled: settings.global_equalizer_enabled,
                    gains: settings.global_equalizer_gains.clone(),
                },
            },
            enabled_toggles: toggles,
            starts_paused: settings.player_start_paused,
        };
        self.state.player.start_item(item, defaults)
    }

    pub fn apply_playback_event(&mut self, generation: u64, event: PlaybackEvent) -> bool {
        self.state.player.apply_event(generation, event)
    }

    pub fn close_player_session(&mut self) {
        self.state.player.close();
    }

    pub fn enqueue_activation(&mut self, request: ActivationRequest) {
        if request == ActivationRequest::Show
            && self
                .activation_requests
                .back()
                .is_some_and(|queued| *queued == ActivationRequest::Show)
        {
            return;
        }
        self.activation_requests.push_back(request);
    }

    pub fn take_activation(&mut self) -> Option<ActivationRequest> {
        self.activation_requests.pop_front()
    }

    pub fn main_menu_model(&self) -> MainMenuModel {
        let settings = self.settings.current();
        let mut availability = self.menu_availability;
        availability.trending = visibility(settings.enable_trending);
        availability.history = visibility(settings.enable_history);
        availability.podcasts = visibility(settings.enable_podcasts_rss);
        MainMenuModel::build(
            &embedded_catalog(&settings.language),
            availability,
            &settings.main_menu_hidden_actions,
            settings.show_shortcuts_in_labels,
            &settings.keyboard_shortcuts,
        )
    }

    pub fn action_finder_model(&self, context: ActionFinderContext) -> ActionFinderModel {
        let settings = self.settings.current();
        ActionFinderModel::build(
            &embedded_catalog(&settings.language),
            settings,
            self.menu_availability,
            context,
        )
    }

    pub fn settings_model(&self, section: SettingsSection) -> SettingsScreenModel {
        let settings = self.settings.current();
        SettingsScreenModel::build(
            &embedded_catalog(&settings.language),
            settings,
            &self.settings.settings_file(),
            section,
        )
    }

    pub fn settings(&self) -> &SettingsDocument {
        self.settings.current()
    }

    pub fn settings_are_dirty(&self) -> bool {
        self.settings.is_dirty()
    }

    /// Updates a string setting in the current draft.
    ///
    /// # Errors
    ///
    /// Returns an error when the setting does not accept a string.
    pub fn set_string_setting(
        &mut self,
        id: SettingId,
        value: impl Into<String>,
    ) -> Result<(), SettingsControllerError> {
        self.settings
            .set_value(id, serde_json::Value::String(value.into()))
    }

    /// Updates an integer setting in the current draft.
    ///
    /// # Errors
    ///
    /// Returns an error when the setting does not accept an integer.
    pub fn set_integer_setting(
        &mut self,
        id: SettingId,
        value: i64,
    ) -> Result<(), SettingsControllerError> {
        self.settings.set_value(id, value.into())
    }

    /// Updates a floating-point setting in the current draft.
    ///
    /// # Errors
    ///
    /// Returns an error when the setting does not accept a finite number.
    pub fn set_float_setting(
        &mut self,
        id: SettingId,
        value: f64,
    ) -> Result<(), SettingsControllerError> {
        self.settings.set_value(id, serde_json::json!(value))
    }

    /// Updates a boolean setting in the current draft.
    ///
    /// # Errors
    ///
    /// Returns an error when the setting does not accept a boolean.
    pub fn set_boolean_setting(
        &mut self,
        id: SettingId,
        value: bool,
    ) -> Result<(), SettingsControllerError> {
        self.settings.set_value(id, value.into())
    }

    /// Changes one customizable main-menu item without affecting its shortcut.
    ///
    /// # Errors
    ///
    /// Returns an error if the hidden-action setting cannot be updated.
    pub fn set_main_menu_item_visible(
        &mut self,
        action_id: &str,
        visible: bool,
    ) -> Result<(), SettingsControllerError> {
        let mut hidden = self.settings.current().main_menu_hidden_actions.clone();
        hidden.retain(|id| id != action_id);
        if !visible {
            hidden.push(action_id.to_owned());
        }
        self.settings
            .set_value(SettingId::MainMenuHiddenActions, serde_json::json!(hidden))
    }

    /// Updates one equalizer band without changing any other band.
    ///
    /// # Errors
    ///
    /// Returns an error if a typed equalizer map cannot be persisted to the draft.
    pub fn set_equalizer_band_gain(
        &mut self,
        preset_id: &str,
        band_id: &str,
        gain_db: f64,
    ) -> Result<(), SettingsControllerError> {
        if !apricot_core::audio::EQUALIZER_BANDS
            .iter()
            .any(|band| band.id == band_id)
        {
            return Ok(());
        }
        let range = f64::from(
            i32::try_from(self.settings.current().equalizer_db_range).unwrap_or(i32::MAX),
        );
        let gain = ((gain_db.clamp(-range, range) * 10.0).round()) / 10.0;
        let mut current_gains = self.settings.current().global_equalizer_gains.clone();
        current_gains.insert(band_id.to_owned(), gain);
        let is_custom = !apricot_core::audio::FACTORY_EQUALIZER_PRESETS
            .iter()
            .any(|preset| preset.id == preset_id);
        if is_custom {
            let mut presets = self.settings.current().equalizer_preset_gains.clone();
            let gains = presets.entry(preset_id.to_owned()).or_default();
            gains.insert(band_id.to_owned(), gain);
            self.settings.set_values([
                (
                    SettingId::GlobalEqualizerGains,
                    serde_json::json!(current_gains),
                ),
                (SettingId::EqualizerPresetGains, serde_json::json!(presets)),
            ])?;
        } else {
            self.settings.set_value(
                SettingId::GlobalEqualizerGains,
                serde_json::json!(current_gains),
            )?;
        }
        Ok(())
    }

    /// Updates the custom name for one equalizer preset.
    ///
    /// # Errors
    ///
    /// Returns an error if the complete custom-name map cannot be updated.
    pub fn set_equalizer_preset_name(
        &mut self,
        preset_id: &str,
        name: &str,
    ) -> Result<(), SettingsControllerError> {
        let mut names = self.settings.current().equalizer_custom_names.clone();
        let fallback = names
            .get(preset_id)
            .cloned()
            .unwrap_or_else(|| preset_id.to_owned());
        let trimmed = name.trim();
        names.insert(
            preset_id.to_owned(),
            if trimmed.is_empty() {
                fallback
            } else {
                trimmed.chars().take(80).collect()
            },
        );
        self.settings
            .set_value(SettingId::EqualizerCustomNames, serde_json::json!(names))
    }

    /// Sets or clears the equalizer preset associated with one output device.
    ///
    /// # Errors
    ///
    /// Returns an error if the device-preset map cannot be updated.
    pub fn set_equalizer_device_preset(
        &mut self,
        device_id: &str,
        preset_id: &str,
    ) -> Result<(), SettingsControllerError> {
        let mut presets = self.settings.current().equalizer_device_presets.clone();
        if preset_id.trim().is_empty() {
            presets.remove(device_id);
        } else {
            presets.insert(device_id.to_owned(), preset_id.to_owned());
        }
        self.settings.set_value(
            SettingId::EqualizerDevicePresets,
            serde_json::json!(presets),
        )
    }

    /// Assigns a shortcut through the central conflict validator.
    ///
    /// # Errors
    ///
    /// Returns an error for unknown actions or shortcut conflicts.
    pub fn set_keyboard_shortcut(
        &mut self,
        action_id: &str,
        shortcut: &str,
    ) -> Result<(), SettingsControllerError> {
        self.settings.set_shortcut(action_id, shortcut)
    }

    /// Resets one settings section in the current draft.
    ///
    /// # Errors
    ///
    /// Returns an error when canonical reset ownership cannot be applied.
    pub fn reset_settings_section(
        &mut self,
        section: SettingsSection,
    ) -> Result<(), SettingsControllerError> {
        self.settings.reset_section(section)
    }

    pub fn reset_all_settings(&mut self) {
        self.settings.reset_all();
    }

    pub fn cancel_settings(&mut self) {
        self.settings.cancel();
    }

    /// Completes the one-time language prompt and persists both values atomically.
    ///
    /// An absent or unknown selection keeps the currently configured language,
    /// matching the Python application's cancel behavior.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings draft or atomic save fails.
    pub fn complete_initial_language(
        &mut self,
        selected_language: Option<&str>,
    ) -> Result<(), SettingsControllerError> {
        let language = selected_language
            .filter(|selected| {
                apricot_core::locale::LANGUAGES
                    .iter()
                    .any(|language| language.code == *selected)
            })
            .unwrap_or(&self.settings.current().language)
            .to_owned();
        self.settings.set_values([
            (SettingId::Language, serde_json::json!(language)),
            (SettingId::LanguagePrompted, serde_json::json!(true)),
        ])?;
        let _ = self.settings.save()?;
        Ok(())
    }

    /// Saves the complete current settings draft atomically.
    ///
    /// # Errors
    ///
    /// Returns an error without committing the draft if persistence fails.
    pub fn save_settings(&mut self) -> Result<(), SettingsControllerError> {
        let _ = self.settings.save()?;
        Ok(())
    }
}

const fn visibility(enabled: bool) -> MenuVisibility {
    if enabled {
        MenuVisibility::Visible
    } else {
        MenuVisibility::Hidden
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
    };

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource, SettingId, SettingsSection};
    use apricot_playback::PlaybackEvent;
    use apricot_storage::{SettingsDocument, SettingsPaths};
    use tempfile::tempdir;

    use super::Application;
    use crate::{ActivationRequest, MainMenuAvailability, SettingsController};

    fn application(root: &Path) -> Application {
        let paths = SettingsPaths::for_app_data(&root.join("beta"), &root.join("stable"));
        Application::new(
            SettingsController::load(paths, SettingsDocument::default()),
            MainMenuAvailability::default(),
        )
    }

    fn media_item(id: &str) -> MediaItem {
        MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: id.to_owned(),
            url: None,
            local_path: Some(format!(r"C:\Music\{id}.mp3")),
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn menu_projection_reflects_unsaved_draft_without_hiding_shortcuts() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.set_main_menu_item_visible("search", false)
            .expect("hide search");
        assert!(
            app.main_menu_model()
                .items
                .iter()
                .all(|item| item.id != "search")
        );
        assert_eq!(
            app.settings().keyboard_shortcuts["open_search"],
            "Ctrl+Alt+Y"
        );
        app.cancel_settings();
        assert!(
            app.main_menu_model()
                .items
                .iter()
                .any(|item| item.id == "search")
        );
    }

    #[test]
    fn typed_updates_reset_and_save_flow_through_one_owner() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.set_string_setting(SettingId::Language, "sl")
            .expect("language");
        app.set_integer_setting(SettingId::ResultsLimit, 50)
            .expect("limit");
        app.set_float_setting(SettingId::AppUpdateIntervalHours, 12.0)
            .expect("interval");
        app.set_boolean_setting(SettingId::CloseToTray, true)
            .expect("checkbox");
        assert!(app.settings_are_dirty());
        app.save_settings().expect("save");
        assert!(!app.settings_are_dirty());
        assert_eq!(app.settings().language, "sl");

        app.reset_settings_section(SettingsSection::General)
            .expect("reset");
        assert_eq!(app.settings().language, "en");
        assert!(!app.settings().close_to_tray);
    }

    #[test]
    fn initial_language_completion_is_atomic_and_one_time() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.complete_initial_language(Some("sl"))
            .expect("complete language prompt");
        assert_eq!(app.settings().language, "sl");
        assert!(app.settings().language_prompted);
        assert!(!app.settings_are_dirty());

        let paths =
            SettingsPaths::for_app_data(&root.path().join("beta"), &root.path().join("stable"));
        let reloaded = Application::new(
            SettingsController::load(paths, SettingsDocument::default()),
            MainMenuAvailability::default(),
        );
        assert_eq!(reloaded.settings().language, "sl");
        assert!(reloaded.settings().language_prompted);
    }

    #[test]
    fn cancelling_initial_language_keeps_the_configured_language() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.complete_initial_language(None)
            .expect("cancel language prompt");
        assert_eq!(app.settings().language, "en");
        assert!(app.settings().language_prompted);
    }

    #[test]
    fn equalizer_band_updates_are_strictly_independent() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let before = app.settings().global_equalizer_gains.clone();
        app.set_equalizer_band_gain("flat", "31", 7.5)
            .expect("gain");
        assert!((app.settings().global_equalizer_gains["31"] - 7.5).abs() < f64::EPSILON);
        for (band, gain) in before {
            if band != "31" {
                assert!(
                    (app.settings().global_equalizer_gains[&band] - gain).abs() < f64::EPSILON,
                    "{band}"
                );
            }
        }
    }

    #[test]
    fn shortcut_conflicts_do_not_modify_the_draft() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let before = app.settings().keyboard_shortcuts.clone();
        assert!(
            app.set_keyboard_shortcut("open_search", "Ctrl+Alt+M")
                .is_err()
        );
        assert_eq!(app.settings().keyboard_shortcuts, before);
        app.set_keyboard_shortcut("open_search", "Ctrl+F8")
            .expect("unused shortcut");
        assert_eq!(app.settings().keyboard_shortcuts["open_search"], "Ctrl+F8");
    }

    #[test]
    fn activation_queue_preserves_files_and_coalesces_repeated_show_requests() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.enqueue_activation(ActivationRequest::Show);
        app.enqueue_activation(ActivationRequest::Show);
        app.enqueue_activation(ActivationRequest::OpenFile(PathBuf::from("track.mp3")));
        assert_eq!(app.take_activation(), Some(ActivationRequest::Show));
        assert_eq!(
            app.take_activation(),
            Some(ActivationRequest::OpenFile(PathBuf::from("track.mp3")))
        );
        assert_eq!(app.take_activation(), None);
    }

    #[test]
    fn application_owns_session_defaults_and_rejects_stale_playback_events() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.set_integer_setting(SettingId::DefaultVolume, 80)
            .expect("volume");
        app.set_string_setting(SettingId::AudioOutputDevice, "speakers")
            .expect("device");
        let first = app.start_player_item(media_item("first"));
        assert!(
            (app.player_session().audio().expect("audio session").volume - 80.0).abs()
                < f64::EPSILON
        );

        let second = app.start_player_item(media_item("second"));
        assert!(!app.apply_playback_event(first, PlaybackEvent::Ended));
        assert!(app.apply_playback_event(second, PlaybackEvent::Started));
        assert_eq!(
            app.player_session()
                .current_item()
                .map(|item| item.id.0.as_str()),
            Some("second")
        );

        app.close_player_session();
        assert!(app.player_session().audio().is_none());
    }
}
