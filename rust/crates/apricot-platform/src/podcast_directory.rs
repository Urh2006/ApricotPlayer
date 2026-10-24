//! Bounded Apple Podcasts directory search.

use std::{io::Read, time::Duration};

use apricot_media::PodcastDirectoryItem;
use reqwest::{Proxy, blocking::Client, redirect::Policy};
use serde_json::Value;
use thiserror::Error;
use url::Url;

const SEARCH_ENDPOINT: &str = "https://itunes.apple.com/search";
const LOOKUP_ENDPOINT: &str = "https://itunes.apple.com/lookup";
const MAX_RESPONSE_BYTES: u64 = 10_000_000;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum PodcastDirectoryError {
    #[error("podcast search query cannot be empty")]
    EmptyQuery,
    #[error("podcast search country must be a two-letter code")]
    InvalidCountry,
    #[error("podcast search limit must be between one and 200")]
    InvalidLimit,
    #[error("podcast category identifier must be positive")]
    InvalidGenre,
    #[error("the configured proxy could not be used for podcast search")]
    InvalidProxy,
    #[error("Apple Podcasts request failed")]
    Request,
    #[error("Apple Podcasts redirected to an untrusted address")]
    UntrustedRedirect,
    #[error("Apple Podcasts response exceeded the 10 MB safety limit")]
    ResponseTooLarge,
    #[error("Apple Podcasts returned invalid data")]
    InvalidResponse,
}

#[derive(Clone)]
pub struct ApplePodcastDirectoryClient {
    client: Client,
}

impl ApplePodcastDirectoryClient {
    /// Creates a client with bounded redirects, timeouts, and optional proxy use.
    ///
    /// # Errors
    ///
    /// Returns an error when the proxy or TLS client cannot be initialized.
    pub fn new(proxy_url: Option<&str>) -> Result<Self, PodcastDirectoryError> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(Policy::limited(5))
            .user_agent("ApricotPlayer/2.0");
        if let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) {
            builder = builder
                .proxy(Proxy::all(proxy_url).map_err(|_| PodcastDirectoryError::InvalidProxy)?);
        }
        let client = builder
            .build()
            .map_err(|_| PodcastDirectoryError::Request)?;
        Ok(Self { client })
    }

    /// Searches Apple Podcasts and normalizes results to stable feed records.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid parameters, transport failures, untrusted
    /// redirects, oversized responses, or malformed JSON.
    pub fn search(
        &self,
        query: &str,
        country: &str,
        limit: u32,
    ) -> Result<Vec<PodcastDirectoryItem>, PodcastDirectoryError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(PodcastDirectoryError::EmptyQuery);
        }
        let country = country.trim().to_ascii_uppercase();
        if country.len() != 2 || !country.bytes().all(|byte| byte.is_ascii_uppercase()) {
            return Err(PodcastDirectoryError::InvalidCountry);
        }
        if !(1..=200).contains(&limit) {
            return Err(PodcastDirectoryError::InvalidLimit);
        }
        let request = self.client.get(SEARCH_ENDPOINT).query(&[
            ("media", "podcast".to_owned()),
            ("entity", "podcast".to_owned()),
            ("term", query.to_owned()),
            ("country", country),
            ("limit", limit.to_string()),
        ]);
        let payload = Self::request_json(request)?;
        normalize_results(&payload)
    }

    /// Loads Apple's ranked top-podcast feed for one public genre and resolves
    /// those ranked identifiers to the same normalized records as search.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid genre, transport failure, untrusted
    /// redirect, oversized response, or malformed JSON.
    pub fn top_by_genre(
        &self,
        genre_id: u32,
    ) -> Result<Vec<PodcastDirectoryItem>, PodcastDirectoryError> {
        if genre_id == 0 {
            return Err(PodcastDirectoryError::InvalidGenre);
        }
        let ranking_url =
            format!("https://itunes.apple.com/us/rss/toppodcasts/limit=40/genre={genre_id}/json");
        let ranking = Self::request_json(self.client.get(ranking_url))?;
        let ids = ranked_podcast_ids(&ranking)?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let lookup = Self::request_json(
            self.client
                .get(LOOKUP_ENDPOINT)
                .query(&[("id", ids.join(","))]),
        )?;
        normalize_ranked_results(&lookup, &ids)
    }

    fn request_json(
        request: reqwest::blocking::RequestBuilder,
    ) -> Result<Value, PodcastDirectoryError> {
        let mut response = request.send().map_err(|_| PodcastDirectoryError::Request)?;
        validate_apple_url(response.url())?;
        if !response.status().is_success() {
            return Err(PodcastDirectoryError::Request);
        }
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take(MAX_RESPONSE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| PodcastDirectoryError::Request)?;
        if bytes.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(PodcastDirectoryError::ResponseTooLarge);
        }
        serde_json::from_slice(&bytes).map_err(|_| PodcastDirectoryError::InvalidResponse)
    }
}

fn ranked_podcast_ids(payload: &Value) -> Result<Vec<String>, PodcastDirectoryError> {
    let entries = payload
        .pointer("/feed/entry")
        .and_then(Value::as_array)
        .ok_or(PodcastDirectoryError::InvalidResponse)?;
    let mut ids = Vec::new();
    for entry in entries {
        let identifier = entry
            .pointer("/id/attributes/im:id")
            .and_then(Value::as_str)
            .and_then(numeric_identifier)
            .or_else(|| {
                entry
                    .pointer("/id/label")
                    .and_then(Value::as_str)
                    .and_then(identifier_from_url)
            });
        if let Some(identifier) = identifier
            && !ids.contains(&identifier)
        {
            ids.push(identifier);
        }
    }
    Ok(ids)
}

