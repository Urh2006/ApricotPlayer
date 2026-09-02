//! Top-level application coordinator consumed by platform UI adapters.

use apricot_core::{SettingId, SettingsSection};
use apricot_storage::SettingsDocument;

use crate::{
    MainMenuAvailability, MainMenuModel, MenuVisibility, SettingsController,
    SettingsControllerError, SettingsScreenModel, embedded_catalog,
};

#[derive(Debug)]
pub struct Application {
    settings: SettingsController,
    menu_availability: MainMenuAvailability,
}

impl Application {
    pub const fn new(
        settings: SettingsController,
        menu_availability: MainMenuAvailability,
    ) -> Self {
        Self {
            settings,
            menu_availability,
        }
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
    use std::path::Path;

    use apricot_core::{SettingId, SettingsSection};
    use apricot_storage::{SettingsDocument, SettingsPaths};
    use tempfile::tempdir;

    use super::Application;
    use crate::{MainMenuAvailability, SettingsController};

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
}
