//! Action Finder entries, matching Python's `action_finder_actions` in
//! `apricot/ui/menus.py` item for item and in the same order.

use apricot_core::TranslationCatalog;
use apricot_storage::SettingsDocument;

use crate::context_menu::shortcut_for;

/// The active player item as Python's `action_finder_actions` sees it.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActionFinderPlayer {
    pub paused: bool,
    /// Python `is_local_media_item`.
    pub local_media: bool,
    /// Python `is_youtube_url` on the item's `url` or `webpage_url`.
    pub youtube: bool,
    /// Python `item.get("kind") == "rss_item"`.
    pub podcast_episode: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ActionFinderContext {
    /// Python `last_player_session_available`.
    pub resume_available: bool,
    /// Python `player_is_active`, with the current item's properties.
    pub player: Option<ActionFinderPlayer>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionFinderItem {
    /// The action the UI runs, as routed by its shortcut dispatcher.
    pub action_id: &'static str,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActionFinderModel {
    pub title: String,
    pub query_label: String,
    pub results_name: String,
    pub no_results_label: String,
    pub open_label: String,
    pub cancel_label: String,
    pub items: Vec<ActionFinderItem>,
}

struct Labels<'a> {
    catalog: &'a TranslationCatalog,
    settings: &'a SettingsDocument,
}

impl Labels<'_> {
    /// Python `label_with_shortcut(label, action, "\t")` with the tab shown
    /// as ", ", as the Action Finder list does.
    fn with_shortcut(&self, label: String, shortcut_action: &str) -> String {
        if !self.settings.show_shortcuts_in_labels {
            return label;
        }
        let shortcut = shortcut_for(self.settings, shortcut_action);
        if shortcut.is_empty() {
            label
        } else {
            format!("{label}, {shortcut}")
        }
    }

    /// Python `menu_label_with_shortcut(label_key, action)`.
    fn item(&self, label_key: &str, action_id: &'static str) -> ActionFinderItem {
        ActionFinderItem {
            action_id,
            label: self.with_shortcut(self.catalog.text(label_key).to_owned(), action_id),
        }
    }

    /// An entry Python lists with a plain `self.t(key)` label.
    fn plain(&self, label_key: &str, action_id: &'static str) -> ActionFinderItem {
        ActionFinderItem {
            action_id,
            label: self.catalog.text(label_key).to_owned(),
        }
    }
}

impl ActionFinderModel {
    pub fn build(
        catalog: &TranslationCatalog,
        settings: &SettingsDocument,
        context: ActionFinderContext,
    ) -> Self {
        let labels = Labels { catalog, settings };
        let search_label = format!(
            "{} / {}",
            catalog.text("search_youtube"),
            catalog.text("soundcloud")
        );
        let mut items = vec![
            labels.item("main_menu", "open_main_menu"),
            ActionFinderItem {
                action_id: "open_search",
                label: labels.with_shortcut(search_label, "open_search"),
            },
            labels.item("audiovault", "open_audiovault"),
            // Spotify (`docs/SPOTIFY_PLAN.md` 5.2), not in Python 1.0.21.
            labels.item("spotify", "open_spotify"),
            labels.item("spotify_accounts", "spotify_accounts"),
            labels.item("spotify_queue", "spotify_queue"),
            labels.item("spotify_devices", "spotify_devices"),
            labels.item("play_folder", "open_play_from_folder"),
            labels.item("play_file", "open_play_file"),
            labels.item("direct_link", "open_direct_link"),
            labels.item("favorites", "open_favorites"),
            labels.item("bookmarks", "open_bookmarks"),
            labels.item("playlists", "open_playlists"),
            labels.item("subscriptions", "open_subscriptions"),
            labels.item("notification_center", "new_subscription_videos"),
            labels.item("playback_queue", "open_playback_queue"),
            labels.plain("file_converter", "file_converter"),
            labels.plain("folder_converter", "folder_converter"),
            labels.item("copy_diagnostic_report", "copy_diagnostic_report"),
            labels.item("settings", "open_settings"),
        ];
        if context.resume_available {
            items.insert(
                2,
                labels.plain("resume_last_session", "resume_last_session"),
            );
        }
        if settings.enable_trending {
            items.insert(2, labels.plain("trending", "trending"));
        }
        if settings.enable_history {
            items.push(labels.item("history", "open_history"));
        }
        if settings.enable_podcasts_rss {
            items.push(labels.item("rss_feeds", "open_podcasts_rss"));
        }
        if let Some(player) = context.player {
            items.extend(player_items(&labels, player));
        }
        Self {
            title: catalog.text("action_finder").to_owned(),
            query_label: catalog.text("action_finder_search").to_owned(),
            results_name: catalog.text("action_finder_results").to_owned(),
            no_results_label: catalog.text("action_finder_no_results").to_owned(),
            open_label: catalog.text("open").to_owned(),
            cancel_label: catalog.text("cancel").to_owned(),
            items,
        }
    }

