use std::{
    io,
    path::{Path, PathBuf},
};

use apricot_core::MediaItem;
use serde_json::Value;
use thiserror::Error;

use crate::{
    JsonFileError, media_item_from_python_value, media_item_to_python_value, read_json,
    write_json_atomic,
};

#[derive(Debug, Error)]
pub enum PlaybackQueueFileError {
    #[error(transparent)]
    Json(#[from] JsonFileError),
    #[error("playback queue item {index} is not a compatible playable media item")]
    InvalidItem { index: usize },
}

#[derive(Clone, Debug)]
pub struct PlaybackQueueFile {
    path: PathBuf,
}

impl PlaybackQueueFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the Python-compatible array. A missing file is an empty queue;
    /// malformed content is an error and must never be overwritten silently.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read or parsed, or when any
    /// entry cannot be converted into a playable media item.
    pub fn load(&self) -> Result<Vec<MediaItem>, PlaybackQueueFileError> {
        match read_json::<Value>(&self.path) {
            Ok(Value::Array(values)) => values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    media_item_from_python_value(value)
                        .filter(MediaItem::is_playable)
                        .ok_or(PlaybackQueueFileError::InvalidItem { index })
                })
                .collect(),
            Ok(_) => Err(PlaybackQueueFileError::InvalidItem { index: 0 }),
            Err(JsonFileError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                Ok(Vec::new())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Saves the queue atomically using the Python-compatible JSON shape.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination cannot be written atomically.
    pub fn save(&self, items: &[MediaItem]) -> Result<(), PlaybackQueueFileError> {
        let values: Vec<_> = items.iter().map(media_item_to_python_value).collect();
        write_json_atomic(&self.path, &values)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;
    use tempfile::tempdir;

    use super::PlaybackQueueFile;

    #[test]
    fn python_queue_loads_and_saves_without_unknown_field_loss() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("playback_queue.json");
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!([{
                "title": "Video",
                "kind": "video",
                "url": "https://www.youtube.com/watch?v=abc",
                "custom": 7
            }]))
            .expect("JSON"),
        )
        .expect("write fixture");
        let file = PlaybackQueueFile::new(&path);
        let items = file.load().expect("load queue");
        file.save(&items).expect("save queue");
        let restored: serde_json::Value =
            serde_json::from_slice(&fs::read(path).expect("read restored")).expect("JSON");
        assert_eq!(restored[0]["custom"], 7);
    }

    #[test]
    fn malformed_queue_is_rejected_and_not_replaced() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("playback_queue.json");
        fs::write(&path, b"not json").expect("write corrupt fixture");
        let file = PlaybackQueueFile::new(&path);
        assert!(file.load().is_err());
        assert_eq!(fs::read(path).expect("preserved"), b"not json");
    }
}
