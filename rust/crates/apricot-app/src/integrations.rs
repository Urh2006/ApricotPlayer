//! The 2.1 integrations: the system media controls, on unless turned off.

use apricot_storage::SettingsDocument;
use serde::{Deserialize, Serialize};

/// Settings key of [`IntegrationSettings`].
pub const INTEGRATIONS_KEY: &str = "integrations_v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct IntegrationSettings {
    pub media_controls: bool,
}

impl Default for IntegrationSettings {
    fn default() -> Self {
        Self {
            media_controls: true,
        }
    }
}

impl IntegrationSettings {
    #[must_use]
    pub fn from_settings(settings: &SettingsDocument) -> Self {
        settings
            .preserved
            .get(INTEGRATIONS_KEY)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_controls_are_on_by_default_and_unknown_fields_are_ignored() {
        assert!(IntegrationSettings::default().media_controls);
        let mut settings = SettingsDocument::default();
        settings.preserved.insert(
            INTEGRATIONS_KEY.to_owned(),
            serde_json::json!({"media_controls": false, "discord": true}),
        );
        assert!(!IntegrationSettings::from_settings(&settings).media_controls);
    }
}