    pub fn filtered_items(&self, query: &str) -> Vec<&ActionFinderItem> {
        self.filtered_indices(query)
            .into_iter()
            .map(|index| &self.items[index])
            .collect()
    }

    pub fn filtered_indices(&self, query: &str) -> Vec<usize> {
        let words: Vec<_> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return (0..self.items.len()).collect();
        }
        self.items
            .iter()
            .enumerate()
            .filter(|(_index, item)| {
                let label = item.label.to_lowercase();
                words.iter().all(|word| label.contains(word))
            })
            .map(|(index, _item)| index)
            .collect()
    }
}

/// Python's `player_actions` block of `action_finder_actions`.
fn player_items(labels: &Labels<'_>, player: ActionFinderPlayer) -> Vec<ActionFinderItem> {
    let mut items = vec![
        labels.item(
            if player.paused { "play" } else { "pause" },
            "player_play_pause",
        ),
        labels.item("previous", "player_previous"),
        labels.item("next", "player_next"),
        labels.item(
            if player.local_media {
                "copy_path"
            } else {
                "copy_link"
            },
            "player_copy_link",
        ),
        labels.item("show_video_details", "player_details"),
        labels.item("output_devices", "player_output_devices"),
        labels.item("fullscreen", "player_fullscreen"),
        labels.item("equalizer", "player_equalizer"),
        labels.item("audio_normalization", "player_replaygain"),
        labels.item("add_bookmark", "player_add_bookmark"),
        labels.item("bookmarks", "player_bookmarks"),
        labels.item("chapters", "player_chapters"),
        labels.item("transcript", "player_transcript"),
        labels.item("lyrics", "player_lyrics"),
        // Python `close_current_player`, labelled with the Back shortcut.
        ActionFinderItem {
            action_id: "close_player",
            label: labels.with_shortcut(
                labels.catalog.text("close_player").to_owned(),
                "player_back",
            ),
        },
    ];
    if player.youtube {
        items.insert(3, labels.item("play_related_video", "player_next_related"));
        items.insert(
            5,
            labels.item("copy_timestamp_link", "player_copy_timestamp_link"),
        );
        let close = items.len() - 1;
        items.insert(close, labels.item("comments", "player_comments"));
    }
    if player.podcast_episode {
        let close = items.len() - 1;
        items.insert(
            close,
            labels.item("save_podcast_speed_preset", "save_podcast_speed_preset"),
        );
    }
    items
}

#[cfg(test)]
mod tests {
    use apricot_storage::SettingsDocument;

    use super::{ActionFinderContext, ActionFinderModel, ActionFinderPlayer};
    use crate::english_catalog;

