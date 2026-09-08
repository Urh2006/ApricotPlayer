//! Python-compatible durable notification-center entries.

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
pub struct AppNotification {
    pub kind: String,
    pub title: String,
    pub message: String,
    pub item: Option<MediaItem>,
    pub timestamp: f64,
    raw_item: Option<Value>,
    extra: Map<String, Value>,
}

impl AppNotification {
    pub fn new(
        kind: impl Into<String>,
        title: impl Into<String>,
        message: impl Into<String>,
        item: Option<MediaItem>,
        timestamp: f64,
    ) -> Self {
        Self {
            kind: kind.into(),
            title: title.into(),
            message: message.into(),
            item,
            timestamp: if timestamp.is_finite() {
                timestamp.max(0.0)
            } else {
                0.0
            },
            raw_item: None,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum NotificationFileError {
    #[error(transparent)]
    Json(#[from] JsonFileError),
    #[error("notification data is not a JSON array")]
    InvalidRoot,
    #[error("notification {index} is not a JSON object")]
    InvalidEntry { index: usize },
}

#[derive(Clone, Debug)]
pub struct NotificationFile {
    path: PathBuf,
}

impl NotificationFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the complete Python notification list without silently dropping
    /// unknown fields or informational entries that have no playable item.
    ///
    /// # Errors
    ///
    /// Returns an error when existing data is malformed.
    pub fn load(&self) -> Result<Vec<AppNotification>, NotificationFileError> {
        let value = match read_json::<Value>(&self.path) {
            Ok(value) => value,
            Err(JsonFileError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error.into()),
        };
        let Value::Array(values) = value else {
            return Err(NotificationFileError::InvalidRoot);
        };
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| parse_notification(value, index))
            .collect()
    }

    /// Atomically writes the complete list in the shape consumed by Python.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or replacement fails.
    pub fn save(&self, notifications: &[AppNotification]) -> Result<(), NotificationFileError> {
        let values: Vec<_> = notifications.iter().map(notification_value).collect();
        write_json_atomic(&self.path, &values)?;
        Ok(())
    }
}

fn parse_notification(
    value: Value,
    index: usize,
) -> Result<AppNotification, NotificationFileError> {
    let Value::Object(mut object) = value else {
        return Err(NotificationFileError::InvalidEntry { index });
    };
    let raw_item = object.remove("item");
    let item = raw_item.as_ref().and_then(media_item_from_python_value);
    let kind = take_text(&mut object, "kind").unwrap_or_else(|| "info".to_owned());
    let title = take_text(&mut object, "title").unwrap_or_default();
    let message = take_text(&mut object, "message").unwrap_or_default();
    let timestamp = object
        .remove("timestamp")
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or_default();
    Ok(AppNotification {
        kind,
        title,
        message,
        item,
        raw_item,
        timestamp,
        extra: object,
    })
}

fn notification_value(notification: &AppNotification) -> Value {
    let mut object = notification.extra.clone();
    object.insert("kind".to_owned(), Value::String(notification.kind.clone()));
    object.insert(
        "title".to_owned(),
        Value::String(notification.title.clone()),
    );
    object.insert(
        "message".to_owned(),
        Value::String(notification.message.clone()),
    );
    object.insert(
        "item".to_owned(),
        notification.item.as_ref().map_or_else(
            || {
                notification
                    .raw_item
                    .clone()
                    .unwrap_or_else(|| Value::Object(Map::new()))
            },
            media_item_to_python_value,
        ),
    );
    object.insert("timestamp".to_owned(), Value::from(notification.timestamp));
    Value::Object(object)
}

fn take_text(object: &mut Map<String, Value>, key: &str) -> Option<String> {
    object
        .remove(key)
        .and_then(|value| value.as_str().map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::{AppNotification, NotificationFile};

    #[test]
    fn python_notifications_round_trip_unknown_and_informational_entries() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("notifications.json");
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!([
                {
                    "kind": "subscription",
                    "title": "New YouTube videos",
                    "message": "Channel: new video Track",
                    "item": {
                        "id": "abc",
                        "title": "Track",
                        "kind": "video",
                        "url": "https://www.youtube.com/watch?v=abc",
                        "future_item": true
                    },
                    "timestamp": 42.5,
                    "future_root": {"kept": true}
                },
                {"kind": "info", "title": "Notice", "message": "Done", "item": {}}
            ]))
            .expect("JSON"),
        )
        .expect("fixture");
        let file = NotificationFile::new(&path);
        let entries = file.load().expect("load");
        assert_eq!(entries.len(), 2);
        assert!(entries[0].item.is_some());
        assert!(entries[1].item.is_none());
        file.save(&entries).expect("save");
        let restored: Value = serde_json::from_slice(&fs::read(path).expect("read")).expect("JSON");
        assert_eq!(restored[0]["future_root"], json!({"kept": true}));
        assert_eq!(restored[0]["item"]["future_item"], true);
        assert_eq!(restored[1]["item"], json!({}));
    }

    #[test]
    fn malformed_existing_file_is_preserved() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("notifications.json");
        fs::write(&path, b"broken").expect("fixture");
        assert!(NotificationFile::new(&path).load().is_err());
        assert_eq!(fs::read(path).expect("preserved"), b"broken");
    }

    #[test]
    fn rust_notification_uses_python_defaults() {
        let notification = AppNotification::new("info", "Title", "Message", None, f64::NAN);
        assert!(notification.timestamp.abs() < f64::EPSILON);
        assert_eq!(notification.kind, "info");
    }
}
