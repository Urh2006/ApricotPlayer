//! Transactional podcast/RSS archive state and refresh merging.

use std::{collections::HashMap, path::PathBuf};

use apricot_core::MediaItem;
use apricot_storage::{RssFeed, RssFeedFile, RssFeedFileError};
use serde_json::Value;
use thiserror::Error;
use url::Url;

use crate::subscription_controller::normalize_category;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RssFeedAddOutcome {
    Added(usize),
    AlreadyPresent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RssRefreshResult {
    pub url: String,
    pub result: Result<RssFeed, String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RssRefreshSummary {
    pub successes: usize,
    pub failures: usize,
    pub new_items: Vec<(String, MediaItem)>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RssFeedImportSummary {
    pub added: usize,
    pub already_present: usize,
}

#[derive(Debug, Error)]
pub enum RssFeedControllerError {
    #[error("RSS changes are blocked because loading {path} failed: {message}")]
    SaveBlocked { path: PathBuf, message: String },
    #[error(transparent)]
    Storage(#[from] RssFeedFileError),
}

#[derive(Debug, Default)]
pub struct RssFeedController {
    feeds: Vec<RssFeed>,
    category_filter: String,
    file: Option<RssFeedFile>,
    load_error: Option<String>,
    save_blocked: bool,
}

impl RssFeedController {
    pub fn load(current: RssFeedFile, legacy: &RssFeedFile) -> Self {
        if current.path().is_file() {
            return match current.load() {
                Ok(feeds) => Self::loaded(current, feeds),
                Err(error) => Self::blocked(current, error.to_string()),
            };
        }
        if legacy.path().is_file() {
            return match legacy.load() {
                Ok(feeds) => Self::loaded(current, feeds),
                Err(error) => Self {
                    feeds: Vec::new(),
                    category_filter: String::new(),
                    file: Some(current),
                    load_error: Some(error.to_string()),
                    save_blocked: false,
                },
            };
        }
        Self::loaded(current, Vec::new())
    }

    fn loaded(file: RssFeedFile, mut feeds: Vec<RssFeed>) -> Self {
        sort_feeds(&mut feeds);
        Self {
            feeds,
            category_filter: String::new(),
            file: Some(file),
            load_error: None,
            save_blocked: false,
        }
    }

    fn blocked(file: RssFeedFile, message: String) -> Self {
        Self {
            feeds: Vec::new(),
            category_filter: String::new(),
            file: Some(file),
            load_error: Some(message),
            save_blocked: true,
        }
    }

    pub fn feeds(&self) -> &[RssFeed] {
        &self.feeds
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
            .feeds
            .iter()
            .map(|feed| normalize_category(&feed.category))
            .filter(|category| !category.is_empty())
            .collect::<Vec<_>>();
        categories.sort_by_key(|category| category.to_lowercase());
        categories.dedup_by(|left, right| left.to_lowercase() == right.to_lowercase());
        categories
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        let filter = self.category_filter.to_lowercase();
        self.feeds
            .iter()
            .enumerate()
            .filter_map(|(index, feed)| {
                (filter.is_empty() || normalize_category(&feed.category).to_lowercase() == filter)
                    .then_some(index)
            })
            .collect()
    }

    /// Adds one fetched feed unless its canonical URL is already present.
    ///
    /// # Errors
    ///
    /// Returns an error when the collection cannot be persisted.
    pub fn add(&mut self, feed: RssFeed) -> Result<RssFeedAddOutcome, RssFeedControllerError> {
        self.ensure_writable()?;
        let identity = feed_identity(&feed);
        if self
            .feeds
            .iter()
            .any(|candidate| feed_identity(candidate).eq_ignore_ascii_case(&identity))
        {
            return Ok(RssFeedAddOutcome::AlreadyPresent);
        }
        let mut candidate = self.feeds.clone();
        candidate.push(feed);
        sort_feeds(&mut candidate);
        let index = candidate
            .iter()
            .position(|feed| feed_identity(feed).eq_ignore_ascii_case(&identity))
            .unwrap_or_default();
        self.commit(candidate)?;
        Ok(RssFeedAddOutcome::Added(index))
    }

    /// Adds multiple fetched feeds with one durable write.
    ///
    /// # Errors
    ///
    /// Returns an error when the updated collection cannot be persisted.
    pub fn add_many(
        &mut self,
        feeds: Vec<RssFeed>,
    ) -> Result<RssFeedImportSummary, RssFeedControllerError> {
        self.ensure_writable()?;
        let mut summary = RssFeedImportSummary::default();
        let mut candidate = self.feeds.clone();
        let mut identities = candidate
            .iter()
            .map(feed_identity)
            .map(|identity| identity.to_ascii_lowercase())
            .collect::<std::collections::HashSet<_>>();
        for feed in feeds {
            let identity = feed_identity(&feed).to_ascii_lowercase();
            if identities.insert(identity) {
                candidate.push(feed);
                summary.added += 1;
            } else {
                summary.already_present += 1;
            }
        }
        if summary.added > 0 {
            sort_feeds(&mut candidate);
            self.commit(candidate)?;
        }
        Ok(summary)
    }

    /// Removes one feed by its unfiltered archive index.
    ///
    /// # Errors
    ///
    /// Returns an error when the collection cannot be persisted.
    pub fn remove(&mut self, index: usize) -> Result<Option<RssFeed>, RssFeedControllerError> {
        self.ensure_writable()?;
        if index >= self.feeds.len() {
            return Ok(None);
        }
        let mut candidate = self.feeds.clone();
        let removed = candidate.remove(index);
        self.commit(candidate)?;
        Ok(Some(removed))
    }

    /// Assigns or clears one normalized feed category.
    ///
    /// # Errors
    ///
    /// Returns an error when the collection cannot be persisted.
    pub fn set_category(
        &mut self,
        index: usize,
        category: &str,
    ) -> Result<bool, RssFeedControllerError> {
        self.ensure_writable()?;
        let Some(feed) = self.feeds.get(index) else {
            return Ok(false);
        };
        let category = normalize_category(category);
        if feed.category == category {
            return Ok(false);
        }
        let mut candidate = self.feeds.clone();
        candidate[index].category = category;
        sort_feeds(&mut candidate);
        self.commit(candidate)?;
        Ok(true)
    }

    /// Saves a per-feed speed or clears it to use the global player setting.
    ///
    /// # Errors
    ///
    /// Returns an error when the collection cannot be persisted.
    pub fn set_speed_preset(
        &mut self,
        index: usize,
        speed: Option<f64>,
    ) -> Result<bool, RssFeedControllerError> {
        self.ensure_writable()?;
        let Some(feed) = self.feeds.get(index) else {
            return Ok(false);
        };
        let speed = normalize_speed(speed);
        if feed.speed_preset == speed {
            return Ok(false);
        }
        let mut candidate = self.feeds.clone();
        candidate[index].speed_preset = speed;
        self.commit(candidate)?;
        Ok(true)
    }

    /// Changes one episode's played state and returns the updated episode.
    ///
    /// # Errors
    ///
    /// Returns an error when the archive cannot be persisted.
    pub fn set_played(
        &mut self,
        feed_index: usize,
        item_index: usize,
        played: bool,
        timestamp: f64,
    ) -> Result<Option<MediaItem>, RssFeedControllerError> {
        self.ensure_writable()?;
        let Some(item) = self
            .feeds
            .get(feed_index)
            .and_then(|feed| feed.items.get(item_index))
        else {
            return Ok(None);
        };
        if metadata_bool(item, "played") == played {
            return Ok(Some(item.clone()));
        }
        let mut candidate = self.feeds.clone();
        let item = &mut candidate[feed_index].items[item_index];
        item.metadata
            .insert("played".to_owned(), Value::Bool(played));
        if played {
            item.metadata.insert(
                "played_at".to_owned(),
                Value::from(finite_timestamp(timestamp)),
            );
            let count = item
                .metadata
                .get("play_count")
                .and_then(Value::as_u64)
                .unwrap_or_default()
                .max(1);
            item.metadata
                .insert("play_count".to_owned(), Value::from(count));
        } else {
            item.metadata.remove("played_at");
        }
        let updated = item.clone();
        self.commit(candidate)?;
        Ok(Some(updated))
    }

    /// Applies a complete refresh batch with one durable write.
    ///
    /// # Errors
    ///
    /// Returns an error when the updated archive cannot be persisted.
    pub fn apply_refreshes(
        &mut self,
        results: Vec<RssRefreshResult>,
        timestamp: f64,
    ) -> Result<RssRefreshSummary, RssFeedControllerError> {
        self.ensure_writable()?;
        let mut candidate = self.feeds.clone();
        let mut summary = RssRefreshSummary::default();
        let timestamp = finite_timestamp(timestamp);
        for refresh in results {
            let target =
                canonical_feed_url(&refresh.url).unwrap_or_else(|| refresh.url.trim().to_owned());
            let Some(index) = candidate
                .iter()
                .position(|feed| feed_identity(feed).eq_ignore_ascii_case(&target))
            else {
                continue;
            };
            match refresh.result {
                Ok(mut refreshed) => {
                    summary.successes += 1;
                    let existing = &candidate[index];
                    let new_items = new_rss_entries(&refreshed.items, &existing.items);
                    preserve_feed_state(&mut refreshed, existing);
                    refreshed.last_checked = Some(timestamp);
                    refreshed.last_error.clear();
                    summary.new_items.extend(
                        new_items
                            .into_iter()
                            .map(|item| (refreshed.title.clone(), item)),
                    );
                    candidate[index] = refreshed;
                }
                Err(error) => {
                    summary.failures += 1;
                    candidate[index].last_checked = Some(timestamp);
                    candidate[index].last_error = error;
                }
            }
        }
        sort_feeds(&mut candidate);
        self.commit(candidate)?;
        Ok(summary)
    }

    fn commit(&mut self, feeds: Vec<RssFeed>) -> Result<(), RssFeedControllerError> {
        self.ensure_writable()?;
        if let Some(file) = &self.file {
            file.save(&feeds)?;
        }
        self.feeds = feeds;
        Ok(())
    }

    fn ensure_writable(&self) -> Result<(), RssFeedControllerError> {
        if self.save_blocked {
            return Err(RssFeedControllerError::SaveBlocked {
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

pub fn canonical_feed_url(value: &str) -> Option<String> {
    let mut url = Url::parse(value.trim()).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    url.set_fragment(None);
    let normalized = url.to_string();
    Some(normalized.trim_end_matches('/').to_owned())
}

pub fn rss_episode_identity(item: &MediaItem) -> String {
    item.url
        .as_ref()
        .map(ToString::to_string)
        .or_else(|| metadata_text(item, "guid"))
        .or_else(|| metadata_text(item, "webpage_url"))
        .unwrap_or_else(|| item.title.trim().to_owned())
}

fn preserve_feed_state(refreshed: &mut RssFeed, existing: &RssFeed) {
    refreshed.created_at = existing.created_at.or(refreshed.created_at);
    refreshed.category = normalize_category(&existing.category);
    refreshed.speed_preset = normalize_speed(existing.speed_preset);
    let old_items: HashMap<_, _> = existing
        .items
        .iter()
        .filter_map(|item| {
            let identity = rss_episode_identity(item);
            (!identity.is_empty()).then_some((identity, item))
        })
        .collect();
    for item in &mut refreshed.items {
        let Some(previous) = old_items.get(&rss_episode_identity(item)) else {
            continue;
        };
        for key in ["played", "played_at", "play_count"] {
            if let Some(value) = previous.metadata.get(key) {
                item.metadata.insert(key.to_owned(), value.clone());
            }
        }
    }
}

fn new_rss_entries(refreshed: &[MediaItem], existing: &[MediaItem]) -> Vec<MediaItem> {
    if existing.is_empty() {
        return Vec::new();
    }
    let known = existing
        .iter()
        .map(rss_episode_identity)
        .filter(|identity| !identity.is_empty())
        .collect::<std::collections::HashSet<_>>();
    let Some(first_known) = refreshed
        .iter()
        .position(|item| known.contains(&rss_episode_identity(item)))
    else {
        return Vec::new();
    };
    refreshed[..first_known]
        .iter()
        .filter(|item| !rss_episode_identity(item).is_empty())
        .cloned()
        .collect()
}

fn feed_identity(feed: &RssFeed) -> String {
    canonical_feed_url(&feed.url).unwrap_or_else(|| feed.url.trim().to_owned())
}

fn normalize_speed(speed: Option<f64>) -> Option<f64> {
    speed
        .filter(|speed| speed.is_finite() && (0.25..=4.0).contains(speed))
        .map(|speed| (speed * 100.0).round() / 100.0)
}

fn finite_timestamp(timestamp: f64) -> f64 {
    if timestamp.is_finite() {
        timestamp.max(0.0)
    } else {
        0.0
    }
}

fn metadata_bool(item: &MediaItem, key: &str) -> bool {
    item.metadata
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn metadata_text(item: &MediaItem, key: &str) -> Option<String> {
    item.metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn sort_feeds(feeds: &mut [RssFeed]) {
    feeds.sort_by_key(|feed| {
        (
            normalize_category(&feed.category).to_lowercase(),
            feed.title.trim().to_lowercase(),
            feed.url.trim().to_lowercase(),
        )
    });
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_storage::{RssFeed, RssFeedFile};
    use tempfile::tempdir;

    use super::{
        RssFeedAddOutcome, RssFeedController, RssRefreshResult, canonical_feed_url,
        rss_episode_identity,
    };

    fn episode(id: &str) -> MediaItem {
        MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Podcast,
            kind: MediaKind::PodcastEpisode,
            title: format!("Episode {id}"),
            url: Some(
                format!("https://media.example/{id}.mp3")
                    .parse()
                    .expect("URL"),
            ),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: "Feed".to_owned(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    fn feed(title: &str, ids: &[&str]) -> RssFeed {
        RssFeed::new(
            title,
            format!("https://podcast.example/{title}.xml"),
            "https://podcast.example",
            ids.iter().map(|id| episode(id)).collect(),
            1.0,
        )
    }

    #[test]
    fn canonical_urls_reject_credentials_and_drop_fragments() {
        assert_eq!(
            canonical_feed_url("https://PODCAST.example/feed.xml#episodes").as_deref(),
            Some("https://podcast.example/feed.xml")
        );
        assert!(canonical_feed_url("https://user:secret@example.com/feed").is_none());
        assert!(canonical_feed_url("file:///feed.xml").is_none());
    }

    #[test]
    fn stable_archive_is_read_only_and_first_change_targets_beta() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("beta/rss_feeds.json");
        let legacy = root.path().join("stable/rss_feeds.json");
        RssFeedFile::new(&legacy)
            .save(&[feed("Legacy", &["old"])])
            .expect("legacy fixture");
        let legacy_bytes = fs::read(&legacy).expect("legacy bytes");
        let mut controller =
            RssFeedController::load(RssFeedFile::new(&current), &RssFeedFile::new(&legacy));
        assert_eq!(controller.feeds().len(), 1);
        assert_eq!(
            controller.add(feed("New", &["one"])).expect("add"),
            RssFeedAddOutcome::Added(1)
        );
        assert_eq!(fs::read(legacy).expect("legacy preserved"), legacy_bytes);
        assert!(current.is_file());
    }

    #[test]
    fn refresh_preserves_state_and_reports_only_items_before_first_known() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("rss_feeds.json");
        let mut original = feed("Archive", &["known-2", "known-1"]);
        original.category = "Shows".to_owned();
        original.speed_preset = Some(1.5);
        original.items[0]
            .metadata
            .insert("played".to_owned(), true.into());
        RssFeedFile::new(&current)
            .save(&[original])
            .expect("fixture");
        let mut controller = RssFeedController::load(
            RssFeedFile::new(&current),
            &RssFeedFile::new(root.path().join("missing.json")),
        );
        let refreshed = feed("Renamed", &["new-2", "new-1", "known-2", "older"]);
        let summary = controller
            .apply_refreshes(
                vec![RssRefreshResult {
                    url: "https://podcast.example/Archive.xml".to_owned(),
                    result: Ok(refreshed),
                }],
                20.0,
            )
            .expect("refresh");
        assert_eq!(summary.new_items.len(), 2);
        assert_eq!(controller.feeds()[0].category, "Shows");
        assert_eq!(controller.feeds()[0].speed_preset, Some(1.5));
        assert_eq!(controller.feeds()[0].items[2].metadata["played"], true);
    }

    #[test]
    fn first_refresh_or_missing_overlap_does_not_report_history_as_new() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("rss_feeds.json");
        RssFeedFile::new(&current)
            .save(&[feed("Archive", &[])])
            .expect("fixture");
        let mut controller = RssFeedController::load(
            RssFeedFile::new(&current),
            &RssFeedFile::new(root.path().join("missing.json")),
        );
        let result = controller
            .apply_refreshes(
                vec![RssRefreshResult {
                    url: "https://podcast.example/Archive.xml".to_owned(),
                    result: Ok(feed("Archive", &["one", "two"])),
                }],
                2.0,
            )
            .expect("refresh");
        assert!(result.new_items.is_empty());

        let result = controller
            .apply_refreshes(
                vec![RssRefreshResult {
                    url: "https://podcast.example/Archive.xml".to_owned(),
                    result: Ok(feed("Archive", &["unrelated"])),
                }],
                3.0,
            )
            .expect("refresh");
        assert!(result.new_items.is_empty());
    }

    #[test]
    fn played_and_speed_changes_are_independent_and_persisted() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("rss_feeds.json");
        let mut controller = RssFeedController::load(
            RssFeedFile::new(&current),
            &RssFeedFile::new(root.path().join("missing.json")),
        );
        controller.add(feed("Archive", &["one"])).expect("add");
        assert!(controller.set_speed_preset(0, Some(1.333)).expect("speed"));
        let updated = controller
            .set_played(0, 0, true, 5.0)
            .expect("played")
            .expect("episode");
        assert_eq!(controller.feeds()[0].speed_preset, Some(1.33));
        assert_eq!(updated.metadata["played"], true);
        assert_eq!(updated.metadata["play_count"], 1);
        assert!(!rss_episode_identity(&updated).is_empty());
        let reloaded = RssFeedFile::new(current).load().expect("reload");
        assert_eq!(reloaded[0].speed_preset, Some(1.33));
        assert_eq!(reloaded[0].items[0].metadata["played"], true);
    }

    #[test]
    fn bulk_import_deduplicates_and_commits_once() {
        let root = tempdir().expect("temporary directory");
        let current = root.path().join("rss_feeds.json");
        let mut controller = RssFeedController::load(
            RssFeedFile::new(&current),
            &RssFeedFile::new(root.path().join("missing.json")),
        );
        controller.add(feed("Existing", &["one"])).expect("seed");
        let summary = controller
            .add_many(vec![
                feed("Existing", &["duplicate"]),
                feed("New", &["two"]),
                feed("New", &["duplicate"]),
            ])
            .expect("import");
        assert_eq!(summary.added, 1);
        assert_eq!(summary.already_present, 2);
        assert_eq!(RssFeedFile::new(current).load().expect("reload").len(), 2);
    }

    #[test]
    fn malformed_current_archive_blocks_destructive_changes() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("rss_feeds.json");
        fs::write(&path, b"broken").expect("fixture");
        let mut controller = RssFeedController::load(
            RssFeedFile::new(&path),
            &RssFeedFile::new(root.path().join("missing.json")),
        );
        assert!(controller.load_error().is_some());
        assert!(controller.add(feed("New", &["one"])).is_err());
        assert_eq!(fs::read(path).expect("preserved"), b"broken");
    }
}