    fn ids(model: &ActionFinderModel) -> Vec<&'static str> {
        model.items.iter().map(|item| item.action_id).collect()
    }

    #[test]
    fn global_entries_follow_python_order_and_conditions() {
        let settings = SettingsDocument {
            enable_trending: true,
            enable_history: true,
            enable_podcasts_rss: true,
            ..SettingsDocument::default()
        };
        let model = ActionFinderModel::build(
            &english_catalog(),
            &settings,
            ActionFinderContext {
                resume_available: true,
                player: None,
            },
        );
        assert_eq!(
            ids(&model),
            [
                "open_main_menu",
                "open_search",
                "trending",
                "resume_last_session",
                "open_audiovault",
                "open_spotify",
                "spotify_accounts",
                "spotify_queue",
                "spotify_devices",
                "open_play_from_folder",
                "open_play_file",
                "open_direct_link",
                "open_favorites",
                "open_bookmarks",
                "open_playlists",
                "open_subscriptions",
                "new_subscription_videos",
                "open_playback_queue",
                "file_converter",
                "folder_converter",
                "copy_diagnostic_report",
                "open_settings",
                "open_history",
                "open_podcasts_rss",
            ]
        );
    }

    #[test]
    fn optional_entries_are_absent_when_disabled() {
        let settings = SettingsDocument {
            enable_trending: false,
            enable_history: false,
            enable_podcasts_rss: false,
            ..SettingsDocument::default()
        };
        let model = ActionFinderModel::build(
            &english_catalog(),
            &settings,
            ActionFinderContext::default(),
        );
        let ids = ids(&model);
        for id in [
            "trending",
            "resume_last_session",
            "open_history",
            "open_podcasts_rss",
            "player_play_pause",
        ] {
            assert!(!ids.contains(&id), "{id}");
        }
        assert_eq!(ids.len(), 20);
    }

    #[test]
    fn labels_use_python_texts_with_shortcut_after_a_comma() {
        let settings = SettingsDocument::default();
        let model = ActionFinderModel::build(
            &english_catalog(),
            &settings,
            ActionFinderContext::default(),
        );
        assert_eq!(model.items[0].label, "Main menu, Ctrl+Alt+M");
        assert_eq!(
            model.items[1].label,
            "Search YouTube / SoundCloud, Ctrl+Alt+Y"
        );
        let converter = model
            .items
            .iter()
            .find(|item| item.action_id == "file_converter")
            .expect("file converter");
        assert!(!converter.label.contains(','));

        let hidden = SettingsDocument {
            show_shortcuts_in_labels: false,
            ..SettingsDocument::default()
        };
        let model =
            ActionFinderModel::build(&english_catalog(), &hidden, ActionFinderContext::default());
        assert_eq!(model.items[0].label, "Main menu");
    }

    #[test]
    fn youtube_player_entries_are_inserted_where_python_inserts_them() {
        let model = ActionFinderModel::build(
            &english_catalog(),
            &SettingsDocument::default(),
            ActionFinderContext {
                resume_available: false,
                player: Some(ActionFinderPlayer {
                    paused: true,
                    youtube: true,
                    ..ActionFinderPlayer::default()
                }),
            },
        );
        let player: Vec<_> = ids(&model)
            .into_iter()
            .skip_while(|id| !id.starts_with("player_"))
            .collect();
        assert_eq!(
            player,
            [
                "player_play_pause",
                "player_previous",
                "player_next",
                "player_next_related",
                "player_copy_link",
                "player_copy_timestamp_link",
                "player_details",
                "player_output_devices",
                "player_fullscreen",
                "player_equalizer",
                "player_replaygain",
                "player_add_bookmark",
                "player_bookmarks",
                "player_chapters",
                "player_transcript",
                "player_lyrics",
                "player_comments",
                "close_player",
            ]
        );
        let first = model
            .items
            .iter()
            .find(|item| item.action_id == "player_play_pause")
            .expect("play or pause");
        assert!(first.label.starts_with("Play,"));
    }

    #[test]
    fn local_podcast_player_entries_match_python() {
        let model = ActionFinderModel::build(
            &english_catalog(),
            &SettingsDocument::default(),
            ActionFinderContext {
                resume_available: false,
                player: Some(ActionFinderPlayer {
                    local_media: true,
                    podcast_episode: true,
                    ..ActionFinderPlayer::default()
                }),
            },
        );
        let player: Vec<_> = model
            .items
            .iter()
            .skip_while(|item| !item.action_id.starts_with("player_"))
            .collect();
        assert!(player[0].label.starts_with("Pause"));
        assert_eq!(player[3].action_id, "player_copy_link");
        assert!(player[3].label.starts_with("Copy path"));
        let close = player.len() - 1;
        assert_eq!(player[close - 1].action_id, "save_podcast_speed_preset");
        assert_eq!(player[close].action_id, "close_player");
        assert_eq!(player[close].label, "Close, Escape");
        assert!(
            player
                .iter()
                .all(|item| item.action_id != "player_comments")
        );
    }

    #[test]
    fn filtering_requires_every_query_word_without_reordering_items() {
        let model = ActionFinderModel::build(
            &english_catalog(),
            &SettingsDocument::default(),
            ActionFinderContext::default(),
        );
        let filtered = model.filtered_items("search ctrl");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].action_id, "open_search");
    }
}
