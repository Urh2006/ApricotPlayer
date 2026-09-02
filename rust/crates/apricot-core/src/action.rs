//! Canonical action and shortcut catalog.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ActionId(pub &'static str);

impl ActionId {
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionScope {
    Global,
    List,
    Player,
    Dialog,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepeatPolicy {
    None,
    Controlled,
    Native,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionDefinition {
    pub id: ActionId,
    pub label_key: &'static str,
    pub default_windows_shortcut: &'static str,
    pub scopes: &'static [ActionScope],
    pub repeat: RepeatPolicy,
}

const GLOBAL: &[ActionScope] = &[ActionScope::Global];
const LIST: &[ActionScope] = &[ActionScope::List];
const PLAYER: &[ActionScope] = &[ActionScope::Player];
const LIST_PLAYER: &[ActionScope] = &[ActionScope::List, ActionScope::Player];
const LIST_PLAYER_DIALOG: &[ActionScope] =
    &[ActionScope::List, ActionScope::Player, ActionScope::Dialog];

macro_rules! action {
    ($id:literal, $shortcut:literal, $scopes:expr) => {
        ActionDefinition {
            id: ActionId($id),
            label_key: concat!("shortcut_", $id),
            default_windows_shortcut: $shortcut,
            scopes: $scopes,
            repeat: RepeatPolicy::None,
        }
    };
    ($id:literal, $shortcut:literal, $scopes:expr, $repeat:ident) => {
        ActionDefinition {
            id: ActionId($id),
            label_key: concat!("shortcut_", $id),
            default_windows_shortcut: $shortcut,
            scopes: $scopes,
            repeat: RepeatPolicy::$repeat,
        }
    };
}

pub const ACTIONS: &[ActionDefinition] = &[
    action!("open_main_menu", "Ctrl+Alt+M", GLOBAL),
    action!("open_search", "Ctrl+Alt+Y", GLOBAL),
    action!("open_audiovault", "Ctrl+Alt+A", GLOBAL),
    action!("open_play_from_folder", "Ctrl+Alt+O", GLOBAL),
    action!("open_play_file", "Ctrl+Alt+I", GLOBAL),
    action!("open_direct_link", "Ctrl+Alt+L", GLOBAL),
    action!("open_favorites", "Ctrl+Alt+F", GLOBAL),
    action!("open_bookmarks", "Ctrl+Alt+K", GLOBAL),
    action!("open_playlists", "Ctrl+Alt+P", GLOBAL),
    action!("open_subscriptions", "Ctrl+Alt+B", GLOBAL),
    action!("open_current_downloads", "Ctrl+Alt+D", GLOBAL),
    action!("open_history", "Ctrl+Alt+H", GLOBAL),
    action!("open_podcasts_rss", "Ctrl+Alt+R", GLOBAL),
    action!("open_settings", "Ctrl+Alt+S", GLOBAL),
    action!("open_action_finder", "Ctrl+Shift+J", GLOBAL),
    action!("background_play_pause", "Ctrl+Space", GLOBAL),
    action!("copy_diagnostic_report", "Ctrl+Alt+Shift+D", GLOBAL),
    action!("download_audio", "Ctrl+Shift+A", LIST_PLAYER),
    action!("download_video", "Ctrl+Shift+D", LIST_PLAYER),
    action!("subscribe_channel", "Ctrl+Shift+S", LIST),
    action!("unsubscribe_channel", "Ctrl+Shift+U", LIST),
    action!("open_channel", "Ctrl+Shift+O", LIST_PLAYER),
    action!("queue_audio", "Shift+A", LIST),
    action!("result_column_previous", "Ctrl+Alt+Left", LIST),
    action!("result_column_next", "Ctrl+Alt+Right", LIST),
    action!("add_to_playback_queue", "Ctrl+Shift+Q", LIST_PLAYER),
    action!(
        "remove_from_playback_queue",
        "Ctrl+Shift+Delete",
        LIST_PLAYER
    ),
    action!("open_playback_queue", "Ctrl+Alt+Q", GLOBAL),
    action!("create_playlist", "Ctrl+Shift+N", LIST),
    action!("add_favorite", "Ctrl+F", LIST_PLAYER),
    action!("remove_favorite", "Ctrl+Shift+F", LIST_PLAYER),
    action!("add_to_playlist", "Ctrl+P", LIST_PLAYER),
    action!("remove_from_playlist", "Ctrl+Shift+P", LIST_PLAYER),
    action!("copy_link", "Ctrl+L", LIST),
    action!("copy_stream_url", "Ctrl+D", LIST_PLAYER),
    action!("context_menu", "Applications", LIST_PLAYER_DIALOG),
    action!("open_selected", "Enter", LIST_PLAYER_DIALOG),
    action!("new_subscription_videos", "Ctrl+Shift+V", GLOBAL),
    action!("remove_selected", "Delete", LIST_PLAYER_DIALOG),
    action!("toggle_podcast_played", "Ctrl+Shift+X", LIST),
    action!("clear_podcast_progress", "Ctrl+Shift+R", LIST),
    action!("save_podcast_speed_preset", "Ctrl+Shift+E", LIST_PLAYER),
    action!("player_copy_link", "L", PLAYER),
    action!("player_copy_timestamp_link", "Ctrl+Shift+L", PLAYER),
    action!("player_play_pause", "Space", PLAYER),
    action!("player_time", "T", PLAYER),
    action!("player_bpm", "B", PLAYER),
    action!("player_speed_down", "S", PLAYER, Controlled),
    action!("player_speed_up", "D", PLAYER, Controlled),
    action!("player_reset_speed_pitch", "Ctrl+0", PLAYER),
    action!("player_pitch_up", "Ctrl+Up", PLAYER, Controlled),
    action!("player_pitch_down", "Ctrl+Down", PLAYER, Controlled),
    action!("player_volume_status", "V", PLAYER),
    action!("player_format_status", "F", PLAYER),
    action!("player_details", "F7", PLAYER),
    action!("player_output_devices", "O", PLAYER),
    action!("player_equalizer", "F4", PLAYER),
    action!("player_fullscreen", "F11", PLAYER),
    action!("player_replaygain", "Ctrl+Shift+G", PLAYER),
    action!("player_add_bookmark", "Ctrl+Shift+B", PLAYER),
    action!("player_bookmarks", "Ctrl+Shift+K", PLAYER),
    action!("player_chapters", "Ctrl+Shift+C", PLAYER),
    action!("player_transcript", "Ctrl+Shift+T", PLAYER),
    action!("player_lyrics", "Ctrl+Shift+Y", PLAYER),
    action!("player_comments", "Ctrl+Shift+M", PLAYER),
    action!("player_previous_chapter", "Alt+Left", PLAYER),
    action!("player_next_chapter", "Alt+Right", PLAYER),
    action!("player_edit_mode", "E", PLAYER),
    action!("player_save_edit_copy", "Ctrl+S", PLAYER),
    action!("player_replace_edit_original", "Ctrl+R", PLAYER),
    action!("player_marker_start", "LeftBracket", PLAYER),
    action!("player_marker_end", "RightBracket", PLAYER),
    action!("player_preview_marked_clip", "P", PLAYER),
    action!("player_previous", "Ctrl+PageUp", PLAYER),
    action!("player_next", "Ctrl+PageDown", PLAYER),
    action!("player_next_related", "Ctrl+Shift+PageDown", PLAYER),
    action!("player_back", "Escape", PLAYER),
    action!("player_volume_boost", "F2", PLAYER),
    action!("player_bass_boost", "F3", PLAYER),
    action!("player_repeat", "R", PLAYER),
    action!("player_shuffle", "Shift+S", PLAYER),
    action!("player_seek_back", "Left", PLAYER, Controlled),
    action!("player_seek_forward", "Right", PLAYER, Controlled),
    action!("player_seek_back_large", "Ctrl+Left", PLAYER, Controlled),
    action!(
        "player_seek_forward_large",
        "Ctrl+Right",
        PLAYER,
        Controlled
    ),
    action!(
        "player_seek_back_huge",
        "Ctrl+Shift+Left",
        PLAYER,
        Controlled
    ),
    action!(
        "player_seek_forward_huge",
        "Ctrl+Shift+Right",
        PLAYER,
        Controlled
    ),
    action!("player_seek_start", "Ctrl+Home", PLAYER),
    action!("player_seek_end", "Ctrl+End", PLAYER),
    action!("player_volume_up", "Up", PLAYER, Native),
    action!("player_volume_down", "Down", PLAYER, Native),
];

pub fn action_by_id(id: &str) -> Option<&'static ActionDefinition> {
    ACTIONS
        .iter()
        .find(|definition| definition.id.as_str() == id)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{ACTIONS, ActionScope, RepeatPolicy, action_by_id};

    #[test]
    fn baseline_contains_91_unique_actions() {
        let ids: HashSet<_> = ACTIONS.iter().map(|action| action.id.as_str()).collect();
        assert_eq!(ACTIONS.len(), 91);
        assert_eq!(ids.len(), ACTIONS.len());
    }

    #[test]
    fn all_actions_have_labels_shortcuts_and_scopes() {
        for action in ACTIONS {
            assert!(
                !action.label_key.is_empty(),
                "missing label for {:?}",
                action.id
            );
            assert!(
                !action.default_windows_shortcut.is_empty(),
                "missing shortcut for {:?}",
                action.id
            );
            assert!(
                !action.scopes.is_empty(),
                "missing scope for {:?}",
                action.id
            );
        }
    }

    #[test]
    fn player_holds_are_explicit_and_scoped() {
        let pitch = action_by_id("player_pitch_up").expect("pitch action");
        assert_eq!(pitch.repeat, RepeatPolicy::Controlled);
        assert_eq!(pitch.scopes, &[ActionScope::Player]);

        let volume = action_by_id("player_volume_up").expect("volume action");
        assert_eq!(volume.repeat, RepeatPolicy::Native);
    }

    #[test]
    fn notification_center_shortcut_is_global_like_python() {
        let action = action_by_id("new_subscription_videos").expect("notification action");
        assert_eq!(action.scopes, &[ActionScope::Global]);
    }
}
