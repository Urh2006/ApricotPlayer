//! Context menus for lists and the player, matching Python's
//! `apricot/ui/menus.py` and `apricot/library/library.py` item for item.
//!
//! The model is platform neutral: the UI turns entries into native menus and
//! runs the chosen [`ContextCommand`]. Labels follow Python's
//! `menu_label_with_shortcut`, which appends the configured shortcut after a
//! tab so screen readers announce it as the menu item's shortcut.

use apricot_core::{MediaItem, MediaKind, TranslationCatalog, action::action_by_id};
use apricot_storage::SettingsDocument;

/// What a context-menu entry does when chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextCommand {
    /// Python `play_selected`, `play_favorite`, `play_history_item`.
    Play,
    DownloadAudio,
    DownloadVideo,
    /// Python `download_all_queued(True)`.
    DownloadAllAudio,
    /// Python `download_all_queued(False)`.
    DownloadAllVideo,
    AddFavorite,
    /// Removes the active item from favorites wherever it is shown.
    RemoveFavorite,
    SubscribeChannel,
    UnsubscribeChannel,
    OpenChannel,
    AddToPlaybackQueue,
    RemoveFromPlaybackQueue,
    RemoveFromPlaylist,
    OpenInBrowser,
    CopyStreamUrl,
    /// Copies the item URL, or its path for local media.
    CopyLocation,
    CopyTimestampLink,
    ChannelOptions,
    ChannelVideos,
    ChannelPopular,
    ChannelPlaylists,
    ChannelStreams,
    PlayPlaylist,
    ShufflePlaylist,
    OpenPlaylistVideos,
    /// Python `add_active_to_playlist`: creates, uses the only playlist or asks.
    AddToPlaylist,
    /// Adds the item to the user playlist at this index.
    AddToUserPlaylist(usize),
    /// Removes the selected favorite or history row.
    RemoveSelected,
    ClearHistory,
    OpenUserPlaylist,
    CreatePlaylist,
    DownloadUserPlaylist,
    RemoveUserPlaylist,
    /// A player action routed through the shortcut dispatcher.
    PlayerAction(&'static str),
    ClosePlayer,
    /// Spotify account list (`docs/SPOTIFY_PLAN.md` 9.1).
    SpotifyUseAccount,
    SpotifyLogIn,
    SpotifyLogOut,
    SpotifyRemoveAccount,
    /// Spotify lists (`docs/SPOTIFY_PLAN.md` 5.1).
    SpotifyPlay,
    SpotifyOpen,
    SpotifyShufflePlay,
    SpotifyAddToQueue,
    SpotifyGoToAlbum,
    SpotifyGoToArtist,
    SpotifyCopyLink,
    SpotifyToggleSaved,
    SpotifyAddToPlaylist,
    SpotifyRemoveFromPlaylist,
    SpotifyMoveUp,
    SpotifyMoveDown,
    SpotifyCreatePlaylist,
    SpotifyRenamePlaylist,
    SpotifyHide,
    SpotifyRadio,
}

/// One row of a context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMenuEntry {
    Command {
        label: String,
        command: ContextCommand,
    },
    Submenu {
        label: String,
        entries: Vec<ContextMenuEntry>,
    },
    Disabled {
        label: String,
    },
}

impl ContextMenuEntry {
    /// Text shown for the entry, including the shortcut after a tab.
    pub fn label(&self) -> &str {
        match self {
            Self::Command { label, .. }
            | Self::Submenu { label, .. }
            | Self::Disabled { label } => label,
        }
    }
}

/// Application state a context menu depends on.
pub struct ContextMenuContext<'a> {
    pub catalog: &'a TranslationCatalog,
    pub settings: &'a SettingsDocument,
    /// Titles of the user's playlists, in order.
    pub user_playlist_titles: &'a [String],
    /// Downloads waiting in the download queue.
    pub queued_download_count: usize,
}

impl ContextMenuContext<'_> {
    fn text(&self, key: &str) -> String {
        self.catalog.text(key).to_owned()
    }

    fn command(&self, key: &str, command: ContextCommand) -> ContextMenuEntry {
        ContextMenuEntry::Command {
            label: self.text(key),
            command,
        }
    }

    fn command_with_shortcut(
        &self,
        key: &str,
        action_id: &str,
        command: ContextCommand,
    ) -> ContextMenuEntry {
        ContextMenuEntry::Command {
            label: self.label_with_shortcut(key, action_id),
            command,
        }
    }

    /// An entry whose label key and shortcut action have the same name.
    fn shortcut(&self, key: &str, command: ContextCommand) -> ContextMenuEntry {
        self.command_with_shortcut(key, key, command)
    }

    /// Python `menu_label_with_shortcut`.
    fn label_with_shortcut(&self, key: &str, action_id: &str) -> String {
        let label = self.text(key);
        if !self.settings.show_shortcuts_in_labels {
            return label;
        }
        let shortcut = shortcut_for(self.settings, action_id);
        if shortcut.is_empty() {
            label
        } else {
            format!("{label}\t{shortcut}")
        }
    }

    /// Python `append_add_to_playlist_menu`.
    fn add_to_playlist_entry(&self) -> ContextMenuEntry {
        let label = self.label_with_shortcut("add_to_playlist", "add_to_playlist");
        if self.user_playlist_titles.is_empty() {
            return ContextMenuEntry::Command {
                label,
                command: ContextCommand::AddToPlaylist,
            };
        }
        let mut entries = self
            .user_playlist_titles
            .iter()
            .enumerate()
            .map(|(index, title)| ContextMenuEntry::Command {
                label: if title.is_empty() {
                    self.text("playlists")
                } else {
                    title.clone()
                },
                command: ContextCommand::AddToUserPlaylist(index),
            })
            .collect::<Vec<_>>();
        // Python binds this entry to `add_active_to_playlist`, which only
        // creates a playlist when none exists.
        entries.push(self.command("create_playlist", ContextCommand::AddToPlaylist));
        ContextMenuEntry::Submenu { label, entries }
    }

    /// Python `append_collection_download_submenu`.
    fn collection_download_entry(&self, item: &MediaItem) -> ContextMenuEntry {
        ContextMenuEntry::Submenu {
            label: self.text(if item.kind == MediaKind::Channel {
                "download_channel"
            } else {
                "download_playlist"
            }),
            entries: vec![
                self.command_with_shortcut(
                    "download_audio",
                    "download_audio",
                    ContextCommand::DownloadAudio,
                ),
                self.command_with_shortcut(
                    "download_video",
                    "download_video",
                    ContextCommand::DownloadVideo,
                ),
            ],
        }
    }
}

