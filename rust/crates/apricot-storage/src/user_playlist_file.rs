//! Python-compatible durable user playlists.

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
pub struct UserPlaylist {
    pub title: String,
    pub items: Vec<MediaItem>,
    pub created_at: Option<f64>,
    pub updated_at: Option<f64>,
    pub metadata: Map<String, Value>,
}

impl UserPlaylist {
    pub fn new(title: impl Into<String>, timestamp: f64) -> Self {
        Self {
            title: title.into(),
            items: Vec::new(),
            created_at: timestamp.is_finite().then_some(timestamp),
            updated_at: timestamp.is_finite().then_some(timestamp),
            metadata: Map::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum UserPlaylistFileError {
    #[error(transparent)]
    Json(#[from] JsonFileError),
    #[error("playlist {index} is not a compatible playlist object")]
    InvalidPlaylist { index: usize },
    #[error("playlist {playlist_index} item {item_index} is not a compatible media item")]
    InvalidItem {
        playlist_index: usize,
        item_index: usize,
    },
}

#[derive(Clone, Debug)]
pub struct UserPlaylistFile {
    path: PathBuf,
}

impl UserPlaylistFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads a Python-compatible playlist array without dropping unknown data.
    /// A missing file represents an empty playlist collection.
    ///
    /// # Errors
    ///
    /// Returns an error when the JSON shape or a nested media item is invalid.
    pub fn load(&self) -> Result<Vec<UserPlaylist>, UserPlaylistFileError> {
        match read_json::<Value>(&self.path) {
            Ok(Value::Array(values)) => values
                .iter()
                .enumerate()
                .map(|(index, value)| playlist_from_python_value(value, index))
                .collect(),
            Ok(_) => Err(UserPlaylistFileError::InvalidPlaylist { index: 0 }),
            Err(JsonFileError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                Ok(Vec::new())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Atomically writes the full playlist collection in the Python shape.
    ///
    /// # Errors
    ///
    /// Returns an error when the destination cannot be replaced atomically.
    pub fn save(&self, playlists: &[UserPlaylist]) -> Result<(), UserPlaylistFileError> {
        let values: Vec<_> = playlists.iter().map(playlist_to_python_value).collect();
        write_json_atomic(&self.path, &values)?;
        Ok(())
    }
}

fn playlist_from_python_value(
    value: &Value,
    playlist_index: usize,
) -> Result<UserPlaylist, UserPlaylistFileError> {
    let object = value
        .as_object()
        .ok_or(UserPlaylistFileError::InvalidPlaylist {
            index: playlist_index,
        })?;
    let title = python_display_text(object.get("title"));
    let item_values = match object.get("items") {
        None | Some(Value::Null) => &[][..],
        Some(Value::Array(values)) => values,
        Some(_) => {
            return Err(UserPlaylistFileError::InvalidPlaylist {
                index: playlist_index,
            });
        }
    };
    let items = item_values
        .iter()
        .enumerate()
        .map(|(item_index, item)| {
            media_item_from_python_value(item).ok_or(UserPlaylistFileError::InvalidItem {
                playlist_index,
                item_index,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let created_at = finite_number(object.get("created_at"));
    let updated_at = finite_number(object.get("updated_at"));
    let mut metadata = object.clone();
    metadata.remove("items");
    if object.get("title").is_some_and(Value::is_string) {
        metadata.remove("title");
    }
    if created_at.is_some() {
        metadata.remove("created_at");
    }
    if updated_at.is_some() {
        metadata.remove("updated_at");
    }
    Ok(UserPlaylist {
        title,
        items,
        created_at,
        updated_at,
        metadata,
    })
}

fn playlist_to_python_value(playlist: &UserPlaylist) -> Value {
    let mut object = playlist.metadata.clone();
    if !object.contains_key("title") {
        object.insert("title".to_owned(), Value::String(playlist.title.clone()));
    }
    object.insert(
        "items".to_owned(),
        Value::Array(
            playlist
                .items
                .iter()
                .map(media_item_to_python_value)
                .collect(),
        ),
    );
    if !object.contains_key("created_at")
        && let Some(value) = playlist.created_at.and_then(serde_json::Number::from_f64)
    {
        object.insert("created_at".to_owned(), Value::Number(value));
    }
    if !object.contains_key("updated_at")
        && let Some(value) = playlist.updated_at.and_then(serde_json::Number::from_f64)
    {
        object.insert("updated_at".to_owned(), Value::Number(value));
    }
    Value::Object(object)
}

fn finite_number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

fn python_display_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Bool(true)) => "True".to_owned(),
        Some(Value::Bool(false)) => "False".to_owned(),
        Some(Value::Number(value)) => value.to_string(),
        None | Some(Value::Null) => String::new(),
        Some(value) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::UserPlaylistFile;

    #[test]
    fn python_playlist_round_trips_unknown_playlist_and_item_metadata() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("playlists.json");
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!([{
                "title": "Mix",
                "created_at": 10.0,
                "updated_at": 20.0,
                "future_playlist_field": {"kept": true},
                "items": [{
                    "title": "Track",
                    "kind": "video",
                    "url": "https://www.youtube.com/watch?v=abcdefghijk",
                    "added_at": 15.0,
                    "future_item_field": [1, 2, 3]
                }]
            }]))
            .expect("JSON"),
        )
        .expect("fixture");
        let file = UserPlaylistFile::new(&path);
        let playlists = file.load().expect("load");
        assert_eq!(playlists[0].title, "Mix");
        file.save(&playlists).expect("save");
        let restored: Value = serde_json::from_slice(&fs::read(path).expect("read")).expect("JSON");
        assert_eq!(restored[0]["future_playlist_field"], json!({"kept": true}));
        assert_eq!(restored[0]["items"][0]["added_at"], 15.0);
        assert_eq!(
            restored[0]["items"][0]["future_item_field"],
            json!([1, 2, 3])
        );
    }

    #[test]
    fn malformed_nested_item_blocks_loading_without_rewriting() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("playlists.json");
        let bytes = br#"[{"title":"Mix","items":[{"title":"missing location"}]}]"#;
        fs::write(&path, bytes).expect("fixture");
        let file = UserPlaylistFile::new(&path);
        assert!(file.load().is_err());
        assert_eq!(fs::read(path).expect("preserved"), bytes);
    }

    #[test]
    fn unusual_known_fields_survive_a_typed_round_trip() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("playlists.json");
        let original = json!([{
            "title": 42,
            "created_at": null,
            "updated_at": "legacy-clock",
            "items": [{
                "title": "Track",
                "channel": "",
                "kind": "video",
                "url": "https://www.youtube.com/watch?v=abcdefghijk"
            }]
        }]);
        fs::write(
            &path,
            serde_json::to_vec_pretty(&original).expect("fixture JSON"),
        )
        .expect("fixture");

        let file = UserPlaylistFile::new(&path);
        let playlists = file.load().expect("load");
        assert_eq!(playlists[0].title, "42");
        file.save(&playlists).expect("save");
        let restored: Value = serde_json::from_slice(&fs::read(path).expect("read")).expect("JSON");
        assert_eq!(restored, original);
    }

    #[test]
    fn missing_file_is_an_empty_collection() {
        let root = tempdir().expect("temporary directory");
        assert!(
            UserPlaylistFile::new(root.path().join("missing.json"))
                .load()
                .expect("missing file")
                .is_empty()
        );
    }
}
