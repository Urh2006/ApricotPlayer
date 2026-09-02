//! Platform-neutral main-menu projection.

use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as _,
};

use apricot_core::{TranslationCatalog, action::action_by_id, menu::CUSTOMIZABLE_MAIN_MENU};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MainMenuItem {
    pub id: &'static str,
    pub label: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MenuVisibility {
    #[default]
    Hidden,
    Visible,
}

impl MenuVisibility {
    const fn is_visible(self) -> bool {
        matches!(self, Self::Visible)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MainMenuAvailability {
    pub download_count: usize,
    pub playback_queue_count: usize,
    pub resume: MenuVisibility,
    pub trending: MenuVisibility,
    pub history: MenuVisibility,
    pub podcasts: MenuVisibility,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MainMenuModel {
    pub accessible_name: String,
    pub items: Vec<MainMenuItem>,
}

impl MainMenuModel {
    pub fn build(
        catalog: &TranslationCatalog,
        availability: MainMenuAvailability,
        hidden_ids: &[String],
        show_shortcuts: bool,
    ) -> Self {
        let hidden: HashSet<&str> = hidden_ids.iter().map(String::as_str).collect();
        let mut items = Vec::new();
        for definition in CUSTOMIZABLE_MAIN_MENU {
            if hidden.contains(definition.action_id)
                || !is_available(definition.action_id, availability)
            {
                continue;
            }
            let mut label = menu_label(catalog, definition.action_id, definition.label_key);
            match definition.action_id {
                "current_downloads" => {
                    write!(label, " ({})", availability.download_count)
                        .expect("writing to a String cannot fail");
                }
                "playback_queue" => {
                    write!(label, " ({})", availability.playback_queue_count)
                        .expect("writing to a String cannot fail");
                }
                _ => {}
            }
            if show_shortcuts
                && let Some(shortcut_action) = shortcut_action_id(definition.action_id)
                && let Some(action) = action_by_id(shortcut_action)
            {
                label.push('\t');
                label.push_str(action.default_windows_shortcut);
            }
            items.push(MainMenuItem {
                id: definition.action_id,
                label,
            });
        }

        let mut settings_label = catalog.text("settings").to_owned();
        if show_shortcuts && let Some(action) = action_by_id("open_settings") {
            settings_label.push('\t');
            settings_label.push_str(action.default_windows_shortcut);
        }
        items.push(MainMenuItem {
            id: "settings",
            label: settings_label,
        });
        items.push(MainMenuItem {
            id: "exit",
            label: catalog.text("exit").to_owned(),
        });

        Self {
            accessible_name: catalog.text("main_menu").to_owned(),
            items,
        }
    }
}

/// Returns the English catalog embedded at compile time.
///
/// # Panics
///
/// Panics only when the repository's compile-time English locale stops being a
/// valid string map, which is also rejected by the locale qualification tests.
pub fn english_catalog() -> TranslationCatalog {
    let english: BTreeMap<String, String> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../apricot/locales/en.json"
    )))
    .expect("embedded English locale must remain valid");
    TranslationCatalog::new("en", english.clone(), english)
}

fn is_available(id: &str, availability: MainMenuAvailability) -> bool {
    match id {
        "current_downloads" => availability.download_count > 0,
        "playback_queue" => availability.playback_queue_count > 0,
        "resume_last_session" => availability.resume.is_visible(),
        "trending" => availability.trending.is_visible(),
        "history" => availability.history.is_visible(),
        "rss_feeds" => availability.podcasts.is_visible(),
        _ => true,
    }
}

fn menu_label(catalog: &TranslationCatalog, id: &str, label_key: &str) -> String {
    if id == "search" {
        format!(
            "{} / {}",
            catalog.text("search_youtube"),
            catalog.text("soundcloud")
        )
    } else {
        catalog.text(label_key).to_owned()
    }
}

fn shortcut_action_id(menu_id: &str) -> Option<&'static str> {
    match menu_id {
        "current_downloads" => Some("open_current_downloads"),
        "playback_queue" => Some("open_playback_queue"),
        "search" => Some("open_search"),
        "audiovault" => Some("open_audiovault"),
        "play_folder" => Some("open_play_from_folder"),
        "play_file" => Some("open_play_file"),
        "direct_link" => Some("open_direct_link"),
        "favorites" => Some("open_favorites"),
        "bookmarks" => Some("open_bookmarks"),
        "playlists" => Some("open_playlists"),
        "subscriptions" => Some("open_subscriptions"),
        "notification_center" => Some("new_subscription_videos"),
        "history" => Some("open_history"),
        "rss_feeds" => Some("open_podcasts_rss"),
        "diagnostic_report" => Some("copy_diagnostic_report"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{MainMenuAvailability, MainMenuModel, MenuVisibility, english_catalog};

    #[test]
    fn default_menu_matches_python_availability_and_permanent_items() {
        let availability = MainMenuAvailability {
            history: MenuVisibility::Visible,
            podcasts: MenuVisibility::Visible,
            ..Default::default()
        };
        let model = MainMenuModel::build(&english_catalog(), availability, &[], true);
        assert_eq!(model.accessible_name, "Main menu");
        assert_eq!(model.items.len(), 17);
        assert_eq!(model.items[0].id, "search");
        assert!(model.items[0].label.ends_with("Ctrl+Alt+Y"));
        assert_eq!(model.items[model.items.len() - 2].id, "settings");
        assert_eq!(model.items.last().expect("exit").id, "exit");
    }

    #[test]
    fn customization_never_hides_settings_or_exit() {
        let availability = MainMenuAvailability {
            history: MenuVisibility::Visible,
            podcasts: MenuVisibility::Visible,
            ..Default::default()
        };
        let hidden = vec![
            "search".to_owned(),
            "settings".to_owned(),
            "exit".to_owned(),
        ];
        let model = MainMenuModel::build(&english_catalog(), availability, &hidden, false);
        assert!(model.items.iter().all(|item| item.id != "search"));
        assert!(model.items.iter().any(|item| item.id == "settings"));
        assert!(model.items.iter().any(|item| item.id == "exit"));
        assert!(model.items.iter().all(|item| !item.label.contains('\t')));
    }

    #[test]
    fn dynamic_entries_show_counts_only_when_available() {
        let availability = MainMenuAvailability {
            download_count: 3,
            playback_queue_count: 2,
            resume: MenuVisibility::Visible,
            trending: MenuVisibility::Visible,
            history: MenuVisibility::Visible,
            podcasts: MenuVisibility::Visible,
        };
        let model = MainMenuModel::build(&english_catalog(), availability, &[], true);
        assert_eq!(model.items.len(), 21);
        assert!(model.items[0].label.starts_with("Current downloads (3)"));
        assert!(model.items[1].label.starts_with("Playback queue (2)"));
    }
}
