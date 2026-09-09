//! Bounded access to the official `YouTube` Data API for metadata enrichment.

use std::{collections::HashMap, io::Read, time::Duration};

use apricot_core::{MediaItem, MediaKind, MediaSource};
use chrono::DateTime;
use reqwest::{Proxy, blocking::Client, redirect::Policy};
use serde_json::{Map, Value};
use thiserror::Error;

const VIDEOS_ENDPOINT: &str = "https://www.googleapis.com/youtube/v3/videos";
const MAX_VIDEO_IDS: usize = 50;
const MAX_RESPONSE_BYTES: u64 = 10_000_000;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum YoutubeDataApiError {
    #[error("YouTube Data API requires a configured API key")]
    MissingApiKey,
    #[error("YouTube Data API metadata requires between one and 50 YouTube videos")]
    InvalidBatch,
    #[error("The configured proxy could not be used for YouTube Data API requests")]
    InvalidProxy,
    #[error("YouTube Data API request failed")]
    Request,
    #[error("YouTube Data API response exceeded the 10 MB safety limit")]
    ResponseTooLarge,
    #[error("YouTube Data API returned invalid data")]
    InvalidResponse,
    #[error("YouTube Data API rejected the request: {0}")]
    Api(String),
}

#[derive(Clone)]
pub struct YoutubeDataApiClient {
    client: Client,
}

impl YoutubeDataApiClient {
    /// Creates a client with fixed time, redirect, TLS, and optional proxy policy.
    ///
    /// # Errors
    ///
    /// Returns an error when the proxy or TLS client cannot be initialized.
    pub fn new(proxy_url: Option<&str>) -> Result<Self, YoutubeDataApiError> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(25))
            .connect_timeout(Duration::from_secs(10))
            .redirect(Policy::none())
            .user_agent("ApricotPlayer/2.0");
        if let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) {
            let proxy = Proxy::all(proxy_url).map_err(|_| YoutubeDataApiError::InvalidProxy)?;
            builder = builder.proxy(proxy);
        }
        let client = builder.build().map_err(|_| YoutubeDataApiError::Request)?;
        Ok(Self { client })
    }

    /// Fetches descriptive metadata in the same order as the supplied media.
    ///
    /// # Errors
    ///
    /// Returns an error for missing credentials, invalid bounds, transport
    /// failures, oversized or malformed JSON, and API-reported failures.
    pub fn fetch_metadata(
        &self,
        api_key: &str,
        items: &[MediaItem],
    ) -> Result<Vec<MediaItem>, YoutubeDataApiError> {
        let api_key = api_key.trim();
        if api_key.is_empty() {
            return Err(YoutubeDataApiError::MissingApiKey);
        }
        if items.is_empty()
            || items.len() > MAX_VIDEO_IDS
            || items.iter().any(|item| {
                item.source != MediaSource::Youtube
                    || !matches!(item.kind, MediaKind::Video | MediaKind::LiveStream)
                    || !valid_video_id(&item.id.0)
            })
        {
            return Err(YoutubeDataApiError::InvalidBatch);
        }
        let ids = items
            .iter()
            .map(|item| item.id.0.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let max_results = items.len().to_string();
        let mut response = self
            .client
            .get(VIDEOS_ENDPOINT)
            .query(&[
                ("part", "snippet,contentDetails,statistics"),
                ("id", ids.as_str()),
                ("key", api_key),
                ("maxResults", max_results.as_str()),
            ])
            .send()
            .map_err(|_| YoutubeDataApiError::Request)?;
        let status = response.status();
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| YoutubeDataApiError::Request)?;
        if bytes.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(YoutubeDataApiError::ResponseTooLarge);
        }
        let payload: Value =
            serde_json::from_slice(&bytes).map_err(|_| YoutubeDataApiError::InvalidResponse)?;
        if !status.is_success() || payload.get("error").is_some() {
            let message = payload
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("request was rejected");
            return Err(YoutubeDataApiError::Api(redact(message, api_key)));
        }
        normalize_metadata(items, &payload)
    }
}

