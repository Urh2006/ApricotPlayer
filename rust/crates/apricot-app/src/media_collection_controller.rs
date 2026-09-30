//! Transactional durable media collections used by favorites and history.

use std::path::PathBuf;

use apricot_core::MediaItem;
use apricot_storage::{MediaListFile, MediaListFileError};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionAddOutcome {
    Added,
    AlreadyPresent,
    Unplayable,
}

#[derive(Debug, Error)]
pub enum MediaCollectionControllerError {
    #[error("media collection changes are blocked because loading {path} failed: {message}")]
    SaveBlocked { path: PathBuf, message: String },
    #[error(transparent)]
    Storage(#[from] MediaListFileError),
}

#[derive(Debug, Default)]
pub struct MediaCollectionController {
    items: Vec<MediaItem>,
    file: Option<MediaListFile>,
    load_error: Option<String>,
    save_blocked: bool,
}

impl MediaCollectionController {
    pub fn load(current: MediaListFile, legacy: &MediaListFile) -> Self {
        if current.path().is_file() {
            return match current.load() {
                Ok(items) => Self::loaded(current, items),
                Err(error) => Self::blocked(current, error.to_string()),
            };
        }
        if legacy.path().is_file() {
            return match legacy.load() {
                Ok(items) => Self::loaded(current, items),
                Err(error) => Self {
                    items: Vec::new(),
                    file: Some(current),
                    load_error: Some(error.to_string()),
                    save_blocked: false,
                },
            };
        }
        Self::loaded(current, Vec::new())
    }

    fn loaded(file: MediaListFile, items: Vec<MediaItem>) -> Self {
        Self {
            items,
            file: Some(file),
            load_error: None,
            save_blocked: false,
        }
    }

    fn blocked(file: MediaListFile, message: String) -> Self {
        Self {
            items: Vec::new(),
            file: Some(file),
            load_error: Some(message),
            save_blocked: true,
        }
    }

