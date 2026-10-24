//! Transactional user playlists compatible with the Python application.

use std::path::PathBuf;

use apricot_core::MediaItem;
use apricot_storage::{UserPlaylist, UserPlaylistFile, UserPlaylistFileError};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaylistCreateOutcome {
    Created(usize),
    EmptyName,
    AlreadyExists,
    UnsupportedItem,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaylistAddOutcome {
    Added(usize),
    AlreadyPresent,
    MissingPlaylist,
    Unsupported,
}

#[derive(Debug, Error)]
pub enum UserPlaylistControllerError {
    #[error("playlist changes are blocked because loading {path} failed: {message}")]
    SaveBlocked { path: PathBuf, message: String },
    #[error(transparent)]
    Storage(#[from] UserPlaylistFileError),
}

#[derive(Debug, Default)]
pub struct UserPlaylistController {
    playlists: Vec<UserPlaylist>,
    file: Option<UserPlaylistFile>,
    load_error: Option<String>,
    save_blocked: bool,
}

impl UserPlaylistController {
    pub fn load(current: UserPlaylistFile, legacy: &UserPlaylistFile) -> Self {
        if current.path().is_file() {
            return match current.load() {
                Ok(playlists) => Self::loaded(current, playlists),
                Err(error) => Self::blocked(current, error.to_string()),
            };
        }
        if legacy.path().is_file() {
            return match legacy.load() {
                Ok(playlists) => Self::loaded(current, playlists),
                Err(error) => Self {
                    playlists: Vec::new(),
                    file: Some(current),
                    load_error: Some(error.to_string()),
                    save_blocked: false,
                },
            };
        }
        Self::loaded(current, Vec::new())
    }

    fn loaded(file: UserPlaylistFile, playlists: Vec<UserPlaylist>) -> Self {
        Self {
            playlists,
            file: Some(file),
            load_error: None,
            save_blocked: false,
        }
    }

    fn blocked(file: UserPlaylistFile, message: String) -> Self {
        Self {
            playlists: Vec::new(),
            file: Some(file),
            load_error: Some(message),
            save_blocked: true,
        }
    }

    pub fn playlists(&self) -> &[UserPlaylist] {
        &self.playlists
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Creates a case-insensitively unique playlist after trimming its title.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed playlist collection cannot be persisted.
    pub fn create(
        &mut self,
        title: &str,
        timestamp: f64,
    ) -> Result<PlaylistCreateOutcome, UserPlaylistControllerError> {
        let title = title.trim();
        if title.is_empty() {
            return Ok(PlaylistCreateOutcome::EmptyName);
        }
        if self
            .playlists
            .iter()
            .any(|playlist| titles_match(&playlist.title, title))
        {
            return Ok(PlaylistCreateOutcome::AlreadyExists);
        }
        let mut candidate = self.playlists.clone();
        let index = candidate.len();
        candidate.push(UserPlaylist::new(title, timestamp));
        self.commit(candidate)?;
        Ok(PlaylistCreateOutcome::Created(index))
    }

    /// Creates a playlist containing one item with a single durable write.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed playlist collection cannot be persisted.
    pub fn create_with_item(
        &mut self,
        title: &str,
        item: MediaItem,
        timestamp: f64,
    ) -> Result<PlaylistCreateOutcome, UserPlaylistControllerError> {
        let title = title.trim();
        if title.is_empty() {
            return Ok(PlaylistCreateOutcome::EmptyName);
        }
        if self
            .playlists
            .iter()
            .any(|playlist| titles_match(&playlist.title, title))
        {
            return Ok(PlaylistCreateOutcome::AlreadyExists);
        }
        let Some(item) = durable_playlist_item(item, timestamp) else {
            return Ok(PlaylistCreateOutcome::UnsupportedItem);
        };
        let mut candidate = self.playlists.clone();
        let index = candidate.len();
        let mut playlist = UserPlaylist::new(title, timestamp);
        playlist.items.push(item);
        candidate.push(playlist);
        self.commit(candidate)?;
        Ok(PlaylistCreateOutcome::Created(index))
    }

    /// Removes one playlist and commits the new collection atomically.
    ///
    /// # Errors
    ///
    /// Returns an error when the playlist collection cannot be persisted.
    pub fn remove_playlist(
        &mut self,
        index: usize,
    ) -> Result<Option<UserPlaylist>, UserPlaylistControllerError> {
        if index >= self.playlists.len() {
            return Ok(None);
        }
        let mut candidate = self.playlists.clone();
        let removed = candidate.remove(index);
        self.commit(candidate)?;
        Ok(Some(removed))
    }

    /// Adds one durable playable item unless that location is already present.
    ///
    /// # Errors
    ///
    /// Returns an error when the updated playlist cannot be persisted.
    pub fn add_item(
        &mut self,
        playlist_index: usize,
        item: MediaItem,
        timestamp: f64,
    ) -> Result<PlaylistAddOutcome, UserPlaylistControllerError> {
        if !item.is_playable() || playlist_identity(&item).is_none() {
            return Ok(PlaylistAddOutcome::Unsupported);
        }
        let Some(playlist) = self.playlists.get(playlist_index) else {
            return Ok(PlaylistAddOutcome::MissingPlaylist);
        };
        let Some(identity) = playlist_identity(&item) else {
            return Ok(PlaylistAddOutcome::Unsupported);
        };
        if playlist
            .items
            .iter()
            .any(|existing| playlist_identity(existing).as_deref() == Some(&identity))
        {
            return Ok(PlaylistAddOutcome::AlreadyPresent);
        }
        let Some(item) = durable_playlist_item(item, timestamp) else {
            return Ok(PlaylistAddOutcome::Unsupported);
        };
        let mut candidate = self.playlists.clone();
        candidate[playlist_index].items.push(item);
        if timestamp.is_finite() {
            candidate[playlist_index].metadata.remove("updated_at");
            candidate[playlist_index].updated_at = Some(timestamp);
        }
        let item_index = candidate[playlist_index].items.len() - 1;
        self.commit(candidate)?;
        Ok(PlaylistAddOutcome::Added(item_index))
    }

    /// Removes one nested playlist item and persists before changing memory.
    ///
    /// # Errors
    ///
    /// Returns an error when the updated playlist cannot be persisted.
    pub fn remove_item(
        &mut self,
        playlist_index: usize,
        item_index: usize,
        timestamp: f64,
    ) -> Result<Option<MediaItem>, UserPlaylistControllerError> {
        let Some(playlist) = self.playlists.get(playlist_index) else {
            return Ok(None);
        };
        if item_index >= playlist.items.len() {
            return Ok(None);
        }
        let mut candidate = self.playlists.clone();
        let removed = candidate[playlist_index].items.remove(item_index);
        if timestamp.is_finite() {
            candidate[playlist_index].metadata.remove("updated_at");
            candidate[playlist_index].updated_at = Some(timestamp);
        }
        self.commit(candidate)?;
        Ok(Some(removed))
    }

    pub fn matching_playlist_indices(&self, item: &MediaItem) -> Vec<usize> {
        let Some(identity) = playlist_identity(item) else {
            return Vec::new();
        };
        self.playlists
            .iter()
            .enumerate()
            .filter_map(|(index, playlist)| {
                playlist
                    .items
                    .iter()
                    .any(|existing| playlist_identity(existing).as_deref() == Some(&identity))
                    .then_some(index)
            })
            .collect()
    }

    fn commit(&mut self, candidate: Vec<UserPlaylist>) -> Result<(), UserPlaylistControllerError> {
        if self.save_blocked {
            let file = self.file.as_ref().expect("blocked playlists have a file");
            return Err(UserPlaylistControllerError::SaveBlocked {
                path: file.path().to_path_buf(),
                message: self
                    .load_error
                    .clone()
                    .unwrap_or_else(|| "unknown load error".to_owned()),
            });
        }
        if let Some(file) = &self.file {
            file.save(&candidate)?;
        }
        self.playlists = candidate;
        Ok(())
    }
}

fn playlist_identity(item: &MediaItem) -> Option<String> {
    item.copy_location().or_else(|| item.stable_identity())
}

fn titles_match(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

fn durable_playlist_item(mut item: MediaItem, timestamp: f64) -> Option<MediaItem> {
    if !item.is_playable() || playlist_identity(&item).is_none() {
        return None;
    }
    item.stream_url = None;
    item.external_audio_url = None;
    if let Some(number) = serde_json::Number::from_f64(timestamp) {
        item.metadata
            .insert("added_at".to_owned(), serde_json::Value::Number(number));
    }
    Some(item)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_storage::UserPlaylistFile;
    use tempfile::tempdir;

    use super::{PlaylistAddOutcome, PlaylistCreateOutcome, UserPlaylistController};

    fn item(id: &str) -> MediaItem {
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
            stream_url: Some("https://cdn.example/temporary".parse().expect("stream URL")),
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn stable_data_is_read_without_writing_back_and_first_change_targets_beta() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("beta/playlists.json");
        let legacy = root.path().join("stable/playlists.json");
        let legacy_file = UserPlaylistFile::new(&legacy);
        let mut seed = UserPlaylistController::load(legacy_file.clone(), &legacy_file);
        assert_eq!(
            seed.create("Legacy", 1.0).expect("legacy create"),
            PlaylistCreateOutcome::Created(0)
        );
        let mut controller = UserPlaylistController::load(
            UserPlaylistFile::new(&current),
            &UserPlaylistFile::new(&legacy),
        );
        assert_eq!(controller.playlists().len(), 1);
        assert_eq!(
            controller.create("Beta", 2.0).expect("beta create"),
            PlaylistCreateOutcome::Created(1)
        );
        assert_eq!(
            UserPlaylistFile::new(current).load().expect("beta").len(),
            2
        );
        assert_eq!(
            UserPlaylistFile::new(legacy).load().expect("stable").len(),
            1
        );
    }

    #[test]
    fn add_is_deduplicated_and_drops_ephemeral_stream_urls() {
        let root = tempdir().expect("temporary directory");
        let file = UserPlaylistFile::new(root.path().join("playlists.json"));
        let mut controller = UserPlaylistController::load(
            file.clone(),
            &UserPlaylistFile::new(root.path().join("missing.json")),
        );
        assert_eq!(
            controller.create("Mix", 1.0).expect("create"),
            PlaylistCreateOutcome::Created(0)
        );
        assert_eq!(
            controller
                .add_item(0, item("abcdefghijk"), 2.0)
                .expect("add"),
            PlaylistAddOutcome::Added(0)
        );
        assert_eq!(
            controller
                .add_item(0, item("abcdefghijk"), 3.0)
                .expect("duplicate"),
            PlaylistAddOutcome::AlreadyPresent
        );
        assert!(controller.playlists()[0].items[0].stream_url.is_none());
        assert_eq!(file.load().expect("persisted")[0].items.len(), 1);
    }

    #[test]
    fn create_with_item_is_one_persisted_playlist_change() {
        let root = tempdir().expect("temporary directory");
        let file = UserPlaylistFile::new(root.path().join("playlists.json"));
        let mut controller = UserPlaylistController::load(
            file.clone(),
            &UserPlaylistFile::new(root.path().join("missing.json")),
        );

        assert_eq!(
            controller
                .create_with_item("Mix", item("abcdefghijk"), 4.0)
                .expect("create with item"),
            PlaylistCreateOutcome::Created(0)
        );
        let persisted = file.load().expect("persisted playlist");
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].items.len(), 1);
        assert_eq!(persisted[0].items[0].metadata["added_at"], 4.0);
        assert!(persisted[0].items[0].stream_url.is_none());
    }

    #[test]
    fn duplicate_titles_follow_python_unicode_lowercase_behavior() {
        let root = tempdir().expect("temporary directory");
        let mut controller = UserPlaylistController::load(
            UserPlaylistFile::new(root.path().join("playlists.json")),
            &UserPlaylistFile::new(root.path().join("missing.json")),
        );
        assert_eq!(
            controller.create("Čas", 1.0).expect("create"),
            PlaylistCreateOutcome::Created(0)
        );
        assert_eq!(
            controller.create("čas", 2.0).expect("duplicate"),
            PlaylistCreateOutcome::AlreadyExists
        );
    }

    #[test]
    fn corrupt_current_file_blocks_mutation_and_preserves_bytes() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("playlists.json");
        fs::write(&current, b"broken").expect("fixture");
        let mut controller = UserPlaylistController::load(
            UserPlaylistFile::new(&current),
            &UserPlaylistFile::new(root.path().join("legacy.json")),
        );
        assert!(controller.create("Mix", 1.0).is_err());
        assert_eq!(fs::read(current).expect("preserved"), b"broken");
    }
}
