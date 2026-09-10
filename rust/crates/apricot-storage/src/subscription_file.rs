//! Python-compatible durable `YouTube` subscriptions.

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
pub struct Subscription {
    pub title: String,
    pub url: String,
    pub category: String,
    pub latest_urls: Vec<String>,
    pub last_checked: Option<f64>,
    pub last_new_count: usize,
    pub last_new_items: Vec<MediaItem>,
    pub created_at: Option<f64>,
    pub last_error: String,
    pub metadata: Map<String, Value>,
}

impl Subscription {
    pub fn new(title: impl Into<String>, url: impl Into<String>, timestamp: f64) -> Self {
        Self {
            title: title.into(),
            url: url.into(),
            category: String::new(),
            latest_urls: Vec::new(),
            last_checked: Some(0.0),
            last_new_count: 0,
            last_new_items: Vec::new(),
            created_at: Some(timestamp).filter(|value| value.is_finite() && *value >= 0.0),
            last_error: String::new(),
            metadata: Map::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum SubscriptionFileError {
    #[error(transparent)]
    Json(#[from] JsonFileError),
    #[error("subscription data is not a JSON array")]
    InvalidRoot,
    #[error("subscription {index} is not a JSON object")]
    InvalidEntry { index: usize },
    #[error("subscription {subscription_index} new item {item_index} is invalid")]
    InvalidNewItem {
        subscription_index: usize,
        item_index: usize,
    },
}

#[derive(Clone, Debug)]
pub struct SubscriptionFile {
    path: PathBuf,
}

impl SubscriptionFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the complete Python subscription list while retaining unknown
    /// root and nested media fields.
    ///
    /// # Errors
    ///
    /// Returns an error when an existing file has an incompatible shape.
    pub fn load(&self) -> Result<Vec<Subscription>, SubscriptionFileError> {
        let value = match read_json::<Value>(&self.path) {
            Ok(value) => value,
            Err(JsonFileError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(Vec::new());
            }
            Err(error) => return Err(error.into()),
        };
        let Value::Array(values) = value else {
            return Err(SubscriptionFileError::InvalidRoot);
        };
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| parse_subscription(value, index))
            .collect()
    }

    /// Atomically writes the full collection in the shape consumed by Python.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or replacement fails.
    pub fn save(&self, subscriptions: &[Subscription]) -> Result<(), SubscriptionFileError> {
        let values: Vec<_> = subscriptions.iter().map(subscription_value).collect();
        write_json_atomic(&self.path, &values)?;
        Ok(())
    }
}

fn parse_subscription(
    value: Value,
    subscription_index: usize,
) -> Result<Subscription, SubscriptionFileError> {
    let Value::Object(mut object) = value else {
        return Err(SubscriptionFileError::InvalidEntry {
            index: subscription_index,
        });
    };
    let title = take_text(&mut object, "title").unwrap_or_default();
    let url = take_text(&mut object, "url").unwrap_or_default();
    let category = take_text(&mut object, "category").unwrap_or_default();
    let latest_urls = take_text_array(&mut object, "latest_urls").unwrap_or_default();
    let last_checked = take_nonnegative_number(&mut object, "last_checked");
    let last_new_count = take_nonnegative_usize(&mut object, "last_new_count").unwrap_or_default();
    let created_at = take_nonnegative_number(&mut object, "created_at");
    let last_error = take_text(&mut object, "last_error").unwrap_or_default();
    let last_new_items = match object.remove("last_new_items") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(values)) => values
            .iter()
            .enumerate()
            .map(|(item_index, item)| {
                media_item_from_python_value(item).ok_or(SubscriptionFileError::InvalidNewItem {
                    subscription_index,
                    item_index,
                })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(value) => {
            object.insert("last_new_items".to_owned(), value);
            Vec::new()
        }
    };
    Ok(Subscription {
        title,
        url,
        category,
        latest_urls,
        last_checked,
        last_new_count,
        last_new_items,
        created_at,
        last_error,
        metadata: object,
    })
}

fn subscription_value(subscription: &Subscription) -> Value {
    let mut object = subscription.metadata.clone();
    object.insert(
        "title".to_owned(),
        Value::String(subscription.title.clone()),
    );
    object.insert("url".to_owned(), Value::String(subscription.url.clone()));
    object.insert(
        "latest_urls".to_owned(),
        Value::Array(
            subscription
                .latest_urls
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        ),
    );
    object.insert(
        "last_checked".to_owned(),
        subscription
            .last_checked
            .map_or(Value::from(0.0), Value::from),
    );
    object.insert(
        "last_new_count".to_owned(),
        Value::from(u64::try_from(subscription.last_new_count).unwrap_or(u64::MAX)),
    );
    object.insert(
        "last_new_items".to_owned(),
        Value::Array(
            subscription
                .last_new_items
                .iter()
                .map(media_item_to_python_value)
                .collect(),
        ),
    );
    if let Some(created_at) = subscription.created_at {
        object.insert("created_at".to_owned(), Value::from(created_at));
    }
    if subscription.category.is_empty() {
        object.remove("category");
    } else {
        object.insert(
            "category".to_owned(),
            Value::String(subscription.category.clone()),
        );
    }
    if subscription.last_error.is_empty() {
        object.remove("last_error");
    } else {
        object.insert(
            "last_error".to_owned(),
            Value::String(subscription.last_error.clone()),
        );
    }
    Value::Object(object)
}

fn take_text(object: &mut Map<String, Value>, key: &str) -> Option<String> {
    match object.get(key) {
        Some(Value::String(value)) => {
            let value = value.clone();
            object.remove(key);
            Some(value)
        }
        _ => None,
    }
}

fn take_text_array(object: &mut Map<String, Value>, key: &str) -> Option<Vec<String>> {
    let values = object.get(key)?.as_array()?;
    let result = values
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .map(str::to_owned)
        .collect();
    object.remove(key);
    Some(result)
}

fn take_nonnegative_number(object: &mut Map<String, Value>, key: &str) -> Option<f64> {
    let value = object
        .get(key)?
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)?;
    object.remove(key);
    Some(value)
}

fn take_nonnegative_usize(object: &mut Map<String, Value>, key: &str) -> Option<usize> {
    let value = object.get(key)?.as_u64()?;
    let value = usize::try_from(value).ok()?;
    object.remove(key);
    Some(value)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::SubscriptionFile;

    #[test]
    fn python_subscription_round_trips_unknown_root_and_media_fields() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("subscriptions.json");
        fs::write(
            &path,
            serde_json::to_vec_pretty(&json!([{
                "title": "Channel",
                "url": "https://www.youtube.com/channel/UC123",
                "category": "Music",
                "latest_urls": ["https://www.youtube.com/watch?v=abcdefghijk"],
                "last_checked": 42.5,
                "last_new_count": 1,
                "created_at": 10.0,
                "future_root": {"kept": true},
                "last_new_items": [{
                    "title": "Track",
                    "kind": "video",
                    "url": "https://www.youtube.com/watch?v=abcdefghijk",
                    "future_item": [1, 2, 3]
                }]
            }]))
            .expect("JSON"),
        )
        .expect("fixture");
        let file = SubscriptionFile::new(&path);
        let subscriptions = file.load().expect("load");
        assert_eq!(subscriptions[0].category, "Music");
        assert_eq!(subscriptions[0].last_new_items.len(), 1);
        file.save(&subscriptions).expect("save");
        let restored: Value = serde_json::from_slice(&fs::read(path).expect("read")).expect("JSON");
        assert_eq!(restored[0]["future_root"], json!({"kept": true}));
        assert_eq!(
            restored[0]["last_new_items"][0]["future_item"],
            json!([1, 2, 3])
        );
    }

    #[test]
    fn malformed_new_item_blocks_loading_without_rewriting() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("subscriptions.json");
        let bytes = br#"[{"title":"Channel","url":"https://www.youtube.com/@channel","last_new_items":[{"title":"missing URL"}]}]"#;
        fs::write(&path, bytes).expect("fixture");
        assert!(SubscriptionFile::new(&path).load().is_err());
        assert_eq!(fs::read(path).expect("preserved"), bytes);
    }

    #[test]
    fn missing_file_is_an_empty_collection() {
        let root = tempdir().expect("temporary directory");
        assert!(
            SubscriptionFile::new(root.path().join("missing.json"))
                .load()
                .expect("missing is empty")
                .is_empty()
        );
    }
}