/// Python `shortcut_for`: the configured shortcut, or the default when unset.
pub(crate) fn shortcut_for(settings: &SettingsDocument, action_id: &str) -> String {
    settings
        .keyboard_shortcuts
        .get(action_id)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .or_else(|| action_by_id(action_id).map(|action| action.default_windows_shortcut))
        .unwrap_or_default()
        .to_owned()
}

/// Python `item_has_openable_youtube_channel`.
pub fn has_openable_youtube_channel(item: &MediaItem) -> bool {
    youtube_channel_item_for_video(item).is_some()
}

/// Python `youtube_channel_item_for_video`: the channel that uploaded a video,
/// as a channel item that "Open channel" can open.
pub fn youtube_channel_item_for_video(item: &MediaItem) -> Option<MediaItem> {
    if item.is_local_media()
        || matches!(
            item.kind,
            MediaKind::Channel
                | MediaKind::Playlist
                | MediaKind::PodcastFeed
                | MediaKind::PodcastEpisode
        )
    {
        return None;
    }
    let url = channel_url(item).filter(|url| url.to_lowercase().contains("youtube.com"))?;
    let title = [
        Some(item.channel.trim().to_owned()),
        metadata_text(item, "uploader"),
        metadata_text(item, "channel_id"),
    ]
    .into_iter()
    .flatten()
    .find(|value| !value.is_empty())
    .unwrap_or_else(|| url.clone());
    let parsed = url::Url::parse(&url).ok()?;
    Some(MediaItem {
        id: apricot_core::MediaId(url.clone()),
        source: apricot_core::MediaSource::Youtube,
        kind: MediaKind::Channel,
        title: title.clone(),
        url: Some(parsed),
        stream_url: None,
        external_audio_url: None,
        local_path: None,
        channel: title,
        duration_seconds: None,
        metadata: [("channel_url".to_owned(), serde_json::Value::String(url))]
            .into_iter()
            .collect(),
    })
}

/// Python `normalize_channel_url`.
fn channel_url(item: &MediaItem) -> Option<String> {
    for key in ["channel_url", "uploader_url"] {
        if let Some(value) = metadata_text(item, key) {
            return Some(if value.starts_with("http") {
                value
            } else {
                format!("https://www.youtube.com/{}", value.trim_start_matches('/'))
            });
        }
    }
    metadata_text(item, "channel_id")
        .or_else(|| metadata_text(item, "uploader_id"))
        .filter(|id| id.starts_with("UC"))
        .map(|id| format!("https://www.youtube.com/channel/{id}"))
}

