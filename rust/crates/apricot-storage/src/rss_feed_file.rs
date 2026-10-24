//! Python-compatible durable podcast and RSS feed archive.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use apricot_core::MediaItem;
use serde_json::{Map, Value};
use thiserror::Error;

use crate::{
    JsonFileError, media_item_from_python_value, media_item_to_python_value, read_json,
    write_json_atomic,
};

pub const MAX_RSS_LIBRARY_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct RssFeed {
    pub title: String,
    pub url: String,
    pub site_url: String,
    pub items: Vec<MediaItem>,
    pub items_complete: Option<bool>,
    pub category: String,
    pub speed_preset: Option<f64>,
    pub last_checked: Option<f64>,
    pub created_at: Option<f64>,
    pub last_error: String,
    pub metadata: Map<String, Value>,
}

impl RssFeed {
    pub fn new(
        title: impl Into<String>,
        url: impl Into<String>,
        site_url: impl Into<String>,
        items: Vec<MediaItem>,
        timestamp: f64,
    ) -> Self {
        let timestamp = Some(timestamp).filter(|value| value.is_finite() && *value >= 0.0);
        Self {
            title: title.into(),
            url: url.into(),
            site_url: site_url.into(),
            items,
            items_complete: Some(true),
            category: String::new(),
            speed_preset: None,
            last_checked: timestamp,
            created_at: timestamp,
            last_error: String::new(),
            metadata: Map::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum RssFeedFileError {
    #[error(transparent)]
    Json(#[from] JsonFileError),
    #[error("RSS archive is larger than the allowed {limit} bytes")]
    TooLarge { limit: u64 },
    #[error("RSS feed data is not a JSON array")]
    InvalidRoot,
    #[error("RSS feed {index} is not a JSON object")]
    InvalidFeed { index: usize },
    #[error("RSS feed {feed_index} episode {item_index} is invalid")]
    InvalidItem {
        feed_index: usize,
        item_index: usize,
    },
}

#[derive(Clone, Debug)]
pub struct RssFeedFile {
    path: PathBuf,
}

impl RssFeedFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the complete Python feed archive while retaining unknown feed and
    /// episode fields.
    ///
    /// # Errors
    ///
    /// Returns an error when an existing archive is too large or malformed.
    pub fn load(&self) -> Result<Vec<RssFeed>, RssFeedFileError> {
        match fs::metadata(&self.path) {
            Ok(metadata) if metadata.len() > MAX_RSS_LIBRARY_BYTES => {
                return Err(RssFeedFileError::TooLarge {
                    limit: MAX_RSS_LIBRARY_BYTES,
                });
            }
            Ok(_) => {}
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(JsonFileError::Read {
                    path: self.path.clone(),
                    source,
                }
                .into());
            }
        }
        let value = read_json::<Value>(&self.path)?;
        let Value::Array(values) = value else {
            return Err(RssFeedFileError::InvalidRoot);
        };
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| parse_feed(value, index))
            .collect()
    }

    /// Atomically writes the full archive in the shape consumed by Python.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or replacement fails.
    pub fn save(&self, feeds: &[RssFeed]) -> Result<(), RssFeedFileError> {
        let values: Vec<_> = feeds.iter().map(feed_value).collect();
        write_json_atomic(&self.path, &values)?;
        Ok(())
    }
}

fn parse_feed(value: Value, feed_index: usize) -> Result<RssFeed, RssFeedFileError> {
    let Value::Object(mut object) = value else {
        return Err(RssFeedFileError::InvalidFeed { index: feed_index });
    };
    let title = take_text(&mut object, "title").unwrap_or_default();
    let url = take_text(&mut object, "url").unwrap_or_default();
    let site_url = take_text(&mut object, "site_url").unwrap_or_default();
    let category = take_text(&mut object, "category").unwrap_or_default();
    let items_complete = take_bool(&mut object, "items_complete");
    let speed_preset = take_number(&mut object, "speed_preset")
        .filter(|value| (0.25..=4.0).contains(value))
        .map(|value| (value * 100.0).round() / 100.0);
    let last_checked = take_nonnegative_number(&mut object, "last_checked");
    let created_at = take_nonnegative_number(&mut object, "created_at");
    let last_error = take_text(&mut object, "last_error").unwrap_or_default();
    let items = match object.remove("items") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(values)) => values
            .iter()
            .enumerate()
            .map(|(item_index, item)| {
                media_item_from_python_value(item).ok_or(RssFeedFileError::InvalidItem {
                    feed_index,
                    item_index,
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(value) => {
            object.insert("items".to_owned(), value);
            Vec::new()
        }
    };
    Ok(RssFeed {
        title,
        url,
        site_url,
        items,
        items_complete,
        category,
        speed_preset,
        last_checked,
        created_at,
        last_error,
        metadata: object,
    })
}

fn feed_value(feed: &RssFeed) -> Value {
    let mut object = feed.metadata.clone();
    object.insert("title".to_owned(), Value::String(feed.title.clone()));
    object.insert("url".to_owned(), Value::String(feed.url.clone()));
    object.insert("site_url".to_owned(), Value::String(feed.site_url.clone()));
    object.insert(
        "items".to_owned(),
        Value::Array(feed.items.iter().map(media_item_to_python_value).collect()),
    );
    set_optional(
        &mut object,
        "items_complete",
        feed.items_complete.map(Value::Bool),
    );
    set_optional(
        &mut object,
        "last_checked",
        feed.last_checked.map(Value::from),
    );
    set_optional(&mut object, "created_at", feed.created_at.map(Value::from));
    set_optional(
        &mut object,
        "speed_preset",
        feed.speed_preset.map(Value::from),
    );
    set_optional_text(&mut object, "category", &feed.category);
    set_optional_text(&mut object, "last_error", &feed.last_error);
    Value::Object(object)
}

fn set_optional(object: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        object.insert(key.to_owned(), value);
    } else {
        object.remove(key);
    }
}

fn set_optional_text(object: &mut Map<String, Value>, key: &str, value: &str) {
    if value.is_empty() {
        object.remove(key);
    } else {
        object.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn take_text(object: &mut Map<String, Value>, key: &str) -> Option<String> {
    let value = object.get(key)?.as_str()?.to_owned();
    object.remove(key);
    Some(value)
}

fn take_bool(object: &mut Map<String, Value>, key: &str) -> Option<bool> {
    let value = object.get(key)?.as_bool()?;
    object.remove(key);
    Some(value)
}

fn take_number(object: &mut Map<String, Value>, key: &str) -> Option<f64> {
    let value = object
        .get(key)?
        .as_f64()
        .filter(|value| value.is_finite())?;
    object.remove(key);
    Some(value)
}

fn take_nonnegative_number(object: &mut Map<String, Value>, key: &str) -> Option<f64> {
    take_number(object, key).filter(|value| *value >= 0.0)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::{MAX_RSS_LIBRARY_BYTES, RssFeedFile, RssFeedFileError};

    #[test]
    fn python_feed_round_trips_episode_state_chapters_and_unknown_fields() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("rss_feeds.json");
        let source = json!([{
            "title": "Archive",
            "url": "https://podcast.example/feed.xml",
            "site_url": "https://podcast.example",
            "items_complete": true,
            "last_checked": 12.5,
            "created_at": 3.0,
            "future_feed_field": {"kept": true},
            "items": [{
                "title": "Episode",
                "url": "https://media.example/episode.mp3",
                "webpage_url": "https://podcast.example/episode",
                "kind": "rss_item",
                "channel": "Archive",
                "played": true,
                "play_count": 2,
                "chapters": [{"title": "Intro", "start_time": 0.0}],
                "future_episode_field": [1, 2, 3]
            }]
        }]);
        fs::write(&path, serde_json::to_vec_pretty(&source).expect("fixture")).expect("write");
        let file = RssFeedFile::new(&path);
        let feeds = file.load().expect("load");
        assert_eq!(feeds[0].items.len(), 1);
        assert_eq!(feeds[0].items[0].metadata["played"], true);
        file.save(&feeds).expect("save");
        let restored: Value = serde_json::from_slice(&fs::read(path).expect("read")).expect("JSON");
        assert_eq!(restored, source);
    }

    #[test]
    fn malformed_episode_blocks_loading_without_rewriting() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("rss_feeds.json");
        let bytes = br#"[{"title":"Feed","items":[{"title":"No location"}]}]"#;
        fs::write(&path, bytes).expect("fixture");
        assert!(matches!(
            RssFeedFile::new(&path).load(),
            Err(RssFeedFileError::InvalidItem { .. })
        ));
        assert_eq!(fs::read(path).expect("preserved"), bytes);
    }

    #[test]
    fn oversized_archive_is_rejected_before_json_parsing() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("rss_feeds.json");
        let file = fs::File::create(&path).expect("create");
        file.set_len(MAX_RSS_LIBRARY_BYTES + 1).expect("resize");
        assert!(matches!(
            RssFeedFile::new(path).load(),
            Err(RssFeedFileError::TooLarge { .. })
        ));
    }

    #[test]
    fn missing_file_is_an_empty_collection() {
        let root = tempdir().expect("temporary directory");
        assert!(
            RssFeedFile::new(root.path().join("missing.json"))
                .load()
                .expect("missing is empty")
                .is_empty()
        );
    }
}
