//! Transactional Settings editing independent of native controls.

use apricot_core::{SETTINGS_SECTIONS, SettingId, SettingsSection};
use apricot_storage::SettingsDocument;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettingsDraftError {
    #[error("settings could not be represented as an object")]
    NotAnObject,
    #[error("settings section {0:?} is missing from the canonical layout")]
    MissingSection(SettingsSection),
    #[error("setting {0} does not exist in the typed schema")]
    MissingSetting(&'static str),
    #[error("settings value is incompatible with the schema: {0}")]
    Decode(#[from] serde_json::Error),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsDraft {
    original: SettingsDocument,
    current: SettingsDocument,
    defaults: SettingsDocument,
}

impl SettingsDraft {
    pub fn new(current: SettingsDocument, defaults: SettingsDocument) -> Self {
        Self {
            original: current.clone(),
            current,
            defaults,
        }
    }

    pub const fn current(&self) -> &SettingsDocument {
        &self.current
    }

    pub fn is_dirty(&self) -> bool {
        self.current != self.original
    }

    /// Replaces one value and validates the resulting complete settings schema.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsDraftError`] when serialization fails, the setting is
    /// missing, or the replacement has an incompatible type.
    pub fn set_value(&mut self, id: SettingId, value: Value) -> Result<(), SettingsDraftError> {
        let mut object = serde_json::to_value(&self.current)?
            .as_object()
            .cloned()
            .ok_or(SettingsDraftError::NotAnObject)?;
        if !object.contains_key(id.key()) {
            return Err(SettingsDraftError::MissingSetting(id.key()));
        }
        object.insert(id.key().to_owned(), value);
        let mut next: SettingsDocument = serde_json::from_value(Value::Object(object))?;
        next.normalize();
        self.current = next;
        Ok(())
    }

    /// Resets exactly the fields owned by one Python Settings section.
    ///
    /// # Errors
    ///
    /// Returns [`SettingsDraftError`] if either document cannot be represented
    /// by the complete typed schema.
    pub fn reset_section(&mut self, section: SettingsSection) -> Result<(), SettingsDraftError> {
        let definition = SETTINGS_SECTIONS
            .iter()
            .find(|definition| definition.section == section)
            .ok_or(SettingsDraftError::MissingSection(section))?;
        let mut current = serde_json::to_value(&self.current)?
            .as_object()
            .cloned()
            .ok_or(SettingsDraftError::NotAnObject)?;
        let defaults = serde_json::to_value(&self.defaults)?;
        let defaults = defaults
            .as_object()
            .ok_or(SettingsDraftError::NotAnObject)?;
        for id in definition.reset_fields {
            let value = defaults
                .get(id.key())
                .cloned()
                .ok_or(SettingsDraftError::MissingSetting(id.key()))?;
            current.insert(id.key().to_owned(), value);
        }
        let mut next: SettingsDocument = serde_json::from_value(Value::Object(current))?;
        next.normalize();
        self.current = next;
        Ok(())
    }

    pub fn reset_all(&mut self) {
        self.current = self.defaults.clone();
    }

    pub fn discard(&mut self) {
        self.current = self.original.clone();
    }

    pub fn commit(&mut self) -> SettingsDocument {
        self.original = self.current.clone();
        self.current.clone()
    }
}

#[cfg(test)]
mod tests {
    use apricot_core::{SettingId, SettingsSection};
    use apricot_storage::SettingsDocument;
    use serde_json::json;

    use super::SettingsDraft;

    #[test]
    fn section_reset_changes_only_owned_fields() {
        let defaults = SettingsDocument::default();
        let mut current = defaults.clone();
        current.language = "sl".to_owned();
        current.autoplay_next = true;
        let mut draft = SettingsDraft::new(current, defaults);
        draft
            .reset_section(SettingsSection::General)
            .expect("reset general");
        assert_eq!(draft.current().language, "en");
        assert!(draft.current().autoplay_next);
        assert!(draft.is_dirty());
    }

    #[test]
    fn duplicate_notification_field_resets_from_either_owner() {
        let defaults = SettingsDocument::default();
        let mut current = defaults.clone();
        current.app_update_notifications = false;
        let mut draft = SettingsDraft::new(current, defaults);
        draft
            .reset_section(SettingsSection::Notifications)
            .expect("reset notifications");
        assert!(draft.current().app_update_notifications);
    }

    #[test]
    fn typed_update_normalizes_and_discard_restores_original() {
        let defaults = SettingsDocument::default();
        let mut draft = SettingsDraft::new(defaults.clone(), defaults);
        draft
            .set_value(SettingId::DefaultVolume, json!(999))
            .expect("set volume");
        assert_eq!(draft.current().default_volume, 100);
        draft
            .set_value(SettingId::Language, json!("sl"))
            .expect("set language");
        assert!(draft.is_dirty());
        draft.discard();
        assert_eq!(draft.current().language, "en");
        assert!(!draft.is_dirty());
    }

    #[test]
    fn reset_all_uses_platform_defaults_and_commit_clears_dirty_state() {
        let defaults = SettingsDocument::with_platform_defaults(
            r"C:\Downloads\ApricotPlayer",
            r"C:\Cache\ApricotPlayer",
            "beta",
        );
        let mut current = defaults.clone();
        current.download_folder = "changed".to_owned();
        let mut draft = SettingsDraft::new(current, defaults.clone());
        draft.reset_all();
        assert_eq!(draft.current().download_folder, defaults.download_folder);
        assert!(draft.is_dirty());
        let committed = draft.commit();
        assert_eq!(committed, defaults);
        assert!(!draft.is_dirty());
    }
}
