//! Ordered customizable main-menu catalog.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MainMenuDefinition {
    pub action_id: &'static str,
    pub label_key: &'static str,
}

pub const CUSTOMIZABLE_MAIN_MENU: &[MainMenuDefinition] = &[
    MainMenuDefinition {
        action_id: "current_downloads",
        label_key: "current_downloads",
    },
    MainMenuDefinition {
        action_id: "playback_queue",
        label_key: "playback_queue",
    },
    MainMenuDefinition {
        action_id: "search",
        label_key: "main_menu_search",
    },
    MainMenuDefinition {
        action_id: "resume_last_session",
        label_key: "resume_last_session",
    },
    MainMenuDefinition {
        action_id: "trending",
        label_key: "trending",
    },
    MainMenuDefinition {
        action_id: "audiovault",
        label_key: "audiovault",
    },
    // Spotify (`docs/SPOTIFY_PLAN.md` D01, 4.1): not in Python 1.0.21.
    MainMenuDefinition {
        action_id: "spotify",
        label_key: "spotify",
    },
    MainMenuDefinition {
        action_id: "play_folder",
        label_key: "play_folder",
    },
    MainMenuDefinition {
        action_id: "play_file",
        label_key: "play_file",
    },
    MainMenuDefinition {
        action_id: "direct_link",
        label_key: "direct_link",
    },
    MainMenuDefinition {
        action_id: "favorites",
        label_key: "favorites",
    },
    MainMenuDefinition {
        action_id: "bookmarks",
        label_key: "bookmarks",
    },
    MainMenuDefinition {
        action_id: "playlists",
        label_key: "playlists",
    },
    MainMenuDefinition {
        action_id: "subscriptions",
        label_key: "subscriptions",
    },
    MainMenuDefinition {
        action_id: "notification_center",
        label_key: "notification_center",
    },
    MainMenuDefinition {
        action_id: "history",
        label_key: "history",
    },
    MainMenuDefinition {
        action_id: "rss_feeds",
        label_key: "rss_feeds",
    },
    MainMenuDefinition {
        action_id: "file_converter",
        label_key: "file_converter",
    },
    MainMenuDefinition {
        action_id: "folder_converter",
        label_key: "folder_converter",
    },
    MainMenuDefinition {
        action_id: "diagnostic_report",
        label_key: "copy_diagnostic_report",
    },
];

pub const PERMANENT_MAIN_MENU_IDS: &[&str] = &["update_available", "settings", "exit"];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{CUSTOMIZABLE_MAIN_MENU, PERMANENT_MAIN_MENU_IDS};

    #[test]
    fn menu_contract_has_19_unique_customizable_items() {
        let ids: HashSet<_> = CUSTOMIZABLE_MAIN_MENU
            .iter()
            .map(|item| item.action_id)
            .collect();
        // 19 Python items plus Spotify.
        assert_eq!(CUSTOMIZABLE_MAIN_MENU.len(), 20);
        assert_eq!(ids.len(), CUSTOMIZABLE_MAIN_MENU.len());
    }

    #[test]
    fn permanent_items_cannot_be_customized() {
        for permanent in PERMANENT_MAIN_MENU_IDS {
            assert!(
                CUSTOMIZABLE_MAIN_MENU
                    .iter()
                    .all(|item| item.action_id != *permanent)
            );
        }
    }
}
