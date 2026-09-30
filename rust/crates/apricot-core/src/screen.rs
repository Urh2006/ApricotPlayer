//! Stable screen and dialog catalog used by parity and accessibility tests.

use crate::navigation::Route;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScreenKind {
    Page,
    Dialog,
    NativeDialog,
    ProgressDialog,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimaryControlRole {
    List,
    Edit,
    Choice,
    Button,
    CheckboxList,
    SliderGroup,
    ReadOnlyText,
    Progress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ScreenDefinition {
    pub id: &'static str,
    pub label_key: &'static str,
    pub kind: ScreenKind,
    pub route: Option<Route>,
    pub primary_control: PrimaryControlRole,
    pub restores_focus: bool,
}

macro_rules! page {
    ($id:literal, $label:literal, $route:ident, $role:ident) => {
        ScreenDefinition {
            id: $id,
            label_key: $label,
            kind: ScreenKind::Page,
            route: Some(Route::$route),
            primary_control: PrimaryControlRole::$role,
            restores_focus: true,
        }
    };
}

macro_rules! dialog {
    ($id:literal, $label:literal, $role:ident) => {
        ScreenDefinition {
            id: $id,
            label_key: $label,
            kind: ScreenKind::Dialog,
            route: None,
            primary_control: PrimaryControlRole::$role,
            restores_focus: true,
        }
    };
    ($id:literal, $label:literal, $route:ident, $role:ident) => {
        ScreenDefinition {
            id: $id,
            label_key: $label,
            kind: ScreenKind::Dialog,
            route: Some(Route::$route),
            primary_control: PrimaryControlRole::$role,
            restores_focus: true,
        }
    };
}

macro_rules! native_dialog {
    ($id:literal, $label:literal, $role:ident) => {
        ScreenDefinition {
            id: $id,
            label_key: $label,
            kind: ScreenKind::NativeDialog,
            route: None,
            primary_control: PrimaryControlRole::$role,
            restores_focus: true,
        }
    };
}

macro_rules! progress_dialog {
    ($id:literal, $label:literal) => {
        ScreenDefinition {
            id: $id,
            label_key: $label,
            kind: ScreenKind::ProgressDialog,
            route: None,
            primary_control: PrimaryControlRole::Progress,
            restores_focus: true,
        }
    };
}

pub const SCREENS: &[ScreenDefinition] = &[
    page!("main_menu", "main_menu", MainMenu, List),
    page!("search", "search_youtube", Search, Edit),
    page!("results", "result_list", Results, List),
    page!("trending", "trending", Trending, List),
    page!("channel_results", "channel_videos", ChannelResults, List),
    page!(
        "playlist_results",
        "open_playlist_videos",
        PlaylistResults,
        List
    ),
    page!(
        "soundcloud_artist_tracks",
        "soundcloud",
        SoundcloudArtistTracks,
        List
    ),
    page!("direct_link", "direct_link", DirectLink, Edit),
    page!("local_folder", "play_folder", LocalFolder, List),
    page!("favorites", "favorites", Favorites, List),
    dialog!("bookmarks", "bookmarks", Bookmarks, List),
    page!("history", "history", History, List),
    page!("user_playlists", "playlists", UserPlaylists, List),
    page!(
        "user_playlist_items",
        "playlist_items",
        UserPlaylistItems,
        List
    ),
    page!("subscriptions", "subscriptions", Subscriptions, List),
    page!(
        "notification_center",
        "notification_center",
        NotificationCenter,
        List
    ),
    page!("rss_feeds", "rss_feeds", RssFeeds, List),
    page!("rss_items", "podcast_episode", RssItems, List),
    page!(
        "podcast_categories",
        "podcast_categories",
        PodcastCategories,
        List
    ),
    page!(
        "podcast_search_results",
        "podcast_search_results",
        PodcastSearchResults,
        List
    ),
    page!("audiovault_menu", "audiovault", AudiovaultMenu, List),
    page!(
        "audiovault_search",
        "search_audiovault",
        AudiovaultSearch,
        Edit
    ),
    page!("audiovault_results", "audiovault", AudiovaultResults, List),
    page!("audiovault_episodes", "episode", AudiovaultEpisodes, List),
    page!("spotify_hub", "spotify", SpotifyHub, List),
    page!(
        "spotify_accounts",
        "spotify_accounts",
        SpotifyAccounts,
        List
    ),
    page!("download_queue", "current_downloads", DownloadQueue, List),
    dialog!("playback_queue", "playback_queue", PlaybackQueue, List),
    page!("settings", "settings", Settings, List),
    page!("player", "player", Player, Button),
    native_dialog!("first_run_language", "language", Choice),
    native_dialog!("missing_audio_device", "audio_device_missing", Choice),
    dialog!("action_finder", "action_finder", Edit),
    dialog!("audiovault_login", "audiovault_login", Edit),
    dialog!("spotify_login", "spotify_login", ReadOnlyText),
    dialog!("file_converter", "file_converter", Edit),
    dialog!("folder_converter", "folder_converter", Edit),
    dialog!("equalizer", "equalizer", SliderGroup),
    dialog!("chapters", "chapters", List),
    dialog!("transcript", "transcript", ReadOnlyText),
    dialog!("lyrics", "lyrics", ReadOnlyText),
    dialog!("comments", "comments", List),
    dialog!("comment_details", "comment_details", ReadOnlyText),
    dialog!(
        "download_progress_details",
        "download_progress_details",
        ReadOnlyText
    ),
    native_dialog!("output_device_picker", "output_devices", Choice),
    native_dialog!("channel_options", "channel_options", Choice),
    native_dialog!("podcast_search_query", "search_podcasts", Edit),
    native_dialog!("download_format_picker", "select_download_format", Choice),
    native_dialog!("download_file_picker", "choose_save_path", Edit),
    native_dialog!("download_folder_picker", "choose_save_folder", Edit),
    native_dialog!("open_media_file_picker", "play_file", Edit),
    native_dialog!("open_media_folder_picker", "play_folder", Edit),
    native_dialog!("playlist_picker", "select_playlist", Choice),
    native_dialog!("playlist_name", "playlist_name", Edit),
    native_dialog!("subscription_category", "set_category", Edit),
    native_dialog!("category_filter", "filter_category", Choice),
    native_dialog!("rss_feed_url", "add_rss_feed", Edit),
    native_dialog!("rss_opml_file", "opml_files", Edit),
    native_dialog!("equalizer_profile_name", "equalizer_profile_name", Edit),
    native_dialog!("equalizer_profile_file", "equalizer_profile_file", Edit),
    native_dialog!(
        "equalizer_global_preset",
        "save_equalizer_as_global",
        Choice
    ),
    native_dialog!("bookmark_name", "bookmark_name_prompt", Edit),
    dialog!("update_available", "update_available_title", ReadOnlyText),
    progress_dialog!("application_update_progress", "update_progress_title"),
    progress_dialog!("download_progress", "download_progress"),
    progress_dialog!("conversion_progress", "conversion_progress_title"),
];

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::{SCREENS, ScreenKind};
    use crate::navigation::Route;

    #[test]
    fn screen_ids_are_unique_and_every_route_has_one_screen() {
        let ids: HashSet<_> = SCREENS.iter().map(|screen| screen.id).collect();
        assert_eq!(SCREENS.len(), 66);
        assert_eq!(ids.len(), SCREENS.len());

        let mut route_counts = HashMap::new();
        for route in SCREENS.iter().filter_map(|screen| screen.route) {
            *route_counts.entry(route).or_insert(0_usize) += 1;
        }
        for route in Route::ALL {
            assert_eq!(route_counts.get(route), Some(&1), "route {route:?}");
        }
    }

    #[test]
    fn every_temporary_surface_restores_focus() {
        for screen in SCREENS
            .iter()
            .filter(|screen| screen.kind != ScreenKind::Page)
        {
            assert!(screen.restores_focus, "screen {}", screen.id);
        }
    }
}
