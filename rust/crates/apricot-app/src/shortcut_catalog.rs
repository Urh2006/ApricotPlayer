//! Presentation categories and search for the canonical shortcut catalog.
//! Categories never change an action's activation scope or saved identity.

use crate::ShortcutActionItem;
use apricot_core::{
    TranslationCatalog,
    action::{ACTIONS, ActionScope},
};
use apricot_storage::SettingsDocument;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShortcutCategory {
    pub id: &'static str,
    pub label_key: &'static str,
}

pub const SHORTCUT_CATEGORIES: &[ShortcutCategory] = &[
    ShortcutCategory {
        id: "all",
        label_key: "shortcuts_category_all",
    },
    ShortcutCategory {
        id: "general",
        label_key: "shortcuts_category_general",
    },
    ShortcutCategory {
        id: "playback",
        label_key: "shortcuts_category_playback",
    },
    ShortcutCategory {
        id: "seeking",
        label_key: "shortcuts_category_seeking",
    },
    ShortcutCategory {
        id: "audio",
        label_key: "shortcuts_category_audio",
    },
    ShortcutCategory {
        id: "bookmarks",
        label_key: "shortcuts_category_bookmarks",
    },
    ShortcutCategory {
        id: "files",
        label_key: "shortcuts_category_files",
    },
    ShortcutCategory {
        id: "youtube",
        label_key: "shortcuts_category_youtube",
    },
    ShortcutCategory {
        id: "spotify",
        label_key: "shortcuts_category_spotify",
    },
    ShortcutCategory {
        id: "podcasts",
        label_key: "shortcuts_category_podcasts",
    },
    ShortcutCategory {
        id: "radio_iptv",
        label_key: "shortcuts_category_radio_iptv",
    },
    ShortcutCategory {
        id: "recording",
        label_key: "shortcuts_category_recording",
    },
    ShortcutCategory {
        id: "downloads",
        label_key: "shortcuts_category_downloads",
    },
    ShortcutCategory {
        id: "system",
        label_key: "shortcuts_category_system",
    },
];

#[must_use]
pub fn action_category(id: &str) -> &'static str {
    if id.starts_with("spotify_") || id == "open_spotify" {
        return "spotify";
    }
    if id.starts_with("recording_") || id.starts_with("open_recording") {
        return "recording";
    }
    if id.contains("radio") || id.contains("iptv") {
        return "radio_iptv";
    }
    if id.contains("bookmark")
        || id.contains("marker")
        || id.contains("chapter")
        || id.contains("range")
        || id.contains("clip")
        || id == "player_ab_loop"
    {
        return "bookmarks";
    }
    if id.contains("seek") || id.contains("go_to_time") || id.contains("go_to_percent") {
        return "seeking";
    }
    if id.contains("equalizer")
        || id.contains("volume")
        || id.contains("pitch")
        || id.contains("speed")
        || id.contains("pan_")
        || id.contains("replaygain")
        || id.contains("bass_boost")
        || id.contains("audio_processing")
        || id.contains("spatial")
    {
        return "audio";
    }
    if id.contains("download") || id.contains("converter") || id == "queue_audio" {
        return "downloads";
    }
    if id.contains("podcast")
        || id.contains("subscription")
        || id.contains("rss")
        || id == "subscribe_channel"
        || id == "unsubscribe_channel"
    {
        return "podcasts";
    }
    if id.contains("playlist")
        || id.contains("file")
        || id.contains("folder")
        || id.contains("playback_queue")
    {
        return "files";
    }
    if id.contains("search")
        || id.contains("channel")
        || id.contains("related")
        || id.contains("youtube")
        || id.contains("soundcloud")
    {
        return "youtube";
    }
    if id.starts_with("player_") || id.contains("sleep_timer") || id == "background_play_pause" {
        return "playback";
    }
    if id.contains("diagnostic")
        || id.contains("notification")
        || id.contains("settings")
        || id.contains("integrations")
        || id.contains("update")
        || id.contains("help")
    {
        return "system";
    }
    "general"
}

#[must_use]
pub fn category_label<'a>(catalog: &'a TranslationCatalog, id: &str) -> &'a str {
    let key = SHORTCUT_CATEGORIES
        .iter()
        .find(|category| category.id == id)
        .map_or("shortcuts_category_general", |category| category.label_key);
    catalog.text(key)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ShortcutFilter {
    #[default]
    All,
    Assigned,
    Unassigned,
    Changed,
}