fn metadata_text(item: &MediaItem, key: &str) -> Option<String> {
    item.metadata
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Python `open_selected_in_browser`: the item's `webpage_url` or `url`, when
/// it passes `validate_remote_http_url` (HTTP or HTTPS with a host).
pub fn browser_url(item: &MediaItem) -> Option<String> {
    let url = metadata_text(item, "webpage_url")
        .or_else(|| item.url.as_ref().map(ToString::to_string))?;
    let parsed = url::Url::parse(&url).ok()?;
    (matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()).then_some(url)
}

/// Python `is_youtube_url` on the item's `url` or `webpage_url`.
pub(crate) fn has_youtube_url(item: &MediaItem) -> bool {
    let url = item
        .url
        .as_ref()
        .map(ToString::to_string)
        .or_else(|| metadata_text(item, "webpage_url"))
        .unwrap_or_default();
    let host = url::Url::parse(&url)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.trim_end_matches('.').to_lowercase())
        })
        .unwrap_or_default();
    ["youtube.com", "youtu.be"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

/// Python `playlist_item_is_supported`.
fn playlist_item_is_supported(item: &MediaItem) -> bool {
    let has_url = item.url.is_some()
        || item
            .local_path
            .as_deref()
            .is_some_and(|path| !path.trim().is_empty());
    has_url
        && !matches!(
            item.kind,
            MediaKind::Channel | MediaKind::Playlist | MediaKind::PodcastFeed
        )
}

/// Python `open_audiovault_context_menu`: Open, then Download audio, or
/// Download TV show for a show.
pub fn audiovault_results_context_menu(
    context: &ContextMenuContext<'_>,
    item: &MediaItem,
) -> Vec<ContextMenuEntry> {
    let download = if item.kind == MediaKind::TvShow {
        context.command("download_tv_show", ContextCommand::DownloadAudio)
    } else {
        context.shortcut("download_audio", ContextCommand::DownloadAudio)
    };
    vec![context.command("open", ContextCommand::Play), download]
}

/// The Spotify account list: the selected account, or the Add account row
/// (`account` is `None`) with only the login.
pub fn spotify_accounts_context_menu(
    context: &ContextMenuContext<'_>,
    account: Option<(bool, bool)>,
) -> Vec<ContextMenuEntry> {
    use ContextCommand as C;
    let Some((logged_in, active)) = account else {
        return vec![context.command("spotify_log_in", C::SpotifyLogIn)];
    };
    let mut entries = Vec::new();
    if logged_in && !active {
        entries.push(context.command("spotify_use_account", C::SpotifyUseAccount));
    }
    if logged_in {
        entries.push(context.command("spotify_log_out", C::SpotifyLogOut));
    } else {
        entries.push(context.command("spotify_log_in_again", C::SpotifyLogIn));
    }
    entries.push(context.command_with_shortcut(
        "spotify_remove_account",
        "remove_selected",
        C::SpotifyRemoveAccount,
    ));
    entries
}

/// What the selected row of a Spotify list offers.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SpotifyRowMenu {
    /// A track or episode that can play.
    pub playable_item: bool,
    /// An album, playlist, artist, show, folder or Liked Songs.
    pub collection: bool,
    /// The collection plays as a whole (not a folder).
    pub collection_plays: bool,
    pub album: bool,
    pub artist: bool,
    pub link: bool,
    /// Liked (track) or saved (collection) state, when known.
    pub saved: Option<bool>,
    /// An artist: saving is following.
    pub is_artist: bool,
    /// The open list is a playlist the account may edit.
    pub in_editable_playlist: bool,
    pub move_up: bool,
    pub move_down: bool,
    /// A playlist row the account may rename.
    pub rename: bool,
    /// The open list is the library or its playlists.
    pub create_playlist: bool,
    /// A track in a personal mix: Spotify's "Hide song" applies.
    pub hide: bool,
    pub hidden: bool,
    /// A track, artist, album or playlist Spotify makes a radio for.
    pub radio: bool,
}

/// Spotify lists: Play or Open first, then Shuffle play, Add to Spotify
/// queue, Go to album, Go to artist and Copy link where they apply.
pub fn spotify_browse_context_menu(
    context: &ContextMenuContext<'_>,
    menu: SpotifyRowMenu,
) -> Vec<ContextMenuEntry> {
    use ContextCommand as C;
    let mut entries = Vec::new();
    if menu.playable_item {
        entries.push(context.command("play", C::SpotifyPlay));
        entries.push(context.command_with_shortcut(
            "spotify_add_to_queue",
            "add_to_playback_queue",
            C::SpotifyAddToQueue,
        ));
    }
    if menu.collection {
        entries.push(context.command("open", C::SpotifyOpen));
        if menu.collection_plays {
            entries.push(context.command("play", C::SpotifyPlay));
            entries.push(context.command("spotify_shuffle_play", C::SpotifyShufflePlay));
        }
    }
    if menu.playable_item {
        let key = if menu.saved == Some(true) {
            "spotify_unlike"
        } else {
            "spotify_like"
        };
        entries.push(context.command_with_shortcut(
            key,
            "spotify_toggle_saved",
            C::SpotifyToggleSaved,
        ));
        entries.push(context.command("spotify_add_to_playlist", C::SpotifyAddToPlaylist));
    }
    if menu.hide {
        let key = if menu.hidden {
            "spotify_unhide_song"
        } else {
            "spotify_hide_song"
        };
        entries.push(context.command_with_shortcut(key, "spotify_dislike", C::SpotifyHide));
    }
    if menu.collection_plays {
        let key = match (menu.is_artist, menu.saved == Some(true)) {
            (true, true) => "spotify_unfollow",
            (true, false) => "spotify_follow",
            (false, true) => "spotify_remove_library",
            (false, false) => "spotify_save_library",
        };
        entries.push(context.command_with_shortcut(
            key,
            "spotify_toggle_saved",
            C::SpotifyToggleSaved,
        ));
    }
    if menu.in_editable_playlist {
        if menu.move_up {
            entries.push(context.command("move_up", C::SpotifyMoveUp));
        }
        if menu.move_down {
            entries.push(context.command("move_down", C::SpotifyMoveDown));
        }
        entries.push(context.command_with_shortcut(
            "spotify_remove_from_playlist",
            "remove_selected",
            C::SpotifyRemoveFromPlaylist,
        ));
    }
    if menu.rename {
        entries.push(context.command("spotify_rename_playlist", C::SpotifyRenamePlaylist));
    }
    if menu.create_playlist {
        entries.push(context.command_with_shortcut(
            "spotify_create_playlist",
            "create_playlist",
            C::SpotifyCreatePlaylist,
        ));
    }
    if menu.radio {
        entries.push(context.command_with_shortcut(
            "spotify_radio",
            "spotify_radio",
            C::SpotifyRadio,
        ));
    }
    if menu.album {
        entries.push(context.command("spotify_go_to_album", C::SpotifyGoToAlbum));
    }
    if menu.artist {
        entries.push(context.command("spotify_go_to_artist", C::SpotifyGoToArtist));
    }
    if menu.link {
        entries.push(context.command("copy_link", C::SpotifyCopyLink));
    }
    entries
}

