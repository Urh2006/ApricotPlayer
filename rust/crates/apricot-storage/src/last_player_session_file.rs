//! Python-compatible snapshot of the most recently started player session.

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

const MAX_SEQUENCE_ITEMS: usize = 200;
const MAX_VALUE_DEPTH: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct LastPlayerSession {
    pub version: u64,
    pub saved_at: f64,
    pub title: String,
    pub item: MediaItem,
    pub return_screen: String,
    pub return_data: Map<String, Value>,
    pub sequence: Vec<MediaItem>,
    extra: Map<String, Value>,
}

impl LastPlayerSession {
    pub fn new(
        saved_at: f64,
        item: MediaItem,
        return_screen: impl Into<String>,
        return_data: Map<String, Value>,
        sequence: Vec<MediaItem>,
    ) -> Self {
        let title = if item.title.trim().is_empty() {
            item.copy_location().unwrap_or_default()
        } else {
            item.title.clone()
        };
        let sequence: Vec<_> = sequence
            .into_iter()
            .filter(MediaItem::is_playable)
            .take(MAX_SEQUENCE_ITEMS)
            .collect();
        let sequence = normalize_current_sequence_identity(sequence, &item);
        Self {
            version: 2,
            saved_at: if saved_at.is_finite() {
                saved_at.max(0.0)
            } else {
                0.0
            },
            title,
            item,
            return_screen: return_screen.into(),
            return_data,
            sequence,
            extra: Map::new(),
        }
    }

    pub fn is_available(&self) -> bool {
        self.item.copy_location().is_some()
    }
}

