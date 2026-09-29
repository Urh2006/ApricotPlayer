//! Python `soundcloud_search_entries` for the Playlist and User types: the
//! `SoundCloud` web API with the public client ID of the `SoundCloud` web
//! player, which Python gets from the `yt-dlp` `SoundcloudSearch` extractor.
//! The standalone `yt-dlp` executable has no such listing, so Rust reads the
//! client ID from the soundcloud.com scripts the same way `yt-dlp` does.

use std::{
    io::Read,
    sync::{LazyLock, Mutex, PoisonError},
    time::Duration,
};

use regex::Regex;
use reqwest::{
    Proxy, StatusCode,
    blocking::{Client, Response},
    redirect::Policy,
};
use serde_json::Value;
use thiserror::Error;
use url::Url;

/// `yt-dlp` `SoundcloudBaseIE._API_V2_BASE`.
const API_V2_BASE: &str = "https://api-v2.soundcloud.com/";
const HOME_PAGE: &str = "https://soundcloud.com/";
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_PAGE_BYTES: u64 = 8_000_000;
const MAX_RESPONSE_BYTES: u64 = 16_000_000;
/// Python pages with `next_href` until it has `limit` entries; the bound only
/// protects against a service that keeps returning empty pages.
const MAX_PAGES: usize = 50;

/// The client ID stays for the whole run, like the `yt-dlp` cache, and is read
/// again when the API rejects it.
static CLIENT_ID: Mutex<Option<String>> = Mutex::new(None);

static SCRIPT_SOURCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<script[^>]+src="([^"]+)""#).expect("valid regex"));
static CLIENT_ID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"client_id\s*:\s*"([0-9a-zA-Z]{32})""#).expect("valid regex"));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SoundcloudCollectionSearch {
    Playlist,
    User,
}

impl SoundcloudCollectionSearch {
    const fn endpoint(self) -> &'static str {
        match self {
            Self::Playlist => "search/playlists_without_albums",
            Self::User => "search/users",
        }
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SoundcloudSearchError {
    #[error("invalid SoundCloud proxy")]
    InvalidProxy,
    #[error("SoundCloud request failed")]
    Request,
    #[error("SoundCloud returned HTTP {0}")]
    Http(u16),
    #[error("SoundCloud response exceeded the size limit")]
    TooLarge,
    #[error("SoundCloud returned invalid data")]
    InvalidData,
    #[error("Unable to extract SoundCloud client id")]
    ClientId,
}

/// Returns the raw API entries of a `SoundCloud` playlist or user search.
///
/// # Errors
///
/// Returns an error when the client ID cannot be read or a request fails.
pub fn search_soundcloud_collections(
    query: &str,
    kind: SoundcloudCollectionSearch,
    limit: u32,
    proxy_url: Option<&str>,
) -> Result<Vec<Value>, SoundcloudSearchError> {
    let client = http_client(proxy_url)?;
    let limit = limit.max(1) as usize;
    let mut next = Some(first_page_url(query, kind, limit)?);
    let mut collected = Vec::new();
    for _ in 0..MAX_PAGES {
        let Some(url) = next.take() else { break };
        if collected.len() >= limit {
            break;
        }
        let page = call_api(&client, &url)?;
        collected.extend(
            page.get("collection")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|item| item.is_object())
                .cloned(),
        );
        next = page
            .get("next_href")
            .and_then(Value::as_str)
            .filter(|href| !href.is_empty())
            .and_then(|href| Url::parse(href).ok())
            .filter(is_api_url);
    }
    collected.truncate(limit);
    Ok(collected)
}

fn http_client(proxy_url: Option<&str>) -> Result<Client, SoundcloudSearchError> {
    let mut builder = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(Policy::limited(5))
        .user_agent(BROWSER_USER_AGENT);
    if let Some(proxy) = proxy_url.filter(|proxy| !proxy.trim().is_empty()) {
        builder = builder
            .proxy(Proxy::all(proxy.trim()).map_err(|_| SoundcloudSearchError::InvalidProxy)?);
    }
    builder.build().map_err(|_| SoundcloudSearchError::Request)
}

fn first_page_url(
    query: &str,
    kind: SoundcloudCollectionSearch,
    limit: usize,
) -> Result<Url, SoundcloudSearchError> {
    let mut url = Url::parse(API_V2_BASE)
        .and_then(|base| base.join(kind.endpoint()))
        .map_err(|_| SoundcloudSearchError::Request)?;
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("limit", &limit.clamp(1, 200).to_string())
        .append_pair("linked_partitioning", "1")
        .append_pair("offset", "0");
    Ok(url)
}

/// `yt-dlp` `SoundcloudBaseIE._call_api`: adds the client ID and reads a new
/// one once when the API answers 401 or 403.
fn call_api(client: &Client, url: &Url) -> Result<Value, SoundcloudSearchError> {
    for attempt in 0..2 {
        let client_id = match cached_client_id() {
            Some(client_id) => client_id,
            None => refresh_client_id(client)?,
        };
        let response = client
            .get(with_client_id(url, &client_id))
            .send()
            .map_err(|_| SoundcloudSearchError::Request)?;
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) && attempt == 0 {
            store_client_id(None);
            continue;
        }
        if !status.is_success() {
            return Err(SoundcloudSearchError::Http(status.as_u16()));
        }
        let body = read_limited(response, MAX_RESPONSE_BYTES)?;
        return serde_json::from_slice(&body).map_err(|_| SoundcloudSearchError::InvalidData);
    }
    Err(SoundcloudSearchError::Http(
        StatusCode::UNAUTHORIZED.as_u16(),
    ))
}

fn with_client_id(url: &Url, client_id: &str) -> Url {
    let pairs = url
        .query_pairs()
        .filter(|(key, _)| key != "client_id")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let mut url = url.clone();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(pairs)
        .append_pair("client_id", client_id);
    url
}

