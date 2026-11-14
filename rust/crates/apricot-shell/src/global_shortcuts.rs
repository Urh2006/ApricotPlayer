//! The Global shortcuts form, shared by both native frontends.

use apricot_app::global_shortcuts::{
    GLOBAL_ACTIONS, GlobalChord, GlobalShortcutSettings, default_binding,
};
use apricot_core::TranslationCatalog;

use crate::{FeatureForm, FormField, FormValue, FormValues};

fn label(catalog: &TranslationCatalog, action: &str) -> String {
    catalog.text(&format!("shortcut_{action}")).to_owned()
}

/// One switch for all of them and a chord field for every command.
#[must_use]
pub fn global_shortcuts_form(
    catalog: &TranslationCatalog,
    settings: &GlobalShortcutSettings,
) -> FeatureForm {
    let mut fields = vec![
        FormField::checkbox(
            "enabled",
            catalog.text("global_shortcuts_enabled"),
            settings.enabled,
        ),
        FormField::read_only(
            "hint",
            catalog.text("global_shortcuts_hint_label"),
            // Control with Option is VoiceOver's, so the Mac example differs.
            catalog.text(if cfg!(target_os = "macos") {
                "global_shortcuts_hint_mac"
            } else {
                "global_shortcuts_hint"
            }),
        ),
    ];
    for (action, _) in GLOBAL_ACTIONS {
        fields.push(FormField::text(
            action,
            label(catalog, action),
            settings.binding(action),
        ));
    }
    FeatureForm::localized(
        catalog,
        catalog.text("shortcut_open_global_shortcuts"),
        fields,
    )
}

/// The settings chosen in [`global_shortcuts_form`].
///
/// # Errors
///
/// Returns the sentence that names a chord that is not valid or is used
/// twice.
pub fn global_shortcuts_from_values(
    catalog: &TranslationCatalog,
    values: &FormValues,
) -> Result<GlobalShortcutSettings, String> {
    let mut settings = GlobalShortcutSettings {
        enabled: values
            .get("enabled")
            .and_then(FormValue::checked)
            .unwrap_or_default(),
        ..GlobalShortcutSettings::default()
    };
    let mut used: Vec<(GlobalChord, &str)> = Vec::new();
    for (action, _) in GLOBAL_ACTIONS {
        let text = values
            .get(action)
            .and_then(FormValue::text)
            .unwrap_or("")
            .trim()
            .to_owned();
        if !text.is_empty() {
            let Some(chord) = GlobalChord::parse(&text) else {
                return Err(catalog
                    .text(if cfg!(target_os = "macos") {
                        "global_shortcut_invalid_mac"
                    } else {
                        "global_shortcut_invalid"
                    })
                    .replace("{field}", &label(catalog, action)));
            };
            if let Some((_, other)) = used.iter().find(|(existing, _)| *existing == chord) {
                return Err(catalog
                    .text("global_shortcut_duplicate")
                    .replace("{field}", &label(catalog, action))
                    .replace("{other}", &label(catalog, other)));
            }
            used.push((chord, action));
        }
        if text != default_binding(action) {
            settings.bindings.insert(action.to_owned(), text);
        }
    }
    Ok(settings)
}

/// What is said after the shortcuts were applied; `failed` are the
/// commands whose chord another program already uses.
#[must_use]
pub fn applied_message(catalog: &TranslationCatalog, failed: &[&str]) -> String {
    if failed.is_empty() {
        return catalog.text("global_shortcuts_saved").to_owned();
    }
    let names: Vec<_> = failed.iter().map(|action| label(catalog, action)).collect();
    catalog
        .text("global_shortcuts_failed")
        .replace("{list}", &names.join(", "))
}

impl crate::Shell {
    /// Global shortcuts from Settings or its own shortcut, saved at once.
    /// The macOS frontend registers them itself.
    pub(crate) fn edit_global_shortcuts(&mut self, ui: &mut dyn crate::ShellUi) {
        let catalog = self.catalog();
        let mut settings = GlobalShortcutSettings::from_settings(self.application.settings());
        loop {
            let Some(values) = ui.feature_form(&global_shortcuts_form(&catalog, &settings)) else {
                return;
            };
            match global_shortcuts_from_values(&catalog, &values) {
                Ok(chosen) => {
                    settings = chosen;
                    break;
                }
                Err(error) => ui.show_error(&error),
            }
        }
        self.application.set_feature_settings(
            apricot_app::global_shortcuts::GLOBAL_SHORTCUTS_KEY,
            serde_json::json!(settings),
        );
        match self.application.save_settings() {
            Ok(()) => ui.set_status(&applied_message(&catalog, &[]), true),
            Err(error) => ui.show_error(
                &catalog
                    .text("settings_save_failed")
                    .replace("{error}", &error.to_string()),
            ),
        }
    }
}

/// The platform's form of a chord.
#[must_use]
pub fn hotkey(chord: GlobalChord) -> apricot_platform::global_hotkeys::Hotkey {
    apricot_platform::global_hotkeys::Hotkey {
        system: chord.system,
        control: chord.chord.control,
        alt: chord.chord.alt,
        shift: chord.chord.shift,
        key: chord.chord.key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(form: &FeatureForm) -> FormValues {
        form.fields
            .iter()
            .filter_map(|field| match &field.kind {
                crate::FormFieldKind::Checkbox { checked } => {
                    Some((field.id.clone(), FormValue::Checkbox(*checked)))
                }
                crate::FormFieldKind::Text { value, .. } => {
                    Some((field.id.clone(), FormValue::Text(value.clone())))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn defaults_round_trip_without_stored_bindings() {
        let catalog = apricot_app::embedded_catalog("en");
        let form = global_shortcuts_form(&catalog, &GlobalShortcutSettings::default());
        let settings = global_shortcuts_from_values(&catalog, &values(&form)).unwrap();
        assert_eq!(settings, GlobalShortcutSettings::default());
    }

    #[test]
    fn invalid_and_repeated_chords_are_named() {
        let catalog = apricot_app::embedded_catalog("en");
        let form = global_shortcuts_form(&catalog, &GlobalShortcutSettings::default());
        let mut chosen = values(&form);
        chosen.insert("player_next".into(), FormValue::Text("Space".into()));
        let error = global_shortcuts_from_values(&catalog, &chosen).unwrap_err();
        assert!(error.contains(&label(&catalog, "player_next")), "{error}");
        chosen.insert("player_next".into(), FormValue::Text("Ctrl+Alt+F1".into()));
        chosen.insert(
            "player_previous".into(),
            FormValue::Text("Ctrl+Alt+F1".into()),
        );
        let error = global_shortcuts_from_values(&catalog, &chosen).unwrap_err();
        assert!(error.contains(&label(&catalog, "player_next")), "{error}");
        chosen.insert("player_next".into(), FormValue::Text(String::new()));
        let settings = global_shortcuts_from_values(&catalog, &chosen).unwrap();
        assert_eq!(settings.binding("player_next"), "");
    }
}
