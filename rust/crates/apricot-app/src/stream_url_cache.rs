//! Python `cached_stream_url`, `cache_stream_url` and `stream_url_cache.json`:
//! a resolved stream is reused while its signed URL is still valid, so opening
//! the same item again (or the prefetched next item) skips yt-dlp.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use apricot_core::MediaItem;
use serde_json::{Map, Value, json};

/// Python `STREAM_URL_CACHE_FILE`, in the data folder.
pub const STREAM_URL_CACHE_FILE: &str = "stream_url_cache.json";
/// Python `STREAM_FORMAT_PROFILE`, with a Rust marker: the Rust stream
/// selection differs, so Python entries never match a Rust key and the other
/// way around. Both keep each other's entries in the shared file.
const STREAM_FORMAT_PROFILE: &str = "apricot-rust-2";
const MAX_ENTRIES: usize = 120;
const TRIMMED_ENTRIES: usize = 100;
/// Python: `minutes <= 0` keeps an entry for a year unless the URL expires.
const UNLIMITED_TTL_SECONDS: f64 = 365.0 * 24.0 * 60.0 * 60.0;

/// What a cache hit gives back: the resolved item and its stream URLs.
#[derive(Clone, Debug, PartialEq)]
pub struct CachedStream {
    pub item: MediaItem,
    pub stream_url: String,
    pub external_audio_url: Option<String>,
}

/// The settings Python `stream_url_cache_key` puts into the key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamKeySettings {
    pub stream_format_preference: String,
    pub video_format: String,
    pub max_height: i64,
    pub restricted: bool,
    pub cookies_file: String,
    pub cookies_signature: String,
    pub cookies_browser: String,
}

/// Python `stream_url_cache_key`: the URL plus every setting that changes the
/// resolved stream, as sorted JSON.
#[must_use]
pub fn cache_key(url: &str, settings: &StreamKeySettings) -> String {
    let parts: BTreeMap<&str, Value> = BTreeMap::from([
        ("url", json!(url)),
        (
            "stream_format_preference",
            json!(settings.stream_format_preference),
        ),
        ("video_format", json!(settings.video_format)),
        ("max_height", json!(settings.max_height)),
        ("restricted", json!(settings.restricted)),
        ("cookies_file", json!(settings.cookies_file)),
        ("cookies_signature", json!(settings.cookies_signature)),
        ("cookies_browser", json!(settings.cookies_browser)),
        ("stream_format_profile", json!(STREAM_FORMAT_PROFILE)),
    ]);
    serde_json::to_string(&parts).unwrap_or_default()
}

/// Python `cookie_source_signature`: size and modification time of the
/// cookies file, so a refreshed file does not reuse streams of the old one.
#[must_use]
pub fn cookies_signature(path: &Path) -> String {
    fs::metadata(path)
        .ok()
        .map(|metadata| {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |duration| duration.as_nanos());
            format!("{}:{modified}", metadata.len())
        })
        .unwrap_or_default()
}

/// Python `stream_url_remote_expiry`: `expire`, `expires` or `exp` in the
/// query, or `/expire/<seconds>/` in the path.
#[must_use]
pub fn remote_expiry(stream_url: &str) -> Option<f64> {
    let url = url::Url::parse(stream_url).ok()?;
    for key in ["expire", "expires", "exp"] {
        if let Some((_, value)) = url.query_pairs().find(|(name, _)| name == key) {
            return value.parse::<u32>().ok().map(f64::from);
        }
    }
    let mut segments = url.path_segments()?;
    while let Some(segment) = segments.next() {
        if segment == "expire" {
            return segments
                .next()
                .and_then(|value| value.parse::<u32>().ok())
                .map(f64::from);
        }
    }
    None
}

#[derive(Debug, Default)]
pub struct StreamUrlCache {
    path: Option<PathBuf>,
    entries: Map<String, Value>,
}