fn normalize_ranked_results(
    payload: &Value,
    ids: &[String],
) -> Result<Vec<PodcastDirectoryItem>, PodcastDirectoryError> {
    let entries = payload
        .get("results")
        .and_then(Value::as_array)
        .ok_or(PodcastDirectoryError::InvalidResponse)?;
    let mut ranked = entries
        .iter()
        .filter_map(|entry| {
            let rank = ["trackId", "collectionId"]
                .iter()
                .find_map(|key| entry.get(*key).and_then(value_identifier))
                .and_then(|identifier| ids.iter().position(|id| id == &identifier))?;
            Some((rank, normalize_result(entry)?))
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(rank, _)| *rank);
    Ok(ranked.into_iter().map(|(_, item)| item).collect())
}

fn value_identifier(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => numeric_identifier(&number.to_string()),
        Value::String(value) => numeric_identifier(value),
        _ => None,
    }
}

fn numeric_identifier(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())).then(|| value.to_owned())
}

fn identifier_from_url(value: &str) -> Option<String> {
    let marker = value.rfind("id")? + 2;
    let digits = value[marker..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    numeric_identifier(&digits)
}

fn normalize_results(payload: &Value) -> Result<Vec<PodcastDirectoryItem>, PodcastDirectoryError> {
    let entries = payload
        .get("results")
        .and_then(Value::as_array)
        .ok_or(PodcastDirectoryError::InvalidResponse)?;
    Ok(entries.iter().filter_map(normalize_result).collect())
}

fn normalize_result(entry: &Value) -> Option<PodcastDirectoryItem> {
    let feed_url = remote_url(text(entry, "feedUrl"))?;
    let webpage_url = remote_url(text(entry, "collectionViewUrl"))
        .or_else(|| remote_url(text(entry, "trackViewUrl")))
        .unwrap_or_else(|| feed_url.clone());
    let title =
        first_text(entry, &["collectionName", "trackName"]).unwrap_or_else(|| feed_url.to_string());
    Some(PodcastDirectoryItem {
        title,
        author: text(entry, "artistName").unwrap_or_default().to_owned(),
        genre: text(entry, "primaryGenreName")
            .unwrap_or_default()
            .to_owned(),
        episode_count: entry
            .get("trackCount")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        feed_url,
        webpage_url,
    })
}

fn first_text(entry: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| text(entry, key).map(str::to_owned))
}

fn text<'a>(entry: &'a Value, key: &str) -> Option<&'a str> {
    entry
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn remote_url(value: Option<&str>) -> Option<Url> {
    let url = Url::parse(value?).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}

fn validate_apple_url(url: &Url) -> Result<(), PodcastDirectoryError> {
    let host = url
        .host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase());
    if url.scheme() == "https"
        && host
            .as_deref()
            .is_some_and(|host| host == "apple.com" || host.ends_with(".apple.com"))
        && url.username().is_empty()
        && url.password().is_none()
    {
        Ok(())
    } else {
        Err(PodcastDirectoryError::UntrustedRedirect)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        PodcastDirectoryError, normalize_ranked_results, normalize_results, ranked_podcast_ids,
        validate_apple_url,
    };

    #[test]
    fn normalizes_only_results_with_remote_feed_urls() {
        let results = normalize_results(&json!({
            "results": [
                {
                    "collectionName": "The Show",
                    "artistName": "Presenter",
                    "feedUrl": "https://feeds.example/show.xml",
                    "collectionViewUrl": "https://podcasts.apple.com/show",
                    "primaryGenreName": "News",
                    "trackCount": 42
                },
                {"collectionName": "Missing Feed"},
                {"collectionName": "Local Feed", "feedUrl": "file:///tmp/feed.xml"}
            ]
        }))
        .expect("results");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "The Show");
        assert_eq!(results[0].author, "Presenter");
        assert_eq!(results[0].genre, "News");
        assert_eq!(results[0].episode_count, 42);
    }

    #[test]
    fn apple_redirect_validation_rejects_lookalikes_and_http() {
        validate_apple_url(&"https://itunes.apple.com/search".parse().expect("URL"))
            .expect("Apple");
        assert_eq!(
            validate_apple_url(&"https://apple.com.example.test/".parse().expect("URL"))
                .expect_err("lookalike"),
            PodcastDirectoryError::UntrustedRedirect
        );
        assert_eq!(
            validate_apple_url(&"http://itunes.apple.com/search".parse().expect("URL"))
                .expect_err("HTTP"),
            PodcastDirectoryError::UntrustedRedirect
        );
    }

    #[test]
    fn ranked_category_ids_and_lookup_results_preserve_apple_order() {
        let ids = ranked_podcast_ids(&json!({
            "feed": {"entry": [
                {"id": {"attributes": {"im:id": "22"}}},
                {"id": {"label": "https://podcasts.apple.com/show/id11"}},
                {"id": {"attributes": {"im:id": "22"}}}
            ]}
        }))
        .expect("ranking");
        assert_eq!(ids, ["22", "11"]);

        let results = normalize_ranked_results(
            &json!({"results": [
                {"trackId": 11, "collectionName": "Second", "feedUrl": "https://feeds.example/second"},
                {"trackId": 22, "collectionName": "First", "feedUrl": "https://feeds.example/first"}
            ]}),
            &ids,
        )
        .expect("lookup");
        assert_eq!(results[0].title, "First");
        assert_eq!(results[1].title, "Second");
    }
}
