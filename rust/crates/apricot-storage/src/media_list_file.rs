//! Python-compatible durable lists of media items.

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
pub enum MediaListFileError {
    #[error(transparent)]
    Json(#[from] JsonFileError),
    #[error("media list item {index} is not a compatible playable media item")]
    InvalidItem { index: usize },
}

#[derive(Clone, Debug)]
pub struct MediaListFile {
    path: PathBuf,
}

impl MediaListFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads a Python-compatible JSON array without silently dropping entries.
    /// A missing file represents an empty list.
    ///
    /// # Errors
    ///
    /// Returns an error when the file is malformed or any entry cannot be
    /// represented as a playable media item.
    pub fn load(&self) -> Result<Vec<MediaItem>, MediaListFileError> {
        match read_json::<Value>(&self.path) {
            Ok(Value::Array(values)) => values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    media_item_from_python_value(value)
                        .ok_or(MediaListFileError::InvalidItem { index })
                })
                .collect(),
            Ok(_) => Err(MediaListFileError::InvalidItem { index: 0 }),
            Err(JsonFileError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                Ok(Vec::new())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Saves the complete list atomically in the current Python-compatible shape.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination cannot be replaced atomically.
    pub fn save(&self, items: &[MediaItem]) -> Result<(), MediaListFileError> {
        let values: Vec<_> = items.iter().map(media_item_to_python_value).collect();
        write_json_atomic(&self.path, &values)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::MediaListFile;

    #[test]
    fn python_list_round_trips_history_metadata() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("history.json");
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!([{
                "title": "Direct item",
                "kind": "unknown",
                "url": "https://media.example/item",
                "action": "played",
                "timestamp": 1234.5,
                "future_field": {"kept": true}
            }]))
            .expect("JSON"),
        )
        .expect("fixture");
        let file = MediaListFile::new(&path);
        let items = file.load().expect("load");
        file.save(&items).expect("save");
        let restored: Value = serde_json::from_slice(&fs::read(path).expect("read")).expect("JSON");
        assert_eq!(restored[0]["action"], "played");
        assert_eq!(restored[0]["timestamp"], 1234.5);
        assert_eq!(restored[0]["future_field"], json!({"kept": true}));
    }

    #[test]
    fn invalid_entry_blocks_loading_without_rewriting_the_file() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("favorites.json");
        let bytes = br#"[{"title":"missing location"}]"#;
        fs::write(&path, bytes).expect("fixture");
        assert!(MediaListFile::new(&path).load().is_err());
        assert_eq!(fs::read(path).expect("preserved"), bytes);
    }

    #[test]
    fn favorite_collections_are_preserved_even_though_they_are_not_tracks() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("favorites.json");
        fs::write(
            &path,
            br#"[{"title":"A playlist","kind":"playlist","url":"https://www.youtube.com/playlist?list=PL123"}]"#,
        )
        .expect("fixture");
        let file = MediaListFile::new(&path);
        let items = file.load().expect("load collection favorite");
        assert_eq!(items.len(), 1);
        assert!(!items[0].is_playable());
        file.save(&items).expect("save collection favorite");
        assert_eq!(file.load().expect("reload").len(), 1);
    }
}
