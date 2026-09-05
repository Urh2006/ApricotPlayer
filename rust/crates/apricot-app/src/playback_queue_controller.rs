//! Durable playback-queue coordination with transactional mutations.

use std::path::PathBuf;

use apricot_core::MediaItem;
use apricot_storage::{PlaybackQueueFile, PlaybackQueueFileError};
use thiserror::Error;

use crate::{PlaybackQueue, QueueAddOutcome};

#[derive(Debug, Error)]
pub enum PlaybackQueueControllerError {
    #[error("playback queue changes are blocked because loading {path} failed: {message}")]
    SaveBlocked { path: PathBuf, message: String },
    #[error(transparent)]
    Storage(#[from] PlaybackQueueFileError),
}

#[derive(Debug, Default)]
pub struct PlaybackQueueController {
    queue: PlaybackQueue,
    file: Option<PlaybackQueueFile>,
    load_error: Option<String>,
    save_blocked: bool,
}

impl PlaybackQueueController {
    pub fn load(current: PlaybackQueueFile, legacy: &PlaybackQueueFile) -> Self {
        if current.path().is_file() {
            return match current.load() {
                Ok(items) => Self::loaded(current, items),
                Err(error) => Self {
                    queue: PlaybackQueue::default(),
                    file: Some(current),
                    load_error: Some(error.to_string()),
                    save_blocked: true,
                },
            };
        }
        if legacy.path().is_file() {
            return match legacy.load() {
                Ok(items) => Self::loaded(current, items),
                Err(error) => Self {
                    queue: PlaybackQueue::default(),
                    file: Some(current),
                    load_error: Some(error.to_string()),
                    save_blocked: false,
                },
            };
        }
        Self::loaded(current, Vec::new())
    }

    fn loaded(file: PlaybackQueueFile, items: Vec<MediaItem>) -> Self {
        let mut queue = PlaybackQueue::default();
        queue.replace(items);
        Self {
            queue,
            file: Some(file),
            load_error: None,
            save_blocked: false,
        }
    }

    pub const fn queue(&self) -> &PlaybackQueue {
        &self.queue
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub const fn save_is_blocked(&self) -> bool {
        self.save_blocked
    }

    /// Replaces the complete queue and persists it before committing memory.
    ///
    /// # Errors
    ///
    /// Returns an error when the queue is write-blocked or storage fails.
    pub fn replace(&mut self, items: Vec<MediaItem>) -> Result<(), PlaybackQueueControllerError> {
        let mut candidate = PlaybackQueue::default();
        candidate.replace(items);
        self.commit(candidate)
    }

    /// Adds one item unless it is unplayable or already present.
    ///
    /// # Errors
    ///
    /// Returns an error when a new item cannot be persisted.
    pub fn add(
        &mut self,
        item: MediaItem,
    ) -> Result<QueueAddOutcome, PlaybackQueueControllerError> {
        let mut candidate = self.queue.clone();
        let outcome = candidate.add(item);
        if outcome == QueueAddOutcome::Added {
            self.commit(candidate)?;
        }
        Ok(outcome)
    }

    /// Removes the matching media item.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed queue cannot be persisted.
    pub fn remove_item(
        &mut self,
        item: &MediaItem,
    ) -> Result<Option<MediaItem>, PlaybackQueueControllerError> {
        let mut candidate = self.queue.clone();
        let removed = candidate.remove_item(item);
        if removed.is_some() {
            self.commit(candidate)?;
        }
        Ok(removed)
    }

    /// Removes one queue position.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed queue cannot be persisted.
    pub fn remove(
        &mut self,
        index: usize,
    ) -> Result<Option<MediaItem>, PlaybackQueueControllerError> {
        let mut candidate = self.queue.clone();
        let removed = candidate.remove(index);
        if removed.is_some() {
            self.commit(candidate)?;
        }
        Ok(removed)
    }

    /// Moves one queue position by the requested delta.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed queue cannot be persisted.
    pub fn move_by(
        &mut self,
        index: usize,
        delta: i32,
    ) -> Result<Option<usize>, PlaybackQueueControllerError> {
        let mut candidate = self.queue.clone();
        let moved = candidate.move_by(index, delta);
        if moved.is_some() {
            self.commit(candidate)?;
        }
        Ok(moved)
    }

    /// Clears every queued item.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed queue cannot be persisted.
    pub fn clear(&mut self) -> Result<bool, PlaybackQueueControllerError> {
        let mut candidate = self.queue.clone();
        let changed = candidate.clear();
        if changed {
            self.commit(candidate)?;
        }
        Ok(changed)
    }

    /// Consumes the front item only when it matches a confirmed player start.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed queue cannot be persisted.
    pub fn consume_front_if(
        &mut self,
        item: &MediaItem,
    ) -> Result<bool, PlaybackQueueControllerError> {
        let mut candidate = self.queue.clone();
        let changed = candidate.consume_front_if(item);
        if changed {
            self.commit(candidate)?;
        }
        Ok(changed)
    }

    fn commit(&mut self, candidate: PlaybackQueue) -> Result<(), PlaybackQueueControllerError> {
        if self.save_blocked {
            let file = self.file.as_ref().expect("blocked queues have a file");
            return Err(PlaybackQueueControllerError::SaveBlocked {
                path: file.path().to_path_buf(),
                message: self
                    .load_error
                    .clone()
                    .unwrap_or_else(|| "unknown load error".to_owned()),
            });
        }
        if let Some(file) = &self.file {
            file.save(candidate.items())?;
        }
        self.queue = candidate;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_storage::PlaybackQueueFile;
    use tempfile::tempdir;

    use super::{PlaybackQueueController, QueueAddOutcome};

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
    fn stable_queue_is_migrated_without_writing_back_to_stable() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("beta/playback_queue.json");
        let legacy = root.path().join("stable/playback_queue.json");
        let legacy_file = PlaybackQueueFile::new(&legacy);
        legacy_file.save(&[item("one")]).expect("legacy queue");
        let mut controller =
            PlaybackQueueController::load(PlaybackQueueFile::new(&current), &legacy_file);
        assert_eq!(controller.queue().len(), 1);
        assert_eq!(
            controller.add(item("two")).expect("add"),
            QueueAddOutcome::Added
        );
        assert_eq!(
            PlaybackQueueFile::new(&current)
                .load()
                .expect("beta queue")
                .len(),
            2
        );
        assert_eq!(
            PlaybackQueueFile::new(&legacy)
                .load()
                .expect("stable queue")
                .len(),
            1
        );
    }

    #[test]
    fn corrupt_current_queue_blocks_mutation_and_preserves_bytes() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("playback_queue.json");
        fs::write(&current, b"broken").expect("fixture");
        let legacy = PlaybackQueueFile::new(root.path().join("legacy.json"));
        let mut controller =
            PlaybackQueueController::load(PlaybackQueueFile::new(&current), &legacy);
        assert!(controller.save_is_blocked());
        assert!(controller.add(item("one")).is_err());
        assert_eq!(fs::read(current).expect("preserved"), b"broken");
        assert!(controller.queue().is_empty());
    }

    #[test]
    fn failed_save_does_not_commit_the_in_memory_candidate() {
        let root = tempdir().expect("temporary directory");
        let destination_is_directory = root.path().join("playback_queue.json");
        fs::create_dir(&destination_is_directory).expect("directory fixture");
        let mut controller = PlaybackQueueController {
            file: Some(PlaybackQueueFile::new(destination_is_directory)),
            ..PlaybackQueueController::default()
        };
        assert!(controller.add(item("one")).is_err());
        assert!(controller.queue().is_empty());
    }
}