    pub fn items(&self) -> &[MediaItem] {
        &self.items
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub const fn save_is_blocked(&self) -> bool {
        self.save_blocked
    }

    /// Appends a unique playable item, as required by favorites.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed collection cannot be persisted.
    pub fn add_unique(
        &mut self,
        item: MediaItem,
    ) -> Result<CollectionAddOutcome, MediaCollectionControllerError> {
        if collection_identity(&item).is_none() {
            return Ok(CollectionAddOutcome::Unplayable);
        }
        if self.contains(&item) {
            return Ok(CollectionAddOutcome::AlreadyPresent);
        }
        let mut candidate = self.items.clone();
        candidate.push(item);
        self.commit(candidate)?;
        Ok(CollectionAddOutcome::Added)
    }

    /// Moves the item to the front, replacing an older matching entry and
    /// truncating to the configured history limit. Like Python `record_history`
    /// any entry with a location is kept, so a downloaded playlist or channel
    /// is recorded as well.
    ///
    /// # Errors
    ///
    /// Returns an error when the history snapshot cannot be persisted.
    pub fn upsert_front(
        &mut self,
        item: MediaItem,
        limit: usize,
    ) -> Result<(), MediaCollectionControllerError> {
        let has_location = item.url.is_some()
            || item
                .local_path
                .as_ref()
                .is_some_and(|path| !path.trim().is_empty());
        if !has_location {
            return Ok(());
        }
        let Some(identity) = collection_identity(&item) else {
            return Ok(());
        };
        let mut candidate = self.items.clone();
        candidate.retain(|existing| collection_identity(existing).as_deref() != Some(&identity));
        candidate.insert(0, item);
        candidate.truncate(limit.max(10));
        self.commit(candidate)
    }

    pub fn contains(&self, item: &MediaItem) -> bool {
        self.position(item).is_some()
    }

    pub fn position(&self, item: &MediaItem) -> Option<usize> {
        let identity = collection_identity(item)?;
        self.items
            .iter()
            .position(|existing| collection_identity(existing).as_deref() == Some(&identity))
    }

    /// Removes one position and persists before changing memory.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed collection cannot be persisted.
    pub fn remove(
        &mut self,
        index: usize,
    ) -> Result<Option<MediaItem>, MediaCollectionControllerError> {
        if index >= self.items.len() {
            return Ok(None);
        }
        let mut candidate = self.items.clone();
        let removed = candidate.remove(index);
        self.commit(candidate)?;
        Ok(Some(removed))
    }

    /// Removes a matching item using the Python collection URL/path identity.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed collection cannot be persisted.
    pub fn remove_item(
        &mut self,
        item: &MediaItem,
    ) -> Result<Option<MediaItem>, MediaCollectionControllerError> {
        let Some(index) = self.position(item) else {
            return Ok(None);
        };
        self.remove(index)
    }

    /// Clears the collection atomically.
    ///
    /// # Errors
    ///
    /// Returns an error when the empty snapshot cannot be persisted.
    pub fn clear(&mut self) -> Result<bool, MediaCollectionControllerError> {
        if self.items.is_empty() {
            return Ok(false);
        }
        self.commit(Vec::new())?;
        Ok(true)
    }

    fn commit(&mut self, candidate: Vec<MediaItem>) -> Result<(), MediaCollectionControllerError> {
        if self.save_blocked {
            let file = self.file.as_ref().expect("blocked collections have a file");
            return Err(MediaCollectionControllerError::SaveBlocked {
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
        self.items = candidate;
        Ok(())
    }
}

fn collection_identity(item: &MediaItem) -> Option<String> {
    item.copy_location().or_else(|| item.stable_identity())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_storage::MediaListFile;
    use tempfile::tempdir;

    use super::{CollectionAddOutcome, MediaCollectionController};

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
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn stable_data_is_read_without_writing_back_and_mutations_target_beta() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("beta/favorites.json");
        let legacy = root.path().join("stable/favorites.json");
        let legacy_file = MediaListFile::new(&legacy);
        legacy_file.save(&[item("one")]).expect("legacy");
        let mut controller =
            MediaCollectionController::load(MediaListFile::new(&current), &legacy_file);
        assert_eq!(controller.items().len(), 1);
        assert_eq!(
            controller.add_unique(item("two")).expect("add"),
            CollectionAddOutcome::Added
        );
        assert_eq!(MediaListFile::new(current).load().expect("beta").len(), 2);
        assert_eq!(MediaListFile::new(legacy).load().expect("stable").len(), 1);
    }

    #[test]
    fn history_replaces_matching_identity_at_front_and_honors_the_limit() {
        let root = tempdir().expect("temporary directory");
        let current = MediaListFile::new(root.path().join("history.json"));
        let missing = MediaListFile::new(root.path().join("missing.json"));
        let mut controller = MediaCollectionController::load(current, &missing);
        for index in 0..12 {
            controller
                .upsert_front(item(&format!("item{index}")), 10)
                .expect("history update");
        }
        assert_eq!(controller.items().len(), 10);
        assert_eq!(controller.items()[0].id.0, "item11");
        controller
            .upsert_front(item("item5"), 10)
            .expect("history refresh");
        assert_eq!(controller.items()[0].id.0, "item5");
        assert_eq!(
            controller
                .items()
                .iter()
                .filter(|candidate| candidate.id.0 == "item5")
                .count(),
            1
        );
    }

    #[test]
    fn downloaded_playlists_and_channels_are_recorded_in_history() {
        let root = tempdir().expect("temporary directory");
        let current = MediaListFile::new(root.path().join("history.json"));
        let missing = MediaListFile::new(root.path().join("missing.json"));
        let mut controller = MediaCollectionController::load(current, &missing);
        let mut playlist = item("PLlist");
        playlist.kind = MediaKind::Playlist;
        playlist.url = Some(
            "https://www.youtube.com/playlist?list=PLlist"
                .parse()
                .expect("URL"),
        );
        let mut channel = item("UCchannel");
        channel.kind = MediaKind::Channel;
        channel.url = Some(
            "https://www.youtube.com/channel/UCchannel"
                .parse()
                .expect("URL"),
        );
        controller.upsert_front(playlist, 10).expect("playlist");
        controller.upsert_front(channel, 10).expect("channel");
        let mut without_location = item("none");
        without_location.url = None;
        controller
            .upsert_front(without_location, 10)
            .expect("ignored");
        let kinds = controller
            .items()
            .iter()
            .map(|entry| entry.kind)
            .collect::<Vec<_>>();
        assert_eq!(kinds, vec![MediaKind::Channel, MediaKind::Playlist]);
    }

    #[test]
    fn corrupt_current_file_blocks_mutation_and_preserves_bytes() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("favorites.json");
        fs::write(&current, b"broken").expect("fixture");
        let legacy = MediaListFile::new(root.path().join("legacy.json"));
        let mut controller = MediaCollectionController::load(MediaListFile::new(&current), &legacy);
        assert!(controller.save_is_blocked());
        assert!(controller.add_unique(item("one")).is_err());
        assert_eq!(fs::read(current).expect("preserved"), b"broken");
    }
}