impl StreamUrlCache {
    /// Python `load_stream_url_cache`: only unexpired entries that are safe
    /// across restarts; any read error gives an empty cache.
    #[must_use]
    pub fn load(path: PathBuf, now: f64) -> Self {
        let entries = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| match value {
                Value::Object(entries) => Some(entries),
                _ => None,
            })
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, entry)| restart_safe(entry) && expires_at(entry) > now)
            .collect();
        Self {
            path: Some(path),
            entries,
        }
    }

    /// Python `cached_stream_url`.
    pub fn get(&mut self, key: &str, now: f64) -> Option<CachedStream> {
        let entry = self.entries.get(key)?;
        if expires_at(entry) <= now {
            self.entries.remove(key);
            return None;
        }
        let stream_url = entry.get("stream_url")?.as_str()?.to_owned();
        let info = entry.get("info")?;
        let item = serde_json::from_value(info.get("apricot_item")?.clone()).ok()?;
        let external_audio_url = info
            .get("external_audio_url")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .map(str::to_owned);
        Some(CachedStream {
            item,
            stream_url,
            external_audio_url,
        })
    }

    pub fn contains(&self, key: &str, now: f64) -> bool {
        self.entries
            .get(key)
            .is_some_and(|entry| expires_at(entry) > now)
    }

    /// Python `cache_stream_url`: the lifetime is the configured minutes, but
    /// never past a minute before the signed URL expires. Returns whether the
    /// stream was stored.
    pub fn insert(&mut self, key: &str, stream: &CachedStream, minutes: i64, now: f64) -> bool {
        if key.is_empty() || stream.stream_url.is_empty() {
            return false;
        }
        let ttl = if minutes <= 0 {
            UNLIMITED_TTL_SECONDS
        } else {
            f64::from(u32::try_from(minutes).unwrap_or(u32::MAX)) * 60.0
        };
        let remote = remote_expiry(&stream.stream_url);
        let external = stream.external_audio_url.as_deref().map(remote_expiry);
        let mut expires = now + ttl;
        for expiry in [remote, external.flatten()].into_iter().flatten() {
            expires = expires.min(expiry - 60.0);
        }
        if expires <= now + 30.0 {
            return false;
        }
        self.entries.retain(|_, entry| expires_at(entry) > now);
        if self.entries.len() > MAX_ENTRIES {
            let mut oldest = self
                .entries
                .iter()
                .map(|(key, entry)| (expires_at(entry), key.clone()))
                .collect::<Vec<_>>();
            oldest.sort_by(|left, right| left.0.total_cmp(&right.0));
            let excess = self.entries.len() - TRIMMED_ENTRIES;
            for (_, key) in oldest.into_iter().take(excess) {
                self.entries.remove(&key);
            }
        }
        let item = serde_json::to_value(&stream.item).unwrap_or(Value::Null);
        let restart_safe = remote.is_some() && external.is_none_or(|expiry| expiry.is_some());
        self.entries.insert(
            key.to_owned(),
            json!({
                "stream_url": stream.stream_url,
                "headers": stream
                    .item
                    .metadata
                    .get("http_headers")
                    .cloned()
                    .unwrap_or_else(|| json!({})),
                "info": {
                    "title": stream.item.title,
                    "webpage_url": stream.item.url.as_ref().map(ToString::to_string),
                    "external_audio_url": stream.external_audio_url,
                    "apricot_item": item,
                },
                "expires_at": expires,
                "restart_safe": restart_safe,
            }),
        );
        self.save(now);
        true
    }

    /// Python `save_stream_url_cache`: unexpired restart-safe entries only,
    /// written atomically. The cache is a pure speed-up, so errors are ignored.
    fn save(&self, now: f64) {
        let Some(path) = &self.path else {
            return;
        };
        let saved = self
            .entries
            .iter()
            .filter(|(_, entry)| restart_safe(entry) && expires_at(entry) > now)
            .map(|(key, entry)| (key.clone(), entry.clone()))
            .collect::<Map<_, _>>();
        let Ok(bytes) = serde_json::to_vec(&Value::Object(saved)) else {
            return;
        };
        let temporary = path.with_extension("json.tmp");
        if fs::write(&temporary, bytes).is_ok() && fs::rename(&temporary, path).is_err() {
            let _ = fs::remove_file(&temporary);
        }
    }
}

