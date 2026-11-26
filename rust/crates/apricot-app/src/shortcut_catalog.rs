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

/// The presentation category of every action. Categories never change an
/// action's activation scope or its saved identity.
const ACTION_CATEGORIES: &[(&str, &str)] = &[
    ("open_main_menu", "general"),
    ("open_action_finder", "general"),
    ("context_menu", "general"),
    ("open_selected", "general"),
    ("remove_selected", "general"),
    ("result_column_previous", "general"),
    ("result_column_next", "general"),
    ("open_favorites", "general"),
    ("add_favorite", "general"),
    ("remove_favorite", "general"),
    ("open_history", "general"),
    ("copy_link", "general"),
    ("copy_stream_url", "general"),
    ("open_audiovault", "general"),
    ("background_play_pause", "playback"),
    ("player_play_pause", "playback"),
    ("player_previous", "playback"),
    ("player_next", "playback"),
    ("player_back", "playback"),
    ("player_repeat", "playback"),
    ("player_shuffle", "playback"),
    ("player_time", "playback"),
    ("player_volume_status", "playback"),
    ("player_format_status", "playback"),
    ("player_details", "playback"),
    ("player_output_devices", "playback"),
    ("player_fullscreen", "playback"),
    ("player_copy_link", "playback"),
    ("player_copy_timestamp_link", "playback"),
    ("player_bpm", "playback"),
    ("player_lyrics", "playback"),
    ("player_transcript", "playback"),
    ("player_comments", "playback"),
    ("open_sleep_timer", "playback"),
    ("cancel_sleep_timer", "playback"),
    ("sleep_timer_remaining", "playback"),
    ("player_audio_tracks", "playback"),
    ("player_audio_track_next", "playback"),
    ("player_audio_track_previous", "playback"),
    ("player_subtitles", "playback"),
    ("player_subtitle_file", "playback"),
    ("player_subtitle_url", "playback"),
    ("player_elapsed", "playback"),
    ("player_remaining", "playback"),
    ("player_duration", "playback"),
    ("player_percent", "playback"),
    ("player_title", "playback"),
    ("player_artist", "playback"),
    ("player_seek_back", "seeking"),
    ("player_seek_forward", "seeking"),
    ("player_seek_back_large", "seeking"),
    ("player_seek_forward_large", "seeking"),
    ("player_seek_back_huge", "seeking"),
    ("player_seek_forward_huge", "seeking"),
    ("player_seek_start", "seeking"),
    ("player_seek_end", "seeking"),
    ("player_go_to_time", "seeking"),
    ("player_go_to_percent", "seeking"),
    ("player_go_to_item", "seeking"),
    ("player_first_item", "playback"),
    ("player_last_item", "playback"),
    ("player_list_info", "playback"),
    ("player_slot_set_1", "bookmarks"),
    ("player_slot_set_2", "bookmarks"),
    ("player_slot_set_3", "bookmarks"),
    ("player_slot_set_4", "bookmarks"),
    ("player_slot_set_5", "bookmarks"),
    ("player_slot_set_6", "bookmarks"),
    ("player_slot_set_7", "bookmarks"),
    ("player_slot_set_8", "bookmarks"),
    ("player_slot_set_9", "bookmarks"),
    ("player_slot_set_0", "bookmarks"),
    ("player_slot_go_1", "bookmarks"),
    ("player_slot_go_2", "bookmarks"),
    ("player_slot_go_3", "bookmarks"),
    ("player_slot_go_4", "bookmarks"),
    ("player_slot_go_5", "bookmarks"),
    ("player_slot_go_6", "bookmarks"),
    ("player_slot_go_7", "bookmarks"),
    ("player_slot_go_8", "bookmarks"),
    ("player_slot_go_9", "bookmarks"),
    ("player_slot_go_0", "bookmarks"),
    ("player_volume_up", "audio"),
    ("player_volume_down", "audio"),
    ("player_volume_boost", "audio"),
    ("player_bass_boost", "audio"),
    ("player_equalizer", "audio"),
    ("player_replaygain", "audio"),
    ("player_speed_down", "audio"),
    ("player_speed_up", "audio"),
    ("player_pitch_up", "audio"),
    ("player_pitch_down", "audio"),
    ("player_reset_speed_pitch", "audio"),
    ("player_edit_mode", "audio"),
    ("player_save_edit_copy", "audio"),
    ("player_replace_edit_original", "audio"),
    ("player_audio_processing", "audio"),
    ("player_pan_left", "audio"),
    ("player_pan_right", "audio"),
    ("player_pan_status", "audio"),
    ("player_spatial_rotate_left", "audio"),
    ("player_spatial_rotate_right", "audio"),
    ("player_spatial_up", "audio"),
    ("player_spatial_down", "audio"),
    ("open_bookmarks", "bookmarks"),
    ("player_add_bookmark", "bookmarks"),
    ("player_bookmarks", "bookmarks"),
    ("player_chapters", "bookmarks"),
    ("player_previous_chapter", "bookmarks"),
    ("player_next_chapter", "bookmarks"),
    ("player_marker_start", "bookmarks"),
    ("player_marker_end", "bookmarks"),
    ("player_preview_marked_clip", "bookmarks"),
    ("player_ab_loop", "bookmarks"),
    ("player_ranges", "bookmarks"),
    ("player_range_previous", "bookmarks"),
    ("player_range_next", "bookmarks"),
    ("player_audiobook_mode", "bookmarks"),
    ("player_chapter_stop", "bookmarks"),
    ("bookmarks_import", "bookmarks"),
    ("bookmarks_export", "bookmarks"),
    ("open_play_from_folder", "files"),
    ("open_play_file", "files"),
    ("open_direct_link", "files"),
    ("open_playlists", "files"),
    ("create_playlist", "files"),
    ("add_to_playlist", "files"),
    ("remove_from_playlist", "files"),
    ("add_to_playback_queue", "files"),
    ("remove_from_playback_queue", "files"),
    ("open_playback_queue", "files"),
    ("playlist_save_current", "files"),
    ("playlist_edit", "files"),
    ("delete_file_permanently", "files"),
    ("manage_files", "files"),
    ("converter_options", "files"),
    ("cancel_conversion", "files"),
    ("download_with_options", "downloads"),
    ("search_suggestions", "youtube"),
    ("open_search", "youtube"),
    ("open_channel", "youtube"),
    ("player_next_related", "youtube"),
    ("open_spotify", "spotify"),
    ("open_my_youtube", "youtube"),
    ("spotify_accounts", "spotify"),
    ("spotify_search", "spotify"),
    ("spotify_library", "spotify"),
    ("spotify_liked_songs", "spotify"),
    ("spotify_playlists", "spotify"),
    ("spotify_queue", "spotify"),
    ("spotify_devices", "spotify"),
    ("spotify_toggle_saved", "spotify"),
    ("spotify_dislike", "spotify"),
    ("spotify_daily_mixes", "spotify"),
    ("spotify_radio", "spotify"),
    ("spotify_first_track", "spotify"),
    ("spotify_last_track", "spotify"),
    ("spotify_subscriptions", "spotify"),
    ("spotify_subscribe_playlist", "spotify"),
    ("spotify_unsubscribe_playlist", "spotify"),
    ("spotify_check_subscriptions", "spotify"),
    ("open_podcasts_rss", "podcasts"),
    ("open_subscriptions", "podcasts"),
    ("new_subscription_videos", "podcasts"),
    ("subscribe_channel", "podcasts"),
    ("unsubscribe_channel", "podcasts"),
    ("toggle_podcast_played", "podcasts"),
    ("clear_podcast_progress", "podcasts"),
    ("save_podcast_speed_preset", "podcasts"),
    ("open_radio", "radio_iptv"),
    ("open_iptv", "radio_iptv"),
    ("radio_now_playing", "radio_iptv"),
    ("open_recording", "recording"),
    ("recording_start", "recording"),
    ("recording_pause", "recording"),
    ("recording_stop", "recording"),
    ("open_recordings_folder", "recording"),
    ("download_audio", "downloads"),
    ("download_video", "downloads"),
    ("queue_audio", "downloads"),
    ("open_current_downloads", "downloads"),
    ("open_settings", "system"),
    ("copy_diagnostic_report", "system"),
    ("open_integrations", "system"),
    ("settings_import", "system"),
    ("settings_export", "system"),
    ("player_seek_amount_1", "seeking"),
    ("player_seek_amount_2", "seeking"),
    ("player_seek_amount_3", "seeking"),
    ("player_seek_amount_4", "seeking"),
    ("player_seek_amount_5", "seeking"),
    ("player_seek_amount_6", "seeking"),
    ("player_seek_amount_7", "seeking"),
    ("player_seek_amount_8", "seeking"),
    ("player_seek_amount_9", "seeking"),
    ("player_seek_amount_0", "seeking"),
    ("player_seek_amount_custom", "seeking"),
    ("player_jump_10", "seeking"),
    ("player_jump_20", "seeking"),
    ("player_jump_30", "seeking"),
    ("player_jump_40", "seeking"),
    ("player_jump_50", "seeking"),
    ("player_jump_60", "seeking"),
    ("player_jump_70", "seeking"),
    ("player_jump_80", "seeking"),
    ("player_jump_90", "seeking"),
    ("player_jump_15", "seeking"),
    ("player_jump_25", "seeking"),
    ("player_jump_35", "seeking"),
    ("player_jump_45", "seeking"),
    ("player_jump_55", "seeking"),
    ("player_jump_65", "seeking"),
    ("player_jump_75", "seeking"),
    ("player_jump_85", "seeking"),
    ("player_jump_95", "seeking"),
    ("player_silence_toggle", "audio"),
    ("player_spatial_toggle", "audio"),
    ("open_global_shortcuts", "system"),
    ("player_volume_max", "audio"),
    ("player_volume_min", "audio"),
    ("player_name_path", "playback"),
    ("player_opened_files", "playback"),
    ("open_recents", "files"),
    ("open_from_clipboard", "files"),
    ("copy_file_to_clipboard", "files"),
    ("rename_file", "files"),
    ("show_in_folder", "files"),
    ("file_properties", "files"),
    ("open_in_browser", "youtube"),
    ("open_channel_in_browser", "youtube"),
    ("player_mark_file", "files"),
    ("player_marked_count", "files"),
    ("player_manage_marked", "files"),
    ("toggle_announcement_verbosity", "system"),
];

/// The category an action is listed under; unknown ids fall back to General.
#[must_use]
pub fn action_category(id: &str) -> &'static str {
    ACTION_CATEGORIES
        .iter()
        .find(|(action, _)| *action == id)
        .map_or("general", |(_, category)| category)
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
            unassigned: catalog.text("shortcut_none").to_owned(),
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
    fn every_action_has_exactly_one_known_category() {
        for action in ACTIONS {
            let id = action.id.as_str();
            let rows: Vec<_> = ACTION_CATEGORIES
                .iter()
                .filter(|(action, _)| *action == id)
                .collect();
            assert_eq!(rows.len(), 1, "{id}");
            assert!(
                SHORTCUT_CATEGORIES
                    .iter()
                    .any(|category| category.id == rows[0].1 && category.id != "all"),
                "{id}"
            );
        }
        assert_eq!(ACTION_CATEGORIES.len(), ACTIONS.len());
    }

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