fn normalize_metadata(
    originals: &[MediaItem],
    payload: &Value,
) -> Result<Vec<MediaItem>, YoutubeDataApiError> {
    let entries = payload
        .get("items")
        .and_then(Value::as_array)
        .ok_or(YoutubeDataApiError::InvalidResponse)?;
    let by_id: HashMap<&str, &Map<String, Value>> = entries
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|entry| entry.get("id")?.as_str().map(|id| (id, entry)))
        .collect();
    Ok(originals
        .iter()
        .filter_map(|original| {
            let entry = by_id.get(original.id.0.as_str())?;
            Some(normalize_item(original, entry))
        })
        .collect())
}

fn normalize_item(original: &MediaItem, entry: &Map<String, Value>) -> MediaItem {
    let mut item = original.clone();
    let snippet = object(Some(entry), "snippet");
    let content = object(Some(entry), "contentDetails");
    let statistics = object(Some(entry), "statistics");
    if let Some(title) = string(snippet, "title") {
        title.clone_into(&mut item.title);
    }
    if let Some(channel) = string(snippet, "channelTitle") {
        channel.clone_into(&mut item.channel);
    }
    if let Some(description) = string(snippet, "description") {
        item.metadata
            .insert("description".to_owned(), description.into());
    }
    if let Some(channel_id) = string(snippet, "channelId") {
        item.metadata
            .insert("channel_id".to_owned(), channel_id.into());
        item.metadata.insert(
            "channel_url".to_owned(),
            format!("https://www.youtube.com/channel/{channel_id}").into(),
        );
    }
    if let Some(published_at) = string(snippet, "publishedAt")
        && let Ok(published) = DateTime::parse_from_rfc3339(published_at)
    {
        item.metadata
            .insert("timestamp".to_owned(), published.timestamp().into());
        item.metadata.insert(
            "upload_date".to_owned(),
            published.format("%Y%m%d").to_string().into(),
        );
    }
    if let Some(view_count) = string(statistics, "viewCount") {
        let value = view_count
            .parse::<u64>()
            .map_or_else(|_| Value::String(view_count.to_owned()), Value::from);
        item.metadata.insert("views".to_owned(), value.clone());
        item.metadata.insert("view_count".to_owned(), value);
    }
    if let Some(duration) = string(content, "duration").and_then(iso8601_duration_seconds) {
        item.duration_seconds = Some(f64::from(duration));
    }
    let live_status = string(snippet, "liveBroadcastContent").unwrap_or("none");
    let is_live = live_status.eq_ignore_ascii_case("live");
    item.metadata
        .insert("live_status".to_owned(), live_status.into());
    item.metadata
        .insert("is_live".to_owned(), Value::Bool(is_live));
    if is_live {
        item.kind = MediaKind::LiveStream;
    }
    item
}

fn object<'a>(parent: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Map<String, Value>> {
    parent?.get(key)?.as_object()
}