fn cached_client_id() -> Option<String> {
    CLIENT_ID
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn store_client_id(client_id: Option<String>) {
    *CLIENT_ID.lock().unwrap_or_else(PoisonError::into_inner) = client_id;
}

/// `yt-dlp` `SoundcloudBaseIE._update_client_id`: the last page script that
/// names a client ID wins.
fn refresh_client_id(client: &Client) -> Result<String, SoundcloudSearchError> {
    let home = Url::parse(HOME_PAGE).map_err(|_| SoundcloudSearchError::Request)?;
    let page = fetch_text(client, &home)?;
    for script_url in script_sources(&page, &home).into_iter().rev() {
        let Ok(script) = fetch_text(client, &script_url) else {
            continue;
        };
        if let Some(client_id) = client_id_from_script(&script) {
            store_client_id(Some(client_id.clone()));
            return Ok(client_id);
        }
    }
    Err(SoundcloudSearchError::ClientId)
}

fn fetch_text(client: &Client, url: &Url) -> Result<String, SoundcloudSearchError> {
    let response = client
        .get(url.clone())
        .send()
        .map_err(|_| SoundcloudSearchError::Request)?;
    if !response.status().is_success() {
        return Err(SoundcloudSearchError::Http(response.status().as_u16()));
    }
    let body = read_limited(response, MAX_PAGE_BYTES)?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

fn read_limited(response: Response, limit: u64) -> Result<Vec<u8>, SoundcloudSearchError> {
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SoundcloudSearchError::Request)?;
    if bytes.len() as u64 > limit {
        return Err(SoundcloudSearchError::TooLarge);
    }
    Ok(bytes)
}

/// Script addresses of the home page, restricted to `SoundCloud` hosts.
fn script_sources(page: &str, base: &Url) -> Vec<Url> {
    SCRIPT_SOURCE
        .captures_iter(page)
        .filter_map(|capture| base.join(capture.get(1)?.as_str()).ok())
        .filter(is_soundcloud_script_url)
        .collect()
}

fn client_id_from_script(script: &str) -> Option<String> {
    CLIENT_ID_PATTERN
        .captures(script)
        .and_then(|capture| capture.get(1))
        .map(|client_id| client_id.as_str().to_owned())
}

fn trusted_host(url: &Url, domains: &[&str]) -> bool {
    let host = url
        .host_str()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && domains
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

fn is_soundcloud_script_url(url: &Url) -> bool {
    trusted_host(url, &["soundcloud.com", "sndcdn.com"])
}

fn is_api_url(url: &Url) -> bool {
    trusted_host(url, &["api-v2.soundcloud.com"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_page_matches_python_query() {
        let url =
            first_page_url("daft punk", SoundcloudCollectionSearch::Playlist, 500).expect("url");
        assert_eq!(
            url.as_str(),
            "https://api-v2.soundcloud.com/search/playlists_without_albums?q=daft+punk&limit=200&linked_partitioning=1&offset=0"
        );
        let users = first_page_url("x", SoundcloudCollectionSearch::User, 20).expect("url");
        assert_eq!(
            users.as_str(),
            "https://api-v2.soundcloud.com/search/users?q=x&limit=20&linked_partitioning=1&offset=0"
        );
    }

    #[test]
    fn client_id_replaces_an_existing_one_and_keeps_other_parameters() {
        let next = Url::parse(
            "https://api-v2.soundcloud.com/search/users?query_urn=a%3Ab&offset=20&client_id=old",
        )
        .expect("url");
        assert_eq!(
            with_client_id(&next, "new").as_str(),
            "https://api-v2.soundcloud.com/search/users?query_urn=a%3Ab&offset=20&client_id=new"
        );
    }

    #[test]
    fn client_id_is_read_like_ytdlp_from_trusted_scripts_only() {
        let base = Url::parse(HOME_PAGE).expect("url");
        let page = r#"<script crossorigin src="https://a-v2.sndcdn.com/assets/0-a.js"></script>
            <script src="https://evil.example/x.js"></script>
            <script crossorigin src="https://a-v2.sndcdn.com/assets/50-b.js"></script>"#;
        let sources = script_sources(page, &base);
        assert_eq!(
            sources.iter().map(Url::as_str).collect::<Vec<_>>(),
            [
                "https://a-v2.sndcdn.com/assets/0-a.js",
                "https://a-v2.sndcdn.com/assets/50-b.js"
            ]
        );
        assert_eq!(
            client_id_from_script(r#"x={client_id:"abcdefghijABCDEFGHIJ0123456789ab",env:1}"#)
                .as_deref(),
            Some("abcdefghijABCDEFGHIJ0123456789ab")
        );
        assert_eq!(client_id_from_script(r#"client_id:"short""#), None);
    }

    #[test]
    fn next_pages_stay_on_the_api_host() {
        assert!(is_api_url(
            &Url::parse("https://api-v2.soundcloud.com/search/users?offset=20").expect("url")
        ));
        assert!(!is_api_url(
            &Url::parse("https://soundcloud.com.evil.example/search").expect("url")
        ));
        assert!(!is_api_url(
            &Url::parse("http://api-v2.soundcloud.com/search").expect("url")
        ));
    }

    #[test]
    #[ignore = "uses the network"]
    fn live_playlist_and_user_search() {
        for kind in [
            SoundcloudCollectionSearch::Playlist,
            SoundcloudCollectionSearch::User,
        ] {
            let items = search_soundcloud_collections("daft punk", kind, 25, None).expect("search");
            assert_eq!(items.len(), 25, "{kind:?}");
            assert!(items.iter().all(|item| item.get("permalink_url").is_some()));
        }
    }
}
