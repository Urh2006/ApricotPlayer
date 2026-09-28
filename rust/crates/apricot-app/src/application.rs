//! Top-level application coordinator consumed by platform UI adapters.

use std::{
    collections::{BTreeSet, HashSet, VecDeque},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use apricot_core::{MediaItem, Route, RouteFrame, SettingId, SettingsSection};
use apricot_playback::PlaybackEvent;
use apricot_storage::{
    AppNotification, Bookmark, BookmarkFile, LastPlayerSession, LastPlayerSessionFile,
    MediaListFile, NotificationFile, PlaybackPositionFile, PlaybackQueueFile, RssFeed, RssFeedFile,
    SettingsDocument, Subscription, SubscriptionFile, UserPlaylist, UserPlaylistFile,
};
use rand::seq::SliceRandom;
use serde_json::{Map, Value};

use crate::{
    ActionFinderContext, ActionFinderModel, ActionFinderPlayer, ActivationRequest, AppState,
    AudioSession, BookmarkController, BookmarkControllerError, CollectionAddOutcome,
    DownloadController, EqualizerSession, LastPlayerSessionController, MainMenuAvailability,
    MainMenuModel, MediaCollectionController, MediaCollectionControllerError, MenuVisibility,
    NotificationController, NotificationControllerError, PlaybackPhase, PlaybackPositionController,
    PlaybackPositionControllerError, PlaybackPositionUpdate, PlaybackQueue,
    PlaybackQueueController, PlaybackQueueControllerError, PlaybackSequenceSource,
    PlayerScreenModel, PlayerSession, PlayerSessionDefaults, PlayerViewState, PlaylistAddOutcome,
    PlaylistCreateOutcome, QueueAddOutcome, QueueBatchAddOutcome, RssFeedAddOutcome,
    RssFeedController, RssFeedControllerError, RssRefreshResult, RssRefreshSummary,
    SearchApplyOutcome, SearchSession, SearchSessionError, SearchWork, SessionToggle,
    SettingsController, SettingsControllerError, SettingsScreenModel, SubscriptionAddOutcome,
    SubscriptionCheckResult, SubscriptionCheckSummary, SubscriptionController,
    SubscriptionControllerError, SubscriptionRemoveOutcome, UserPlaylistController,
    UserPlaylistControllerError, YoutubeCollectionApplyOutcome, YoutubeCollectionError,
    YoutubeCollectionKind, YoutubeCollectionSession, YoutubeCollectionWork, YoutubeSearchKind,
    YoutubeTrendingWork, embedded_catalog,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerNavigationOrigin {
    Sequence,
    Queue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerNavigationOutcome {
    Item {
        item: Box<MediaItem>,
        origin: PlayerNavigationOrigin,
    },
    LoadingMore(SearchWork),
    LoadingMoreCollection(YoutubeCollectionWork),
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LastSessionResume {
    pub item: MediaItem,
    pub sequence_active: bool,
    pub return_screen: String,
    pub return_data: Map<String, Value>,
}

#[derive(Debug)]
pub struct Application {
    settings: SettingsController,
    menu_availability: MainMenuAvailability,
    activation_requests: VecDeque<ActivationRequest>,
    startup_announcement: Option<&'static str>,
    /// Python `related_autoplay_seen_ids`: related videos already played in
    /// this player session.
    related_seen_ids: HashSet<String>,
    state: AppState,
}

impl Application {
    pub fn new(settings: SettingsController, menu_availability: MainMenuAvailability) -> Self {
        Self {
            settings,
            menu_availability,
            activation_requests: VecDeque::new(),
            startup_announcement: None,
            related_seen_ids: HashSet::new(),
            state: AppState::default(),
        }
    }

    pub const fn player_session(&self) -> &PlayerSession {
        &self.state.player
    }

    pub const fn search_session(&self) -> &SearchSession {
        &self.state.search
    }

    pub fn favorites(&self) -> &[MediaItem] {
        self.state.favorites.items()
    }

    pub fn is_favorite(&self, item: &MediaItem) -> bool {
        self.state.favorites.contains(item)
    }

    pub fn history(&self) -> &[MediaItem] {
        self.state.history.items()
    }

    pub fn bookmarks(&self) -> &[Bookmark] {
        self.state.bookmarks.bookmarks()
    }

    pub const fn downloads(&self) -> &DownloadController {
        &self.state.downloads
    }

    pub const fn downloads_mut(&mut self) -> &mut DownloadController {
        &mut self.state.downloads
    }

    pub fn sorted_bookmarks(&self) -> Vec<&Bookmark> {
        self.state.bookmarks.sorted()
    }

    pub fn bookmark(&self, id: &str) -> Option<&Bookmark> {
        self.state
            .bookmarks
            .bookmarks()
            .iter()
            .find(|bookmark| bookmark.id == id)
    }

    pub fn bookmarks_for_item(&self, item: &MediaItem) -> Vec<&Bookmark> {
        self.state.bookmarks.for_item(item)
    }

    pub fn configure_bookmarks(
        &mut self,
        current: BookmarkFile,
        legacy: &BookmarkFile,
        timestamp: f64,
    ) {
        self.state.bookmarks = BookmarkController::load(current, legacy, timestamp);
    }

    pub fn configure_playback_positions(
        &mut self,
        current: PlaybackPositionFile,
        legacy: &PlaybackPositionFile,
    ) {
        self.state.playback_positions = PlaybackPositionController::load(current, legacy);
    }

    pub fn configure_last_player_session(
        &mut self,
        current: LastPlayerSessionFile,
        legacy: &LastPlayerSessionFile,
    ) {
        self.state.last_player_session = LastPlayerSessionController::load(current, legacy);
    }

    pub fn last_player_session(&self) -> Option<&LastPlayerSession> {
        self.state.last_player_session.session()
    }

    pub fn last_player_session_load_error(&self) -> Option<&str> {
        self.state.last_player_session.load_error()
    }

    pub fn last_player_session_write_error(&self) -> Option<String> {
        self.state.last_player_session.write_error()
    }

    pub fn playback_resume_position(&self, item: &MediaItem) -> Option<f64> {
        self.state
            .playback_positions
            .resume_position(item, self.settings.current().resume_playback)
    }

    /// Clears one item's durable resume position without affecting other media.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed position map cannot be persisted.
    pub fn clear_playback_position(
        &mut self,
        item: &MediaItem,
    ) -> Result<PlaybackPositionUpdate, PlaybackPositionControllerError> {
        self.state.playback_positions.update(item, 0.0, None, true)
    }

    /// Persists the current item's projected position before replacement or a
    /// real player-session close.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed position map cannot be persisted.
    pub fn save_current_playback_position(
        &mut self,
    ) -> Result<PlaybackPositionUpdate, PlaybackPositionControllerError> {
        let session = &self.state.player;
        let Some(item) = session.current_item().cloned() else {
            return Ok(PlaybackPositionUpdate::Unchanged);
        };
        let position = session.position_seconds();
        let duration = session.duration_seconds().or(item.duration_seconds);
        self.state.playback_positions.update(
            &item,
            position,
            duration,
            self.settings.current().resume_playback,
        )
    }

    /// Adds a Python-compatible bookmark for one playable media item.
    ///
    /// # Errors
    ///
    /// Returns an error when the bookmark file cannot be updated.
    pub fn add_bookmark(
        &mut self,
        name: &str,
        position: f64,
        media: MediaItem,
        timestamp: f64,
    ) -> Result<Option<Bookmark>, BookmarkControllerError> {
        self.state.bookmarks.add(name, position, media, timestamp)
    }

    /// Renames one bookmark by its durable bookmark id.
    ///
    /// # Errors
    ///
    /// Returns an error when the bookmark file cannot be updated.
    pub fn rename_bookmark(
        &mut self,
        id: &str,
        name: &str,
        timestamp: f64,
    ) -> Result<bool, BookmarkControllerError> {
        self.state.bookmarks.rename(id, name, timestamp)
    }

    /// Deletes one bookmark by its durable bookmark id.
    ///
    /// # Errors
    ///
    /// Returns an error when the bookmark file cannot be updated.
    pub fn delete_bookmark(&mut self, id: &str) -> Result<bool, BookmarkControllerError> {
        self.state.bookmarks.delete(id)
    }

    pub fn configure_media_collections(
        &mut self,
        favorites: MediaListFile,
        legacy_favorites: &MediaListFile,
        history: MediaListFile,
        legacy_history: &MediaListFile,
    ) {
        self.state.favorites = MediaCollectionController::load(favorites, legacy_favorites);
        self.state.history = MediaCollectionController::load(history, legacy_history);
    }

    pub fn configure_notifications(
        &mut self,
        current: NotificationFile,
        legacy: &NotificationFile,
    ) {
        self.state.notifications = NotificationController::load(current, legacy);
    }

    pub fn configure_subscriptions(
        &mut self,
        current: SubscriptionFile,
        legacy: &SubscriptionFile,
    ) {
        self.state.subscriptions = SubscriptionController::load(current, legacy);
    }

    pub fn configure_rss_feeds(&mut self, current: RssFeedFile, legacy: &RssFeedFile) {
        self.state.rss_feeds = RssFeedController::load(current, legacy);
    }

    pub fn rss_feeds(&self) -> &[RssFeed] {
        self.state.rss_feeds.feeds()
    }

    pub fn rss_feed_load_error(&self) -> Option<&str> {
        self.state.rss_feeds.load_error()
    }

    pub fn rss_category_filter(&self) -> &str {
        self.state.rss_feeds.category_filter()
    }

    pub fn set_rss_category_filter(&mut self, category: &str) {
        self.state.rss_feeds.set_category_filter(category);
    }

    pub fn rss_categories(&self) -> Vec<String> {
        self.state.rss_feeds.categories()
    }

    pub fn visible_rss_feed_indices(&self) -> Vec<usize> {
        self.state.rss_feeds.visible_indices()
    }

    pub fn rss_episode_location(&self, item: &MediaItem) -> Option<(usize, usize)> {
        let identity = item.stable_identity()?;
        let indexed = item
            .metadata
            .get("rss_feed_index")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .zip(
                item.metadata
                    .get("rss_item_index")
                    .and_then(Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok()),
            );
        if let Some((feed_index, item_index)) = indexed
            && self
                .state
                .rss_feeds
                .feeds()
                .get(feed_index)
                .and_then(|feed| feed.items.get(item_index))
                .and_then(MediaItem::stable_identity)
                .as_deref()
                == Some(identity.as_str())
        {
            return Some((feed_index, item_index));
        }
        self.state
            .rss_feeds
            .feeds()
            .iter()
            .enumerate()
            .find_map(|(feed_index, feed)| {
                feed.items
                    .iter()
                    .position(|candidate| {
                        candidate.stable_identity().as_deref() == Some(identity.as_str())
                    })
                    .map(|item_index| (feed_index, item_index))
            })
    }

    /// Adds one fully fetched feed to the durable RSS archive.
    ///
    /// # Errors
    ///
    /// Returns an error when the archive cannot be persisted.
    pub fn add_rss_feed(
        &mut self,
        feed: RssFeed,
    ) -> Result<RssFeedAddOutcome, RssFeedControllerError> {
        self.state.rss_feeds.add(feed)
    }

    /// Adds a fetched OPML batch with one durable archive update.
    ///
    /// # Errors
    ///
    /// Returns an error when the RSS archive cannot be persisted.
    pub fn import_rss_feeds(
        &mut self,
        feeds: Vec<RssFeed>,
    ) -> Result<crate::RssFeedImportSummary, RssFeedControllerError> {
        self.state.rss_feeds.add_many(feeds)
    }

    /// Removes one feed by its unfiltered durable index.
    ///
    /// # Errors
    ///
    /// Returns an error when the archive cannot be persisted.
    pub fn remove_rss_feed(
        &mut self,
        index: usize,
    ) -> Result<Option<RssFeed>, RssFeedControllerError> {
        self.state.rss_feeds.remove(index)
    }

    /// Assigns or clears one feed category.
    ///
    /// # Errors
    ///
    /// Returns an error when the archive cannot be persisted.
    pub fn set_rss_feed_category(
        &mut self,
        index: usize,
        category: &str,
    ) -> Result<bool, RssFeedControllerError> {
        self.state.rss_feeds.set_category(index, category)
    }

    /// Saves a per-feed podcast speed or clears the override.
    ///
    /// # Errors
    ///
    /// Returns an error when the archive cannot be persisted.
    pub fn set_rss_feed_speed(
        &mut self,
        index: usize,
        speed: Option<f64>,
    ) -> Result<bool, RssFeedControllerError> {
        self.state.rss_feeds.set_speed_preset(index, speed)
    }

    /// Marks one podcast episode as played or unplayed.
    ///
    /// # Errors
    ///
    /// Returns an error when the archive cannot be persisted.
    pub fn set_rss_episode_played(
        &mut self,
        feed_index: usize,
        item_index: usize,
        played: bool,
        timestamp: f64,
    ) -> Result<Option<MediaItem>, RssFeedControllerError> {
        self.state
            .rss_feeds
            .set_played(feed_index, item_index, played, timestamp)
    }

    /// Applies a complete feed refresh batch with one durable archive write.
    ///
    /// # Errors
    ///
    /// Returns an error when the archive cannot be persisted.
    pub fn apply_rss_refreshes(
        &mut self,
        results: Vec<RssRefreshResult>,
        timestamp: f64,
    ) -> Result<RssRefreshSummary, RssFeedControllerError> {
        self.state.rss_feeds.apply_refreshes(results, timestamp)
    }

    pub fn prepare_rss_episode_playback(
        &mut self,
        feed_index: usize,
        item_index: usize,
    ) -> Option<MediaItem> {
        let feed = self.state.rss_feeds.feeds().get(feed_index)?;
        let speed_preset = feed.speed_preset;
        let items = feed
            .items
            .iter()
            .enumerate()
            .map(|(index, source)| {
                let mut item = source.clone();
                item.metadata
                    .insert("rss_feed_index".to_owned(), feed_index.into());
                item.metadata
                    .insert("rss_item_index".to_owned(), index.into());
                if let Some(speed) = speed_preset {
                    item.metadata
                        .insert("podcast_speed_preset".to_owned(), speed.into());
                }
                item
            })
            .collect::<Vec<_>>();
        let item = items.get(item_index)?.clone();
        if !item.is_playable() {
            return None;
        }
        let _ = self.state.player_sequence.set(
            PlaybackSequenceSource::RssFeed { feed_index },
            &items,
            &item,
        );
        Some(item)
    }

    pub fn subscriptions(&self) -> &[Subscription] {
        self.state.subscriptions.subscriptions()
    }

    pub fn subscription_load_error(&self) -> Option<&str> {
        self.state.subscriptions.load_error()
    }

    pub fn subscription_category_filter(&self) -> &str {
        self.state.subscriptions.category_filter()
    }

    pub fn set_subscription_category_filter(&mut self, category: &str) {
        self.state.subscriptions.set_category_filter(category);
    }

    pub fn subscription_categories(&self) -> Vec<String> {
        self.state.subscriptions.categories()
    }

    pub fn visible_subscription_indices(&self) -> Vec<usize> {
        self.state.subscriptions.visible_indices()
    }

    pub fn is_subscribed(&self, item: &MediaItem) -> bool {
        self.state.subscriptions.contains_item(item)
    }

    pub fn can_subscribe_to_item(item: &MediaItem) -> bool {
        SubscriptionController::supports_item(item)
    }

    /// Adds the channel represented by a selected media row.
    ///
    /// # Errors
    ///
    /// Returns an error when the subscription collection cannot be persisted.
    pub fn subscribe_to_item(
        &mut self,
        item: &MediaItem,
        timestamp: f64,
    ) -> Result<SubscriptionAddOutcome, SubscriptionControllerError> {
        self.state.subscriptions.add_from_item(item, timestamp)
    }

    /// Removes the channel represented by a selected media row.
    ///
    /// # Errors
    ///
    /// Returns an error when the subscription collection cannot be persisted.
    pub fn unsubscribe_from_item(
        &mut self,
        item: &MediaItem,
    ) -> Result<SubscriptionRemoveOutcome, SubscriptionControllerError> {
        self.state.subscriptions.remove_from_item(item)
    }

    /// Removes one subscription by its unfiltered durable index.
    ///
    /// # Errors
    ///
    /// Returns an error when the subscription collection cannot be persisted.
    pub fn remove_subscription(
        &mut self,
        index: usize,
    ) -> Result<Option<Subscription>, SubscriptionControllerError> {
        self.state.subscriptions.remove(index)
    }

    /// Assigns or clears one subscription category.
    ///
    /// # Errors
    ///
    /// Returns an error when the subscription collection cannot be persisted.
    pub fn set_subscription_category(
        &mut self,
        index: usize,
        category: &str,
    ) -> Result<bool, SubscriptionControllerError> {
        self.state.subscriptions.set_category(index, category)
    }

    /// Applies a complete subscription check with one durable replacement.
    ///
    /// # Errors
    ///
    /// Returns an error when the refreshed subscription collection cannot be persisted.
    pub fn apply_subscription_checks(
        &mut self,
        results: Vec<SubscriptionCheckResult>,
        timestamp: f64,
    ) -> Result<SubscriptionCheckSummary, SubscriptionControllerError> {
        self.state.subscriptions.apply_checks(results, timestamp)
    }

    pub fn show_saved_subscription_results(
        &mut self,
        query: impl Into<String>,
        items: Vec<MediaItem>,
    ) -> bool {
        self.state.youtube_collections.clear();
        let restored =
            self.state
                .search
                .restore_snapshot(query, YoutubeSearchKind::Video, items, 0);
        if restored {
            let source = PlaybackSequenceSource::Search {
                generation: self.state.search.generation(),
            };
            let _ = self
                .state
                .player_sequence
                .sync(source, self.state.search.items());
        }
        restored
    }

    /// Records a successful subscription check in the persisted settings.
    ///
    /// # Errors
    ///
    /// Returns an error when the timestamp is invalid or settings cannot be saved.
    pub fn record_subscription_check(
        &mut self,
        timestamp: f64,
    ) -> Result<(), SettingsControllerError> {
        let timestamp = if timestamp.is_finite() {
            timestamp.max(0.0)
        } else {
            0.0
        };
        self.settings.set_value(
            SettingId::LastSubscriptionCheck,
            serde_json::json!(timestamp),
        )?;
        let _ = self.settings.save()?;
        Ok(())
    }

    pub fn notifications(&self) -> &[AppNotification] {
        self.state.notifications.notifications()
    }

    pub fn notification_load_error(&self) -> Option<&str> {
        self.state.notifications.load_error()
    }

    /// Adds one newest-first notification and applies Python's durable bound.
    ///
    /// # Errors
    ///
    /// Returns an error when the notification file cannot be updated.
    pub fn add_notification(
        &mut self,
        notification: AppNotification,
    ) -> Result<(), NotificationControllerError> {
        self.state.notifications.add(notification)
    }

    /// Removes one displayed notification.
    ///
    /// # Errors
    ///
    /// Returns an error when the notification file cannot be updated.
    pub fn remove_notification(
        &mut self,
        index: usize,
    ) -> Result<Option<AppNotification>, NotificationControllerError> {
        self.state.notifications.remove(index)
    }

    /// Clears all notifications.
    ///
    /// # Errors
    ///
    /// Returns an error when the notification file cannot be updated.
    pub fn clear_notifications(&mut self) -> Result<bool, NotificationControllerError> {
        self.state.notifications.clear()
    }

    pub fn prepare_notification_playback(&mut self, index: usize) -> Option<MediaItem> {
        let item = self
            .state
            .notifications
            .notifications()
            .get(index)?
            .item
            .as_ref()?
            .clone();
        if !item.is_playable() {
            return None;
        }
        let mut frame = self.state.navigation.current().clone();
        frame.selected_index = index;
        frame
            .parameters
            .insert("index".to_owned(), Value::from(index));
        self.state.navigation.replace(frame);
        self.state.player_sequence.clear();
        Some(item)
    }

    /// Adds one playable item to favorites unless it is already present.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed favorites file cannot be persisted.
    pub fn add_favorite(
        &mut self,
        item: MediaItem,
    ) -> Result<CollectionAddOutcome, MediaCollectionControllerError> {
        self.state.favorites.add_unique(item)
    }

    /// Removes one favorite by its displayed position.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed favorites file cannot be persisted.
    pub fn remove_favorite(
        &mut self,
        index: usize,
    ) -> Result<Option<MediaItem>, MediaCollectionControllerError> {
        self.state.favorites.remove(index)
    }

    /// Removes a matching favorite by its durable URL or local path.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed favorites file cannot be persisted.
    pub fn remove_favorite_item(
        &mut self,
        item: &MediaItem,
    ) -> Result<Option<MediaItem>, MediaCollectionControllerError> {
        self.state.favorites.remove_item(item)
    }

    pub fn prepare_favorite_playback(&mut self, index: usize) -> Option<MediaItem> {
        let item = self.state.favorites.items().get(index)?.clone();
        if !is_library_collection(&item) {
            let _ = self.state.player_sequence.set(
                PlaybackSequenceSource::Collection,
                self.state.favorites.items(),
                &item,
            );
        }
        Some(item)
    }

    /// Records a played or downloaded item at the front of history.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed history file cannot be persisted.
    pub fn record_history(
        &mut self,
        mut item: MediaItem,
        action: &str,
        timestamp: f64,
    ) -> Result<(), MediaCollectionControllerError> {
        if !self.settings.current().enable_history {
            return Ok(());
        }
        item.metadata.insert(
            "action".to_owned(),
            serde_json::Value::String(action.to_owned()),
        );
        if let Some(timestamp) = serde_json::Number::from_f64(timestamp) {
            item.metadata
                .insert("timestamp".to_owned(), serde_json::Value::Number(timestamp));
        }
        let limit =
            usize::try_from(self.settings.current().history_limit.max(10)).unwrap_or(usize::MAX);
        self.state.history.upsert_front(item, limit)
    }

    /// Removes one history entry by its displayed position.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed history file cannot be persisted.
    pub fn remove_history_item(
        &mut self,
        index: usize,
    ) -> Result<Option<MediaItem>, MediaCollectionControllerError> {
        self.state.history.remove(index)
    }

    pub fn prepare_history_playback(&mut self, index: usize) -> Option<MediaItem> {
        let item = self.state.history.items().get(index)?.clone();
        if !is_library_collection(&item) {
            let _ = self.state.player_sequence.set(
                PlaybackSequenceSource::Collection,
                self.state.history.items(),
                &item,
            );
        }
        Some(item)
    }

    /// Clears all history entries.
    ///
    /// # Errors
    ///
    /// Returns an error when the empty history file cannot be persisted.
    pub fn clear_history(&mut self) -> Result<bool, MediaCollectionControllerError> {
        self.state.history.clear()
    }

    pub fn user_playlists(&self) -> &[UserPlaylist] {
        self.state.user_playlists.playlists()
    }

    pub fn configure_user_playlists(
        &mut self,
        current: UserPlaylistFile,
        legacy: &UserPlaylistFile,
    ) {
        self.state.user_playlists = UserPlaylistController::load(current, legacy);
    }

    /// Creates one user playlist with Python-compatible timestamps.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed playlist collection cannot be persisted.
    pub fn create_user_playlist(
        &mut self,
        title: &str,
        timestamp: f64,
    ) -> Result<PlaylistCreateOutcome, UserPlaylistControllerError> {
        self.state.user_playlists.create(title, timestamp)
    }

    /// Creates one user playlist containing an initial item in one transaction.
    ///
    /// # Errors
    ///
    /// Returns an error when the complete playlist cannot be persisted.
    pub fn create_user_playlist_with_item(
        &mut self,
        title: &str,
        item: MediaItem,
        timestamp: f64,
    ) -> Result<PlaylistCreateOutcome, UserPlaylistControllerError> {
        self.state
            .user_playlists
            .create_with_item(title, item, timestamp)
    }

    /// Removes one complete user playlist.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed playlist collection cannot be persisted.
    pub fn remove_user_playlist(
        &mut self,
        index: usize,
    ) -> Result<Option<UserPlaylist>, UserPlaylistControllerError> {
        self.state.user_playlists.remove_playlist(index)
    }

    /// Adds one item to a user playlist using durable identity deduplication.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed playlist cannot be persisted.
    pub fn add_item_to_user_playlist(
        &mut self,
        playlist_index: usize,
        item: MediaItem,
        timestamp: f64,
    ) -> Result<PlaylistAddOutcome, UserPlaylistControllerError> {
        self.state
            .user_playlists
            .add_item(playlist_index, item, timestamp)
    }

    /// Removes one item from a user playlist.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed playlist cannot be persisted.
    pub fn remove_user_playlist_item(
        &mut self,
        playlist_index: usize,
        item_index: usize,
        timestamp: f64,
    ) -> Result<Option<MediaItem>, UserPlaylistControllerError> {
        self.state
            .user_playlists
            .remove_item(playlist_index, item_index, timestamp)
    }

    pub fn user_playlist_matches(&self, item: &MediaItem) -> Vec<usize> {
        self.state.user_playlists.matching_playlist_indices(item)
    }

    pub fn prepare_user_playlist_item_playback(
        &mut self,
        playlist_index: usize,
        item_index: usize,
    ) -> Option<MediaItem> {
        let playlist = self.state.user_playlists.playlists().get(playlist_index)?;
        let item = playlist.items.get(item_index)?.clone();
        self.state.player_sequence.clear();
        Some(item)
    }

    pub fn prepare_user_playlist_playback(
        &mut self,
        playlist_index: usize,
        shuffle: bool,
    ) -> Option<MediaItem> {
        let playlist = self.state.user_playlists.playlists().get(playlist_index)?;
        let mut items: Vec<_> = playlist
            .items
            .iter()
            .filter(|item| item.is_playable())
            .cloned()
            .collect();
        if shuffle {
            items.shuffle(&mut rand::rng());
        }
        let current = items.first()?.clone();
        let _ = self.state.player_sequence.set(
            PlaybackSequenceSource::UserPlaylist { playlist_index },
            &items,
            &current,
        );
        Some(current)
    }

    /// Adds every playable item from one user playlist with one durable queue write.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed playback queue cannot be persisted.
    pub fn add_user_playlist_to_playback_queue(
        &mut self,
        playlist_index: usize,
    ) -> Result<Option<QueueBatchAddOutcome>, PlaybackQueueControllerError> {
        let Some(playlist) = self.state.user_playlists.playlists().get(playlist_index) else {
            return Ok(None);
        };
        self.state
            .playback_queue
            .add_many(
                playlist
                    .items
                    .iter()
                    .filter(|item| item.is_playable())
                    .cloned(),
            )
            .map(Some)
    }

    pub const fn local_folder_session(&self) -> &crate::LocalFolderSession {
        &self.state.local_folder
    }

    pub fn current_route(&self) -> Route {
        self.state.navigation.current().route
    }

    pub fn navigate_to(&mut self, frame: RouteFrame) {
        self.state.navigation.push(frame);
    }

    pub fn navigate_back(&mut self) -> Option<RouteFrame> {
        self.state.navigation.back()
    }

    pub fn navigate_main_menu(&mut self) {
        self.state.navigation.reset();
        self.state.youtube_collections.clear();
    }

    pub fn update_trending_route_context(
        &mut self,
        country_index: usize,
        category_index: usize,
        country_code: &str,
        category_code: &str,
    ) {
        if self.state.navigation.current().route != Route::Trending {
            return;
        }
        let mut frame = self.state.navigation.current().clone();
        frame
            .parameters
            .insert("country_index".to_owned(), Value::from(country_index));
        frame
            .parameters
            .insert("category_index".to_owned(), Value::from(category_index));
        frame.parameters.insert(
            "country_code".to_owned(),
            Value::String(country_code.to_owned()),
        );
        frame.parameters.insert(
            "category_code".to_owned(),
            Value::String(category_code.to_owned()),
        );
        self.state.navigation.replace(frame);
    }

    /// Starts a `YouTube` search using the current result-limit setting.
    ///
    /// # Errors
    ///
    /// Returns an error when the normalized query is empty.
    pub fn begin_youtube_search(
        &mut self,
        query: &str,
        kind: YoutubeSearchKind,
    ) -> Result<SearchWork, SearchSessionError> {
        self.state.youtube_collections.clear();
        self.state
            .search
            .begin(query, kind, self.settings.current().results_limit)
    }

    /// Starts one fixed-size official Trending result session.
    ///
    /// # Errors
    ///
    /// Returns an error only if the generated session identity is invalid.
    pub fn begin_youtube_trending(
        &mut self,
        country_code: &'static str,
        category_code: &'static str,
    ) -> Result<YoutubeTrendingWork, SearchSessionError> {
        self.state.youtube_collections.clear();
        let configured = self.settings.current().results_limit;
        let limit = if configured == 0 {
            50
        } else {
            u32::try_from(configured.clamp(1, 50)).unwrap_or(50)
        };
        let query = format!("official trending {country_code} {category_code}");
        let search = self
            .state
            .search
            .begin_fixed(&query, YoutubeSearchKind::Video, limit)?;
        Ok(YoutubeTrendingWork {
            generation: search.generation,
            country_code,
            category_code,
            limit,
        })
    }

    pub fn request_more_search_results(&mut self) -> Option<SearchWork> {
        self.state.search.request_more()
    }

    pub fn cancel_pending_search(&mut self) -> bool {
        self.state.search.cancel_pending()
    }

    pub fn apply_search_results(
        &mut self,
        generation: u64,
        items: Vec<MediaItem>,
        continuation: Option<String>,
    ) -> SearchApplyOutcome {
        let outcome = self
            .state
            .search
            .apply_results(generation, items, continuation);
        if matches!(
            outcome,
            SearchApplyOutcome::Replaced | SearchApplyOutcome::Appended { .. }
        ) {
            let source = PlaybackSequenceSource::Search { generation };
            let _ = self
                .state
                .player_sequence
                .sync(source, self.state.search.items());
        }
        outcome
    }

    pub fn apply_search_metadata(&mut self, generation: u64, hydrated: &MediaItem) -> bool {
        if !self.state.search.apply_metadata(generation, hydrated) {
            return false;
        }
        let source = PlaybackSequenceSource::Search { generation };
        let _ = self
            .state
            .player_sequence
            .sync(source, self.state.search.items());
        true
    }

    pub fn fail_search(&mut self, generation: u64, message: impl Into<String>) -> bool {
        self.state.search.fail(generation, message)
    }

    pub fn select_search_result(&mut self, index: usize) -> bool {
        self.state.search.select(index)
    }

    pub fn prepare_search_playback(&mut self, index: usize) -> Option<MediaItem> {
        if !self.state.search.select(index) {
            return None;
        }
        let item = self.state.search.selected_item()?.clone();
        if item.is_playable() {
            let source = PlaybackSequenceSource::Search {
                generation: self.state.search.generation(),
            };
            let _ = self
                .state
                .player_sequence
                .set(source, self.state.search.items(), &item);
        }
        Some(item)
    }

    pub fn youtube_collection(&self) -> Option<&YoutubeCollectionSession> {
        self.state.youtube_collections.current()
    }

    /// Pushes a nested `YouTube` collection without modifying its parent search
    /// or collection session.
    ///
    /// # Errors
    ///
    /// Returns an error when the collection URL is empty.
    pub fn begin_youtube_collection(
        &mut self,
        title: impl Into<String>,
        url: impl Into<String>,
        kind: YoutubeCollectionKind,
    ) -> Result<YoutubeCollectionWork, YoutubeCollectionError> {
        self.state.youtube_collections.begin(
            title,
            url,
            kind,
            self.settings.current().results_limit,
        )
    }

    pub fn request_more_youtube_collection_results(&mut self) -> Option<YoutubeCollectionWork> {
        self.state.youtube_collections.request_more()
    }

    pub fn cancel_pending_youtube_collection(&mut self) -> bool {
        self.state.youtube_collections.cancel_pending()
    }

    pub fn apply_youtube_collection_results(
        &mut self,
        generation: u64,
        items: Vec<MediaItem>,
    ) -> YoutubeCollectionApplyOutcome {
        let outcome = self
            .state
            .youtube_collections
            .apply_results(generation, items);
        if matches!(
            outcome,
            YoutubeCollectionApplyOutcome::Replaced
                | YoutubeCollectionApplyOutcome::Appended { .. }
        ) {
            let source = PlaybackSequenceSource::YoutubeCollection { generation };
            if let Some(collection) = self.state.youtube_collections.current() {
                let _ = self.state.player_sequence.sync(source, collection.items());
            }
        }
        outcome
    }

    pub fn apply_youtube_collection_metadata(
        &mut self,
        generation: u64,
        hydrated: &MediaItem,
    ) -> bool {
        if !self
            .state
            .youtube_collections
            .apply_metadata(generation, hydrated)
        {
            return false;
        }
        let source = PlaybackSequenceSource::YoutubeCollection { generation };
        if let Some(collection) = self.state.youtube_collections.current() {
            let _ = self.state.player_sequence.sync(source, collection.items());
        }
        true
    }

    pub fn fail_youtube_collection(&mut self, generation: u64, message: impl Into<String>) -> bool {
        self.state.youtube_collections.fail(generation, message)
    }

    pub fn select_youtube_collection_result(&mut self, index: usize) -> bool {
        self.state.youtube_collections.select(index)
    }

    pub fn prepare_youtube_collection_playback(&mut self, index: usize) -> Option<MediaItem> {
        if !self.state.youtube_collections.select(index) {
            return None;
        }
        let collection = self.state.youtube_collections.current()?;
        let item = collection.selected_item()?.clone();
        if item.is_playable() {
            let source = PlaybackSequenceSource::YoutubeCollection {
                generation: collection.generation(),
            };
            let _ = self
                .state
                .player_sequence
                .set(source, collection.items(), &item);
        }
        Some(item)
    }

    pub fn prepare_youtube_playlist_playback(
        &mut self,
        token: u64,
        items: Vec<MediaItem>,
        shuffle: bool,
    ) -> Option<MediaItem> {
        let mut playable: Vec<_> = items.into_iter().filter(MediaItem::is_playable).collect();
        if shuffle {
            playable.shuffle(&mut rand::rng());
        }
        let current = playable.first()?.clone();
        let _ = self.state.player_sequence.set(
            PlaybackSequenceSource::YoutubePlaylist { token },
            &playable,
            &current,
        );
        Some(current)
    }

    pub fn pop_youtube_collection(&mut self) -> bool {
        self.state.youtube_collections.pop().is_some()
    }

    pub fn load_local_folder(&mut self, path: PathBuf, items: Vec<MediaItem>) {
        self.state.local_folder.load(path, items);
    }

    pub fn select_local_folder_item(&mut self, index: usize) -> bool {
        self.state.local_folder.select(index)
    }

    pub fn prepare_local_folder_playback(
        &mut self,
        index: usize,
        shuffle: bool,
    ) -> Option<MediaItem> {
        if !self.state.local_folder.select(index) {
            return None;
        }
        let mut items = self.state.local_folder.items().to_vec();
        let mut current = self.state.local_folder.selected_item()?.clone();
        if shuffle {
            items.shuffle(&mut rand::rng());
            current = items.first()?.clone();
            if let Some(source_index) = self
                .state
                .local_folder
                .items()
                .iter()
                .position(|item| item.stable_identity() == current.stable_identity())
            {
                let _ = self.state.local_folder.select(source_index);
            }
        }
        let source = PlaybackSequenceSource::LocalFolder {
            generation: self.state.local_folder.generation(),
        };
        let _ = self.state.player_sequence.set(source, &items, &current);
        Some(current)
    }

    /// Adds every item in the current local folder with one durable write.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed queue cannot be persisted.
    pub fn add_local_folder_to_playback_queue(
        &mut self,
    ) -> Result<QueueBatchAddOutcome, PlaybackQueueControllerError> {
        self.state
            .playback_queue
            .add_many(self.state.local_folder.items().iter().cloned())
    }

    pub fn request_relative_player_item(&mut self, delta: i32) -> PlayerNavigationOutcome {
        let sequence_active = self.state.player_sequence.is_active();
        if delta > 0
            && !sequence_active
            && let Some(item) = self.state.playback_queue.queue().front()
        {
            return PlayerNavigationOutcome::Item {
                item: Box::new(item.clone()),
                origin: PlayerNavigationOrigin::Queue,
            };
        }
        // Python `relative_player_item`: podcast and user playlist episodes
        // keep their order while a next one exists; otherwise shuffle picks
        // any other item of the list.
        let ordered_source = matches!(
            self.state.player_sequence.source(),
            Some(
                PlaybackSequenceSource::RssFeed { .. }
                    | PlaybackSequenceSource::UserPlaylist { .. }
            )
        ) && self.state.player_sequence.relative(delta).is_some();
        if delta > 0
            && !ordered_source
            && self
                .state
                .player
                .enabled_toggles()
                .contains(&SessionToggle::Shuffle)
            && let Some(item) = self.state.player_sequence.random_next()
        {
            return PlayerNavigationOutcome::Item {
                item: Box::new(item),
                origin: PlayerNavigationOrigin::Sequence,
            };
        }
        if let Some(item) = self.state.player_sequence.relative(delta) {
            return PlayerNavigationOutcome::Item {
                item: Box::new(item),
                origin: PlayerNavigationOrigin::Sequence,
            };
        }
        if delta > 0
            && self.state.player_sequence.source()
                == Some(PlaybackSequenceSource::Search {
                    generation: self.state.search.generation(),
                })
            && let Some(work) = self.state.search.request_more()
        {
            return PlayerNavigationOutcome::LoadingMore(work);
        }
        if delta > 0
            && self
                .state
                .youtube_collections
                .current()
                .is_some_and(|collection| {
                    self.state.player_sequence.source()
                        == Some(PlaybackSequenceSource::YoutubeCollection {
                            generation: collection.generation(),
                        })
                })
            && let Some(work) = self.state.youtube_collections.request_more()
        {
            return PlayerNavigationOutcome::LoadingMoreCollection(work);
        }
        if delta > 0
            && let Some(item) = self.state.playback_queue.queue().front()
        {
            return PlayerNavigationOutcome::Item {
                item: Box::new(item.clone()),
                origin: PlayerNavigationOrigin::Queue,
            };
        }
        PlayerNavigationOutcome::Unavailable
    }

    pub const fn player_sequence_source(&self) -> Option<PlaybackSequenceSource> {
        self.state.player_sequence.source()
    }

    pub fn player_sequence_contains(&self, item: &MediaItem) -> bool {
        self.state.player_sequence.contains(item)
    }

    pub const fn playback_queue(&self) -> &PlaybackQueue {
        self.state.playback_queue.queue()
    }

    pub fn configure_playback_queue(
        &mut self,
        current: PlaybackQueueFile,
        legacy: &PlaybackQueueFile,
    ) {
        self.state.playback_queue = PlaybackQueueController::load(current, legacy);
    }

    pub fn playback_queue_load_error(&self) -> Option<&str> {
        self.state.playback_queue.load_error()
    }

    /// Replaces and atomically persists the complete playback queue.
    ///
    /// # Errors
    ///
    /// Returns an error when queue persistence is blocked or fails.
    pub fn replace_playback_queue(
        &mut self,
        items: Vec<MediaItem>,
    ) -> Result<(), PlaybackQueueControllerError> {
        self.state.playback_queue.replace(items)
    }

    /// Adds and atomically persists one playable queue item.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed queue cannot be persisted.
    pub fn add_to_playback_queue(
        &mut self,
        item: MediaItem,
    ) -> Result<QueueAddOutcome, PlaybackQueueControllerError> {
        self.state.playback_queue.add(item)
    }

    /// Removes and atomically persists the matching queue item.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed queue cannot be persisted.
    pub fn remove_from_playback_queue(
        &mut self,
        item: &MediaItem,
    ) -> Result<bool, PlaybackQueueControllerError> {
        Ok(self.state.playback_queue.remove_item(item)?.is_some())
    }

    /// Removes and atomically persists one queue position.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed queue cannot be persisted.
    pub fn remove_playback_queue_index(
        &mut self,
        index: usize,
    ) -> Result<Option<MediaItem>, PlaybackQueueControllerError> {
        self.state.playback_queue.remove(index)
    }

    /// Reorders and atomically persists one queue position.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed queue cannot be persisted.
    pub fn move_playback_queue_item(
        &mut self,
        index: usize,
        delta: i32,
    ) -> Result<Option<usize>, PlaybackQueueControllerError> {
        self.state.playback_queue.move_by(index, delta)
    }

    /// Clears and atomically persists the playback queue.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed queue cannot be persisted.
    pub fn clear_playback_queue(&mut self) -> Result<bool, PlaybackQueueControllerError> {
        self.state.playback_queue.clear()
    }

    /// Consumes the front item after its player start has been confirmed.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed queue cannot be persisted.
    pub fn confirm_queued_item_started(
        &mut self,
        item: &MediaItem,
    ) -> Result<bool, PlaybackQueueControllerError> {
        self.state.playback_queue.consume_front_if(item)
    }

    pub fn player_screen_model(&self) -> Option<PlayerScreenModel> {
        let item = self.state.player.current_item()?;
        Some(PlayerScreenModel::build(
            &embedded_catalog(&self.settings.current().language),
            self.settings.current(),
            item,
            &PlayerViewState::from(&self.state.player),
        ))
    }

    pub fn player_format_status(&self) -> Option<String> {
        let item = self.state.player.current_item()?;
        Some(crate::player_information::format_status(
            &embedded_catalog(&self.settings.current().language),
            item,
            self.state.player.media_info(),
        ))
    }

    pub fn player_details_text(&self) -> Option<String> {
        let session = &self.state.player;
        let item = session.current_item()?;
        let audio = session.audio()?;
        Some(crate::player_information::details_text(
            &embedded_catalog(&self.settings.current().language),
            item,
            audio.speed,
            audio.pitch,
        ))
    }

    pub fn prepare_last_player_session_resume(&mut self) -> Option<LastSessionResume> {
        let mut session = self.state.last_player_session.session()?.clone();
        let item = session.item.clone();
        let current_identity = item.stable_identity()?;
        if session.return_screen == "rss_items"
            && let Some((feed_index, item_index)) = self.rss_episode_location(&item)
        {
            session
                .return_data
                .insert("feed_index".to_owned(), Value::from(feed_index));
            session
                .return_data
                .insert("item_index".to_owned(), Value::from(item_index));
        }
        let sequence: Vec<_> = session
            .sequence
            .iter()
            .filter(|candidate| candidate.is_playable())
            .cloned()
            .collect();
        let sequence = if sequence
            .iter()
            .any(|candidate| candidate.stable_identity().as_deref() == Some(&current_identity))
        {
            sequence
        } else {
            Vec::new()
        };

        self.state.player_sequence.clear();
        self.state.navigation.reset();
        let return_route = self.restore_last_session_context(&session, &sequence, &item);
        if return_route != Route::MainMenu {
            let mut frame = RouteFrame::new(return_route);
            frame.parameters.clone_from(&session.return_data);
            self.state.navigation.push(frame);
        }
        let sequence_active = self.restore_last_session_sequence(&session, &sequence, &item);
        Some(LastSessionResume {
            item,
            sequence_active,
            return_screen: session.return_screen,
            return_data: session.return_data,
        })
    }

    fn restore_last_session_context(
        &mut self,
        session: &LastPlayerSession,
        sequence: &[MediaItem],
        current: &MediaItem,
    ) -> Route {
        match session.return_screen.as_str() {
            "search" | "trending" if !sequence.is_empty() => {
                let selected_index = sequence
                    .iter()
                    .position(|candidate| candidate.stable_identity() == current.stable_identity())
                    .unwrap_or_else(|| json_usize(&session.return_data, "index"));
                let query = json_text(&session.return_data, "query");
                let kind = session
                    .return_data
                    .get("search_kind")
                    .cloned()
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or(YoutubeSearchKind::All);
                if self.state.search.restore_snapshot(
                    query,
                    kind,
                    sequence.to_vec(),
                    selected_index,
                ) {
                    if session.return_screen == "trending" {
                        Route::Trending
                    } else {
                        Route::Results
                    }
                } else {
                    Route::MainMenu
                }
            }
            "folder" if !sequence.is_empty() => {
                let folder = PathBuf::from(json_text(&session.return_data, "folder"));
                if folder.as_os_str().is_empty()
                    || sequence
                        .iter()
                        .any(|candidate| candidate.local_path.is_none())
                {
                    return Route::MainMenu;
                }
                self.state.local_folder.load(folder, sequence.to_vec());
                if let Some(index) = sequence
                    .iter()
                    .position(|candidate| candidate.stable_identity() == current.stable_identity())
                {
                    let _ = self.state.local_folder.select(index);
                }
                Route::LocalFolder
            }
            "favorites" => Route::Favorites,
            "history" => Route::History,
            "notification_center" => Route::NotificationCenter,
            "direct_link" => Route::DirectLink,
            "bookmarks" => Route::Bookmarks,
            "user_playlist_items"
                if json_usize(&session.return_data, "playlist_index")
                    < self.state.user_playlists.playlists().len() =>
            {
                Route::UserPlaylistItems
            }
            "rss_items" => {
                let feed_index = json_usize(&session.return_data, "feed_index");
                let item_index = json_usize(&session.return_data, "item_index");
                if self
                    .state
                    .rss_feeds
                    .feeds()
                    .get(feed_index)
                    .and_then(|feed| feed.items.get(item_index))
                    .is_some()
                {
                    Route::RssItems
                } else {
                    Route::MainMenu
                }
            }
            _ => Route::MainMenu,
        }
    }

    fn restore_last_session_sequence(
        &mut self,
        session: &LastPlayerSession,
        sequence: &[MediaItem],
        current: &MediaItem,
    ) -> bool {
        if sequence.is_empty() {
            return false;
        }
        let source = match session.return_screen.as_str() {
            "search" | "trending" => PlaybackSequenceSource::Search {
                generation: self.state.search.generation(),
            },
            "folder" => PlaybackSequenceSource::LocalFolder {
                generation: self.state.local_folder.generation(),
            },
            "user_playlist_items" => PlaybackSequenceSource::UserPlaylist {
                playlist_index: json_usize(&session.return_data, "playlist_index"),
            },
            "rss_items" => PlaybackSequenceSource::RssFeed {
                feed_index: json_usize(&session.return_data, "feed_index"),
            },
            _ => PlaybackSequenceSource::Collection,
        };
        self.state.player_sequence.set(source, sequence, current)
    }

    pub fn start_player_item(&mut self, item: MediaItem) -> u64 {
        self.start_player_item_at(item, None)
    }

    pub fn start_player_item_at(
        &mut self,
        item: MediaItem,
        initial_position_seconds: Option<f64>,
    ) -> u64 {
        let sequence_source = self.state.player_sequence.source();
        if self.state.player_sequence.activate(&item) {
            self.sync_sequence_source_selection(sequence_source, &item);
        }
        // Python `play_url`: a new session forgets the related videos, and
        // every played YouTube video counts as seen.
        if !self.state.player.is_open() {
            self.related_seen_ids.clear();
        }
        if let Some(video_id) = item.youtube_video_id() {
            self.related_seen_ids.insert(video_id);
        }
        let settings = self.settings.current();
        let mut toggles = BTreeSet::new();
        if settings.autoplay_next {
            toggles.insert(SessionToggle::AutoplayNext);
        }
        if settings.volume_boost_by_default {
            toggles.insert(SessionToggle::VolumeBoost);
        }
        if settings.player_fullscreen {
            toggles.insert(SessionToggle::Fullscreen);
        }
        let defaults = PlayerSessionDefaults {
            audio: AudioSession {
                volume: f64::from(i32::try_from(settings.default_volume).unwrap_or(100)),
                output_device: settings.audio_output_device.clone(),
                speed: player_start_speed(&settings.player_speed),
                pitch: 1.0,
                equalizer: EqualizerSession {
                    enabled: settings.global_equalizer_enabled,
                    gains: settings.global_equalizer_gains.clone(),
                },
            },
            enabled_toggles: toggles,
            starts_paused: settings.player_start_paused,
        };
        let generation = self
            .state
            .player
            .start_item_at(item, defaults, initial_position_seconds);
        self.save_last_player_session_snapshot();
        generation
    }

    pub fn start_player_item_with_shuffle(
        &mut self,
        item: MediaItem,
        shuffle: Option<bool>,
    ) -> u64 {
        self.start_player_item_with_shuffle_at(item, shuffle, None)
    }

    pub fn start_player_item_with_shuffle_at(
        &mut self,
        item: MediaItem,
        shuffle: Option<bool>,
        initial_position_seconds: Option<f64>,
    ) -> u64 {
        let generation = self.start_player_item_at(item, initial_position_seconds);
        if let Some(enabled) = shuffle {
            self.state
                .player
                .set_toggle(SessionToggle::Shuffle, enabled);
        }
        generation
    }

    fn sync_sequence_source_selection(
        &mut self,
        source: Option<PlaybackSequenceSource>,
        item: &MediaItem,
    ) {
        let Some(identity) = item.stable_identity() else {
            return;
        };
        match source {
            Some(PlaybackSequenceSource::Search { generation })
                if generation == self.state.search.generation() =>
            {
                if let Some(index) =
                    self.state.search.items().iter().position(|candidate| {
                        candidate.stable_identity().as_deref() == Some(&identity)
                    })
                {
                    let _ = self.state.search.select(index);
                }
            }
            Some(PlaybackSequenceSource::LocalFolder { generation })
                if generation == self.state.local_folder.generation() =>
            {
                if let Some(index) = self
                    .state
                    .local_folder
                    .items()
                    .iter()
                    .position(|candidate| candidate.stable_identity().as_deref() == Some(&identity))
                {
                    let _ = self.state.local_folder.select(index);
                }
            }
            _ => {}
        }
    }

    fn save_last_player_session_snapshot(&mut self) {
        let Some(item) = self.state.player.current_item().cloned() else {
            return;
        };
        let frame = self.state.navigation.player_return_frame().clone();
        let (return_screen, mut return_data) = self.last_session_return_context(&frame, &item);
        for (key, value) in frame.parameters {
            return_data.entry(key).or_insert(value);
        }
        let sequence = if self.state.player_sequence.contains(&item) {
            self.state
                .player_sequence
                .items()
                .iter()
                .take(200)
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let snapshot =
            LastPlayerSession::new(unix_timestamp(), item, return_screen, return_data, sequence);
        let _ = self.state.last_player_session.replace(snapshot);
    }

    fn last_session_return_context(
        &self,
        frame: &RouteFrame,
        item: &MediaItem,
    ) -> (&'static str, Map<String, Value>) {
        let mut data = Map::new();
        match frame.route {
            Route::Results => {
                data.insert(
                    "index".to_owned(),
                    Value::from(self.state.search.selected_index()),
                );
                data.insert(
                    "query".to_owned(),
                    Value::String(self.state.search.query().to_owned()),
                );
                data.insert(
                    "search_kind".to_owned(),
                    serde_json::to_value(self.state.search.kind()).unwrap_or(Value::Null),
                );
                ("search", data)
            }
            Route::LocalFolder => {
                data.insert(
                    "index".to_owned(),
                    Value::from(self.state.local_folder.selected_index()),
                );
                data.insert(
                    "folder".to_owned(),
                    Value::String(
                        self.state
                            .local_folder
                            .path()
                            .to_string_lossy()
                            .into_owned(),
                    ),
                );
                ("folder", data)
            }
            Route::Favorites => {
                insert_matching_index(&mut data, self.state.favorites.items(), item);
                ("favorites", data)
            }
            Route::History => {
                insert_matching_index(&mut data, self.state.history.items(), item);
                ("history", data)
            }
            Route::DirectLink => ("direct_link", data),
            Route::Bookmarks => ("bookmarks", data),
            Route::UserPlaylistItems => {
                if let Some(PlaybackSequenceSource::UserPlaylist { playlist_index }) =
                    self.state.player_sequence.source()
                {
                    data.insert("playlist_index".to_owned(), Value::from(playlist_index));
                    if let Some(playlist) =
                        self.state.user_playlists.playlists().get(playlist_index)
                    {
                        insert_matching_index_as(&mut data, "item_index", &playlist.items, item);
                    }
                }
                ("user_playlist_items", data)
            }
            Route::PlaybackQueue => ("playback_queue", data),
            Route::RssItems => {
                if let Some((feed_index, item_index)) = self.rss_episode_location(item) {
                    data.insert("feed_index".to_owned(), Value::from(feed_index));
                    data.insert("item_index".to_owned(), Value::from(item_index));
                }
                ("rss_items", data)
            }
            Route::NotificationCenter => ("notification_center", data),
            Route::Subscriptions => ("subscriptions", data),
            Route::Trending => {
                data.insert(
                    "index".to_owned(),
                    Value::from(self.state.search.selected_index()),
                );
                data.insert(
                    "query".to_owned(),
                    Value::String(self.state.search.query().to_owned()),
                );
                data.insert(
                    "search_kind".to_owned(),
                    serde_json::to_value(self.state.search.kind()).unwrap_or(Value::Null),
                );
                ("trending", data)
            }
            Route::AudiovaultMenu
            | Route::AudiovaultSearch
            | Route::AudiovaultResults
            | Route::AudiovaultEpisodes => ("audiovault", data),
            _ if item.local_path.is_some() => ("local_file", data),
            _ => ("main_menu", data),
        }
    }

    pub fn apply_playback_event(&mut self, generation: u64, event: PlaybackEvent) -> bool {
        self.state.player.apply_event(generation, event)
    }

    pub fn cache_external_chapters(&mut self, generation: u64, chapters: Vec<Value>) -> bool {
        self.state
            .player
            .cache_external_chapters(generation, chapters)
    }

    pub fn set_player_volume(&mut self, volume: f64) {
        self.state.player.set_volume(volume);
    }

    pub fn cache_transcript(
        &mut self,
        generation: u64,
        transcript: crate::transcript::CachedTranscript,
    ) -> bool {
        self.state.player.cache_transcript(generation, transcript)
    }

    pub fn toggle_player_clip_marker(&mut self, start: bool) -> Option<f64> {
        if start {
            self.state.player.toggle_clip_start()
        } else {
            self.state.player.toggle_clip_end()
        }
    }

    pub fn set_player_speed(&mut self, speed: f64) {
        self.state.player.set_speed(speed);
    }

    pub fn set_player_pitch(&mut self, pitch: f64) {
        self.state.player.set_pitch(pitch);
    }

    pub fn set_player_toggle(&mut self, toggle: SessionToggle, enabled: bool) {
        self.state.player.set_toggle(toggle, enabled);
    }

    pub fn prepare_standalone_playback(&mut self) {
        self.state.player_sequence.clear();
    }

    pub fn close_player_session(&mut self) {
        self.state.player.close();
        self.state.player_sequence.clear();
        self.related_seen_ids.clear();
    }

    /// Python `apply_related_videos_and_play`: the unseen related videos
    /// replace the search results and the first of them plays next.
    pub fn apply_related_videos(&mut self, videos: Vec<MediaItem>) -> Option<MediaItem> {
        let mut seen = self.related_seen_ids.clone();
        let unseen: Vec<_> = videos
            .into_iter()
            .filter(|video| {
                video
                    .youtube_video_id()
                    .is_some_and(|video_id| seen.insert(video_id))
            })
            .collect();
        if unseen.is_empty() {
            return None;
        }
        let query = self.state.search.query().to_owned();
        let kind = self.state.search.kind();
        if !self.state.search.restore_snapshot(query, kind, unseen, 0) {
            return None;
        }
        let item = self.prepare_search_playback(0)?;
        if let Some(video_id) = item.youtube_video_id() {
            self.related_seen_ids.insert(video_id);
        }
        Some(item)
    }

    /// Python `cycle_replaygain_mode`: Off, Track and Album in turn, saved
    /// at once. Returns the new mode.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings cannot be saved.
    pub fn cycle_replaygain_mode(&mut self) -> Result<String, SettingsControllerError> {
        let next = match self.settings.current().replaygain_mode.as_str() {
            "no" => "track",
            "track" => "album",
            _ => "no",
        };
        self.settings
            .set_value(SettingId::ReplaygainMode, serde_json::json!(next))?;
        let _ = self.settings.save()?;
        Ok(next.to_owned())
    }

    pub fn enqueue_activation(&mut self, request: ActivationRequest) {
        if request == ActivationRequest::Show
            && self
                .activation_requests
                .back()
                .is_some_and(|queued| *queued == ActivationRequest::Show)
        {
            return;
        }
        self.activation_requests.push_back(request);
    }

    pub fn take_activation(&mut self) -> Option<ActivationRequest> {
        self.activation_requests.pop_front()
    }

    pub fn main_menu_model(&self) -> MainMenuModel {
        let settings = self.settings.current();
        let availability = self.current_menu_availability();
        MainMenuModel::build(
            &embedded_catalog(&settings.language),
            availability,
            &settings.main_menu_hidden_actions,
            settings.show_shortcuts_in_labels,
            &settings.keyboard_shortcuts,
        )
    }

    /// Python `action_finder_actions` for the current application state.
    pub fn action_finder_model(&self) -> ActionFinderModel {
        let settings = self.settings.current();
        let player = &self.state.player;
        let context = ActionFinderContext {
            resume_available: self.state.last_player_session.is_available(),
            player: player
                .is_open()
                .then(|| player.current_item())
                .flatten()
                .map(|item| ActionFinderPlayer {
                    paused: player.phase() == PlaybackPhase::Paused,
                    local_media: item.is_local_media(),
                    youtube: crate::context_menu::has_youtube_url(item),
                    podcast_episode: item.kind == apricot_core::MediaKind::PodcastEpisode,
                }),
        };
        ActionFinderModel::build(&embedded_catalog(&settings.language), settings, context)
    }

    fn current_menu_availability(&self) -> MainMenuAvailability {
        let settings = self.settings.current();
        let mut availability = self.menu_availability;
        availability.trending = visibility(settings.enable_trending);
        availability.history = visibility(settings.enable_history);
        availability.podcasts = visibility(settings.enable_podcasts_rss);
        availability.download_count = self.state.downloads.count();
        availability.playback_queue_count = self.state.playback_queue.queue().len();
        availability.resume = visibility(
            settings.show_resume_in_menu && self.state.last_player_session.is_available(),
        );
        availability
    }

    pub fn settings_model(&self, section: SettingsSection) -> SettingsScreenModel {
        let settings = self.settings.current();
        SettingsScreenModel::build(
            &embedded_catalog(&settings.language),
            settings,
            &self.settings.settings_file(),
            section,
        )
    }

    pub fn settings(&self) -> &SettingsDocument {
        self.settings.current()
    }

    pub fn settings_file(&self) -> PathBuf {
        self.settings.settings_file()
    }

    pub fn settings_are_dirty(&self) -> bool {
        self.settings.is_dirty()
    }

    /// Updates a string setting in the current draft.
    ///
    /// # Errors
    ///
    /// Returns an error when the setting does not accept a string.
    pub fn set_string_setting(
        &mut self,
        id: SettingId,
        value: impl Into<String>,
    ) -> Result<(), SettingsControllerError> {
        self.settings
            .set_value(id, serde_json::Value::String(value.into()))
    }

    /// Updates an integer setting in the current draft.
    ///
    /// # Errors
    ///
    /// Returns an error when the setting does not accept an integer.
    pub fn set_integer_setting(
        &mut self,
        id: SettingId,
        value: i64,
    ) -> Result<(), SettingsControllerError> {
        self.settings.set_value(id, value.into())
    }

    /// Updates a floating-point setting in the current draft.
    ///
    /// # Errors
    ///
    /// Returns an error when the setting does not accept a finite number.
    pub fn set_float_setting(
        &mut self,
        id: SettingId,
        value: f64,
    ) -> Result<(), SettingsControllerError> {
        self.settings.set_value(id, serde_json::json!(value))
    }

    /// Updates a boolean setting in the current draft.
    ///
    /// # Errors
    ///
    /// Returns an error when the setting does not accept a boolean.
    pub fn set_boolean_setting(
        &mut self,
        id: SettingId,
        value: bool,
    ) -> Result<(), SettingsControllerError> {
        self.settings.set_value(id, value.into())
    }

    /// Changes one customizable main-menu item without affecting its shortcut.
    ///
    /// # Errors
    ///
    /// Returns an error if the hidden-action setting cannot be updated.
    pub fn set_main_menu_item_visible(
        &mut self,
        action_id: &str,
        visible: bool,
    ) -> Result<(), SettingsControllerError> {
        let mut hidden = self.settings.current().main_menu_hidden_actions.clone();
        hidden.retain(|id| id != action_id);
        if !visible {
            hidden.push(action_id.to_owned());
        }
        self.settings
            .set_value(SettingId::MainMenuHiddenActions, serde_json::json!(hidden))
    }

    /// Updates one equalizer band without changing any other band.
    ///
    /// # Errors
    ///
    /// Returns an error if a typed equalizer map cannot be persisted to the draft.
    pub fn set_equalizer_band_gain(
        &mut self,
        preset_id: &str,
        band_id: &str,
        gain_db: f64,
    ) -> Result<(), SettingsControllerError> {
        if !apricot_core::audio::EQUALIZER_BANDS
            .iter()
            .any(|band| band.id == band_id)
        {
            return Ok(());
        }
        let range = f64::from(
            i32::try_from(self.settings.current().equalizer_db_range).unwrap_or(i32::MAX),
        );
        let gain = ((gain_db.clamp(-range, range) * 10.0).round()) / 10.0;
        let mut current_gains = self.settings.current().global_equalizer_gains.clone();
        current_gains.insert(band_id.to_owned(), gain);
        let is_custom = !apricot_core::audio::FACTORY_EQUALIZER_PRESETS
            .iter()
            .any(|preset| preset.id == preset_id);
        if is_custom {
            let mut presets = self.settings.current().equalizer_preset_gains.clone();
            let gains = presets.entry(preset_id.to_owned()).or_default();
            gains.insert(band_id.to_owned(), gain);
            self.settings.set_values([
                (
                    SettingId::GlobalEqualizerGains,
                    serde_json::json!(current_gains),
                ),
                (SettingId::EqualizerPresetGains, serde_json::json!(presets)),
            ])?;
        } else {
            self.settings.set_value(
                SettingId::GlobalEqualizerGains,
                serde_json::json!(current_gains),
            )?;
        }
        Ok(())
    }

    /// Updates the custom name for one equalizer preset.
    ///
    /// # Errors
    ///
    /// Returns an error if the complete custom-name map cannot be updated.
    pub fn set_equalizer_preset_name(
        &mut self,
        preset_id: &str,
        name: &str,
    ) -> Result<(), SettingsControllerError> {
        let mut names = self.settings.current().equalizer_custom_names.clone();
        let fallback = names
            .get(preset_id)
            .cloned()
            .unwrap_or_else(|| preset_id.to_owned());
        let trimmed = name.trim();
        names.insert(
            preset_id.to_owned(),
            if trimmed.is_empty() {
                fallback
            } else {
                trimmed.chars().take(80).collect()
            },
        );
        self.settings
            .set_value(SettingId::EqualizerCustomNames, serde_json::json!(names))
    }

    /// Sets or clears the equalizer preset associated with one output device.
    ///
    /// # Errors
    ///
    /// Returns an error if the device-preset map cannot be updated.
    pub fn set_equalizer_device_preset(
        &mut self,
        device_id: &str,
        preset_id: &str,
    ) -> Result<(), SettingsControllerError> {
        let mut presets = self.settings.current().equalizer_device_presets.clone();
        if preset_id.trim().is_empty() {
            presets.remove(device_id);
        } else {
            presets.insert(device_id.to_owned(), preset_id.to_owned());
        }
        self.settings.set_value(
            SettingId::EqualizerDevicePresets,
            serde_json::json!(presets),
        )
    }

    /// Assigns a shortcut through the central conflict validator.
    ///
    /// # Errors
    ///
    /// Returns an error for unknown actions or shortcut conflicts.
    pub fn set_keyboard_shortcut(
        &mut self,
        action_id: &str,
        shortcut: &str,
    ) -> Result<(), SettingsControllerError> {
        self.settings.set_shortcut(action_id, shortcut)
    }

    /// Resets one settings section in the current draft.
    ///
    /// # Errors
    ///
    /// Returns an error when canonical reset ownership cannot be applied.
    pub fn reset_settings_section(
        &mut self,
        section: SettingsSection,
    ) -> Result<(), SettingsControllerError> {
        self.settings.reset_section(section)
    }

    pub fn reset_all_settings(&mut self) {
        self.settings.reset_all();
    }

    pub fn cancel_settings(&mut self) {
        self.settings.cancel();
    }

    /// Python `wx_main.py` asks for the language only on a first run without
    /// any settings file, and never when the app starts hidden in the tray.
    pub fn initial_language_prompt_due(
        &self,
        first_run_without_settings: bool,
        started_hidden: bool,
    ) -> bool {
        first_run_without_settings && !self.settings.current().language_prompted && !started_hidden
    }

    /// Completes the one-time language prompt and persists both values atomically.
    ///
    /// An absent or unknown selection keeps the currently configured language,
    /// matching the Python application's cancel behavior.
    ///
    /// # Errors
    ///
    /// Returns an error when the settings draft or atomic save fails.
    pub fn complete_initial_language(
        &mut self,
        selected_language: Option<&str>,
    ) -> Result<(), SettingsControllerError> {
        let language = selected_language
            .filter(|selected| {
                apricot_core::locale::LANGUAGES
                    .iter()
                    .any(|language| language.code == *selected)
            })
            .unwrap_or(&self.settings.current().language)
            .to_owned();
        self.settings.set_values([
            (SettingId::Language, serde_json::json!(language)),
            (SettingId::LanguagePrompted, serde_json::json!(true)),
        ])?;
        let _ = self.settings.save()?;
        self.startup_announcement = Some("settings_saved");
        Ok(())
    }

    /// The catalog key Python announces once the main menu is shown after
    /// the first-run language prompt (`prompt_initial_language`).
    pub fn take_startup_announcement(&mut self) -> Option<&'static str> {
        self.startup_announcement.take()
    }

    /// Saves the complete current settings draft atomically.
    ///
    /// # Errors
    ///
    /// Returns an error without committing the draft if persistence fails.
    pub fn save_settings(&mut self) -> Result<(), SettingsControllerError> {
        let _ = self.settings.save()?;
        Ok(())
    }
}

/// Python's `open_library_item` opens channels and playlists instead of
/// playing them, so they never become the current playback sequence.
const fn is_library_collection(item: &MediaItem) -> bool {
    matches!(
        item.kind,
        apricot_core::MediaKind::Channel | apricot_core::MediaKind::Playlist
    )
}

const fn visibility(enabled: bool) -> MenuVisibility {
    if enabled {
        MenuVisibility::Visible
    } else {
        MenuVisibility::Hidden
    }
}

fn json_text(object: &Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn json_usize(object: &Map<String, Value>, key: &str) -> usize {
    object
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or_default()
}

fn insert_matching_index(
    object: &mut Map<String, Value>,
    items: &[MediaItem],
    current: &MediaItem,
) {
    insert_matching_index_as(object, "index", items, current);
}

fn insert_matching_index_as(
    object: &mut Map<String, Value>,
    key: &str,
    items: &[MediaItem],
    current: &MediaItem,
) {
    if let Some(index) = items
        .iter()
        .position(|candidate| candidate.stable_identity() == current.stable_identity())
    {
        object.insert(key.to_owned(), Value::from(index));
    }
}

fn unix_timestamp() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
}

/// Python `player_start_speed_value`/`default_speed_value`: accepts an
/// optional `x` suffix and clamps to the supported 0.25-4.0 range.
pub fn player_start_speed(value: &str) -> f64 {
    value
        .trim()
        .trim_end_matches(['x', 'X'])
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|speed| speed.is_finite())
        .map_or(1.0, |speed| speed.clamp(0.25, 4.0))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
    };

    use apricot_core::{
        MediaId, MediaItem, MediaKind, MediaSource, Route, RouteFrame, SettingId, SettingsSection,
    };
    use apricot_playback::PlaybackEvent;
    use apricot_storage::{
        AppNotification, LastPlayerSessionFile, MediaListFile, NotificationFile,
        PlaybackPositionFile, RssFeed, RssFeedFile, SettingsDocument, SettingsPaths,
        UserPlaylistFile,
    };
    use tempfile::tempdir;

    use super::{Application, PlayerNavigationOrigin, PlayerNavigationOutcome};
    use crate::{
        ActivationRequest, MainMenuAvailability, PlaybackSequenceSource, PlaylistAddOutcome,
        PlaylistCreateOutcome, SessionToggle, SettingsController, YoutubeCollectionKind,
        YoutubeSearchKind,
    };

    fn application(root: &Path) -> Application {
        let paths = SettingsPaths::for_app_data(&root.join("beta"), &root.join("stable"));
        Application::new(
            SettingsController::load(paths, SettingsDocument::default()),
            MainMenuAvailability::default(),
        )
    }

    fn media_item(id: &str) -> MediaItem {
        MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: id.to_owned(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: Some(format!(r"C:\Music\{id}.mp3")),
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    fn youtube_item(index: usize, kind: MediaKind) -> MediaItem {
        MediaItem {
            id: MediaId(index.to_string()),
            source: MediaSource::Youtube,
            kind,
            title: format!("Item {index}"),
            url: Some(
                format!("https://www.youtube.com/watch?v=item{index}")
                    .parse()
                    .expect("URL"),
            ),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    fn podcast_item(index: usize) -> MediaItem {
        MediaItem {
            id: MediaId(format!("episode-{index}")),
            source: MediaSource::Podcast,
            kind: MediaKind::PodcastEpisode,
            title: format!("Episode {index}"),
            url: Some(
                format!("https://media.example/episode-{index}.mp3")
                    .parse()
                    .expect("URL"),
            ),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: "Podcast".to_owned(),
            duration_seconds: Some(1_800.0),
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn player_start_speed_matches_python_parsing_and_clamping() {
        assert!((super::player_start_speed("1.25x") - 1.25).abs() < f64::EPSILON);
        assert!((super::player_start_speed(" 0.1 ") - 0.25).abs() < f64::EPSILON);
        assert!((super::player_start_speed("9") - 4.0).abs() < f64::EPSILON);
        assert!((super::player_start_speed("fast") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn menu_projection_reflects_unsaved_draft_without_hiding_shortcuts() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.set_main_menu_item_visible("search", false)
            .expect("hide search");
        assert!(
            app.main_menu_model()
                .items
                .iter()
                .all(|item| item.id != "search")
        );
        assert_eq!(
            app.settings().keyboard_shortcuts["open_search"],
            "Ctrl+Alt+Y"
        );
        app.cancel_settings();
        assert!(
            app.main_menu_model()
                .items
                .iter()
                .any(|item| item.id == "search")
        );
    }

    #[test]
    fn typed_updates_reset_and_save_flow_through_one_owner() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.set_string_setting(SettingId::Language, "sl")
            .expect("language");
        app.set_integer_setting(SettingId::ResultsLimit, 50)
            .expect("limit");
        app.set_float_setting(SettingId::AppUpdateIntervalHours, 12.0)
            .expect("interval");
        app.set_boolean_setting(SettingId::CloseToTray, true)
            .expect("checkbox");
        assert!(app.settings_are_dirty());
        app.save_settings().expect("save");
        assert!(!app.settings_are_dirty());
        assert_eq!(app.settings().language, "sl");

        app.reset_settings_section(SettingsSection::General)
            .expect("reset");
        assert_eq!(app.settings().language, "en");
        assert!(!app.settings().close_to_tray);
    }

    #[test]
    fn initial_language_completion_is_atomic_and_one_time() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.complete_initial_language(Some("sl"))
            .expect("complete language prompt");
        assert_eq!(app.settings().language, "sl");
        assert!(app.settings().language_prompted);
        assert!(!app.settings_are_dirty());

        let paths =
            SettingsPaths::for_app_data(&root.path().join("beta"), &root.path().join("stable"));
        let reloaded = Application::new(
            SettingsController::load(paths, SettingsDocument::default()),
            MainMenuAvailability::default(),
        );
        assert_eq!(reloaded.settings().language, "sl");
        assert!(reloaded.settings().language_prompted);
    }

    #[test]
    fn cancelling_initial_language_keeps_the_configured_language() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.complete_initial_language(None)
            .expect("cancel language prompt");
        assert_eq!(app.settings().language, "en");
        assert!(app.settings().language_prompted);
    }

    #[test]
    fn equalizer_band_updates_are_strictly_independent() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let before = app.settings().global_equalizer_gains.clone();
        app.set_equalizer_band_gain("flat", "31", 7.5)
            .expect("gain");
        assert!((app.settings().global_equalizer_gains["31"] - 7.5).abs() < f64::EPSILON);
        for (band, gain) in before {
            if band != "31" {
                assert!(
                    (app.settings().global_equalizer_gains[&band] - gain).abs() < f64::EPSILON,
                    "{band}"
                );
            }
        }
    }

    #[test]
    fn shortcut_conflicts_do_not_modify_the_draft() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let before = app.settings().keyboard_shortcuts.clone();
        assert!(
            app.set_keyboard_shortcut("open_search", "Ctrl+Alt+M")
                .is_err()
        );
        assert_eq!(app.settings().keyboard_shortcuts, before);
        app.set_keyboard_shortcut("open_search", "Ctrl+F8")
            .expect("unused shortcut");
        assert_eq!(app.settings().keyboard_shortcuts["open_search"], "Ctrl+F8");
    }

    #[test]
    fn activation_queue_preserves_files_and_coalesces_repeated_show_requests() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.enqueue_activation(ActivationRequest::Show);
        app.enqueue_activation(ActivationRequest::Show);
        app.enqueue_activation(ActivationRequest::OpenFile(PathBuf::from("track.mp3")));
        assert_eq!(app.take_activation(), Some(ActivationRequest::Show));
        assert_eq!(
            app.take_activation(),
            Some(ActivationRequest::OpenFile(PathBuf::from("track.mp3")))
        );
        assert_eq!(app.take_activation(), None);
    }

    #[test]
    fn application_owns_session_defaults_and_rejects_stale_playback_events() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.set_integer_setting(SettingId::DefaultVolume, 80)
            .expect("volume");
        app.set_string_setting(SettingId::AudioOutputDevice, "speakers")
            .expect("device");
        let first = app.start_player_item(media_item("first"));
        assert!(
            (app.player_session().audio().expect("audio session").volume - 80.0).abs()
                < f64::EPSILON
        );

        let second = app.start_player_item(media_item("second"));
        assert!(!app.apply_playback_event(first, PlaybackEvent::Ended));
        assert!(app.apply_playback_event(second, PlaybackEvent::Started));
        assert_eq!(
            app.player_session()
                .current_item()
                .map(|item| item.id.0.as_str()),
            Some("second")
        );

        app.close_player_session();
        assert!(app.player_session().audio().is_none());
    }

    #[test]
    fn trending_uses_one_fixed_page_and_honors_the_configured_result_limit() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.set_integer_setting(SettingId::ResultsLimit, 0)
            .expect("dynamic result limit");
        let work = app
            .begin_youtube_trending("SI", "music")
            .expect("trending work");
        assert_eq!(work.limit, 50);
        app.apply_search_results(
            work.generation,
            (0..50)
                .map(|index| youtube_item(index, MediaKind::Video))
                .collect(),
            Some("ignored-continuation".to_owned()),
        );
        assert!(app.request_more_search_results().is_none());

        app.set_integer_setting(SettingId::ResultsLimit, 12)
            .expect("fixed result limit");
        let work = app
            .begin_youtube_trending("US", "all")
            .expect("bounded trending work");
        assert_eq!(work.limit, 12);
    }

    #[test]
    fn trending_player_session_restores_filters_and_selection() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("beta/last_player_session.json");
        let legacy = root.path().join("stable/last_player_session.json");
        let mut original = application(root.path());
        original.configure_last_player_session(
            LastPlayerSessionFile::new(&current),
            &LastPlayerSessionFile::new(&legacy),
        );
        original.navigate_to(RouteFrame::new(Route::Trending));
        original.update_trending_route_context(42, 1, "SI", "music");
        let work = original
            .begin_youtube_trending("SI", "music")
            .expect("trending work");
        original.apply_search_results(
            work.generation,
            (0..5)
                .map(|index| youtube_item(index, MediaKind::Video))
                .collect(),
            None,
        );
        let item = original
            .prepare_search_playback(3)
            .expect("trending result");
        original.start_player_item(item);
        drop(original);

        let mut restored = application(root.path());
        restored.configure_last_player_session(
            LastPlayerSessionFile::new(&current),
            &LastPlayerSessionFile::new(&legacy),
        );
        let resume = restored
            .prepare_last_player_session_resume()
            .expect("resume plan");
        assert_eq!(restored.current_route(), Route::Trending);
        assert_eq!(restored.search_session().selected_index(), 3);
        assert_eq!(resume.return_data["country_index"], 42);
        assert_eq!(resume.return_data["category_index"], 1);
        assert_eq!(resume.return_data["country_code"], "SI");
        assert_eq!(resume.return_data["category_code"], "music");
    }

    #[test]
    fn resume_position_is_saved_and_restored_only_for_the_matching_item() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.configure_playback_positions(
            PlaybackPositionFile::new(root.path().join("beta/playback_positions.json")),
            &PlaybackPositionFile::new(root.path().join("stable/playback_positions.json")),
        );
        let first = media_item("first");
        let second = media_item("second");
        let generation = app.start_player_item(first.clone());
        assert!(app.apply_playback_event(
            generation,
            PlaybackEvent::Position {
                elapsed: 25.0,
                duration: Some(100.0),
            },
        ));
        app.save_current_playback_position().expect("save position");
        app.close_player_session();

        assert!(
            app.playback_resume_position(&first)
                .is_some_and(|position| (position - 25.0).abs() < f64::EPSILON)
        );
        assert_eq!(app.playback_resume_position(&second), None);
        let resume_position = app.playback_resume_position(&first);
        let resumed = app.start_player_item_at(first, resume_position);
        assert_eq!(app.player_session().generation(), resumed);
        assert!((app.player_session().position_seconds() - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn last_session_restores_result_selection_and_exact_next_item() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("beta/last_player_session.json");
        let legacy = root.path().join("stable/last_player_session.json");
        let mut original = application(root.path());
        original.configure_last_player_session(
            LastPlayerSessionFile::new(&current),
            &LastPlayerSessionFile::new(&legacy),
        );
        let work = original
            .begin_youtube_search("remembered query", YoutubeSearchKind::Video)
            .expect("search");
        original.apply_search_results(
            work.generation,
            (0..3)
                .map(|index| youtube_item(index, MediaKind::Video))
                .collect(),
            None,
        );
        original.navigate_to(RouteFrame::new(Route::Results));
        let current_item = original.prepare_search_playback(1).expect("current result");
        original.start_player_item(current_item);
        drop(original);

        let mut restored = application(root.path());
        restored.configure_last_player_session(
            LastPlayerSessionFile::new(&current),
            &LastPlayerSessionFile::new(&legacy),
        );
        assert!(
            restored
                .main_menu_model()
                .items
                .iter()
                .any(|item| item.id == "resume_last_session")
        );
        let resume = restored
            .prepare_last_player_session_resume()
            .expect("resume plan");
        assert!(resume.sequence_active);
        assert_eq!(resume.item.id.0, "1");
        assert_eq!(restored.current_route(), Route::Results);
        assert_eq!(restored.search_session().query(), "remembered query");
        assert_eq!(restored.search_session().selected_index(), 1);
        restored.start_player_item(resume.item);
        let PlayerNavigationOutcome::Item { item, origin } =
            restored.request_relative_player_item(1)
        else {
            panic!("next restored item");
        };
        assert_eq!(origin, PlayerNavigationOrigin::Sequence);
        assert_eq!(item.id.0, "2");
    }

    #[test]
    fn last_session_restores_rss_feed_episode_and_sequence() {
        let root = tempdir().expect("temporary directory");
        let session = root.path().join("beta/last_player_session.json");
        let legacy_session = root.path().join("stable/last_player_session.json");
        let feeds = root.path().join("beta/rss_feeds.json");
        let legacy_feeds = root.path().join("stable/rss_feeds.json");
        let mut original = application(root.path());
        original.configure_last_player_session(
            LastPlayerSessionFile::new(&session),
            &LastPlayerSessionFile::new(&legacy_session),
        );
        original.configure_rss_feeds(RssFeedFile::new(&feeds), &RssFeedFile::new(&legacy_feeds));
        original
            .add_rss_feed(RssFeed::new(
                "Podcast",
                "https://feeds.example/podcast.xml",
                "https://podcast.example",
                (0..3).map(podcast_item).collect(),
                10.0,
            ))
            .expect("feed");
        original.navigate_to(RouteFrame::new(Route::RssFeeds));
        let mut frame = RouteFrame::new(Route::RssItems);
        frame.parameters.insert("feed_index".to_owned(), 0.into());
        original.navigate_to(frame);
        let item = original
            .prepare_rss_episode_playback(0, 1)
            .expect("episode");
        original.start_player_item(item);
        drop(original);

        let mut restored = application(root.path());
        restored.configure_last_player_session(
            LastPlayerSessionFile::new(&session),
            &LastPlayerSessionFile::new(&legacy_session),
        );
        restored.configure_rss_feeds(RssFeedFile::new(&feeds), &RssFeedFile::new(&legacy_feeds));
        let resume = restored
            .prepare_last_player_session_resume()
            .expect("resume plan");
        assert_eq!(restored.current_route(), Route::RssItems);
        assert_eq!(resume.return_data["feed_index"], 0);
        assert_eq!(resume.return_data["item_index"], 1);
        assert!(resume.sequence_active);
        assert_eq!(
            restored.state.player_sequence.source(),
            Some(PlaybackSequenceSource::RssFeed { feed_index: 0 })
        );
        assert_eq!(resume.item.title, "Episode 1");
    }

    #[test]
    fn language_prompt_follows_python_first_run_and_tray_conditions() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        assert!(app.initial_language_prompt_due(true, false));
        assert!(!app.initial_language_prompt_due(true, true));
        assert!(!app.initial_language_prompt_due(false, false));
        assert_eq!(app.take_startup_announcement(), None);

        app.complete_initial_language(Some("sl"))
            .expect("save language");
        assert_eq!(app.settings().language, "sl");
        assert!(!app.initial_language_prompt_due(true, false));
        assert_eq!(app.take_startup_announcement(), Some("settings_saved"));
        assert_eq!(app.take_startup_announcement(), None);
    }

    #[test]
    fn hiding_resume_menu_does_not_hide_it_from_action_finder() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.configure_last_player_session(
            LastPlayerSessionFile::new(root.path().join("beta/last_player_session.json")),
            &LastPlayerSessionFile::new(root.path().join("stable/last_player_session.json")),
        );
        app.start_player_item(media_item("remembered"));
        app.set_boolean_setting(SettingId::ShowResumeInMenu, false)
            .expect("hide resume menu");
        assert!(
            app.main_menu_model()
                .items
                .iter()
                .all(|item| item.id != "resume_last_session")
        );
        assert!(
            app.action_finder_model()
                .items
                .iter()
                .any(|item| item.action_id == "resume_last_session")
        );
    }

    #[test]
    fn last_session_restores_local_folder_context_without_rescanning() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("beta/last_player_session.json");
        let legacy = root.path().join("stable/last_player_session.json");
        let folder = PathBuf::from(r"C:\Music\Album");
        let folder_items: Vec<_> = (0..3)
            .map(|index| media_item(&format!("track-{index}")))
            .collect();
        let mut original = application(root.path());
        original.configure_last_player_session(
            LastPlayerSessionFile::new(&current),
            &LastPlayerSessionFile::new(&legacy),
        );
        original.load_local_folder(folder.clone(), folder_items);
        original.navigate_to(RouteFrame::new(Route::LocalFolder));
        let current_item = original
            .prepare_local_folder_playback(1, false)
            .expect("folder item");
        original.start_player_item(current_item);
        drop(original);

        let mut restored = application(root.path());
        restored.configure_last_player_session(
            LastPlayerSessionFile::new(&current),
            &LastPlayerSessionFile::new(&legacy),
        );
        let resume = restored
            .prepare_last_player_session_resume()
            .expect("resume plan");
        assert!(resume.sequence_active);
        assert_eq!(restored.current_route(), Route::LocalFolder);
        assert_eq!(restored.local_folder_session().path(), folder);
        assert_eq!(restored.local_folder_session().selected_index(), 1);
        restored.start_player_item(resume.item);
        let PlayerNavigationOutcome::Item { item, origin } =
            restored.request_relative_player_item(1)
        else {
            panic!("next restored folder item");
        };
        assert_eq!(origin, PlayerNavigationOrigin::Sequence);
        assert_eq!(item.title, "track-2");
    }

    #[test]
    fn result_sequence_is_independent_from_focus_and_extends_without_skipping() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let work = app
            .begin_youtube_search("query", YoutubeSearchKind::All)
            .expect("search");
        let mut first_page: Vec<_> = (0..20)
            .map(|index| youtube_item(index, MediaKind::Video))
            .collect();
        first_page[5].kind = MediaKind::Playlist;
        app.apply_search_results(work.generation, first_page.clone(), None);

        let current = app.prepare_search_playback(18).expect("selected item");
        app.start_player_item(current);
        assert!(app.select_search_result(2));
        assert_eq!(
            app.request_relative_player_item(-1),
            PlayerNavigationOutcome::Item {
                item: Box::new(youtube_item(17, MediaKind::Video)),
                origin: PlayerNavigationOrigin::Sequence,
            }
        );
        let last = match app.request_relative_player_item(1) {
            PlayerNavigationOutcome::Item { item, .. } => item,
            other => panic!("expected last item, got {other:?}"),
        };
        assert_eq!(last.id.0, "19");
        app.start_player_item(*last);
        assert_eq!(app.search_session().selected_index(), 19);

        let more = match app.request_relative_player_item(1) {
            PlayerNavigationOutcome::LoadingMore(work) => work,
            other => panic!("expected dynamic page, got {other:?}"),
        };
        let mut cumulative = first_page;
        cumulative.extend((20..40).map(|index| youtube_item(index, MediaKind::Video)));
        app.apply_search_results(more.generation, cumulative, None);
        assert_eq!(
            app.request_relative_player_item(1),
            PlayerNavigationOutcome::Item {
                item: Box::new(youtube_item(20, MediaKind::Video)),
                origin: PlayerNavigationOrigin::Sequence,
            }
        );
    }

    #[test]
    fn youtube_collection_sequence_extends_without_losing_its_parent_search() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let search = app
            .begin_youtube_search("query", YoutubeSearchKind::Playlist)
            .expect("search");
        let playlist = youtube_item(7, MediaKind::Playlist);
        app.apply_search_results(search.generation, vec![playlist.clone()], None);
        assert!(app.select_search_result(0));

        let initial = app
            .begin_youtube_collection(
                playlist.title.clone(),
                playlist.url.as_ref().expect("playlist URL").to_string(),
                YoutubeCollectionKind::PlaylistVideos,
            )
            .expect("collection");
        let first_page: Vec<_> = (0..20)
            .map(|index| youtube_item(index, MediaKind::Video))
            .collect();
        app.apply_youtube_collection_results(initial.generation, first_page.clone());
        let current = app
            .prepare_youtube_collection_playback(19)
            .expect("last visible video");
        app.start_player_item(current);

        let more = match app.request_relative_player_item(1) {
            PlayerNavigationOutcome::LoadingMoreCollection(work) => work,
            other => panic!("expected collection page, got {other:?}"),
        };
        assert_eq!(more.limit, 40);
        let mut cumulative = first_page;
        cumulative.extend((20..40).map(|index| youtube_item(index, MediaKind::Video)));
        app.apply_youtube_collection_results(more.generation, cumulative);
        assert_eq!(
            app.request_relative_player_item(1),
            PlayerNavigationOutcome::Item {
                item: Box::new(youtube_item(20, MediaKind::Video)),
                origin: PlayerNavigationOrigin::Sequence,
            }
        );

        assert!(app.pop_youtube_collection());
        assert_eq!(
            app.search_session().items(),
            std::slice::from_ref(&playlist)
        );
        assert_eq!(app.search_session().selected_index(), 0);
    }

    #[test]
    fn complete_youtube_playlist_creates_an_exact_sequence_only_when_played() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let items = vec![
            youtube_item(0, MediaKind::Video),
            youtube_item(99, MediaKind::Playlist),
            youtube_item(1, MediaKind::Video),
            youtube_item(2, MediaKind::LiveStream),
        ];

        assert_eq!(app.player_sequence_source(), None);
        let current = app
            .prepare_youtube_playlist_playback(41, items, false)
            .expect("first playlist item");
        assert_eq!(current.id.0, "0");
        assert_eq!(
            app.player_sequence_source(),
            Some(PlaybackSequenceSource::YoutubePlaylist { token: 41 })
        );
        app.start_player_item(current);
        assert_eq!(
            app.request_relative_player_item(1),
            PlayerNavigationOutcome::Item {
                item: Box::new(youtube_item(1, MediaKind::Video)),
                origin: PlayerNavigationOrigin::Sequence,
            }
        );
    }

    #[test]
    fn shuffled_youtube_playlist_contains_each_playable_item_once() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let items: Vec<_> = (0..12)
            .map(|index| youtube_item(index, MediaKind::Video))
            .collect();

        let current = app
            .prepare_youtube_playlist_playback(42, items, true)
            .expect("shuffled playlist item");
        let mut identities: Vec<_> = app
            .state
            .player_sequence
            .items()
            .iter()
            .map(|item| item.id.0.clone())
            .collect();
        identities.sort();
        assert_eq!(identities.len(), 12);
        let mut expected = (0..12).map(|index| index.to_string()).collect::<Vec<_>>();
        expected.sort();
        assert_eq!(identities, expected);
        assert!(app.state.player_sequence.contains(&current));
    }

    #[test]
    fn closing_player_invalidates_its_source_sequence() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let work = app
            .begin_youtube_search("query", YoutubeSearchKind::Video)
            .expect("search");
        let items = vec![
            youtube_item(0, MediaKind::Video),
            youtube_item(1, MediaKind::Video),
        ];
        app.apply_search_results(work.generation, items, None);
        let current = app.prepare_search_playback(0).expect("selected item");
        app.start_player_item(current);
        app.close_player_session();
        assert_eq!(
            app.request_relative_player_item(1),
            PlayerNavigationOutcome::Unavailable
        );
    }

    #[test]
    fn standalone_playback_does_not_inherit_a_previous_search_sequence() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let work = app
            .begin_youtube_search("query", YoutubeSearchKind::Video)
            .expect("search");
        app.apply_search_results(
            work.generation,
            vec![
                youtube_item(0, MediaKind::Video),
                youtube_item(1, MediaKind::Video),
            ],
            None,
        );
        let current = app.prepare_search_playback(0).expect("selected item");
        app.start_player_item(current);

        app.prepare_standalone_playback();
        let direct =
            MediaItem::from_direct_link("https://media.example/song.mp3").expect("direct link");
        app.start_player_item(direct);

        assert_eq!(
            app.request_relative_player_item(1),
            PlayerNavigationOutcome::Unavailable
        );
    }

    #[test]
    fn history_is_durable_deduplicated_and_builds_its_own_sequence() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let favorites = MediaListFile::new(root.path().join("beta/favorites.json"));
        let history_path = root.path().join("beta/history.json");
        app.configure_media_collections(
            favorites,
            &MediaListFile::new(root.path().join("stable/favorites.json")),
            MediaListFile::new(&history_path),
            &MediaListFile::new(root.path().join("stable/history.json")),
        );
        app.record_history(media_item("first"), "played", 1.0)
            .expect("first history item");
        app.record_history(media_item("second"), "played", 2.0)
            .expect("second history item");
        app.record_history(media_item("first"), "played", 3.0)
            .expect("refresh first item");

        assert_eq!(
            app.history()
                .iter()
                .map(|item| item.id.0.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(app.history()[0].metadata["timestamp"], 3.0);
        assert_eq!(
            MediaListFile::new(history_path)
                .load()
                .expect("persisted history")
                .len(),
            2
        );

        let current = app.prepare_history_playback(0).expect("history playback");
        app.start_player_item(current);
        let PlayerNavigationOutcome::Item { item, origin } = app.request_relative_player_item(1)
        else {
            panic!("expected the next history item");
        };
        assert_eq!(origin, PlayerNavigationOrigin::Sequence);
        assert_eq!(item.id.0, "second");
    }

    #[test]
    fn notification_playback_preserves_selection_and_stays_standalone() {
        let root = tempdir().expect("temporary directory");
        let notification_path = root.path().join("beta/notifications.json");
        let mut app = application(root.path());
        app.configure_notifications(
            NotificationFile::new(&notification_path),
            &NotificationFile::new(root.path().join("stable/notifications.json")),
        );
        app.add_notification(AppNotification::new(
            "subscription_video",
            "First",
            "Message",
            Some(youtube_item(1, MediaKind::Video)),
            1.0,
        ))
        .expect("first notification");
        app.add_notification(AppNotification::new(
            "subscription_video",
            "Second",
            "Message",
            Some(youtube_item(2, MediaKind::Video)),
            2.0,
        ))
        .expect("second notification");
        app.navigate_to(RouteFrame::new(Route::NotificationCenter));

        let item = app
            .prepare_notification_playback(1)
            .expect("playable notification");

        assert_eq!(item.id.0, "1");
        assert_eq!(app.current_route(), Route::NotificationCenter);
        assert_eq!(app.state.navigation.current().selected_index, 1);
        assert_eq!(app.state.navigation.current().parameters["index"], 1);
        assert_eq!(app.player_sequence_source(), None);
        assert_eq!(
            NotificationFile::new(notification_path)
                .load()
                .expect("persisted notifications")
                .len(),
            2
        );
    }

    #[test]
    fn selected_user_playlist_item_is_standalone_and_durable() {
        let root = tempfile::tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let playlist_path = root.path().join("beta/playlists.json");
        app.configure_user_playlists(
            UserPlaylistFile::new(&playlist_path),
            &UserPlaylistFile::new(root.path().join("stable/playlists.json")),
        );
        assert_eq!(
            app.create_user_playlist("Road trip", 1.0)
                .expect("create playlist"),
            PlaylistCreateOutcome::Created(0)
        );
        for id in ["first", "second", "third"] {
            assert!(matches!(
                app.add_item_to_user_playlist(0, media_item(id), 2.0)
                    .expect("add item"),
                PlaylistAddOutcome::Added(_)
            ));
        }
        let second = app
            .prepare_user_playlist_item_playback(0, 1)
            .expect("second item");
        assert_eq!(second.id.0, "second");
        assert_eq!(
            app.request_relative_player_item(-1),
            PlayerNavigationOutcome::Unavailable
        );
        assert_eq!(
            app.request_relative_player_item(1),
            PlayerNavigationOutcome::Unavailable
        );
        assert_eq!(
            UserPlaylistFile::new(playlist_path)
                .load()
                .expect("persisted playlists")[0]
                .items
                .len(),
            3
        );
    }

    #[test]
    fn whole_user_playlist_drives_exact_previous_next_order() {
        let root = tempfile::tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.configure_user_playlists(
            UserPlaylistFile::new(root.path().join("playlists.json")),
            &UserPlaylistFile::new(root.path().join("legacy.json")),
        );
        app.create_user_playlist("Road trip", 1.0)
            .expect("create playlist");
        for id in ["first", "second", "third"] {
            app.add_item_to_user_playlist(0, media_item(id), 2.0)
                .expect("add item");
        }

        let first = app
            .prepare_user_playlist_playback(0, false)
            .expect("playlist start");
        assert!(app.player_sequence_contains(&first));
        assert!(matches!(
            app.request_relative_player_item(1),
            PlayerNavigationOutcome::Item { item, .. } if item.id.0 == "second"
        ));
    }

    #[test]
    fn shuffled_user_playlist_contains_each_item_once() {
        let root = tempfile::tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.configure_user_playlists(
            UserPlaylistFile::new(root.path().join("playlists.json")),
            &UserPlaylistFile::new(root.path().join("legacy.json")),
        );
        app.create_user_playlist("Shuffle", 1.0)
            .expect("create playlist");
        for id in ["one", "two", "three", "four"] {
            app.add_item_to_user_playlist(0, media_item(id), 2.0)
                .expect("add item");
        }
        let current = app
            .prepare_user_playlist_playback(0, true)
            .expect("shuffle start");
        assert!(app.state.player_sequence.activate(&current));
        let identities: std::collections::BTreeSet<_> = app
            .state
            .player_sequence
            .items()
            .iter()
            .map(|item| item.id.0.as_str())
            .collect();
        assert_eq!(
            identities,
            std::collections::BTreeSet::from(["four", "one", "three", "two"])
        );
    }

    #[test]
    fn explicit_shuffle_choice_survives_a_fresh_or_continuing_session_start() {
        let root = tempfile::tempdir().expect("temporary directory");
        let mut app = application(root.path());

        app.start_player_item_with_shuffle(media_item("first"), Some(true));
        assert!(
            app.player_session()
                .enabled_toggles()
                .contains(&SessionToggle::Shuffle)
        );

        app.start_player_item_with_shuffle(media_item("second"), Some(false));
        assert!(
            !app.player_session()
                .enabled_toggles()
                .contains(&SessionToggle::Shuffle)
        );
    }

    fn watch_video(id: &str) -> MediaItem {
        MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: id.to_owned(),
            url: Some(
                format!("https://www.youtube.com/watch?v={id}")
                    .parse()
                    .expect("URL"),
            ),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn shuffle_picks_another_list_item_and_previous_keeps_order() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let work = app
            .begin_youtube_search("query", YoutubeSearchKind::All)
            .expect("search");
        let videos: Vec<_> = (0..4)
            .map(|index| watch_video(&format!("video{index:06}")))
            .collect();
        app.apply_search_results(work.generation, videos, None);
        let current = app.prepare_search_playback(3).expect("last result");
        app.start_player_item(current);
        assert_eq!(
            app.request_relative_player_item(1),
            PlayerNavigationOutcome::Unavailable
        );
        app.set_player_toggle(SessionToggle::Shuffle, true);
        for _ in 0..20 {
            let PlayerNavigationOutcome::Item { item, origin } =
                app.request_relative_player_item(1)
            else {
                panic!("shuffle picks an item");
            };
            assert_eq!(origin, PlayerNavigationOrigin::Sequence);
            assert_ne!(item.id.0, "video000003");
        }
        // Previous keeps the list order with shuffle on.
        let PlayerNavigationOutcome::Item { item, .. } = app.request_relative_player_item(-1)
        else {
            panic!("previous item");
        };
        assert_eq!(item.id.0, "video000002");
    }

    #[test]
    fn related_videos_skip_seen_ones_and_replace_the_results() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let work = app
            .begin_youtube_search("query", YoutubeSearchKind::All)
            .expect("search");
        app.apply_search_results(work.generation, vec![watch_video("current0001")], None);
        let current = app.prepare_search_playback(0).expect("current");
        app.start_player_item(current);

        assert_eq!(
            app.apply_related_videos(vec![watch_video("current0001")]),
            None
        );
        let next = app
            .apply_related_videos(vec![
                watch_video("current0001"),
                watch_video("related0001"),
                watch_video("related0002"),
            ])
            .expect("unseen related video");
        assert_eq!(next.id.0, "related0001");
        let titles: Vec<_> = app
            .search_session()
            .items()
            .iter()
            .map(|item| item.id.0.as_str())
            .collect();
        assert_eq!(titles, ["related0001", "related0002"]);
        app.start_player_item(next);
        let PlayerNavigationOutcome::Item { item, .. } = app.request_relative_player_item(1) else {
            panic!("next related video");
        };
        assert_eq!(item.id.0, "related0002");
        // Played related videos stay seen until the session closes.
        assert_eq!(
            app.apply_related_videos(vec![watch_video("related0001")]),
            None
        );
        app.close_player_session();
        app.start_player_item(watch_video("current0001"));
        assert!(
            app.apply_related_videos(vec![watch_video("related0001")])
                .is_some()
        );
    }

    #[test]
    fn replaygain_cycles_off_track_album_and_is_saved() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        assert_eq!(app.settings().replaygain_mode, "no");
        assert_eq!(app.cycle_replaygain_mode().expect("track"), "track");
        assert_eq!(app.cycle_replaygain_mode().expect("album"), "album");
        assert_eq!(app.cycle_replaygain_mode().expect("off"), "no");
        app.cycle_replaygain_mode().expect("track again");
        let reloaded = application(root.path());
        assert_eq!(reloaded.settings().replaygain_mode, "track");
    }

    #[test]
    fn favorite_channels_and_playlists_open_without_replacing_the_sequence() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        app.add_favorite(youtube_item(1, MediaKind::Channel))
            .expect("favorite channel");
        app.add_favorite(youtube_item(2, MediaKind::Video))
            .expect("favorite video");

        let channel = app.prepare_favorite_playback(0).expect("channel");
        assert_eq!(channel.kind, MediaKind::Channel);
        assert!(!app.player_sequence_contains(&channel));

        let video = app.prepare_favorite_playback(1).expect("video");
        assert!(app.player_sequence_contains(&video));
    }

    #[test]
    fn local_folder_builds_a_sequence_only_when_playback_starts() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let items: Vec<_> = (0..45)
            .map(|index| media_item(&format!("track{index}")))
            .collect();
        app.load_local_folder(PathBuf::from(r"C:\Music"), items);

        assert_eq!(app.local_folder_session().items().len(), 45);
        assert!(app.playback_queue().is_empty());
        assert_eq!(
            app.request_relative_player_item(1),
            PlayerNavigationOutcome::Unavailable
        );

        let current = app
            .prepare_local_folder_playback(0, false)
            .expect("selected track");
        app.start_player_item(current);
        let next = app.request_relative_player_item(1);
        assert_eq!(
            next,
            PlayerNavigationOutcome::Item {
                item: Box::new(media_item("track1")),
                origin: PlayerNavigationOrigin::Sequence,
            }
        );
        let PlayerNavigationOutcome::Item { item, .. } = next else {
            unreachable!("asserted item outcome")
        };
        app.start_player_item(*item);
        for expected in 2..=25 {
            let PlayerNavigationOutcome::Item { item, .. } = app.request_relative_player_item(1)
            else {
                panic!("expected track {expected}")
            };
            app.start_player_item(*item);
        }
        assert_eq!(app.local_folder_session().selected_index(), 25);
        assert!(app.playback_queue().is_empty());

        let outcome = app
            .add_local_folder_to_playback_queue()
            .expect("folder queue");
        assert_eq!(outcome.added, 45);
        assert_eq!(app.playback_queue().len(), 45);
    }

    #[test]
    fn sequence_precedes_queue_until_its_real_end() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let queued = youtube_item(99, MediaKind::Video);
        assert_eq!(
            app.add_to_playback_queue(queued.clone())
                .expect("queue add"),
            crate::QueueAddOutcome::Added
        );
        let work = app
            .begin_youtube_search("query", YoutubeSearchKind::Video)
            .expect("search");
        let items = vec![
            youtube_item(0, MediaKind::Video),
            youtube_item(1, MediaKind::Video),
        ];
        app.apply_search_results(work.generation, items, None);
        let current = app.prepare_search_playback(0).expect("selected item");
        app.start_player_item(current);

        let next = match app.request_relative_player_item(1) {
            PlayerNavigationOutcome::Item { item, origin } => {
                assert_eq!(origin, PlayerNavigationOrigin::Sequence);
                *item
            }
            other => panic!("expected sequence item, got {other:?}"),
        };
        assert_eq!(next.id.0, "1");
        app.start_player_item(next);
        assert_eq!(
            app.request_relative_player_item(1),
            PlayerNavigationOutcome::Item {
                item: Box::new(queued),
                origin: PlayerNavigationOrigin::Queue,
            }
        );
        assert_eq!(app.playback_queue().len(), 1);
    }

    #[test]
    fn queue_is_consumed_only_after_explicit_start_confirmation() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let queued = youtube_item(7, MediaKind::Video);
        app.add_to_playback_queue(queued.clone())
            .expect("queue add");
        app.start_player_item(media_item("unrelated"));

        assert_eq!(
            app.request_relative_player_item(-1),
            PlayerNavigationOutcome::Unavailable
        );
        let candidate = match app.request_relative_player_item(1) {
            PlayerNavigationOutcome::Item { item, origin } => {
                assert_eq!(origin, PlayerNavigationOrigin::Queue);
                *item
            }
            other => panic!("expected queued item, got {other:?}"),
        };
        assert_eq!(app.playback_queue().len(), 1);
        assert!(
            app.confirm_queued_item_started(&candidate)
                .expect("queue consume")
        );
        assert!(app.playback_queue().is_empty());
        assert!(
            !app.confirm_queued_item_started(&candidate)
                .expect("second queue consume")
        );
    }

    #[test]
    fn queue_count_is_projected_into_main_menu() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        assert!(
            app.main_menu_model()
                .items
                .iter()
                .all(|item| item.id != "playback_queue")
        );
        app.add_to_playback_queue(youtube_item(3, MediaKind::Video))
            .expect("queue add");
        let queue = app
            .main_menu_model()
            .items
            .into_iter()
            .find(|item| item.id == "playback_queue")
            .expect("queue menu item");
        assert!(queue.label.contains("(1)"));
    }
}
