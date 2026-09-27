//! Platform-neutral main-menu projection.

use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as _,
};

use apricot_core::{
    TranslationCatalog, action::action_by_id, locale::LANGUAGES, menu::CUSTOMIZABLE_MAIN_MENU,
};

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
        shortcuts: &BTreeMap<String, String>,
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
                label.push_str(
                    shortcuts
                        .get(shortcut_action)
                        .map_or(action.default_windows_shortcut, String::as_str),
                );
            }
            items.push(MainMenuItem {
                id: definition.action_id,
                label,
            });
        }

        let mut settings_label = catalog.text("settings").to_owned();
        if show_shortcuts && let Some(action) = action_by_id("open_settings") {
            settings_label.push('\t');
            settings_label.push_str(
                shortcuts
                    .get("open_settings")
                    .map_or(action.default_windows_shortcut, String::as_str),
            );
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
    embedded_catalog("en")
}

/// Returns a compile-time catalog for any shipped language, with English as
/// the fallback for missing keys. Unknown language codes select English.
///
/// # Panics
///
/// Panics only when a repository locale stops being a valid string map. The
/// locale qualification tests reject that condition before packaging.
pub fn embedded_catalog(requested_code: &str) -> TranslationCatalog {
    let selected_code = LANGUAGES
        .iter()
        .find(|language| language.code == requested_code)
        .map_or("en", |language| language.code);
    let rust_strings = rust_only_strings();
    let mut english = parse_embedded_locale(locale_source("en"));
    add_rust_only_strings(&mut english, &rust_strings, "en");
    let selected = if selected_code == "en" {
        english.clone()
    } else {
        let mut selected = parse_embedded_locale(locale_source(selected_code));
        add_rust_only_strings(&mut selected, &rust_strings, selected_code);
        selected
    };
    TranslationCatalog::new(selected_code, english, selected)
}

fn parse_embedded_locale(source: &str) -> BTreeMap<String, String> {
    serde_json::from_str(source).expect("embedded locale must remain valid")
}

/// Texts that only the Rust port needs, keyed by text key and then language
/// code. They never replace a text from the Python locales.
type RustOnlyStrings = BTreeMap<String, BTreeMap<String, String>>;

fn rust_only_strings() -> RustOnlyStrings {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/locales/rust_strings.json"
    )))
    .expect("embedded Rust-only strings must remain valid")
}

fn add_rust_only_strings(
    locale: &mut BTreeMap<String, String>,
    strings: &RustOnlyStrings,
    code: &str,
) {
    for (key, translations) in strings {
        if let Some(text) = translations.get(code) {
            locale.entry(key.clone()).or_insert_with(|| text.clone());
        }
    }
}

macro_rules! locale_json {
    ($code:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../apricot/locales/",
            $code,
            ".json"
        ))
    };
}

fn locale_source(code: &str) -> &'static str {
    match code {
        "sl" => locale_json!("sl"),
        "de" => locale_json!("de"),
        "fr" => locale_json!("fr"),
        "es" => locale_json!("es"),
        "pt" => locale_json!("pt"),
        "it" => locale_json!("it"),
        "pl" => locale_json!("pl"),
        "nl" => locale_json!("nl"),
        "sv" => locale_json!("sv"),
        "hr" => locale_json!("hr"),
        "sr" => locale_json!("sr"),
        "cs" => locale_json!("cs"),
        "sk" => locale_json!("sk"),
        "hu" => locale_json!("hu"),
        "ro" => locale_json!("ro"),
        "tr" => locale_json!("tr"),
        "uk" => locale_json!("uk"),
        "ru" => locale_json!("ru"),
        "ja" => locale_json!("ja"),
        "ko" => locale_json!("ko"),
        "zh" => locale_json!("zh"),
        "ar" => locale_json!("ar"),
        "hi" => locale_json!("hi"),
        "id" => locale_json!("id"),
        "fi" => locale_json!("fi"),
        "el" => locale_json!("el"),
        _ => locale_json!("en"),
    }
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
    use std::collections::BTreeMap;

    use apricot_core::locale::LANGUAGES;

    use super::{
        MainMenuAvailability, MainMenuModel, MenuVisibility, embedded_catalog, english_catalog,
    };

    #[test]
    fn default_menu_matches_python_availability_and_permanent_items() {
        let availability = MainMenuAvailability {
            history: MenuVisibility::Visible,
            podcasts: MenuVisibility::Visible,
            ..Default::default()
        };
        let model = MainMenuModel::build(
            &english_catalog(),
            availability,
            &[],
            true,
            &BTreeMap::new(),
        );
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
        let model = MainMenuModel::build(
            &english_catalog(),
            availability,
            &hidden,
            false,
            &BTreeMap::new(),
        );
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
        let model = MainMenuModel::build(
            &english_catalog(),
            availability,
            &[],
            true,
            &BTreeMap::new(),
        );
        assert_eq!(model.items.len(), 21);
        assert!(model.items[0].label.starts_with("Current downloads (3)"));
        assert!(model.items[1].label.starts_with("Playback queue (2)"));
    }

    #[test]
    fn menu_labels_use_the_user_shortcut_map() {
        let shortcuts = [("open_search".to_owned(), "Ctrl+F8".to_owned())].into();
        let model = MainMenuModel::build(
            &english_catalog(),
            MainMenuAvailability {
                history: MenuVisibility::Visible,
                podcasts: MenuVisibility::Visible,
                ..Default::default()
            },
            &[],
            true,
            &shortcuts,
        );
        assert!(model.items[0].label.ends_with("Ctrl+F8"));
    }

    #[test]
    fn every_shipped_locale_is_embedded() {
        for language in LANGUAGES {
            let catalog = embedded_catalog(language.code);
            assert_eq!(catalog.selected_code(), language.code);
            assert!(catalog.english_key_count() > 500);
            assert!(catalog.selected_key_count() > 0);
        }
        assert_eq!(embedded_catalog("unknown").selected_code(), "en");
    }

    #[test]
    fn rust_only_strings_cover_every_language_without_replacing_python_texts() {
        let python_english = super::parse_embedded_locale(super::locale_source("en"));
        for (key, translations) in super::rust_only_strings() {
            assert!(
                !python_english.contains_key(&key),
                "{key} must not replace a Python text"
            );
            let english = translations.get("en").expect("English text");
            let placeholders = |text: &str| {
                text.split('{')
                    .skip(1)
                    .filter_map(|part| part.split_once('}').map(|(name, _)| name.to_owned()))
                    .collect::<Vec<_>>()
            };
            for language in LANGUAGES {
                let text = translations
                    .get(language.code)
                    .unwrap_or_else(|| panic!("{key} lacks {}", language.code));
                assert_eq!(
                    placeholders(text),
                    placeholders(english),
                    "{key}/{}",
                    language.code
                );
                assert_eq!(embedded_catalog(language.code).text(&key), text);
            }
        }
    }
}
