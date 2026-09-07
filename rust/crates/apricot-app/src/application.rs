//! Top-level application coordinator consumed by platform UI adapters.

use std::{
    collections::{BTreeSet, VecDeque},
    path::PathBuf,
};

use apricot_core::{MediaItem, Route, RouteFrame, SettingId, SettingsSection};
use apricot_playback::PlaybackEvent;
use apricot_storage::{
    Bookmark, BookmarkFile, MediaListFile, PlaybackPositionFile, PlaybackQueueFile,
    SettingsDocument, UserPlaylist, UserPlaylistFile,
};
use rand::seq::SliceRandom;

use crate::{
    ActionFinderContext, ActionFinderModel, ActivationRequest, AppState, AudioSession,
    BookmarkController, BookmarkControllerError, CollectionAddOutcome, EqualizerSession,
    MainMenuAvailability, MainMenuModel, MediaCollectionController, MediaCollectionControllerError,
    MenuVisibility, PlaybackPositionController, PlaybackPositionControllerError,
    PlaybackPositionUpdate, PlaybackQueue, PlaybackQueueController, PlaybackQueueControllerError,
    PlaybackSequenceSource, PlayerScreenModel, PlayerSession, PlayerSessionDefaults,
    PlayerViewState, PlaylistAddOutcome, PlaylistCreateOutcome, QueueAddOutcome,
    QueueBatchAddOutcome, SearchApplyOutcome, SearchSession, SearchSessionError, SearchWork,
    SessionToggle, SettingsController, SettingsControllerError, SettingsScreenModel,
    UserPlaylistController, UserPlaylistControllerError, YoutubeSearchKind, embedded_catalog,
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
    Unavailable,
}

#[derive(Debug)]
pub struct Application {
    settings: SettingsController,
    menu_availability: MainMenuAvailability,
    activation_requests: VecDeque<ActivationRequest>,
    state: AppState,
}

impl Application {
    pub fn new(settings: SettingsController, menu_availability: MainMenuAvailability) -> Self {
        Self {
            settings,
            menu_availability,
            activation_requests: VecDeque::new(),
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

    pub fn playback_resume_position(&self, item: &MediaItem) -> Option<f64> {
        self.state
            .playback_positions
            .resume_position(item, self.settings.current().resume_playback)
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
        let _ = self.state.player_sequence.set(
            PlaybackSequenceSource::Collection,
            self.state.favorites.items(),
            &item,
        );
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
        let _ = self.state.player_sequence.set(
            PlaybackSequenceSource::Collection,
            self.state.history.items(),
            &item,
        );
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
        self.state
            .search
            .begin(query, kind, self.settings.current().results_limit)
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

    pub fn load_local_folder(&mut self, path: PathBuf, items: Vec<MediaItem>) {
        let batch_size = usize::try_from(self.settings.current().results_limit.max(0))
            .unwrap_or(crate::DEFAULT_FOLDER_BATCH_SIZE);
        self.state.local_folder.load(path, items, batch_size);
    }

    pub fn select_local_folder_item(&mut self, index: usize) -> bool {
        self.state.local_folder.select(index)
    }

    pub fn append_local_folder_batch(&mut self) -> usize {
        self.state.local_folder.append_visible_batch()
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
                let _ = self.state.local_folder.reveal_and_select(source_index);
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
                speed: settings.player_speed.parse().unwrap_or(1.0),
                pitch: 1.0,
                equalizer: EqualizerSession {
                    enabled: settings.global_equalizer_enabled,
                    gains: settings.global_equalizer_gains.clone(),
                },
            },
            enabled_toggles: toggles,
            starts_paused: settings.player_start_paused,
        };
        self.state
            .player
            .start_item_at(item, defaults, initial_position_seconds)
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
                    let _ = self.state.local_folder.reveal_and_select(index);
                }
            }
            _ => {}
        }
    }

    pub fn apply_playback_event(&mut self, generation: u64, event: PlaybackEvent) -> bool {
        self.state.player.apply_event(generation, event)
    }

    pub fn set_player_volume(&mut self, volume: f64) {
        self.state.player.set_volume(volume);
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

    pub fn action_finder_model(&self, context: ActionFinderContext) -> ActionFinderModel {
        let settings = self.settings.current();
        ActionFinderModel::build(
            &embedded_catalog(&settings.language),
            settings,
            self.current_menu_availability(),
            context,
        )
    }

    fn current_menu_availability(&self) -> MainMenuAvailability {
        let settings = self.settings.current();
        let mut availability = self.menu_availability;
        availability.trending = visibility(settings.enable_trending);
        availability.history = visibility(settings.enable_history);
        availability.podcasts = visibility(settings.enable_podcasts_rss);
        availability.playback_queue_count = self.state.playback_queue.queue().len();
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
        Ok(())
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

const fn visibility(enabled: bool) -> MenuVisibility {
    if enabled {
        MenuVisibility::Visible
    } else {
        MenuVisibility::Hidden
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        path::{Path, PathBuf},
    };

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource, SettingId, SettingsSection};
    use apricot_playback::PlaybackEvent;
    use apricot_storage::{
        MediaListFile, PlaybackPositionFile, SettingsDocument, SettingsPaths, UserPlaylistFile,
    };
    use tempfile::tempdir;

    use super::{Application, PlayerNavigationOrigin, PlayerNavigationOutcome};
    use crate::{
        ActivationRequest, MainMenuAvailability, PlaylistAddOutcome, PlaylistCreateOutcome,
        SessionToggle, SettingsController, YoutubeSearchKind,
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

    #[test]
    fn local_folder_builds_a_sequence_only_when_playback_starts() {
        let root = tempdir().expect("temporary directory");
        let mut app = application(root.path());
        let items: Vec<_> = (0..45)
            .map(|index| media_item(&format!("track{index}")))
            .collect();
        app.load_local_folder(PathBuf::from(r"C:\Music"), items);

        assert_eq!(app.local_folder_session().visible_items().len(), 20);
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
        assert_eq!(app.local_folder_session().visible_items().len(), 26);
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