/// Python `open_context_menu` for search results, trending, channel and
/// playlist contents and local folders.
pub fn results_context_menu(
    context: &ContextMenuContext<'_>,
    item: Option<&MediaItem>,
) -> Vec<ContextMenuEntry> {
    use ContextCommand as C;
    let collection =
        item.filter(|item| matches!(item.kind, MediaKind::Playlist | MediaKind::Channel));
    let mut entries: Vec<ContextMenuEntry> = if let Some(collection) = collection {
        let tail = [
            context.collection_download_entry(collection),
            context.shortcut("add_favorite", C::AddFavorite),
            context.shortcut("remove_favorite", C::RemoveFavorite),
            context.command("open_browser", C::OpenInBrowser),
            context.command_with_shortcut("copy_url", "copy_link", C::CopyLocation),
        ];
        if collection.kind == MediaKind::Channel
            && collection.source == apricot_core::MediaSource::Soundcloud
        {
            // Python: a SoundCloud artist opens its tracks and cannot be
            // subscribed to.
            let mut entries = vec![context.command("open", C::Play)];
            entries.extend(tail);
            entries
        } else if collection.kind == MediaKind::Channel {
            let mut entries = vec![
                context.command("channel_options", C::ChannelOptions),
                context.command("channel_videos", C::ChannelVideos),
                context.command("channel_popular", C::ChannelPopular),
                context.command("channel_playlists", C::ChannelPlaylists),
                context.command("channel_live_streams", C::ChannelStreams),
                context.shortcut("subscribe_channel", C::SubscribeChannel),
                context.shortcut("unsubscribe_channel", C::UnsubscribeChannel),
            ];
            entries.extend(tail);
            entries
        } else {
            let mut entries = vec![
                context.command("play_playlist", C::PlayPlaylist),
                context.command("shuffle_playlist", C::ShufflePlaylist),
                context.command("open_playlist_videos", C::OpenPlaylistVideos),
            ];
            entries.extend(tail);
            entries
        }
    } else if item.is_some_and(MediaItem::is_local_media) {
        vec![
            context.command("play", C::Play),
            context.shortcut("add_to_playback_queue", C::AddToPlaybackQueue),
            context.shortcut("remove_from_playback_queue", C::RemoveFromPlaybackQueue),
            context.shortcut("remove_from_playlist", C::RemoveFromPlaylist),
            context.command_with_shortcut("copy_path", "copy_link", C::CopyLocation),
        ]
    } else {
        let mut entries = vec![
            context.command("play", C::Play),
            context.shortcut("download_audio", C::DownloadAudio),
            context.shortcut("download_video", C::DownloadVideo),
            context.shortcut("add_favorite", C::AddFavorite),
            context.shortcut("remove_favorite", C::RemoveFavorite),
            context.shortcut("subscribe_channel", C::SubscribeChannel),
            context.shortcut("unsubscribe_channel", C::UnsubscribeChannel),
            context.shortcut("add_to_playback_queue", C::AddToPlaybackQueue),
            context.shortcut("remove_from_playback_queue", C::RemoveFromPlaybackQueue),
            context.shortcut("remove_from_playlist", C::RemoveFromPlaylist),
            context.command("open_browser", C::OpenInBrowser),
            context.shortcut("copy_stream_url", C::CopyStreamUrl),
            context.command_with_shortcut("copy_url", "copy_link", C::CopyLocation),
        ];
        if item.is_some_and(has_openable_youtube_channel) {
            entries.insert(7, context.shortcut("open_channel", C::OpenChannel));
        }
        entries
    };
    if context.queued_download_count > 1 {
        let insert_at = entries.len().min(1);
        entries.splice(
            insert_at..insert_at,
            [
                context.command("download_all_as_audio", C::DownloadAllAudio),
                context.command("download_all_as_video", C::DownloadAllVideo),
            ],
        );
    }
    if item.is_some() && collection.is_none() {
        entries.push(context.add_to_playlist_entry());
    }
    entries
}

/// Python `open_favorites_context_menu`.
pub fn favorites_context_menu(
    context: &ContextMenuContext<'_>,
    item: Option<&MediaItem>,
) -> Vec<ContextMenuEntry> {
    use ContextCommand as C;
    let Some(item) = item else {
        return vec![ContextMenuEntry::Disabled {
            label: context.text("favorites_empty"),
        }];
    };
    if item.is_local_media() {
        return vec![
            context.command("play", C::Play),
            context.shortcut("add_to_playlist", C::AddToPlaylist),
            context.shortcut("add_to_playback_queue", C::AddToPlaybackQueue),
            context.shortcut("remove_from_playback_queue", C::RemoveFromPlaybackQueue),
            context.command("copy_path", C::CopyLocation),
            context.command("remove", C::RemoveSelected),
        ];
    }
    let mut entries = vec![
        context.command("play", C::Play),
        context.shortcut("download_audio", C::DownloadAudio),
        context.shortcut("download_video", C::DownloadVideo),
        context.shortcut("subscribe_channel", C::SubscribeChannel),
        context.shortcut("unsubscribe_channel", C::UnsubscribeChannel),
        context.shortcut("add_to_playlist", C::AddToPlaylist),
        context.shortcut("add_to_playback_queue", C::AddToPlaybackQueue),
        context.shortcut("remove_from_playback_queue", C::RemoveFromPlaybackQueue),
        context.shortcut("copy_stream_url", C::CopyStreamUrl),
        context.command("copy_url", C::CopyLocation),
        context.command("remove", C::RemoveSelected),
    ];
    if has_openable_youtube_channel(item) {
        entries.insert(5, context.shortcut("open_channel", C::OpenChannel));
    }
    entries
}

