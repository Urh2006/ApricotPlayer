//! Context-menu surface catalog, including all conditionally visible commands.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContextMenuDefinition {
    pub id: &'static str,
    pub screen_ids: &'static [&'static str],
    pub item_ids: &'static [&'static str],
}

pub const CONTEXT_MENUS: &[ContextMenuDefinition] = &[
    ContextMenuDefinition {
        id: "results",
        screen_ids: &[
            "results",
            "trending",
            "channel_results",
            "playlist_results",
            "soundcloud_artist_tracks",
            "local_folder",
        ],
        item_ids: &[
            "play",
            "play_playlist",
            "shuffle_playlist",
            "open_playlist_videos",
            "channel_options",
            "channel_videos",
            "channel_popular",
            "channel_playlists",
            "channel_live_streams",
            "download_audio",
            "download_video",
            "download_all_as_audio",
            "download_all_as_video",
            "download_collection",
            "add_favorite",
            "remove_favorite",
            "subscribe_channel",
            "unsubscribe_channel",
            "open_channel",
            "add_to_playlist",
            "add_to_playback_queue",
            "remove_from_playback_queue",
            "remove_from_playlist",
            "open_browser",
            "copy_stream_url",
            "copy_url",
            "copy_path",
        ],
    },
    ContextMenuDefinition {
        id: "player",
        screen_ids: &["player"],
        item_ids: &[
            "download_audio",
            "download_video",
            "add_favorite",
            "remove_favorite",
            "subscribe_channel",
            "unsubscribe_channel",
            "open_channel",
            "add_to_playlist",
            "add_to_playback_queue",
            "remove_from_playback_queue",
            "remove_from_playlist",
            "copy_link",
            "copy_path",
            "copy_stream_url",
            "copy_timestamp_link",
            "output_devices",
            "fullscreen",
            "equalizer",
            "audio_normalization",
            "save_podcast_speed_preset",
            "play_related_video",
            "add_bookmark",
            "bookmarks",
            "chapters",
            "transcript",
            "lyrics",
            "comments",
            "open_browser",
            "close_player",
        ],
    },
    ContextMenuDefinition {
        id: "user_playlists",
        screen_ids: &["user_playlists"],
        item_ids: &[
            "open_playlist",
            "create_playlist",
            "download_user_playlist",
            "remove_playlist",
        ],
    },
    ContextMenuDefinition {
        id: "user_playlist_items",
        screen_ids: &["user_playlist_items"],
        item_ids: &[
            "play",
            "download_audio",
            "download_video",
            "download_user_playlist",
            "add_to_playback_queue",
            "remove_from_playback_queue",
            "remove_from_playlist",
            "open_channel",
            "copy_url",
            "copy_path",
            "copy_stream_url",
        ],
    },
    ContextMenuDefinition {
        id: "favorites",
        screen_ids: &["favorites"],
        item_ids: &[
            "play",
            "download_audio",
            "download_video",
            "subscribe_channel",
            "unsubscribe_channel",
            "open_channel",
            "add_to_playlist",
            "add_to_playback_queue",
            "remove_from_playback_queue",
            "copy_stream_url",
            "copy_url",
            "copy_path",
            "remove",
        ],
    },
    ContextMenuDefinition {
        id: "history",
        screen_ids: &["history"],
        item_ids: &[
            "play",
            "download_audio",
            "download_video",
            "add_favorite",
            "subscribe_channel",
            "unsubscribe_channel",
            "open_channel",
            "add_to_playlist",
            "add_to_playback_queue",
            "remove_from_playback_queue",
            "copy_stream_url",
            "copy_url",
            "copy_path",
            "remove_history_item",
            "clear_history",
        ],
    },
    ContextMenuDefinition {
        id: "subscriptions",
        screen_ids: &["subscriptions"],
        item_ids: &[
            "subscription_open_videos",
            "subscription_new_videos_button",
            "subscription_check_now",
            "set_category",
            "filter_category",
            "copy_url",
            "unsubscribe_channel",
            "remove",
        ],
    },
    ContextMenuDefinition {
        id: "notifications",
        screen_ids: &["notification_center"],
        item_ids: &["play", "copy_url", "clear_notifications"],
    },
    ContextMenuDefinition {
        id: "download_queue",
        screen_ids: &["download_queue"],
        item_ids: &[
            "cancel_download",
            "cancel_all_downloads",
            "download_selected_queued",
            "download_audio",
            "download_video",
            "download_all_as_audio",
            "download_all_as_video",
            "remove_from_queue",
        ],
    },
    ContextMenuDefinition {
        id: "playback_queue",
        screen_ids: &["playback_queue"],
        item_ids: &[
            "play",
            "move_up",
            "move_down",
            "remove_from_playback_queue",
            "clear_playback_queue",
        ],
    },
    ContextMenuDefinition {
        id: "rss_feeds",
        screen_ids: &["rss_feeds"],
        item_ids: &[
            "open_feed",
            "download_feed",
            "podcast_speed_preset",
            "refresh_feed",
            "set_category",
            "filter_category",
            "copy_url",
            "remove_feed",
        ],
    },
    ContextMenuDefinition {
        id: "rss_items",
        screen_ids: &["rss_items"],
        item_ids: &[
            "play_episode",
            "toggle_podcast_played",
            "clear_podcast_progress",
            "download_episode_audio",
            "queue_episode_audio",
            "add_to_playlist",
            "add_to_playback_queue",
            "remove_from_playback_queue",
            "download_feed",
            "open_episode_page",
            "copy_url",
        ],
    },
    ContextMenuDefinition {
        id: "podcast_search_results",
        screen_ids: &["podcast_search_results"],
        item_ids: &["add_podcast", "open_browser", "copy_url"],
    },
    ContextMenuDefinition {
        id: "audiovault_results",
        screen_ids: &["audiovault_results", "audiovault_episodes"],
        item_ids: &["open", "download_audio", "download_tv_show"],
    },
    ContextMenuDefinition {
        id: "spotify_accounts",
        screen_ids: &["spotify_accounts"],
        item_ids: &[
            "spotify_use_account",
            "spotify_log_in",
            "spotify_log_in_again",
            "spotify_log_out",
            "spotify_remove_account",
        ],
    },
    ContextMenuDefinition {
        id: "spotify_browse",
        screen_ids: &["spotify_browse"],
        item_ids: &[
            "play",
            "open",
            "spotify_shuffle_play",
            "spotify_add_to_queue",
            "spotify_like",
            "spotify_save_library",
            "spotify_hide_song",
            "spotify_radio",
            "spotify_add_to_playlist",
            "move_up",
            "move_down",
            "spotify_remove_from_playlist",
            "spotify_rename_playlist",
            "spotify_create_playlist",
            "spotify_go_to_album",
            "spotify_go_to_artist",
            "copy_link",
        ],
    },
    ContextMenuDefinition {
        id: "bookmarks",
        screen_ids: &["bookmarks"],
        item_ids: &[
            "play",
            "add_bookmark",
            "rename_bookmark",
            "delete_bookmark",
            "copy_timestamp_link",
        ],
    },
    ContextMenuDefinition {
        id: "comments",
        screen_ids: &["comments"],
        item_ids: &[
            "open_comment",
            "copy_comment",
            "copy_visible_comments",
            "open_comment_author_channel",
            "load_more_comments",
        ],
    },
    ContextMenuDefinition {
        id: "system_tray",
        screen_ids: &[],
        item_ids: &[
            "tray_show",
            "tray_settings",
            "tray_check_subscriptions",
            "tray_exit",
        ],
    },
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::CONTEXT_MENUS;
    use crate::screen::SCREENS;

    #[test]
    fn context_menu_ids_and_items_are_stable_and_unique() {
        let menu_ids: HashSet<_> = CONTEXT_MENUS.iter().map(|menu| menu.id).collect();
        // 17 Python menus plus the Spotify account list and Spotify lists.
        assert_eq!(CONTEXT_MENUS.len(), 19);
        assert_eq!(menu_ids.len(), CONTEXT_MENUS.len());
        for menu in CONTEXT_MENUS {
            let item_ids: HashSet<_> = menu.item_ids.iter().copied().collect();
            assert!(!menu.item_ids.is_empty(), "menu {}", menu.id);
            assert_eq!(item_ids.len(), menu.item_ids.len(), "menu {}", menu.id);
        }
    }

    #[test]
    fn context_menus_only_reference_registered_screens() {
        let screen_ids: HashSet<_> = SCREENS.iter().map(|screen| screen.id).collect();
        for menu in CONTEXT_MENUS {
            for screen_id in menu.screen_ids {
                assert!(screen_ids.contains(screen_id), "menu {}", menu.id);
            }
        }
    }
}
