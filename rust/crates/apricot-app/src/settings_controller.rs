//! Settings load, draft, and save coordination outside the native UI layer.

use std::path::PathBuf;

use apricot_core::{SettingId, SettingsSection};
use apricot_storage::{
    SettingsDocument, SettingsLoadOutcome, SettingsPaths, SettingsSaveError, SettingsSource,
    load_settings, save_loaded_settings,
};
use serde_json::Value;
use thiserror::Error;

use crate::{SettingsDraft, SettingsDraftError};

#[derive(Debug, Error)]
pub enum SettingsControllerError {
    #[error(transparent)]
    Draft(#[from] SettingsDraftError),
    #[error(transparent)]
    Save(#[from] SettingsSaveError),
    #[error("shortcut {shortcut} is already assigned to {action}")]
    ShortcutConflict { shortcut: String, action: String },
    #[error("unknown shortcut action {0}")]
    UnknownShortcutAction(String),
    #[error("{0}")]
    Spotify(String),
}

#[derive(Debug)]
pub struct SettingsController {
    paths: SettingsPaths,
    loaded: SettingsLoadOutcome,
    draft: SettingsDraft,
}

impl SettingsController {
    pub fn load(paths: SettingsPaths, defaults: SettingsDocument) -> Self {
        let loaded = load_settings(&paths, defaults.clone());
        let draft = SettingsDraft::new(loaded.settings.clone(), defaults);
        Self {
            paths,
            loaded,
            draft,
        }
    }

    pub const fn current(&self) -> &SettingsDocument {
        self.draft.current()
    }

    pub fn is_dirty(&self) -> bool {
        self.draft.is_dirty()
    }

    pub fn source(&self) -> &SettingsSource {
        &self.loaded.source
    }

    pub fn settings_file(&self) -> PathBuf {
        self.paths.primary.clone()
    }

    pub fn load_errors(&self) -> &[String] {
        &self.loaded.errors
    }

    pub const fn save_is_blocked(&self) -> bool {
        self.loaded.save_blocked
    }

    /// Updates one typed setting in the current unsaved draft.
    ///
    /// # Errors
    ///
    /// Returns an error when the value does not match the complete settings schema.
    pub fn set_value(
        &mut self,
        id: SettingId,
        value: Value,
    ) -> Result<(), SettingsControllerError> {
        self.draft.set_value(id, value)?;
        Ok(())
    }

    /// Updates several typed settings as one validated draft transition.
    ///
    /// # Errors
    ///
    /// Returns an error without changing the draft when any value is invalid.
    pub fn set_values(
        &mut self,
        values: impl IntoIterator<Item = (SettingId, Value)>,
    ) -> Result<(), SettingsControllerError> {
        self.draft.set_values(values)?;
        Ok(())
    }

    /// Resets the selected section in the unsaved draft.
    ///
    /// # Errors
    ///
    /// Returns an error when the canonical section ownership cannot be applied.
    pub fn reset_section(
        &mut self,
        section: SettingsSection,
    ) -> Result<(), SettingsControllerError> {
        self.draft.reset_section(section)?;
        Ok(())
    }

    pub fn reset_all(&mut self) {
        self.draft.reset_all();
    }

    pub fn cancel(&mut self) {
        self.draft.discard();
    }

    /// Assigns one non-empty shortcut while rejecting conflicts in the
    /// canonical form (Python `canonical_shortcut`), so `Control+F8` and
    /// `ctrl-f8` are the same key.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown action or a shortcut already in use.
    pub fn set_shortcut(
        &mut self,
        action_id: &str,
        shortcut: &str,
    ) -> Result<(), SettingsControllerError> {
        let action = apricot_core::action::action_by_id(action_id)
            .ok_or_else(|| SettingsControllerError::UnknownShortcutAction(action_id.to_owned()))?;
        let shortcut = shortcut.trim();
        let shortcut = if shortcut.is_empty() {
            action.default_windows_shortcut
        } else {
            shortcut
        };
        let wanted = apricot_core::shortcut::ShortcutChord::parse(shortcut);
        if let Some((conflicting_action, _)) =
            self.draft
                .current()
                .keyboard_shortcuts
                .iter()
                .find(|(other_id, other)| {
                    other_id.as_str() != action_id
                        && wanted.is_some()
                        && apricot_core::shortcut::ShortcutChord::parse(other) == wanted
                })
        {
            return Err(SettingsControllerError::ShortcutConflict {
                shortcut: shortcut.to_owned(),
                action: conflicting_action.clone(),
            });
        }
        let mut shortcuts = self.draft.current().keyboard_shortcuts.clone();
        shortcuts.insert(action_id.to_owned(), shortcut.to_owned());
        self.draft
            .set_value(SettingId::KeyboardShortcuts, serde_json::json!(shortcuts))?;
        Ok(())
    }

    /// Atomically saves the current draft, then marks it committed in memory.
    ///
    /// # Errors
    ///
    /// Returns an error without committing the draft when storage rejects the save.
    pub fn save(&mut self) -> Result<&SettingsDocument, SettingsControllerError> {
        let candidate = self.draft.current().clone();
        save_loaded_settings(&self.paths, &mut self.loaded, &candidate)?;
        let _ = self.draft.commit();
        Ok(self.draft.current())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use apricot_core::{SettingId, SettingsSection};
    use apricot_storage::{SettingsDocument, SettingsPaths, SettingsSource};
    use serde_json::json;
    use tempfile::tempdir;

    use super::SettingsController;

    #[test]
    fn shortcut_conflicts_use_the_canonical_chord() {
        let root = tempdir().expect("temporary directory");
        let paths =
            SettingsPaths::for_app_data(&root.path().join("beta"), &root.path().join("old"));
        let mut controller = SettingsController::load(paths, SettingsDocument::default());
        controller
            .set_shortcut("open_search", "Control+F8")
            .expect("free shortcut");
        let error = controller
            .set_shortcut("open_settings", "ctrl-F8")
            .expect_err("same key in another spelling");
        assert!(matches!(
            error,
            super::SettingsControllerError::ShortcutConflict { ref action, .. }
                if action == "open_search"
        ));
        controller
            .set_shortcut("open_search", "Ctrl+Shift+F8")
            .expect("own action may change");
    }

    #[test]
    fn cancel_discards_the_complete_unsaved_draft() {
        let root = tempdir().expect("temporary directory");
        let paths =
            SettingsPaths::for_app_data(&root.path().join("beta"), &root.path().join("old"));
        let mut controller = SettingsController::load(paths, SettingsDocument::default());
        controller
            .set_value(SettingId::Language, json!("sl"))
            .expect("draft update");
        controller
            .set_value(SettingId::AutoplayNext, json!(true))
            .expect("draft update");
        assert!(controller.is_dirty());
        controller.cancel();
        assert_eq!(controller.current().language, "en");
        assert!(!controller.current().autoplay_next);
        assert!(!controller.is_dirty());
    }

    #[test]
    fn leaving_settings_keeps_applied_edits_until_the_next_save() {
        // Python's Back from Settings neither saves nor reverts: sections the
        // user left stay applied in memory and the next save writes them.
        let root = tempdir().expect("temporary directory");
        let paths =
            SettingsPaths::for_app_data(&root.path().join("beta"), &root.path().join("old"));
        let primary = paths.primary.clone();
        let mut controller = SettingsController::load(paths, SettingsDocument::default());
        controller
            .set_value(SettingId::AutoplayNext, json!(true))
            .expect("draft update");
        assert!(controller.current().autoplay_next);
        assert!(!primary.exists());

        controller.save().expect("save");
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(primary).expect("settings bytes")).expect("settings");
        assert_eq!(saved["autoplay_next"], true);
    }

    #[test]
    fn save_commits_only_after_atomic_storage_succeeds() {
        let root = tempdir().expect("temporary directory");
        let paths =
            SettingsPaths::for_app_data(&root.path().join("beta"), &root.path().join("old"));
        let primary = paths.primary.clone();
        let mut controller = SettingsController::load(paths, SettingsDocument::default());
        controller
            .set_value(SettingId::EnableHistory, json!(false))
            .expect("draft update");
        controller.save().expect("save");
        assert!(!controller.is_dirty());
        assert_eq!(controller.source(), &SettingsSource::Primary);
        assert!(primary.exists());

        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(primary).expect("settings bytes")).expect("settings");
        assert_eq!(saved["enable_history"], false);
    }

    #[test]
    fn section_and_full_reset_remain_draft_operations() {
        let root = tempdir().expect("temporary directory");
        let paths =
            SettingsPaths::for_app_data(&root.path().join("beta"), &root.path().join("old"));
        let current = SettingsDocument {
            language: "sl".to_owned(),
            ..SettingsDocument::default()
        };
        fs::create_dir_all(paths.primary.parent().expect("parent")).expect("directory");
        fs::write(
            &paths.primary,
            serde_json::to_vec(&current).expect("settings JSON"),
        )
        .expect("settings");
        let mut controller = SettingsController::load(paths, SettingsDocument::default());
        controller
            .reset_section(SettingsSection::General)
            .expect("section reset");
        assert_eq!(controller.current().language, "en");
        assert!(controller.is_dirty());
        controller.cancel();
        assert_eq!(controller.current().language, "sl");
        controller.reset_all();
        assert_eq!(controller.current(), &SettingsDocument::default());
    }
}