#[derive(Debug, Error)]
pub enum LastPlayerSessionFileError {
    #[error(transparent)]
    Json(#[from] JsonFileError),
    #[error("last player session data is not a JSON object")]
    InvalidRoot,
    #[error("last player session does not contain a compatible media item")]
    InvalidItem,
}

#[derive(Clone, Debug)]
pub struct LastPlayerSessionFile {
    path: PathBuf,
}

impl LastPlayerSessionFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads one Python-compatible snapshot. A missing file has no snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when existing data is malformed or has no compatible
    /// media identity.
    pub fn load(&self) -> Result<Option<LastPlayerSession>, LastPlayerSessionFileError> {
        let value = match read_json::<Value>(&self.path) {
            Ok(value) => value,
            Err(JsonFileError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        let Value::Object(mut object) = value else {
            return Err(LastPlayerSessionFileError::InvalidRoot);
        };
        let item_value = object
            .remove("item")
            .ok_or(LastPlayerSessionFileError::InvalidItem)?;
        let item = media_item_from_python_value(&item_value)
            .filter(MediaItem::is_playable)
            .ok_or(LastPlayerSessionFileError::InvalidItem)?;
        let sequence: Vec<_> = object
            .remove("sequence")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .take(MAX_SEQUENCE_ITEMS)
            .filter_map(|value| media_item_from_python_value(&value))
            .filter(MediaItem::is_playable)
            .collect();
        let sequence = normalize_current_sequence_identity(sequence, &item);
        let return_data = object
            .remove("return_data")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        let version = object
            .remove("version")
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        let saved_at = object
            .remove("saved_at")
            .and_then(|value| value.as_f64())
            .filter(|value| value.is_finite() && *value >= 0.0)
            .unwrap_or_default();
        let title = object
            .remove("title")
            .and_then(|value| value.as_str().map(str::to_owned))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| item.title.clone());
        let return_screen = object
            .remove("return_screen")
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        Ok(Some(LastPlayerSession {
            version,
            saved_at,
            title,
            item,
            return_screen,
            return_data,
            sequence,
            extra: object,
        }))
    }

    /// Atomically writes one bounded Python-compatible snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or atomic replacement fails.
    pub fn save(&self, session: &LastPlayerSession) -> Result<(), LastPlayerSessionFileError> {
        let mut object = sanitize_object(session.extra.clone(), 0);
        object.insert("version".to_owned(), Value::from(session.version));
        object.insert("saved_at".to_owned(), Value::from(session.saved_at));
        object.insert("title".to_owned(), Value::String(session.title.clone()));
        object.insert(
            "item".to_owned(),
            sanitize_session_value(session_media_value(&session.item), 0),
        );
        object.insert(
            "return_screen".to_owned(),
            Value::String(session.return_screen.clone()),
        );
        object.insert(
            "return_data".to_owned(),
            Value::Object(sanitize_object(session.return_data.clone(), 0)),
        );
        if session.sequence.is_empty() {
            object.remove("sequence");
        } else {
            object.insert(
                "sequence".to_owned(),
                Value::Array(
                    session
                        .sequence
                        .iter()
                        .take(MAX_SEQUENCE_ITEMS)
                        .map(session_media_value)
                        .map(|value| sanitize_session_value(value, 0))
                        .collect(),
                ),
            );
        }
        write_json_atomic(&self.path, &object)?;
        Ok(())
    }
}

fn session_media_value(item: &MediaItem) -> Value {
    let mut value = media_item_to_python_value(item);
    if let Some(object) = value.as_object_mut() {
        object.insert("id".to_owned(), Value::String(item.id.0.clone()));
    }
    value
}

fn normalize_current_sequence_identity(
    mut sequence: Vec<MediaItem>,
    current: &MediaItem,
) -> Vec<MediaItem> {
    let Some(location) = current.copy_location() else {
        return Vec::new();
    };
    let Some(candidate) = sequence
        .iter_mut()
        .find(|candidate| candidate.copy_location().as_deref() == Some(&location))
    else {
        return Vec::new();
    };
    candidate.id = current.id.clone();
    sequence
}

fn sanitize_object(object: Map<String, Value>, depth: usize) -> Map<String, Value> {
    object
        .into_iter()
        .filter(|(key, _value)| !is_large_ephemeral_field(key))
        .map(|(key, value)| (key, sanitize_session_value(value, depth + 1)))
        .collect()
}

fn sanitize_session_value(value: Value, depth: usize) -> Value {
    if depth > MAX_VALUE_DEPTH {
        return match value {
            Value::Null => Value::Null,
            Value::String(value) => Value::String(value),
            other => Value::String(other.to_string()),
        };
    }
    match value {
        Value::Object(object) => Value::Object(sanitize_object(object, depth)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .take(MAX_SEQUENCE_ITEMS)
                .map(|value| sanitize_session_value(value, depth + 1))
                .collect(),
        ),
        scalar => scalar,
    }
}

fn is_large_ephemeral_field(key: &str) -> bool {
    matches!(
        key,
        "formats"
            | "requested_formats"
            | "automatic_captions"
            | "subtitles"
            | "requested_subtitles"
            | "thumbnails"
            | "heatmap"
            | "entries"
            | "comments"
    )
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use serde_json::{Map, Value, json};
    use tempfile::tempdir;

    use super::{LastPlayerSession, LastPlayerSessionFile};

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
            duration_seconds: Some(100.0),
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn python_snapshot_loads_and_round_trips_unknown_fields() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("last_player_session.json");
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "version": 2,
                "saved_at": 42.5,
                "title": "Two",
                "item": {
                    "id": "two",
                    "title": "Two",
                    "kind": "video",
                    "url": "https://www.youtube.com/watch?v=two",
                    "future_item": {"kept": true}
                },
                "return_screen": "search",
                "return_data": {"index": 1, "future_return": true},
                "sequence": [
                    {"id": "one", "title": "One", "kind": "video", "url": "https://www.youtube.com/watch?v=one"},
                    {"id": "two", "title": "Two", "kind": "video", "url": "https://www.youtube.com/watch?v=two"}
                ],
                "future_root": [1, 2, 3]
            }))
            .expect("JSON"),
        )
        .expect("fixture");
        let file = LastPlayerSessionFile::new(&path);
        let session = file.load().expect("load").expect("session");
        assert_eq!(session.sequence.len(), 2);
        file.save(&session).expect("save");
        let restored: Value = serde_json::from_slice(&fs::read(path).expect("read")).expect("JSON");
        assert_eq!(restored["future_root"], json!([1, 2, 3]));
        assert_eq!(restored["item"]["future_item"], json!({"kept": true}));
        assert_eq!(restored["return_data"]["future_return"], true);
    }

    #[test]
    fn writer_bounds_sequence_and_removes_large_ephemeral_fields() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("last_player_session.json");
        let mut current = item("current");
        current
            .metadata
            .insert("formats".to_owned(), json!([{"large": true}]));
        current
            .metadata
            .insert("description".to_owned(), Value::String("kept".to_owned()));
        let sequence = std::iter::once(item("current"))
            .chain((0..249).map(|index| item(&index.to_string())))
            .collect();
        let session = LastPlayerSession::new(10.0, current, "search", Map::new(), sequence);
        let file = LastPlayerSessionFile::new(&path);
        file.save(&session).expect("save");
        let raw: Value = serde_json::from_slice(&fs::read(path).expect("read")).expect("JSON");
        assert_eq!(raw["sequence"].as_array().expect("sequence").len(), 200);
        assert!(raw["item"].get("formats").is_none());
        assert_eq!(raw["item"]["description"], "kept");
    }

    #[test]
    fn malformed_existing_snapshot_is_never_rewritten() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("last_player_session.json");
        fs::write(&path, b"broken").expect("fixture");
        let file = LastPlayerSessionFile::new(&path);
        assert!(file.load().is_err());
        assert_eq!(fs::read(path).expect("preserved"), b"broken");
    }

    #[test]
    fn python_sequence_without_ids_still_matches_current_item_by_url() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("last_player_session.json");
        fs::write(
            &path,
            br#"{
                "item":{"id":"video-id","title":"Current","kind":"video","url":"https://www.youtube.com/watch?v=video-id"},
                "sequence":[
                    {"title":"Previous","kind":"video","url":"https://www.youtube.com/watch?v=previous"},
                    {"title":"Current","kind":"video","url":"https://www.youtube.com/watch?v=video-id"}
                ]
            }"#,
        )
        .expect("fixture");
        let session = LastPlayerSessionFile::new(path)
            .load()
            .expect("load")
            .expect("session");
        assert_eq!(session.sequence.len(), 2);
        assert_eq!(session.sequence[1].id, session.item.id);
    }
}
