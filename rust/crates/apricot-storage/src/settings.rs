use std::collections::BTreeMap;

use apricot_core::action::ACTIONS;
use serde::{Deserialize, Serialize};

/// Initial compatibility envelope. Typed settings are added section by section;
/// unknown Python keys are retained so an early beta can never erase data.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct SettingsDocument {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keyboard_shortcuts: BTreeMap<String, String>,
    #[serde(flatten)]
    pub preserved: BTreeMap<String, serde_json::Value>,
}

fn default_language() -> String {
    "en".to_owned()
}

impl SettingsDocument {
    pub fn normalize_shortcuts(&mut self) {
        for action in ACTIONS {
            self.keyboard_shortcuts
                .entry(action.id.as_str().to_owned())
                .or_insert_with(|| action.default_windows_shortcut.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::SettingsDocument;

    #[test]
    fn unknown_python_settings_survive_round_trip() {
        let input = json!({
            "language": "sl",
            "stream_format_preference": "audio",
            "future_python_key": {"enabled": true}
        });
        let document: SettingsDocument =
            serde_json::from_value(input.clone()).expect("deserialize");
        assert_eq!(serde_json::to_value(document).expect("serialize"), input);
    }

    #[test]
    fn shortcut_normalization_populates_all_current_actions() {
        let mut document = SettingsDocument::default();
        document.normalize_shortcuts();
        assert_eq!(document.keyboard_shortcuts.len(), 91);
    }
}