fn expires_at(entry: &Value) -> f64 {
    entry
        .get("expires_at")
        .and_then(Value::as_f64)
        .unwrap_or_default()
}

fn restart_safe(entry: &Value) -> bool {
    entry
        .get("restart_safe")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use apricot_core::MediaItem;

    use super::{CachedStream, StreamKeySettings, StreamUrlCache, cache_key, remote_expiry};

    fn stream(expire: i64) -> CachedStream {
        CachedStream {
            item: MediaItem::from_direct_link("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
                .expect("item"),
            stream_url: format!("https://rr1.googlevideo.com/videoplayback?expire={expire}&id=1"),
            external_audio_url: None,
        }
    }

    #[test]
    fn remote_expiry_reads_query_and_path_like_python() {
        assert_eq!(remote_expiry("https://a.test/v?exp=100"), Some(100.0));
        assert_eq!(remote_expiry("https://a.test/v?expires=7"), Some(7.0));
        assert_eq!(
            remote_expiry("https://a.test/api/manifest/expire/1790000000/ei/x"),
            Some(1_790_000_000.0)
        );
        assert_eq!(remote_expiry("https://a.test/v?id=1"), None);
    }

    #[test]
    fn keys_follow_the_settings_that_change_the_stream() {
        let settings = StreamKeySettings::default();
        let other = StreamKeySettings {
            max_height: 720,
            ..StreamKeySettings::default()
        };
        assert_eq!(cache_key("u", &settings), cache_key("u", &settings));
        assert_ne!(cache_key("u", &settings), cache_key("u", &other));
        assert_ne!(cache_key("u", &settings), cache_key("v", &settings));
    }

    #[test]
    fn entries_live_until_a_minute_before_the_url_expires_and_survive_a_restart() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("stream_url_cache.json");
        let now = 1_000_000.0;
        let mut cache = StreamUrlCache::load(path.clone(), now);
        assert!(cache.insert("k", &stream(1_000_600), 360, now));
        assert_eq!(cache.get("k", now), Some(stream(1_000_600)));
        let mut reloaded = StreamUrlCache::load(path.clone(), now + 10.0);
        assert_eq!(reloaded.get("k", now + 10.0), Some(stream(1_000_600)));
        // expire - 60 s
        assert!(cache.get("k", now + 541.0).is_none());
        // A URL that expires within 90 s is not worth storing.
        assert!(!cache.insert("short", &stream(1_000_080), 360, now));
        // Without a signed expiry the entry stays in memory but is not saved.
        let mut unsigned = stream(0);
        unsigned.stream_url = "https://media.example/a.mp3".to_owned();
        assert!(cache.insert("unsigned", &unsigned, 5, now));
        assert!(cache.contains("unsigned", now));
        assert!(!StreamUrlCache::load(path, now).contains("unsigned", now));
    }

    #[test]
    fn python_entries_are_kept_in_the_shared_file() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("stream_url_cache.json");
        let python = serde_json::json!({
            "{\"url\": \"x\"}": {
                "stream_url": "https://a.test/v?expire=5000000",
                "headers": {},
                "info": {"title": "x"},
                "expires_at": 4_000_000.0,
                "restart_safe": true
            }
        });
        std::fs::write(&path, serde_json::to_vec(&python).expect("json")).expect("write");
        let mut cache = StreamUrlCache::load(path.clone(), 1_000_000.0);
        // A Python entry has no Rust item, so it is never a Rust hit.
        assert!(cache.get("{\"url\": \"x\"}", 1_000_000.0).is_none());
        assert!(cache.insert("k", &stream(3_000_000), 360, 1_000_000.0));
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
        assert!(saved.get("{\"url\": \"x\"}").is_some());
        assert!(saved.get("k").is_some());
    }
}
