//! Python-compatible durable playback bookmarks.

use std::{
    io,
    path::{Path, PathBuf},
};

use apricot_core::MediaItem;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::{
    JsonFileError, media_item_from_python_value, media_item_to_python_value, read_json,
    write_json_atomic,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Bookmark {
    pub id: String,
    pub name: String,
    pub position: f64,
    pub media_key: String,
    pub media_title: String,
    pub media: MediaItem,
    pub created_at: Option<f64>,
    pub updated_at: Option<f64>,
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Error)]
pub enum BookmarkFileError {
    #[error(transparent)]
    Json(#[from] JsonFileError),
    #[error("bookmarks data is not a JSON array")]
    InvalidRoot,
}

#[derive(Clone, Debug)]
pub struct BookmarkFile {
    path: PathBuf,
}

impl BookmarkFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads every compatible Python bookmark while matching Python's
    /// normalization behavior for malformed individual entries.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read, parsed, or is not an array.
    pub fn load(&self) -> Result<Vec<Bookmark>, BookmarkFileError> {
        match read_json::<Value>(&self.path) {
            Ok(Value::Array(values)) => Ok(values
                .iter()
                .filter_map(bookmark_from_python_value)
                .collect()),
            Ok(_) => Err(BookmarkFileError::InvalidRoot),
            Err(JsonFileError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                Ok(Vec::new())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Atomically saves the complete collection in the Python data shape.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination cannot be replaced atomically.
    pub fn save(&self, bookmarks: &[Bookmark]) -> Result<(), BookmarkFileError> {
        let values: Vec<_> = bookmarks.iter().map(bookmark_to_python_value).collect();
        write_json_atomic(&self.path, &values)?;
        Ok(())
    }
}

pub fn bookmark_media_key(item: &MediaItem) -> Option<String> {
    for key in ["url", "webpage_url", "path", "original_url", "watch_url"] {
        if let Some(value) = item
            .metadata
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_owned());
        }
    }
    item.copy_location()
}

fn bookmark_from_python_value(value: &Value) -> Option<Bookmark> {
    let object = value.as_object()?;
    let mut media_object = object
        .get("media")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in [
        "title",
        "channel",
        "kind",
        "url",
        "webpage_url",
        "path",
        "local_path",
        "original_url",
        "watch_url",
        "duration",
        "duration_seconds",
        "type",
    ] {
        if !media_object.contains_key(key)
            && let Some(value) = object.get(key).filter(|value| !value.is_null())
        {
            media_object.insert(key.to_owned(), value.clone());
        }
    }
    if !media_object.contains_key("title") {
        let title = first_text(object, &["media_title", "name"]);
        if !title.is_empty() {
            media_object.insert("title".to_owned(), Value::String(title));
        }
    }
    let media = media_item_from_python_value(&Value::Object(media_object))?;
    let media_key = {
        let explicit = text(object, "media_key");
        if explicit.is_empty() {
            bookmark_media_key(&media)?
        } else {
            explicit
        }
    };
    let position = flexible_number(object.get("position"))
        .unwrap_or(0.0)
        .max(0.0);
    let media_title = {
        let title = text(object, "media_title");
        if title.is_empty() {
            media.title.clone()
        } else {
            title
        }
    };
    let mut metadata = object.clone();
    for key in [
        "id",
        "name",
        "position",
        "media_key",
        "media_title",
        "media",
        "created_at",
        "updated_at",
    ] {
        metadata.remove(key);
    }
    Some(Bookmark {
        id: text(object, "id"),
        name: text(object, "name"),
        position: round_tenth(position),
        media_key,
        media_title,
        media,
        created_at: flexible_number(object.get("created_at")),
        updated_at: flexible_number(object.get("updated_at")),
        metadata,
    })
}

fn bookmark_to_python_value(bookmark: &Bookmark) -> Value {
    let mut object = bookmark.metadata.clone();
    object.insert("id".to_owned(), Value::String(bookmark.id.clone()));
    object.insert("name".to_owned(), Value::String(bookmark.name.clone()));
    object.insert(
        "position".to_owned(),
        Value::from(round_tenth(bookmark.position)),
    );
    object.insert(
        "media_key".to_owned(),
        Value::String(bookmark.media_key.clone()),
    );
    object.insert(
        "media_title".to_owned(),
        Value::String(bookmark.media_title.clone()),
    );
    object.insert(
        "media".to_owned(),
        media_item_to_python_value(&bookmark.media),
    );
    if let Some(created_at) = bookmark.created_at {
        object.insert("created_at".to_owned(), Value::from(created_at));
    }
    if let Some(updated_at) = bookmark.updated_at {
        object.insert("updated_at".to_owned(), Value::from(updated_at));
    }
    Value::Object(object)
}

fn text(object: &Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn first_text(object: &Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .map(|key| text(object, key))
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn flexible_number(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    let number = value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())?;
    number.is_finite().then_some(number)
}

fn round_tenth(value: f64) -> f64 {
    (value.max(0.0) * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::BookmarkFile;

    #[test]
    fn old_top_level_python_shape_is_normalized_without_losing_unknown_fields() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("bookmarks.json");
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!([{
                "name": "Intro",
                "position": "12.26",
                "url": "https://www.youtube.com/watch?v=abcdefghijk",
                "title": "Video",
                "kind": "video",
                "future": {"kept": true}
            }]))
            .expect("JSON"),
        )
        .expect("fixture");
        let file = BookmarkFile::new(&path);
        let bookmarks = file.load().expect("bookmarks");
        assert_eq!(bookmarks.len(), 1);
        assert!((bookmarks[0].position - 12.3).abs() < f64::EPSILON);
        assert_eq!(bookmarks[0].media.title, "Video");
        assert_eq!(bookmarks[0].metadata["future"], json!({"kept": true}));
        file.save(&bookmarks).expect("save");
        let restored: Value = serde_json::from_slice(&fs::read(path).expect("read")).expect("JSON");
        assert_eq!(restored[0]["future"], json!({"kept": true}));
        assert_eq!(
            restored[0]["media"]["url"],
            "https://www.youtube.com/watch?v=abcdefghijk"
        );
    }

    #[test]
    fn invalid_individual_entries_are_filtered_like_python_normalization() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("bookmarks.json");
        fs::write(
            &path,
            br#"[null,{"name":"missing media"},{"name":"Valid","position":2,"media":{"kind":"local_file","url":"C:\\Music\\track.mp3"}}]"#,
        )
        .expect("fixture");
        let bookmarks = BookmarkFile::new(path).load().expect("bookmarks");
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].name, "Valid");
    }

    #[test]
    fn malformed_current_file_is_rejected_without_rewrite() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("bookmarks.json");
        fs::write(&path, b"broken").expect("fixture");
        assert!(BookmarkFile::new(&path).load().is_err());
        assert_eq!(fs::read(path).expect("preserved"), b"broken");
    }
}