/// Python `open_history_context_menu`. Python shows the online variant even
/// when nothing is selected.
pub fn history_context_menu(
    context: &ContextMenuContext<'_>,
    item: Option<&MediaItem>,
) -> Vec<ContextMenuEntry> {
    use ContextCommand as C;
    if item.is_some_and(MediaItem::is_local_media) {
        return vec![
            context.command("play", C::Play),
            context.command("add_favorite", C::AddFavorite),
            context.shortcut("add_to_playlist", C::AddToPlaylist),
            context.shortcut("add_to_playback_queue", C::AddToPlaybackQueue),
            context.shortcut("remove_from_playback_queue", C::RemoveFromPlaybackQueue),
            context.command("copy_path", C::CopyLocation),
            context.command("remove_history_item", C::RemoveSelected),
            context.command("clear_history", C::ClearHistory),
        ];
    }
    let mut entries = vec![
        context.command("play", C::Play),
        context.shortcut("download_audio", C::DownloadAudio),
        context.shortcut("download_video", C::DownloadVideo),
        context.command("add_favorite", C::AddFavorite),
        context.shortcut("subscribe_channel", C::SubscribeChannel),
        context.shortcut("unsubscribe_channel", C::UnsubscribeChannel),
        context.shortcut("add_to_playlist", C::AddToPlaylist),
        context.shortcut("add_to_playback_queue", C::AddToPlaybackQueue),
        context.shortcut("remove_from_playback_queue", C::RemoveFromPlaybackQueue),
        context.shortcut("copy_stream_url", C::CopyStreamUrl),
        context.command("copy_url", C::CopyLocation),
        context.command("remove_history_item", C::RemoveSelected),
        context.command("clear_history", C::ClearHistory),
    ];
    if item.is_some_and(has_openable_youtube_channel) {
        entries.insert(6, context.shortcut("open_channel", C::OpenChannel));
    }
    entries
}

/// Python `open_user_playlists_context_menu`.
pub fn user_playlists_context_menu(context: &ContextMenuContext<'_>) -> Vec<ContextMenuEntry> {
    use ContextCommand as C;
    let create = context.shortcut("create_playlist", C::CreatePlaylist);
    if context.user_playlist_titles.is_empty() {
        return vec![create];
    }
    vec![
        context.command("open_playlist", C::OpenUserPlaylist),
        create,
        context.command("download_user_playlist", C::DownloadUserPlaylist),
        context.command("remove_playlist", C::RemoveUserPlaylist),
    ]
}

/// Python `open_user_playlist_items_context_menu`.
pub fn user_playlist_items_context_menu(
    context: &ContextMenuContext<'_>,
    item: Option<&MediaItem>,
) -> Vec<ContextMenuEntry> {
    use ContextCommand as C;
    let Some(item) = item else {
        return vec![ContextMenuEntry::Disabled {
            label: context.text("playlist_empty"),
        }];
    };
    if item.is_local_media() {
        return vec![
            context.command("play", C::Play),
            context.shortcut("add_to_playback_queue", C::AddToPlaybackQueue),
            context.shortcut("remove_from_playback_queue", C::RemoveFromPlaybackQueue),
            context.shortcut("remove_from_playlist", C::RemoveFromPlaylist),
            context.command("copy_path", C::CopyLocation),
        ];
    }
    let mut entries = vec![
        context.command("play", C::Play),
        context.shortcut("download_audio", C::DownloadAudio),
        context.shortcut("download_video", C::DownloadVideo),
        context.command("download_user_playlist", C::DownloadUserPlaylist),
        context.shortcut("add_to_playback_queue", C::AddToPlaybackQueue),
        context.shortcut("remove_from_playback_queue", C::RemoveFromPlaybackQueue),
        context.shortcut("remove_from_playlist", C::RemoveFromPlaylist),
        context.command("copy_url", C::CopyLocation),
        context.shortcut("copy_stream_url", C::CopyStreamUrl),
    ];
    if has_openable_youtube_channel(item) {
        entries.insert(7, context.shortcut("open_channel", C::OpenChannel));
    }
    entries
}