fn string<'a>(parent: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a str> {
    parent?
        .get(key)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn valid_video_id(value: &str) -> bool {
    value.len() >= 8
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn iso8601_duration_seconds(value: &str) -> Option<u32> {
    let mut chars = value.trim().chars();
    if chars.next()? != 'P' {
        return None;
    }
    let mut total = 0_u32;
    let mut number = String::new();
    let mut in_time = false;
    for character in chars {
        if character.is_ascii_digit() {
            number.push(character);
            continue;
        }
        if character == 'T' && number.is_empty() && !in_time {
            in_time = true;
            continue;
        }
        let amount = number.parse::<u32>().ok()?;
        number.clear();
        let multiplier = match (in_time, character) {
            (false, 'D') => 86_400,
            (true, 'H') => 3_600,
            (true, 'M') => 60,
            (true, 'S') => 1,
            _ => return None,
        };
        total = total.checked_add(amount.checked_mul(multiplier)?)?;
    }
    number.is_empty().then_some(total)
}

fn redact(message: &str, secret: &str) -> String {
    if secret.is_empty() {
        message.to_owned()
    } else {
        message.replace(secret, "[API key]")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use serde_json::json;

    use super::{
        YoutubeDataApiClient, YoutubeDataApiError, iso8601_duration_seconds, normalize_metadata,
        redact,
    };

    fn item(id: &str) -> MediaItem {
        MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: "Old title".to_owned(),
            url: Some(
                format!("https://www.youtube.com/watch?v={id}")
                    .parse()
                    .expect("URL"),
            ),
            stream_url: Some("https://media.example/stream".parse().expect("stream")),
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn api_metadata_is_ordered_and_preserves_durable_and_ephemeral_locations() {
        let originals = vec![item("abcdefghijk"), item("zyxwvutsrqp")];
        let payload = json!({
            "items": [
                {
                    "id": "zyxwvutsrqp",
                    "snippet": {
                        "title": "Second",
                        "channelTitle": "Channel",
                        "channelId": "UC123",
                        "publishedAt": "2026-01-02T03:04:05Z",
                        "description": "Description",
                        "liveBroadcastContent": "none"
                    },
                    "contentDetails": { "duration": "PT1H2M3S" },
                    "statistics": { "viewCount": "1234567" }
                },
                {
                    "id": "abcdefghijk",
                    "snippet": {
                        "title": "First",
                        "channelTitle": "Other",
                        "publishedAt": "2025-12-31T00:00:00Z",
                        "liveBroadcastContent": "live"
                    },
                    "contentDetails": { "duration": "PT45S" },
                    "statistics": { "viewCount": "9" }
                }
            ]
        });
        let hydrated = normalize_metadata(&originals, &payload).expect("metadata");
        assert_eq!(hydrated[0].id.0, "abcdefghijk");
        assert_eq!(hydrated[0].title, "First");
        assert_eq!(hydrated[0].kind, MediaKind::LiveStream);
        assert_eq!(hydrated[0].duration_seconds, Some(45.0));
        assert_eq!(hydrated[1].id.0, "zyxwvutsrqp");
        assert_eq!(hydrated[1].metadata["view_count"], 1_234_567_u64);
        assert_eq!(hydrated[1].metadata["upload_date"], "20260102");
        assert_eq!(hydrated[1].url, originals[1].url);
        assert_eq!(hydrated[1].stream_url, originals[1].stream_url);
    }

    #[test]
    fn duration_parser_accepts_youtube_shapes_and_rejects_ambiguous_input() {
        assert_eq!(iso8601_duration_seconds("PT1H2M3S"), Some(3_723));
        assert_eq!(iso8601_duration_seconds("P1DT2S"), Some(86_402));
        assert_eq!(iso8601_duration_seconds("1:02"), None);
        assert_eq!(iso8601_duration_seconds("PT"), Some(0));
    }

    #[test]
    fn malformed_payload_and_secret_redaction_are_explicit() {
        assert_eq!(
            normalize_metadata(&[item("abcdefghijk")], &json!({})),
            Err(YoutubeDataApiError::InvalidResponse)
        );
        assert_eq!(redact("bad SECRET value", "SECRET"), "bad [API key] value");
    }

    #[test]
    fn invalid_batch_bounds_are_rejected_before_network_access() {
        let client = YoutubeDataApiClient::new(None).expect("client");
        assert_eq!(
            client.fetch_metadata("key", &[]),
            Err(YoutubeDataApiError::InvalidBatch)
        );
        assert_eq!(
            client.fetch_metadata("key", &vec![item("abcdefghijk"); 51]),
            Err(YoutubeDataApiError::InvalidBatch)
        );
        assert_eq!(
            client.fetch_metadata("", &[item("abcdefghijk")]),
            Err(YoutubeDataApiError::MissingApiKey)
        );
    }
}
