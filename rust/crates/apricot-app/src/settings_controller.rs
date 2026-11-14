//! Settings load, draft, and save coordination outside the native UI layer.

use std::path::{Path, PathBuf};

use apricot_core::{SettingId, SettingsSection};
use apricot_storage::{
    JsonFileError, SettingsDocument, SettingsLoadError, SettingsLoadOutcome, SettingsPaths,
    SettingsSaveError, SettingsSource, load_settings, save_loaded_settings, write_bytes_atomic,
    write_json_atomic,
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
    #[error("invalid shortcut {0}")]
    InvalidShortcut(String),
    #[error("{0}")]
    Spotify(String),
    #[error(transparent)]
    File(#[from] JsonFileError),
    #[error("the settings backup is larger than 4 MiB")]
    ImportTooLarge,
    #[error("the file is not an ApricotPlayer settings backup: {0}")]
    ImportInvalid(#[from] SettingsLoadError),
}

/// The largest settings backup an import reads.
const IMPORT_LIMIT: u64 = 4 * 1024 * 1024;

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

    pub fn set_extension(&mut self, key: &str, value: serde_json::Value) {
        self.draft.set_extension(key, value);
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
            action.default_shortcut()
        } else {
            shortcut
        };
        let wanted: Vec<_> = shortcut
            .split('|')
            .filter(|part| !part.trim().is_empty())
            .map(apricot_core::shortcut::ShortcutChord::parse)
            .collect();
        if wanted.iter().any(Option::is_none) {
            return Err(SettingsControllerError::InvalidShortcut(
                shortcut.to_owned(),
            ));
        }
        if let Some((conflicting_action, _)) =
            self.draft
                .current()
                .keyboard_shortcuts
                .iter()
                .find(|(other_id, other)| {
                    other_id.as_str() != action_id
                        && wanted
                            .iter()
                            .flatten()
                            .any(|chord| chord.matches_configured(other))
                        && !apricot_core::action::actions_may_share_a_chord(action_id, other_id)
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

    /// Writes the saved settings to a user-selected backup file in the
    /// same shape as `settings.json`, so either version can read it. Changes
    /// left unsaved in the Settings window are not part of a backup.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be written.
    pub fn export(&self, path: &Path) -> Result<(), SettingsControllerError> {
        write_json_atomic(path, &self.loaded.settings)?;
        Ok(())
    }

    /// Replaces every setting with a user-selected backup and saves at once.
    /// The backup is merged over the platform defaults like `settings.json`
    /// itself, and the replaced file is kept as `settings.json.before-import`.
    /// An explicit import also works when the current file could not be
    /// loaded, so a backup can repair damaged settings.
    ///
    /// # Errors
    ///
    /// Returns an error without changing anything when the backup cannot be
    /// read or is not a settings file, or when saving fails.
    pub fn import(&mut self, path: &Path) -> Result<(), SettingsControllerError> {
        use std::io::Read;
        let read_error = |source| JsonFileError::Read {
            path: path.to_owned(),
            source,
        };
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(read_error)?
            .take(IMPORT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(read_error)?;
        if bytes.len() as u64 > IMPORT_LIMIT {
            return Err(SettingsControllerError::ImportTooLarge);
        }
        let text = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
        let raw: Value = serde_json::from_slice(text).map_err(SettingsLoadError::from)?;
        let imported =
            SettingsDocument::from_value_with_defaults(&raw, self.draft.defaults().clone())?;
        if self.paths.primary.exists() {
            let previous =
                std::fs::read(&self.paths.primary).map_err(|source| JsonFileError::Read {
                    path: self.paths.primary.clone(),
                    source,
                })?;
            write_bytes_atomic(
                &self.paths.primary.with_extension("json.before-import"),
                &previous,
            )?;
        }
        let draft = self.draft.current().clone();
        let blocked = self.loaded.save_blocked;
        self.loaded.save_blocked = false;
        self.draft.replace_all(imported);
        if let Err(error) = self.save() {
            self.loaded.save_blocked = blocked;
            self.draft.replace_all(draft);
            return Err(error);
        }
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

    /// Beta 17: the Spotify last-song key may repeat the next related
    /// video's key, since only one of them applies to the playing item.
    #[test]
    fn spotify_list_edges_may_share_the_related_video_key_only() {
        let root = tempdir().expect("temporary directory");
        let paths =
            SettingsPaths::for_app_data(&root.path().join("beta"), &root.path().join("old"));
        let mut controller = SettingsController::load(paths, SettingsDocument::default());
        controller
            .set_shortcut("spotify_last_track", "Ctrl+Shift+PageDown")
            .expect("shared with the next related video");
        controller
            .set_shortcut("player_next_related", "Ctrl+Shift+PageDown")
            .expect("shared with the last Spotify song");
        assert!(
            controller
                .set_shortcut("spotify_first_track", "Ctrl+PageDown")
                .is_err(),
            "Ctrl+PageDown is Next"
        );
    }

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
    fn shortcut_alternatives_conflict_in_either_slot_and_invalid_input_is_atomic() {
        let root = tempdir().expect("temporary directory");
        let paths =
            SettingsPaths::for_app_data(&root.path().join("beta"), &root.path().join("old"));
        let mut controller = SettingsController::load(paths, SettingsDocument::default());
        controller
            .set_shortcut("open_search", "Ctrl+F8 | Shift+F8")
            .expect("two free chords");
        let before = controller.current().clone();
        assert!(matches!(
            controller.set_shortcut("open_settings", "Shift+F8"),
            Err(super::SettingsControllerError::ShortcutConflict { .. })
        ));
        assert!(matches!(
            controller.set_shortcut("open_settings", "Ctrl+F9 | Control+F8"),
            Err(super::SettingsControllerError::ShortcutConflict { .. })
        ));
        assert!(matches!(
            controller.set_shortcut("open_settings", "not a keyboard key"),
            Err(super::SettingsControllerError::InvalidShortcut(_))
        ));
        assert_eq!(controller.current(), &before);
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
