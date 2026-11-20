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

impl Bookmark {
    /// The exclusive end of a saved excerpt. Point bookmarks have no end.
    #[must_use]
    pub fn end_position(&self) -> Option<f64> {
        flexible_number(self.metadata.get("end_position")).filter(|end| *end > self.position)
    }
}

#[derive(Debug, Error)]
pub enum BookmarkFileError {
    #[error(transparent)]
    Json(#[from] JsonFileError),
    #[error("bookmarks data is not a JSON array")]
    InvalidRoot,
    #[error("bookmark entry {0} is invalid")]
    InvalidEntry(usize),
    #[error("bookmark import is too large (maximum 8 MiB)")]
    ImportTooLarge,
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

    /// Reads a user-selected backup strictly. A damaged entry must not be
    /// silently dropped while replacing the user's collection.
    ///
    /// # Errors
    ///
    /// Returns an error when the file cannot be read, is larger than 8 MiB,
    /// is not a JSON array or contains an invalid entry.
    pub fn load_import(&self) -> Result<Vec<Bookmark>, BookmarkFileError> {
        use std::io::Read;
        const LIMIT: u64 = 8 * 1024 * 1024;
        let file = std::fs::File::open(&self.path).map_err(|source| JsonFileError::Read {
            path: self.path.clone(),
            source,
        })?;
        let mut bytes = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| JsonFileError::Read {
                path: self.path.clone(),
                source,
            })?;
        if bytes.len() as u64 > LIMIT {
            return Err(BookmarkFileError::ImportTooLarge);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|source| JsonFileError::Parse {
                path: self.path.clone(),
                source,
            })?;
        let entries = value.as_array().ok_or(BookmarkFileError::InvalidRoot)?;
        entries
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let bookmark = bookmark_from_python_value(value)
                    .ok_or(BookmarkFileError::InvalidEntry(index + 1))?;
                if value.get("end_position").is_some() && bookmark.end_position().is_none() {
                    return Err(BookmarkFileError::InvalidEntry(index + 1));
                }
                // A damaged position would be read as the start; an import
                // refuses it instead (review 4, A8).
                if value.get("position").is_some_and(|position| {
                    !position.is_null() && flexible_number(Some(position)).is_none()
                }) {
                    return Err(BookmarkFileError::InvalidEntry(index + 1));
                }
                Ok(bookmark)
            })
            .collect()
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

    /// Loads the bookmarks and writes them again when a bookmark of a
    /// private IPTV channel still holds its address or login. Only for this
    /// version's own file: a fallback profile is read, never written
    /// (review 5, A1 and A6).
    ///
    /// # Errors
    ///
    /// As [`Self::load`], or when the cleaned bookmarks cannot be written.
    pub fn load_repairing(&self) -> Result<Vec<Bookmark>, BookmarkFileError> {
        let bookmarks = self.load()?;
        let stale = match read_json::<Value>(&self.path) {
            Ok(Value::Array(values)) => values.iter().any(|value| {
                bookmark_from_python_value(value).is_some_and(|bookmark| {
                    is_private(&bookmark.media) && *value != bookmark_to_python_value(&bookmark)
                })
            }),
            _ => false,
        };
        if stale {
            self.save(&bookmarks)?;
        }
        Ok(bookmarks)
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
    // A private IPTV channel is known by its source and channel, never by
    // its address with the login (review 5, A1).
    if let Some(reference) = item.private_reference() {
        return Some(reference);
    }
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
    let private = is_private(&media);
    let media_key = {
        let explicit = text(object, "media_key");
        if explicit.is_empty() || private {
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
    if private {
        strip_private_fields(&mut metadata);
    }
    Some(Bookmark {
        id: text(object, "id"),
        name: text(object, "name"),
        position: if object.contains_key("end_position") {
            round_millisecond(position)
        } else {
            round_tenth(position)
        },
        media_key,
        media_title,
        media,
        created_at: flexible_number(object.get("created_at")),
        updated_at: flexible_number(object.get("updated_at")),
        metadata,
    })
}

/// A private IPTV channel, saved or playing.
fn is_private(media: &MediaItem) -> bool {
    media.is_saved_iptv_reference() || media.has_private_address()
}

/// The address fields of an older bookmark of a private IPTV channel, kept
/// next to its media; they may hold the login.
fn strip_private_fields(object: &mut serde_json::Map<String, Value>) {
    for key in apricot_core::PRIVATE_ADDRESS_KEYS
        .iter()
        .chain(&["path", "local_path", "watch_url"])
    {
        object.remove(*key);
    }
}

fn bookmark_to_python_value(bookmark: &Bookmark) -> Value {
    let mut object = bookmark.metadata.clone();
    let private = is_private(&bookmark.media);
    if private {
        strip_private_fields(&mut object);
    }
    object.insert("id".to_owned(), Value::String(bookmark.id.clone()));
    object.insert("name".to_owned(), Value::String(bookmark.name.clone()));
    object.insert(
        "position".to_owned(),
        Value::from(if bookmark.end_position().is_some() {
            round_millisecond(bookmark.position)
        } else {
            round_tenth(bookmark.position)
        }),
    );
    let media_key = if private {
        bookmark_media_key(&bookmark.media).unwrap_or_default()
    } else {
        bookmark.media_key.clone()
    };
    object.insert("media_key".to_owned(), Value::String(media_key));
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

fn round_millisecond(value: f64) -> f64 {
    (value.max(0.0) * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    /// Review 5, A1: a bookmark of a private IPTV channel is keyed and
    /// written by its source and channel; an older bookmark that kept the
    /// address with the login is cleaned by this version's own load only.
    #[test]
    fn private_iptv_bookmarks_keep_no_login() {
        let folder = tempdir().expect("folder");
        let path = folder.path().join("bookmarks.json");
        let address = "http://tv.example.test/live/USER/FAKE_SECRET/7.ts";
        let media = json!({"title": "News", "kind": "live_stream", "url": address,
            "apricot_private_address": true, "iptv_source_id": "s",
            "iptv_channel_id": "7", "stalker_token": "FAKE_TOKEN"});
        let playing = apricot_core::MediaItem {
            id: apricot_core::MediaId(address.into()),
            source: apricot_core::MediaSource::Direct,
            kind: apricot_core::MediaKind::LiveStream,
            title: "News".into(),
            url: Some(address.parse().expect("url")),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: media
                .as_object()
                .expect("object")
                .clone()
                .into_iter()
                .collect(),
        };
        let key = super::bookmark_media_key(&playing).expect("key");
        assert_eq!(key, "apricot-iptv://saved/s/7");
        fs::write(
            &path,
            json!([{"id": "b1", "name": "Start", "position": 4.0, "media_key": address,
                "url": address, "media": media}])
            .to_string(),
        )
        .expect("write");
        let original = fs::read(&path).expect("read");
        let file = super::BookmarkFile::new(&path);
        let loaded = file.load().expect("load");
        assert_eq!(
            fs::read(&path).expect("read"),
            original,
            "plain read writes"
        );
        assert_eq!(loaded[0].media_key, key);
        file.load_repairing().expect("repair");
        let text = fs::read_to_string(&path).expect("read");
        for secret in ["FAKE_SECRET", "FAKE_TOKEN", "USER"] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
        file.save(&loaded).expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(!text.contains("FAKE_SECRET"), "{text}");
        assert_eq!(file.load().expect("load")[0].media_key, key);
    }

    /// Review 4, A8: an import with a damaged position is refused with the
    /// number of the entry; a position written as text still imports.
    #[test]
    fn import_refuses_a_damaged_position() {
        let folder = tempdir().expect("folder");
        let path = folder.path().join("import.json");
        let entry = |position: Value| json!({"name": "Start", "position": position, "media": {"title": "Song", "kind": "local_file", "url": "C:/Music/song.mp3"}});
        for damaged in [json!("damaged"), json!({"seconds": 4})] {
            fs::write(
                &path,
                json!([entry(json!(1.5)), entry(damaged)]).to_string(),
            )
            .expect("write");
            assert!(matches!(
                super::BookmarkFile::new(&path).load_import(),
                Err(super::BookmarkFileError::InvalidEntry(2))
            ));
        }
        fs::write(&path, json!([entry(json!("12.5"))]).to_string()).expect("write");
        let imported = super::BookmarkFile::new(&path)
            .load_import()
            .expect("import");
        assert!((imported[0].position - 12.5).abs() < f64::EPSILON);
    }

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
