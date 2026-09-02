//! Platform-neutral projection of Settings sections and controls.

use std::path::Path;

use apricot_core::{
    CUSTOMIZABLE_MAIN_MENU, SETTINGS_SECTIONS, SettingId, SettingsSection, TranslationCatalog,
    locale::LANGUAGES,
};
use apricot_storage::SettingsDocument;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsChoiceOption {
    pub value: String,
    pub label: String,
}

impl SettingsChoiceOption {
    fn raw(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            label: value.clone(),
            value,
        }
    }

    fn labeled(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsCommand {
    BrowseDownloadFolder,
    SetDefaultPlayer,
    CheckYtDlpUpdates,
    CheckAppUpdates,
    ResetSection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsControl {
    ReadOnlyText {
        id: &'static str,
        label: String,
        value: String,
    },
    Text {
        setting: SettingId,
        label: String,
        value: String,
        secret: bool,
    },
    Choice {
        setting: SettingId,
        label: String,
        value: String,
        options: Vec<SettingsChoiceOption>,
    },
    Checkbox {
        setting: SettingId,
        label: String,
        checked: bool,
    },
    MenuItemCheckbox {
        action_id: &'static str,
        label: String,
        checked: bool,
    },
    Command {
        command: SettingsCommand,
        label: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsSectionItem {
    pub section: SettingsSection,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsScreenModel {
    pub title: String,
    pub section_list_name: String,
    pub sections: Vec<SettingsSectionItem>,
    pub selected_section: SettingsSection,
    pub controls: Vec<SettingsControl>,
}

impl SettingsScreenModel {
    pub fn build(
        catalog: &TranslationCatalog,
        settings: &SettingsDocument,
        settings_file: &Path,
        selected_section: SettingsSection,
    ) -> Self {
        let sections = SETTINGS_SECTIONS
            .iter()
            .map(|definition| SettingsSectionItem {
                section: definition.section,
                label: catalog.text(definition.label_key).to_owned(),
            })
            .collect();
        let mut controls = match selected_section {
            SettingsSection::General => general_controls(catalog, settings, settings_file),
            SettingsSection::MainMenu => main_menu_controls(catalog, settings),
            _ => Vec::new(),
        };
        let section_label = SETTINGS_SECTIONS
            .iter()
            .find(|definition| definition.section == selected_section)
            .map_or(selected_section.id(), |definition| {
                catalog.text(definition.label_key)
            });
        controls.push(SettingsControl::Command {
            command: SettingsCommand::ResetSection,
            label: catalog
                .text("reset_settings_for_section")
                .replace("{section}", section_label),
        });
        Self {
            title: catalog.text("settings").to_owned(),
            section_list_name: catalog.text("settings_sections").to_owned(),
            sections,
            selected_section,
            controls,
        }
    }
}

#[allow(clippy::too_many_lines)]
fn general_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    settings_file: &Path,
) -> Vec<SettingsControl> {
    let result_limits = ["0", "10", "20", "50", "100", "150", "200", "250"]
        .into_iter()
        .map(|value| {
            if value == "0" {
                SettingsChoiceOption::labeled(value, catalog.text("dynamic_results"))
            } else {
                SettingsChoiceOption::raw(value)
            }
        })
        .collect();
    let direct_link_options = [
        ("play", "direct_link_enter_play"),
        ("audio", "direct_link_enter_audio"),
        ("video", "direct_link_enter_video"),
        ("stream", "direct_link_enter_stream"),
    ]
    .into_iter()
    .map(|(value, key)| SettingsChoiceOption::labeled(value, catalog.text(key)))
    .collect();
    let update_intervals = ["0.5", "1", "2", "3", "6", "12", "24"]
        .into_iter()
        .map(|value| {
            let label = match value {
                "0.5" => catalog.text("interval_30_minutes").to_owned(),
                "1" => catalog.text("interval_1_hour").to_owned(),
                hours => catalog.text("interval_hours").replace("{hours}", hours),
            };
            SettingsChoiceOption::labeled(value, label)
        })
        .collect();

    vec![
        SettingsControl::Choice {
            setting: SettingId::Language,
            label: catalog.text("language").to_owned(),
            value: settings.language.clone(),
            options: LANGUAGES
                .iter()
                .map(|language| SettingsChoiceOption::labeled(language.code, language.name))
                .collect(),
        },
        SettingsControl::ReadOnlyText {
            id: "settings_file",
            label: catalog.text("settings_file").to_owned(),
            value: settings_file.display().to_string(),
        },
        SettingsControl::Text {
            setting: SettingId::DownloadFolder,
            label: catalog.text("download_folder").to_owned(),
            value: settings.download_folder.clone(),
            secret: false,
        },
        SettingsControl::Command {
            command: SettingsCommand::BrowseDownloadFolder,
            label: catalog.text("browse").to_owned(),
        },
        SettingsControl::Command {
            command: SettingsCommand::SetDefaultPlayer,
            label: catalog.text("set_default_player").to_owned(),
        },
        SettingsControl::Choice {
            setting: SettingId::ResultsLimit,
            label: catalog.text("results_limit").to_owned(),
            value: settings.results_limit.min(250).to_string(),
            options: result_limits,
        },
        SettingsControl::Choice {
            setting: SettingId::DirectLinkEnterAction,
            label: catalog.text("direct_link_enter_action").to_owned(),
            value: settings.direct_link_enter_action.clone(),
            options: direct_link_options,
        },
        checkbox(
            SettingId::ShowShortcutsInLabels,
            "show_shortcuts_in_labels",
            settings.show_shortcuts_in_labels,
            catalog,
        ),
        checkbox(
            SettingId::AutoUpdateYtdlp,
            "auto_update",
            settings.auto_update_ytdlp,
            catalog,
        ),
        checkbox(
            SettingId::AutoUpdateApp,
            "auto_update_app",
            settings.auto_update_app,
            catalog,
        ),
        SettingsControl::Choice {
            setting: SettingId::UpdateChannel,
            label: catalog.text("update_channel").to_owned(),
            value: settings.update_channel.clone(),
            options: [
                SettingsChoiceOption::labeled("stable", catalog.text("update_channel_stable")),
                SettingsChoiceOption::labeled("beta", catalog.text("update_channel_beta")),
            ]
            .into(),
        },
        SettingsControl::Choice {
            setting: SettingId::AppUpdateIntervalHours,
            label: catalog.text("app_update_interval").to_owned(),
            value: compact_number(settings.app_update_interval_hours),
            options: update_intervals,
        },
        SettingsControl::Command {
            command: SettingsCommand::CheckYtDlpUpdates,
            label: catalog.text("check_ytdlp_updates_now").to_owned(),
        },
        SettingsControl::Command {
            command: SettingsCommand::CheckAppUpdates,
            label: catalog.text("check_app_updates_now").to_owned(),
        },
        checkbox(
            SettingId::CloseToTray,
            "close_to_tray",
            settings.close_to_tray,
            catalog,
        ),
        checkbox(
            SettingId::StartWithWindows,
            "start_with_windows",
            settings.start_with_windows,
            catalog,
        ),
        checkbox(
            SettingId::TrayNotification,
            "tray_notification",
            settings.tray_notification,
            catalog,
        ),
    ]
}

fn main_menu_controls(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<SettingsControl> {
    CUSTOMIZABLE_MAIN_MENU
        .iter()
        .map(|definition| SettingsControl::MenuItemCheckbox {
            action_id: definition.action_id,
            label: catalog.text(definition.label_key).to_owned(),
            checked: !settings
                .main_menu_hidden_actions
                .iter()
                .any(|hidden| hidden == definition.action_id),
        })
        .collect()
}

fn checkbox(
    setting: SettingId,
    label_key: &str,
    checked: bool,
    catalog: &TranslationCatalog,
) -> SettingsControl {
    SettingsControl::Checkbox {
        setting,
        label: catalog.text(label_key).to_owned(),
        checked,
    }
}

fn compact_number(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use apricot_core::{CUSTOMIZABLE_MAIN_MENU, SettingId, SettingsSection};
    use apricot_storage::SettingsDocument;

    use crate::english_catalog;

    use super::{SettingsControl, SettingsScreenModel};

    #[test]
    fn settings_shell_has_all_sections_in_canonical_order() {
        let model = SettingsScreenModel::build(
            &english_catalog(),
            &SettingsDocument::default(),
            Path::new(r"C:\Profile\settings.json"),
            SettingsSection::General,
        );
        assert_eq!(model.sections.len(), 11);
        assert_eq!(model.sections[0].section, SettingsSection::General);
        assert_eq!(model.sections[1].section, SettingsSection::MainMenu);
        assert_eq!(model.selected_section, SettingsSection::General);
        assert_eq!(model.section_list_name, "Settings sections");
    }

    #[test]
    fn general_controls_preserve_python_order_and_current_values() {
        let settings = SettingsDocument {
            language: "sl".to_owned(),
            results_limit: 50,
            ..SettingsDocument::default()
        };
        let model = SettingsScreenModel::build(
            &english_catalog(),
            &settings,
            Path::new(r"C:\Profile\settings.json"),
            SettingsSection::General,
        );
        assert_eq!(model.controls.len(), 18);
        assert!(matches!(
            &model.controls[0],
            SettingsControl::Choice {
                setting: SettingId::Language,
                value,
                options,
                ..
            } if value == "sl" && options.len() == 27
        ));
        assert!(matches!(
            &model.controls[5],
            SettingsControl::Choice {
                setting: SettingId::ResultsLimit,
                value,
                ..
            } if value == "50"
        ));
    }

    #[test]
    fn main_menu_customization_is_nineteen_real_checkboxes() {
        let settings = SettingsDocument {
            main_menu_hidden_actions: vec!["search".to_owned()],
            ..SettingsDocument::default()
        };
        let model = SettingsScreenModel::build(
            &english_catalog(),
            &settings,
            Path::new("settings.json"),
            SettingsSection::MainMenu,
        );
        assert_eq!(model.controls.len(), CUSTOMIZABLE_MAIN_MENU.len() + 1);
        assert!(matches!(
            &model.controls[0],
            SettingsControl::MenuItemCheckbox {
                action_id: "current_downloads",
                checked: true,
                ..
            }
        ));
        assert!(model.controls.iter().any(|control| matches!(
            control,
            SettingsControl::MenuItemCheckbox {
                action_id: "search",
                checked: false,
                ..
            }
        )));
        assert!(model.controls.iter().all(|control| !matches!(
            control,
            SettingsControl::MenuItemCheckbox {
                action_id: "settings" | "exit",
                ..
            }
        )));
    }
}