impl ShortcutFilter {
    #[must_use]
    pub const fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Assigned,
            2 => Self::Unassigned,
            3 => Self::Changed,
            _ => Self::All,
        }
    }
}

#[must_use]
pub fn shortcut_actions(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
) -> Vec<ShortcutActionItem> {
    ACTIONS
        .iter()
        .map(|action| ShortcutActionItem {
            action_id: action.id.as_str().to_owned(),
            label: catalog.text(action.label_key).to_owned(),
            scope: action_scope_label(catalog, action.id.as_str()),
            shortcut: settings
                .keyboard_shortcuts
                .get(action.id.as_str())
                .cloned()
                .unwrap_or_else(|| action.default_shortcut().to_owned()),
        })
        .collect()
}

/// Search deliberately spans all categories. Clearing it restores the selected category.
#[must_use]
pub fn filtered_actions(
    catalog: &TranslationCatalog,
    settings: &SettingsDocument,
    category: &str,
    query: &str,
    filter: ShortcutFilter,
) -> Vec<ShortcutActionItem> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    shortcut_actions(catalog, settings)
        .into_iter()
        .filter(|action| {
            let assigned = !action.shortcut.trim().is_empty();
            let changed = ACTIONS
                .iter()
                .find(|definition| definition.id.as_str() == action.action_id)
                .is_some_and(|definition| definition.default_shortcut() != action.shortcut);
            let allowed = match filter {
                ShortcutFilter::All => true,
                ShortcutFilter::Assigned => assigned,
                ShortcutFilter::Unassigned => !assigned,
                ShortcutFilter::Changed => changed,
            };
            if !allowed {
                return false;
            }
            let action_category = action_category(&action.action_id);
            if terms.is_empty() {
                return category == "all" || category == action_category;
            }
            let haystack = format!(
                "{} {} {} {}",
                action.label,
                action.action_id,
                action.shortcut,
                category_label(catalog, action_category)
            )
            .to_lowercase();
            terms.iter().all(|term| haystack.contains(term))
        })
        .collect()
}

#[must_use]
pub fn action_scope_label(catalog: &TranslationCatalog, action_id: &str) -> String {
    let Some(action) = ACTIONS
        .iter()
        .find(|action| action.id.as_str() == action_id)
    else {
        return String::new();
    };
    let parts: Vec<&str> = action
        .scopes
        .iter()
        .map(|scope| {
            catalog.text(match scope {
                ActionScope::Global => "shortcut_scope_app",
                ActionScope::Player => "shortcut_scope_player",
                ActionScope::List => "shortcut_scope_list",
                ActionScope::Dialog => "shortcut_scope_dialog",
            })
        })
        .collect();
    parts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::english_catalog;

    #[test]
    fn category_navigation_cannot_change_assignments_and_search_spans_categories() {
        let mut settings = SettingsDocument::default();
        settings
            .keyboard_shortcuts
            .insert("spotify_toggle_saved".into(), "Ctrl+F8".into());
        let before = settings.clone();
        let spotify = filtered_actions(
            &english_catalog(),
            &settings,
            "spotify",
            "",
            ShortcutFilter::All,
        );
        assert!(
            spotify
                .iter()
                .any(|action| action.action_id == "spotify_toggle_saved")
        );
        assert!(
            spotify
                .iter()
                .all(|action| action_category(&action.action_id) == "spotify")
        );
        let search = filtered_actions(
            &english_catalog(),
            &settings,
            "audio",
            "Ctrl+F8",
            ShortcutFilter::All,
        );
        assert!(
            search
                .iter()
                .any(|action| action.action_id == "spotify_toggle_saved")
        );
        assert_eq!(settings, before);
    }

    #[test]
    fn changed_filter_and_empty_search_are_stable() {
        let mut settings = SettingsDocument::default();
        settings
            .keyboard_shortcuts
            .insert("open_search".into(), "Ctrl+F8".into());
        let changed = filtered_actions(
            &english_catalog(),
            &settings,
            "all",
            "",
            ShortcutFilter::Changed,
        );
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].action_id, "open_search");
        assert!(
            filtered_actions(
                &english_catalog(),
                &settings,
                "all",
                "no-such-shortcut-987",
                ShortcutFilter::All
            )
            .is_empty()
        );
        for action in ACTIONS {
            assert!(
                SHORTCUT_CATEGORIES
                    .iter()
                    .any(|category| category.id == action_category(action.id.as_str()))
            );
        }
    }
}
