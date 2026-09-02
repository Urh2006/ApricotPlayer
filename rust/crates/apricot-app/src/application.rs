//! Top-level application coordinator consumed by platform UI adapters.

use std::collections::VecDeque;

use apricot_core::{SettingId, SettingsSection};
use apricot_storage::SettingsDocument;

use crate::{
    ActivationRequest, MainMenuAvailability, MainMenuModel, MenuVisibility, SettingsController,
    SettingsControllerError, SettingsScreenModel, embedded_catalog,
};

#[derive(Debug)]
pub struct Application {
    settings: SettingsController,
    menu_availability: MainMenuAvailability,
    activation_requests: VecDeque<ActivationRequest>,
}

impl Application {
    pub const fn new(
        settings: SettingsController,
        menu_availability: MainMenuAvailability,
    ) -> Self {
        Self {
            settings,
            menu_availability,
            activation_requests: VecDeque::new(),
        }
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
    use std::path::{Path, PathBuf};

    use apricot_core::{SettingId, SettingsSection};
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
}
