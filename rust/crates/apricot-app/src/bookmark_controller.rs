//! Transactional playback bookmarks compatible with the Python application.

use std::path::PathBuf;

use apricot_core::MediaItem;
use apricot_storage::{Bookmark, BookmarkFile, BookmarkFileError, bookmark_media_key};
use rand::Rng;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BookmarkControllerError {
    #[error("bookmark changes are blocked because loading {path} failed: {message}")]
    SaveBlocked { path: PathBuf, message: String },
    #[error(transparent)]
    Storage(#[from] BookmarkFileError),
}

#[derive(Debug, Default)]
pub struct BookmarkController {
    bookmarks: Vec<Bookmark>,
    file: Option<BookmarkFile>,
    load_error: Option<String>,
    save_blocked: bool,
}

impl BookmarkController {
    pub fn load(current: BookmarkFile, legacy: &BookmarkFile, now: f64) -> Self {
        let (mut controller, imported_legacy) = if current.path().is_file() {
            match current.load() {
                Ok(bookmarks) => (Self::loaded(current, bookmarks), false),
                Err(error) => (Self::blocked(current, error.to_string()), false),
            }
        } else if legacy.path().is_file() {
            match legacy.load() {
                Ok(bookmarks) => (Self::loaded(current, bookmarks), true),
                Err(error) => (
                    Self {
                        bookmarks: Vec::new(),
                        file: Some(current),
                        load_error: Some(error.to_string()),
                        save_blocked: false,
                    },
                    false,
                ),
            }
        } else {
            (Self::loaded(current, Vec::new()), false)
        };
        let changed = controller.fill_missing_ids(now);
        if (changed || imported_legacy)
            && !controller.save_blocked
            && let Some(file) = &controller.file
            && let Err(error) = file.save(&controller.bookmarks)
        {
            controller.load_error = Some(error.to_string());
        }
        controller
    }

    fn loaded(file: BookmarkFile, bookmarks: Vec<Bookmark>) -> Self {
        Self {
            bookmarks,
            file: Some(file),
            load_error: None,
            save_blocked: false,
        }
    }

    fn blocked(file: BookmarkFile, message: String) -> Self {
        Self {
            bookmarks: Vec::new(),
            file: Some(file),
            load_error: Some(message),
            save_blocked: true,
        }
    }

    pub fn bookmarks(&self) -> &[Bookmark] {
        &self.bookmarks
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub fn sorted(&self) -> Vec<&Bookmark> {
        let mut bookmarks: Vec<_> = self.bookmarks.iter().collect();
        bookmarks.sort_by(|left, right| {
            left.media_title
                .to_lowercase()
                .cmp(&right.media_title.to_lowercase())
                .then_with(|| left.position.total_cmp(&right.position))
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        bookmarks
    }

    pub fn for_item(&self, item: &MediaItem) -> Vec<&Bookmark> {
        let Some(key) = bookmark_media_key(item) else {
            return Vec::new();
        };
        let mut bookmarks: Vec<_> = self
            .bookmarks
            .iter()
            .filter(|bookmark| bookmark.media_key == key)
            .collect();
        bookmarks.sort_by(|left, right| {
            left.position
                .total_cmp(&right.position)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        bookmarks
    }

    /// Adds one bookmark and atomically persists it before publishing the change.
    ///
    /// # Errors
    ///
    /// Returns an error when the bookmark file cannot be updated.
    pub fn add(
        &mut self,
        name: &str,
        position: f64,
        media: MediaItem,
        timestamp: f64,
    ) -> Result<Option<Bookmark>, BookmarkControllerError> {
        let Some(media_key) = bookmark_media_key(&media) else {
            return Ok(None);
        };
        let position = round_tenth(position);
        let name = name.trim().to_owned();
        let bookmark = Bookmark {
            id: format!(
                "{}-{}",
                milliseconds(timestamp),
                rand::rng().random_range(1000..=9999)
            ),
            name,
            position,
            media_key,
            media_title: media.title.trim().to_owned(),
            media,
            created_at: timestamp.is_finite().then_some(timestamp),
            updated_at: timestamp.is_finite().then_some(timestamp),
            metadata: serde_json::Map::new(),
        };
        let mut candidate = self.bookmarks.clone();
        candidate.push(bookmark.clone());
        self.commit(candidate)?;
        Ok(Some(bookmark))
    }

    /// Renames one bookmark identified by its durable bookmark id.
    ///
    /// # Errors
    ///
    /// Returns an error when the bookmark file cannot be updated.
    pub fn rename(
        &mut self,
        id: &str,
        name: &str,
        timestamp: f64,
    ) -> Result<bool, BookmarkControllerError> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(false);
        }
        let Some(index) = self.bookmarks.iter().position(|bookmark| bookmark.id == id) else {
            return Ok(false);
        };
        let mut candidate = self.bookmarks.clone();
        name.clone_into(&mut candidate[index].name);
        candidate[index].updated_at = timestamp.is_finite().then_some(timestamp);
        self.commit(candidate)?;
        Ok(true)
    }

    /// Deletes one bookmark identified by its durable bookmark id.
    ///
    /// # Errors
    ///
    /// Returns an error when the bookmark file cannot be updated.
    pub fn delete(&mut self, id: &str) -> Result<bool, BookmarkControllerError> {
        let Some(index) = self.bookmarks.iter().position(|bookmark| bookmark.id == id) else {
            return Ok(false);
        };
        let mut candidate = self.bookmarks.clone();
        candidate.remove(index);
        self.commit(candidate)?;
        Ok(true)
    }

    fn fill_missing_ids(&mut self, timestamp: f64) -> bool {
        let base = milliseconds(timestamp);
        let mut changed = false;
        for (index, bookmark) in self.bookmarks.iter_mut().enumerate() {
            if bookmark.id.trim().is_empty() {
                bookmark.id = format!("{base}-{index}");
                changed = true;
            }
        }
        changed
    }

    fn commit(&mut self, candidate: Vec<Bookmark>) -> Result<(), BookmarkControllerError> {
        if self.save_blocked {
            let file = self.file.as_ref().expect("blocked bookmarks have a file");
            return Err(BookmarkControllerError::SaveBlocked {
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
        self.bookmarks = candidate;
        Ok(())
    }
}

fn milliseconds(timestamp: f64) -> u128 {
    if !timestamp.is_finite() || timestamp <= 0.0 {
        return 0;
    }
    std::time::Duration::try_from_secs_f64(timestamp).map_or(0, |duration| duration.as_millis())
}

fn round_tenth(value: f64) -> f64 {
    if value.is_finite() {
        (value.max(0.0) * 10.0).round() / 10.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_storage::BookmarkFile;
    use tempfile::tempdir;

    use super::BookmarkController;

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
    fn stable_bookmarks_are_imported_without_modifying_stable_data() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("beta/bookmarks.json");
        let legacy = root.path().join("stable/bookmarks.json");
        let mut seed = BookmarkController::load(
            BookmarkFile::new(&legacy),
            &BookmarkFile::new(root.path().join("missing.json")),
            1.0,
        );
        seed.add("Intro", 12.0, item("abcdefghijk"), 2.0)
            .expect("add");
        let stable_before = fs::read(&legacy).expect("stable bytes");
        let controller = BookmarkController::load(
            BookmarkFile::new(&current),
            &BookmarkFile::new(&legacy),
            3.0,
        );
        assert_eq!(controller.bookmarks().len(), 1);
        assert!(current.is_file());
        assert_eq!(fs::read(legacy).expect("stable remains"), stable_before);
    }

    #[test]
    fn bookmarks_are_independent_per_media_key_and_sorted_by_position() {
        let root = tempdir().expect("temporary directory");
        let file = BookmarkFile::new(root.path().join("bookmarks.json"));
        let mut controller = BookmarkController::load(
            file,
            &BookmarkFile::new(root.path().join("missing.json")),
            1.0,
        );
        controller
            .add("Later", 20.0, item("firstvideo1"), 2.0)
            .expect("add");
        controller
            .add("Earlier", 5.0, item("firstvideo1"), 3.0)
            .expect("add");
        controller
            .add("Other", 10.0, item("secondvideo"), 4.0)
            .expect("add");
        let first = item("firstvideo1");
        let visible = controller.for_item(&first);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].name, "Earlier");
        assert_eq!(visible[1].name, "Later");
    }

    #[test]
    fn failed_current_load_blocks_destructive_mutation() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("bookmarks.json");
        fs::write(&current, b"broken").expect("fixture");
        let mut controller = BookmarkController::load(
            BookmarkFile::new(&current),
            &BookmarkFile::new(root.path().join("missing.json")),
            1.0,
        );
        assert!(
            controller
                .add("Name", 1.0, item("abcdefghijk"), 2.0)
                .is_err()
        );
        assert_eq!(fs::read(current).expect("preserved"), b"broken");
    }
}
