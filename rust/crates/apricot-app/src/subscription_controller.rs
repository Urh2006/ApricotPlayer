//! Transactional `YouTube` subscription state and refresh merging.

use std::{collections::HashSet, path::PathBuf};

use apricot_core::{MediaItem, MediaKind};
use apricot_storage::{Subscription, SubscriptionFile, SubscriptionFileError};
use thiserror::Error;
use url::Url;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionAddOutcome {
    Added(usize),
    AlreadyPresent,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubscriptionRemoveOutcome {
    Removed,
    NotFound,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubscriptionCheckResult {
    pub url: String,
    pub result: Result<Vec<MediaItem>, String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubscriptionCheckSummary {
    pub successes: usize,
    pub failures: usize,
    pub total_new: usize,
    pub new_items: Vec<(String, MediaItem)>,
}

#[derive(Debug, Error)]
pub enum SubscriptionControllerError {
    #[error("subscription changes are blocked because loading {path} failed: {message}")]
    SaveBlocked { path: PathBuf, message: String },
    #[error(transparent)]
    Storage(#[from] SubscriptionFileError),
}

#[derive(Debug, Default)]
pub struct SubscriptionController {
    subscriptions: Vec<Subscription>,
    category_filter: String,
    file: Option<SubscriptionFile>,
    load_error: Option<String>,
    save_blocked: bool,
}

impl SubscriptionController {
    pub fn load(current: SubscriptionFile, legacy: &SubscriptionFile) -> Self {
        if current.path().is_file() {
            return match current.load() {
                Ok(subscriptions) => Self::loaded(current, subscriptions),
                Err(error) => Self::blocked(current, error.to_string()),
            };
        }
        if legacy.path().is_file() {
            return match legacy.load() {
                Ok(subscriptions) => Self::loaded(current, subscriptions),
                Err(error) => Self {
                    subscriptions: Vec::new(),
                    category_filter: String::new(),
                    file: Some(current),
                    load_error: Some(error.to_string()),
                    save_blocked: false,
                },
            };
        }
        Self::loaded(current, Vec::new())
    }

    fn loaded(file: SubscriptionFile, mut subscriptions: Vec<Subscription>) -> Self {
        sort_subscriptions(&mut subscriptions);
        Self {
            subscriptions,
            category_filter: String::new(),
            file: Some(file),
            load_error: None,
            save_blocked: false,
        }
    }

    fn blocked(file: SubscriptionFile, message: String) -> Self {
        Self {
            subscriptions: Vec::new(),
            category_filter: String::new(),
            file: Some(file),
            load_error: Some(message),
            save_blocked: true,
        }
    }

    pub fn subscriptions(&self) -> &[Subscription] {
        &self.subscriptions
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub fn category_filter(&self) -> &str {
        &self.category_filter
    }

    pub fn set_category_filter(&mut self, category: &str) {
        self.category_filter = normalize_category(category);
    }

    pub fn categories(&self) -> Vec<String> {
        let mut categories = self
            .subscriptions
            .iter()
            .map(|subscription| normalize_category(&subscription.category))
            .filter(|category| !category.is_empty())
            .collect::<Vec<_>>();
        categories.sort_by_key(|category| category.to_lowercase());
        categories.dedup_by(|left, right| left.to_lowercase() == right.to_lowercase());
        categories
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        let filter = self.category_filter.to_lowercase();
        self.subscriptions
            .iter()
            .enumerate()
            .filter_map(|(index, subscription)| {
                (filter.is_empty()
                    || normalize_category(&subscription.category).to_lowercase() == filter)
                    .then_some(index)
            })
            .collect()
    }

    pub fn contains_item(&self, item: &MediaItem) -> bool {
        let Some((_, url, _)) = subscription_target(item) else {
            return false;
        };
        self.subscriptions.iter().any(|subscription| {
            canonical_channel_url(&subscription.url).as_deref() == Some(url.as_str())
        })
    }

    pub fn supports_item(item: &MediaItem) -> bool {
        subscription_target(item).is_some()
    }

    /// Adds a durable channel subscription derived from a channel or video row.
    ///
    /// # Errors
    ///
    /// Returns an error when the new collection cannot be persisted atomically.
    pub fn add_from_item(
        &mut self,
        item: &MediaItem,
        timestamp: f64,
    ) -> Result<SubscriptionAddOutcome, SubscriptionControllerError> {
        let Some((title, url, latest_url)) = subscription_target(item) else {
            return Ok(SubscriptionAddOutcome::Unsupported);
        };
        if self
            .subscriptions
            .iter()
            .any(|subscription| canonical_channel_url(&subscription.url).as_deref() == Some(&url))
        {
            return Ok(SubscriptionAddOutcome::AlreadyPresent);
        }
        let mut subscription = Subscription::new(title, url.clone(), timestamp);
        subscription.last_checked = Some(0.0);
        if let Some(latest_url) = latest_url {
            subscription.latest_urls.push(latest_url);
        }
        let mut candidate = self.subscriptions.clone();
        candidate.push(subscription);
        sort_subscriptions(&mut candidate);
        let index = candidate
            .iter()
            .position(|subscription| {
                canonical_channel_url(&subscription.url).as_deref() == Some(&url)
            })
            .unwrap_or_default();
        self.commit(candidate)?;
        Ok(SubscriptionAddOutcome::Added(index))
    }

    /// Removes the subscription represented by a channel or video row.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed collection cannot be persisted.
    pub fn remove_from_item(
        &mut self,
        item: &MediaItem,
    ) -> Result<SubscriptionRemoveOutcome, SubscriptionControllerError> {
        let Some((_, url, _)) = subscription_target(item) else {
            return Ok(SubscriptionRemoveOutcome::Unsupported);
        };
        let Some(index) = self.subscriptions.iter().position(|subscription| {
            canonical_channel_url(&subscription.url).as_deref() == Some(&url)
        }) else {
            return Ok(SubscriptionRemoveOutcome::NotFound);
        };
        self.remove(index)?;
        Ok(SubscriptionRemoveOutcome::Removed)
    }

    /// Removes one subscription by its unfiltered collection index.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed collection cannot be persisted.
    pub fn remove(
        &mut self,
        index: usize,
    ) -> Result<Option<Subscription>, SubscriptionControllerError> {
        self.ensure_writable()?;
        if index >= self.subscriptions.len() {
            return Ok(None);
        }
        let mut candidate = self.subscriptions.clone();
        let removed = candidate.remove(index);
        self.commit(candidate)?;
        Ok(Some(removed))
    }

    /// Assigns or clears the normalized category of one subscription.
    ///
    /// # Errors
    ///
    /// Returns an error when the changed collection cannot be persisted.
    pub fn set_category(
        &mut self,
        index: usize,
        category: &str,
    ) -> Result<bool, SubscriptionControllerError> {
        self.ensure_writable()?;
        let Some(subscription) = self.subscriptions.get(index) else {
            return Ok(false);
        };
        let category = normalize_category(category);
        if subscription.category == category {
            return Ok(false);
        }
        let mut candidate = self.subscriptions.clone();
        candidate[index].category = category;
        sort_subscriptions(&mut candidate);
        self.commit(candidate)?;
        Ok(true)
    }

    /// Applies one complete refresh batch with a single durable write.
    ///
    /// Results are matched by canonical channel URL, so sorting or a stale UI
    /// index cannot update the wrong subscription.
    ///
    /// # Errors
    ///
    /// Returns an error when the complete updated collection cannot be saved.
    pub fn apply_checks(
        &mut self,
        results: Vec<SubscriptionCheckResult>,
        timestamp: f64,
    ) -> Result<SubscriptionCheckSummary, SubscriptionControllerError> {
        self.ensure_writable()?;
        let mut candidate = self.subscriptions.clone();
        let mut summary = SubscriptionCheckSummary::default();
        let timestamp = if timestamp.is_finite() {
            timestamp.max(0.0)
        } else {
            0.0
        };
        for check in results {
            let Some(target_url) = canonical_channel_url(&check.url) else {
                continue;
            };
            let Some(subscription) = candidate.iter_mut().find(|subscription| {
                canonical_channel_url(&subscription.url).as_deref() == Some(target_url.as_str())
            }) else {
                continue;
            };
            subscription.last_checked = Some(timestamp);
            match check.result {
                Ok(entries) => {
                    summary.successes += 1;
                    let known: HashSet<_> = subscription.latest_urls.iter().cloned().collect();
                    let current_urls = entries
                        .iter()
                        .filter_map(|entry| entry.url.clone())
                        .map(|url| url.to_string())
                        .collect::<Vec<_>>();
                    let new_items = if known.is_empty() {
                        Vec::new()
                    } else {
                        entries
                            .into_iter()
                            .filter(|entry| {
                                entry
                                    .url
                                    .as_ref()
                                    .is_some_and(|url| !known.contains(url.as_str()))
                            })
                            .collect::<Vec<_>>()
                    };
                    summary.total_new += new_items.len();
                    summary.new_items.extend(
                        new_items
                            .iter()
                            .take(20)
                            .cloned()
                            .map(|item| (subscription.title.clone(), item)),
                    );
                    subscription.latest_urls = current_urls.into_iter().take(20).collect();
                    subscription.last_new_count = new_items.len();
                    subscription.last_new_items = new_items.into_iter().take(20).collect();
                    subscription.last_error.clear();
                }
                Err(error) => {
                    summary.failures += 1;
                    subscription.last_error = error;
                }
            }
        }
        sort_subscriptions(&mut candidate);
        self.commit(candidate)?;
        Ok(summary)
    }

    fn commit(
        &mut self,
        subscriptions: Vec<Subscription>,
    ) -> Result<(), SubscriptionControllerError> {
        self.ensure_writable()?;
        if let Some(file) = &self.file {
            file.save(&subscriptions)?;
        }
        self.subscriptions = subscriptions;
        Ok(())
    }

    fn ensure_writable(&self) -> Result<(), SubscriptionControllerError> {
        if self.save_blocked {
            return Err(SubscriptionControllerError::SaveBlocked {
                path: self
                    .file
                    .as_ref()
                    .map(|file| file.path().to_path_buf())
                    .unwrap_or_default(),
                message: self
                    .load_error
                    .clone()
                    .unwrap_or_else(|| "unknown load error".to_owned()),
            });
        }
        Ok(())
    }
}

pub fn normalize_category(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
}

pub fn canonical_channel_url(value: &str) -> Option<String> {
    let mut value = value.trim().to_owned();
    if !value.contains("://") {
        value = format!("https://www.youtube.com/{}", value.trim_start_matches('/'));
    }
    let mut url = Url::parse(&value).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url
        .host_str()?
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    if !matches!(host.as_str(), "youtube.com" | "m.youtube.com") {
        return None;
    }
    let mut segments = url
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if segments.last().is_some_and(|segment| {
        matches!(
            segment.to_ascii_lowercase().as_str(),
            "videos" | "playlists" | "featured" | "streams" | "shorts" | "community" | "about"
        )
    }) {
        segments.pop();
    }
    if segments.is_empty() {
        return None;
    }
    url.set_query(None);
    url.set_fragment(None);
    url.set_path(&format!("/{}", segments.join("/")));
    Some(url.as_str().trim_end_matches('/').to_owned())
}

fn subscription_target(item: &MediaItem) -> Option<(String, String, Option<String>)> {
    let mut channel_url = if item.kind == MediaKind::Channel {
        item.url.as_ref().map(Url::to_string).unwrap_or_default()
    } else {
        metadata_text(item, "channel_url")
            .or_else(|| metadata_text(item, "uploader_url"))
            .unwrap_or_default()
    };
    if channel_url.is_empty()
        && let Some(channel_id) = metadata_text(item, "channel_id")
            .or_else(|| metadata_text(item, "uploader_id"))
            .filter(|value| value.starts_with("UC"))
    {
        channel_url = format!("https://www.youtube.com/channel/{channel_id}");
    }
    let url = canonical_channel_url(&channel_url)?;
    let title = if item.kind == MediaKind::Channel {
        item.title.trim()
    } else {
        item.channel.trim()
    };
    let title = if title.is_empty() { &url } else { title }.to_owned();
    let latest_url = (item.kind == MediaKind::Video)
        .then(|| item.url.as_ref().map(Url::to_string))
        .flatten();
    Some((title, url, latest_url))
}

fn metadata_text(item: &MediaItem, key: &str) -> Option<String> {
    item.metadata
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn sort_subscriptions(subscriptions: &mut [Subscription]) {
    subscriptions.sort_by_key(|subscription| {
        (
            normalize_category(&subscription.category).to_lowercase(),
            subscription.title.trim().to_lowercase(),
            subscription.url.trim().to_lowercase(),
        )
    });
}

#[cfg(test)]
mod tests {
    use std::fs;

    use apricot_core::{MediaId, MediaKind, MediaSource};
    use apricot_storage::{Subscription, SubscriptionFile};
    use tempfile::tempdir;
    use url::Url;

    use super::{
        SubscriptionAddOutcome, SubscriptionCheckResult, SubscriptionController,
        canonical_channel_url, normalize_category,
    };

    fn video(id: &str, channel_id: &str) -> apricot_core::MediaItem {
        let mut item = apricot_core::MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: format!("Video {id}"),
            url: Some(
                Url::parse(&format!("https://www.youtube.com/watch?v={id}")).expect("test URL"),
            ),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: "Channel".to_owned(),
            duration_seconds: Some(60.0),
            metadata: std::collections::BTreeMap::new(),
        };
        item.metadata
            .insert("channel_id".to_owned(), channel_id.into());
        item
    }

    #[test]
    fn canonical_urls_drop_tabs_queries_and_reject_other_hosts() {
        assert_eq!(
            canonical_channel_url("/@apricot/videos?view=0").as_deref(),
            Some("https://www.youtube.com/@apricot")
        );
        assert_eq!(
            canonical_channel_url("https://youtube.com/channel/UC123/streams#live").as_deref(),
            Some("https://youtube.com/channel/UC123")
        );
        assert!(canonical_channel_url("https://example.com/@apricot").is_none());
    }

    #[test]
    fn category_normalization_matches_python_limit_and_whitespace() {
        assert_eq!(normalize_category("  Music\tLibrary  "), "Music Library");
        assert_eq!(normalize_category(&"a".repeat(81)).chars().count(), 80);
    }

    #[test]
    fn adding_video_derives_channel_and_deduplicates_canonical_url() {
        let root = tempdir().expect("temporary directory");
        let current = SubscriptionFile::new(root.path().join("subscriptions.json"));
        let mut controller = SubscriptionController::load(
            current,
            &SubscriptionFile::new(root.path().join("missing.json")),
        );
        assert_eq!(
            controller
                .add_from_item(&video("abcdefghijk", "UC123"), 42.0)
                .expect("add"),
            SubscriptionAddOutcome::Added(0)
        );
        assert_eq!(controller.subscriptions()[0].latest_urls.len(), 1);
        assert_eq!(
            controller
                .add_from_item(&video("lmnopqrstuv", "UC123"), 43.0)
                .expect("duplicate"),
            SubscriptionAddOutcome::AlreadyPresent
        );
    }

    #[test]
    fn check_batch_uses_known_urls_and_persists_once_complete() {
        let root = tempdir().expect("temporary directory");
        let current_path = root.path().join("beta/subscriptions.json");
        let legacy_path = root.path().join("stable/subscriptions.json");
        let mut subscription =
            Subscription::new("Channel", "https://www.youtube.com/channel/UC123", 1.0);
        subscription.latest_urls = vec!["https://www.youtube.com/watch?v=oldoldold00".to_owned()];
        SubscriptionFile::new(&legacy_path)
            .save(&[subscription])
            .expect("legacy fixture");
        let legacy_bytes = fs::read(&legacy_path).expect("legacy bytes");
        let mut controller = SubscriptionController::load(
            SubscriptionFile::new(&current_path),
            &SubscriptionFile::new(&legacy_path),
        );
        let summary = controller
            .apply_checks(
                vec![SubscriptionCheckResult {
                    url: "https://www.youtube.com/channel/UC123/videos".to_owned(),
                    result: Ok(vec![
                        video("newnewnew00", "UC123"),
                        video("oldoldold00", "UC123"),
                    ]),
                }],
                50.0,
            )
            .expect("apply");
        assert_eq!(summary.successes, 1);
        assert_eq!(summary.total_new, 1);
        assert_eq!(controller.subscriptions()[0].last_new_items.len(), 1);
        assert_eq!(
            fs::read(legacy_path).expect("legacy preserved"),
            legacy_bytes
        );
        assert!(current_path.is_file());
    }

    #[test]
    fn first_check_establishes_baseline_without_reporting_history_as_new() {
        let root = tempdir().expect("temporary directory");
        let current = SubscriptionFile::new(root.path().join("subscriptions.json"));
        let mut controller = SubscriptionController::load(
            current,
            &SubscriptionFile::new(root.path().join("missing.json")),
        );
        controller
            .add_from_item(&video("seedseed000", "UC123"), 1.0)
            .expect("add");
        controller.subscriptions[0].latest_urls.clear();
        let summary = controller
            .apply_checks(
                vec![SubscriptionCheckResult {
                    url: "https://www.youtube.com/channel/UC123".to_owned(),
                    result: Ok(vec![video("historical0", "UC123")]),
                }],
                2.0,
            )
            .expect("check");
        assert_eq!(summary.total_new, 0);
        assert!(controller.subscriptions()[0].last_new_items.is_empty());
    }

    #[test]
    fn malformed_current_file_blocks_destructive_changes() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("subscriptions.json");
        fs::write(&path, b"broken").expect("fixture");
        let mut controller = SubscriptionController::load(
            SubscriptionFile::new(&path),
            &SubscriptionFile::new(root.path().join("missing.json")),
        );
        assert!(
            controller
                .add_from_item(&video("abcdefghijk", "UC123"), 1.0)
                .is_err()
        );
        assert!(controller.remove(0).is_err());
        assert_eq!(fs::read(path).expect("preserved"), b"broken");
    }
}