/// Python `open_player_context_menu`.
pub fn player_context_menu(
    context: &ContextMenuContext<'_>,
    item: &MediaItem,
) -> Vec<ContextMenuEntry> {
    use ContextCommand as C;
    let local = item.is_local_media();
    let mut entries = Vec::new();
    if !local {
        entries.push(context.shortcut("download_audio", C::DownloadAudio));
        entries.push(context.shortcut("download_video", C::DownloadVideo));
    }
    entries.push(context.shortcut("add_favorite", C::AddFavorite));
    entries.push(context.shortcut("remove_favorite", C::RemoveFavorite));
    if !local {
        entries.push(context.shortcut("subscribe_channel", C::SubscribeChannel));
        entries.push(context.shortcut("unsubscribe_channel", C::UnsubscribeChannel));
        if has_openable_youtube_channel(item) {
            entries.push(context.shortcut("open_channel", C::OpenChannel));
        }
    }
    entries.push(context.shortcut("add_to_playback_queue", C::AddToPlaybackQueue));
    entries.push(context.shortcut("remove_from_playback_queue", C::RemoveFromPlaybackQueue));
    entries.push(context.shortcut("remove_from_playlist", C::RemoveFromPlaylist));
    entries.push(context.command_with_shortcut(
        if local { "copy_path" } else { "copy_link" },
        "player_copy_link",
        C::CopyLocation,
    ));
    if !local {
        entries.push(context.shortcut("copy_stream_url", C::CopyStreamUrl));
        if item.youtube_url_at_timestamp(0.0).is_some() {
            entries.push(context.command_with_shortcut(
                "copy_timestamp_link",
                "player_copy_timestamp_link",
                C::CopyTimestampLink,
            ));
        }
    }
    entries.push(context.command("output_devices", C::PlayerAction("player_output_devices")));
    entries.push(context.command_with_shortcut(
        "fullscreen",
        "player_fullscreen",
        C::PlayerAction("player_fullscreen"),
    ));
    entries.push(context.command("equalizer", C::PlayerAction("player_equalizer")));
    entries.push(context.command_with_shortcut(
        "audio_normalization",
        "player_replaygain",
        C::PlayerAction("player_replaygain"),
    ));
    if item.kind == MediaKind::PodcastEpisode {
        entries.push(context.shortcut(
            "save_podcast_speed_preset",
            C::PlayerAction("save_podcast_speed_preset"),
        ));
    }
    if has_youtube_url(item) {
        entries.push(context.command_with_shortcut(
            "play_related_video",
            "player_next_related",
            C::PlayerAction("player_next_related"),
        ));
    }
    for (key, action_id) in [
        ("add_bookmark", "player_add_bookmark"),
        ("bookmarks", "player_bookmarks"),
        ("chapters", "player_chapters"),
        ("transcript", "player_transcript"),
        ("lyrics", "player_lyrics"),
    ] {
        entries.push(context.command_with_shortcut(key, action_id, C::PlayerAction(action_id)));
    }
    if !local {
        entries.push(context.command_with_shortcut(
            "comments",
            "player_comments",
            C::PlayerAction("player_comments"),
        ));
        entries.push(context.command("open_browser", C::OpenInBrowser));
    }
    entries.push(context.command("close_player", C::ClosePlayer));
    if playlist_item_is_supported(item) {
        entries.push(context.add_to_playlist_entry());
    }
    entries
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_storage::SettingsDocument;

    use super::{
        ContextCommand, ContextMenuContext, ContextMenuEntry, browser_url, favorites_context_menu,
        history_context_menu, player_context_menu, results_context_menu,
        user_playlist_items_context_menu, user_playlists_context_menu,
        youtube_channel_item_for_video,
    };
    use crate::embedded_catalog;

    fn youtube_video() -> MediaItem {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "channel_url".to_owned(),
            serde_json::json!("https://www.youtube.com/channel/UCabc"),
        );
        MediaItem {
            id: MediaId("dQw4w9WgXcQ".to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: "Video".to_owned(),
            local_path: None,
            url: Some(
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
                    .parse()
                    .unwrap(),
            ),
            stream_url: None,
            external_audio_url: None,
            channel: "Channel".to_owned(),
            duration_seconds: None,
            metadata,
        }
    }

    fn video_without_channel() -> MediaItem {
        MediaItem {
            metadata: BTreeMap::new(),
            ..youtube_video()
        }
    }

    fn local_file() -> MediaItem {
        MediaItem {
            id: MediaId("song".to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: "Song".to_owned(),
            local_path: Some(r"C:\Music\song.mp3".to_owned()),
            url: None,
            stream_url: None,
            external_audio_url: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    fn collection(kind: MediaKind) -> MediaItem {
        MediaItem {
            kind,
            title: "Collection".to_owned(),
            url: Some("https://www.youtube.com/@channel".parse().unwrap()),
            ..video_without_channel()
        }
    }

    fn podcast_episode() -> MediaItem {
        MediaItem {
            id: MediaId("episode".to_owned()),
            source: MediaSource::Podcast,
            kind: MediaKind::PodcastEpisode,
            title: "Episode".to_owned(),
            local_path: None,
            url: Some("https://example.com/episode.mp3".parse().unwrap()),
            stream_url: None,
            external_audio_url: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    fn settings(show_shortcuts: bool) -> SettingsDocument {
        SettingsDocument {
            show_shortcuts_in_labels: show_shortcuts,
            ..SettingsDocument::default()
        }
    }

    fn labels(entries: &[ContextMenuEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                ContextMenuEntry::Submenu { label, entries } => {
                    format!("{label} > [{}]", labels(entries).join(", "))
                }
                ContextMenuEntry::Disabled { label } => format!("{label} (disabled)"),
                ContextMenuEntry::Command { label, .. } => label.clone(),
            })
            .collect()
    }

    struct Fixture {
        settings: SettingsDocument,
        playlists: Vec<String>,
        queued: usize,
    }

    impl Fixture {
        fn new() -> Self {
            Self {
                settings: settings(false),
                playlists: Vec::new(),
                queued: 0,
            }
        }

        fn with<R>(&self, build: impl FnOnce(&ContextMenuContext<'_>) -> R) -> R {
            let catalog = embedded_catalog("en");
            let context = ContextMenuContext {
                catalog: &catalog,
                settings: &self.settings,
                user_playlist_titles: &self.playlists,
                queued_download_count: self.queued,
            };
            build(&context)
        }
    }

    #[test]
    fn remote_result_matches_python_order_with_channel_and_playlist_submenu() {
        let mut fixture = Fixture::new();
        fixture.playlists = vec!["Road".to_owned(), "Gym".to_owned()];
        let entries = fixture.with(|context| results_context_menu(context, Some(&youtube_video())));
        assert_eq!(
            labels(&entries),
            [
                "Play",
                "Download audio",
                "Download video",
                "Add to favorites",
                "Remove from favorites",
                "Subscribe to channel",
                "Unsubscribe from channel",
                "Open channel",
                "Add to playback queue",
                "Remove from playback queue",
                "Remove from playlist",
                "Open in browser",
                "Copy direct media URL",
                "Copy URL",
                "Add to playlist > [Road, Gym, Create playlist]",
            ]
        );
    }

    #[test]
    fn result_without_youtube_channel_has_no_open_channel_and_flat_playlist_entry() {
        let fixture = Fixture::new();
        let entries =
            fixture.with(|context| results_context_menu(context, Some(&video_without_channel())));
        let labels = labels(&entries);
        assert!(!labels.iter().any(|label| label == "Open channel"));
        assert_eq!(labels.last().map(String::as_str), Some("Add to playlist"));
    }

    #[test]
    fn local_result_uses_copy_path_and_shortcut_after_tab() {
        let mut fixture = Fixture::new();
        fixture.settings = settings(true);
        let entries = fixture.with(|context| results_context_menu(context, Some(&local_file())));
        assert_eq!(
            labels(&entries),
            [
                "Play",
                "Add to playback queue\tCtrl+Shift+Q",
                "Remove from playback queue\tCtrl+Shift+Delete",
                "Remove from playlist\tCtrl+Shift+P",
                "Copy path\tCtrl+L",
                "Add to playlist\tCtrl+P",
            ]
        );
    }

    #[test]
    fn download_all_entries_follow_play_when_more_than_one_download_is_queued() {
        let mut fixture = Fixture::new();
        fixture.queued = 2;
        let entries = fixture.with(|context| results_context_menu(context, Some(&local_file())));
        assert_eq!(
            labels(&entries)[..3],
            ["Play", "Download all as audio", "Download all as video"]
        );
        fixture.queued = 1;
        let entries = fixture.with(|context| results_context_menu(context, Some(&local_file())));
        assert_eq!(labels(&entries)[1], "Add to playback queue");
    }

    #[test]
    fn channel_result_matches_python_including_download_submenu() {
        let fixture = Fixture::new();
        let entries = fixture
            .with(|context| results_context_menu(context, Some(&collection(MediaKind::Channel))));
        assert_eq!(
            labels(&entries),
            [
                "Channel options",
                "Videos",
                "Popular videos",
                "Channel playlists",
                "Live streams",
                "Subscribe to channel",
                "Unsubscribe from channel",
                "Download channel > [Download audio, Download video]",
                "Add to favorites",
                "Remove from favorites",
                "Open in browser",
                "Copy URL",
            ]
        );
    }

    #[test]
    fn soundcloud_artist_matches_python_without_subscribe_or_channel_tabs() {
        let fixture = Fixture::new();
        let artist = MediaItem {
            source: MediaSource::Soundcloud,
            url: Some("https://soundcloud.com/artist".parse().unwrap()),
            ..collection(MediaKind::Channel)
        };
        let entries = fixture.with(|context| results_context_menu(context, Some(&artist)));
        assert_eq!(
            labels(&entries),
            [
                "Open",
                "Download channel > [Download audio, Download video]",
                "Add to favorites",
                "Remove from favorites",
                "Open in browser",
                "Copy URL",
            ]
        );
        assert!(matches!(
            entries[0],
            ContextMenuEntry::Command {
                command: ContextCommand::Play,
                ..
            }
        ));
    }

    #[test]
    fn playlist_result_matches_python() {
        let fixture = Fixture::new();
        let entries = fixture
            .with(|context| results_context_menu(context, Some(&collection(MediaKind::Playlist))));
        assert_eq!(
            labels(&entries),
            [
                "Play playlist",
                "Shuffle playlist",
                "Open playlist videos",
                "Download playlist > [Download audio, Download video]",
                "Add to favorites",
                "Remove from favorites",
                "Open in browser",
                "Copy URL",
            ]
        );
    }

    #[test]
    fn favorites_split_local_and_online_items() {
        let fixture = Fixture::new();
        let local = fixture.with(|context| favorites_context_menu(context, Some(&local_file())));
        assert_eq!(
            labels(&local),
            [
                "Play",
                "Add to playlist",
                "Add to playback queue",
                "Remove from playback queue",
                "Copy path",
                "Remove",
            ]
        );
        let online =
            fixture.with(|context| favorites_context_menu(context, Some(&youtube_video())));
        assert_eq!(
            labels(&online),
            [
                "Play",
                "Download audio",
                "Download video",
                "Subscribe to channel",
                "Unsubscribe from channel",
                "Open channel",
                "Add to playlist",
                "Add to playback queue",
                "Remove from playback queue",
                "Copy direct media URL",
                "Copy URL",
                "Remove",
            ]
        );
        let empty = fixture.with(|context| favorites_context_menu(context, None));
        assert_eq!(labels(&empty), ["No favorites. (disabled)"]);
    }

    #[test]
    fn history_split_local_and_online_items() {
        let fixture = Fixture::new();
        let local = fixture.with(|context| history_context_menu(context, Some(&local_file())));
        assert_eq!(
            labels(&local),
            [
                "Play",
                "Add to favorites",
                "Add to playlist",
                "Add to playback queue",
                "Remove from playback queue",
                "Copy path",
                "Remove from history",
                "Clear history",
            ]
        );
        let online = fixture.with(|context| history_context_menu(context, Some(&youtube_video())));
        assert_eq!(
            labels(&online),
            [
                "Play",
                "Download audio",
                "Download video",
                "Add to favorites",
                "Subscribe to channel",
                "Unsubscribe from channel",
                "Open channel",
                "Add to playlist",
                "Add to playback queue",
                "Remove from playback queue",
                "Copy direct media URL",
                "Copy URL",
                "Remove from history",
                "Clear history",
            ]
        );
        let empty = fixture.with(|context| history_context_menu(context, None));
        assert_eq!(labels(&empty).len(), 13);
    }

    #[test]
    fn user_playlists_have_python_items_only() {
        let mut fixture = Fixture::new();
        let empty = fixture.with(user_playlists_context_menu);
        assert_eq!(labels(&empty), ["Create playlist"]);
        fixture.playlists = vec!["Road".to_owned()];
        let entries = fixture.with(user_playlists_context_menu);
        assert_eq!(
            labels(&entries),
            [
                "Open playlist",
                "Create playlist",
                "Download playlist",
                "Remove playlist",
            ]
        );
    }

    #[test]
    fn user_playlist_items_split_local_and_online_items() {
        let fixture = Fixture::new();
        let online = fixture
            .with(|context| user_playlist_items_context_menu(context, Some(&youtube_video())));
        assert_eq!(
            labels(&online),
            [
                "Play",
                "Download audio",
                "Download video",
                "Download playlist",
                "Add to playback queue",
                "Remove from playback queue",
                "Remove from playlist",
                "Open channel",
                "Copy URL",
                "Copy direct media URL",
            ]
        );
        let local =
            fixture.with(|context| user_playlist_items_context_menu(context, Some(&local_file())));
        assert_eq!(labels(&local).last().map(String::as_str), Some("Copy path"));
        assert_eq!(labels(&local).len(), 5);
        let empty = fixture.with(|context| user_playlist_items_context_menu(context, None));
        assert_eq!(labels(&empty), ["Playlist is empty. (disabled)"]);
    }

    #[test]
    fn player_menu_matches_python_for_youtube_video() {
        let fixture = Fixture::new();
        let entries = fixture.with(|context| player_context_menu(context, &youtube_video()));
        assert_eq!(
            labels(&entries),
            [
                "Download audio",
                "Download video",
                "Add to favorites",
                "Remove from favorites",
                "Subscribe to channel",
                "Unsubscribe from channel",
                "Open channel",
                "Add to playback queue",
                "Remove from playback queue",
                "Remove from playlist",
                "Copy link",
                "Copy direct media URL",
                "Copy link at current time",
                "Audio output devices",
                "Start full screen",
                "Equalizer",
                "Audio normalization",
                "Play related video",
                "Add bookmark",
                "Bookmarks",
                "Chapters",
                "Transcript",
                "Lyrics",
                "Comments",
                "Open in browser",
                "Close",
                "Add to playlist",
            ]
        );
    }

    #[test]
    fn player_menu_for_local_file_and_podcast_episode() {
        let fixture = Fixture::new();
        let local = labels(&fixture.with(|context| player_context_menu(context, &local_file())));
        assert_eq!(
            local[..3],
            [
                "Add to favorites",
                "Remove from favorites",
                "Add to playback queue"
            ]
        );
        assert!(local.contains(&"Copy path".to_owned()));
        for absent in [
            "Comments",
            "Open in browser",
            "Download audio",
            "Play related video",
        ] {
            assert!(!local.iter().any(|label| label == absent), "{absent}");
        }
        let episode =
            labels(&fixture.with(|context| player_context_menu(context, &podcast_episode())));
        assert!(episode.contains(&"Save speed for this podcast".to_owned()));
        assert!(!episode.contains(&"Play related video".to_owned()));
        assert!(!episode.contains(&"Copy link at current time".to_owned()));
    }

    #[test]
    fn player_actions_route_through_shortcut_ids() {
        let fixture = Fixture::new();
        let entries = fixture.with(|context| player_context_menu(context, &youtube_video()));
        let actions = entries
            .iter()
            .filter_map(|entry| match entry {
                ContextMenuEntry::Command {
                    command: ContextCommand::PlayerAction(action),
                    ..
                } => Some(*action),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            actions,
            [
                "player_output_devices",
                "player_fullscreen",
                "player_equalizer",
                "player_replaygain",
                "player_next_related",
                "player_add_bookmark",
                "player_bookmarks",
                "player_chapters",
                "player_transcript",
                "player_lyrics",
                "player_comments",
            ]
        );
    }

    #[test]
    fn open_channel_builds_the_uploader_channel_like_python() {
        let channel = youtube_channel_item_for_video(&youtube_video()).expect("channel");
        assert_eq!(channel.kind, MediaKind::Channel);
        assert_eq!(channel.title, "Channel");
        assert_eq!(
            channel.url.map(|url| url.to_string()).as_deref(),
            Some("https://www.youtube.com/channel/UCabc")
        );
        assert!(youtube_channel_item_for_video(&video_without_channel()).is_none());
        assert!(youtube_channel_item_for_video(&local_file()).is_none());
        assert!(youtube_channel_item_for_video(&collection(MediaKind::Channel)).is_none());
    }

    #[test]
    fn browser_url_prefers_webpage_url_and_rejects_local_paths() {
        let mut episode = podcast_episode();
        episode.metadata.insert(
            "webpage_url".to_owned(),
            serde_json::json!("https://example.com/episode"),
        );
        assert_eq!(
            browser_url(&episode).as_deref(),
            Some("https://example.com/episode")
        );
        assert_eq!(
            browser_url(&youtube_video()).as_deref(),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
        );
        assert_eq!(browser_url(&local_file()), None);
    }
}
