//! Bounded network retrieval for RSS and Atom podcast feeds.

use std::{io::Read, time::Duration};

use apricot_media::{PodcastFeedDocument, PodcastFeedParseError, decode_xml, parse_podcast_feed};
use reqwest::{Proxy, blocking::Client, redirect::Policy};
use thiserror::Error;
use url::Url;

const MAX_FEED_BYTES: u64 = 3_000_000;

#[derive(Debug, Error, Eq, PartialEq)]
pub enum RssClientError {
    #[error("RSS feed URL must use HTTP or HTTPS")]
    InvalidUrl,
    #[error("the configured proxy could not be used for RSS requests")]
    InvalidProxy,
    #[error("RSS feed request failed")]
    Request,
    #[error("RSS feed server returned HTTP {0}")]
    Http(u16),
    #[error("RSS feed response exceeded the 3 MB safety limit")]
    ResponseTooLarge,
    #[error("RSS feed uses an unsupported or malformed text encoding")]
    InvalidEncoding,
    #[error(transparent)]
    InvalidFeed(#[from] PodcastFeedParseError),
}

#[derive(Clone)]
pub struct RssClient {
    client: Client,
}

impl RssClient {
    /// Creates an RSS client with bounded redirects, timeouts, and optional proxy use.
    ///
    /// # Errors
    ///
    /// Returns an error when the proxy or TLS client cannot be initialized.
    pub fn new(proxy_url: Option<&str>) -> Result<Self, RssClientError> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .redirect(Policy::limited(5))
            .user_agent("ApricotPlayer/2.0");
        if let Some(proxy_url) = proxy_url.map(str::trim).filter(|value| !value.is_empty()) {
            builder =
                builder.proxy(Proxy::all(proxy_url).map_err(|_| RssClientError::InvalidProxy)?);
        }
        let client = builder.build().map_err(|_| RssClientError::Request)?;
        Ok(Self { client })
    }

    /// Downloads and parses one complete podcast feed.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid URLs, request failures, non-success HTTP
    /// status, oversized responses, invalid encodings, or malformed feeds.
    pub fn fetch(&self, source: &str) -> Result<PodcastFeedDocument, RssClientError> {
        let source = parse_remote_url(source)?;
        let mut response = self
            .client
            .get(source)
            .send()
            .map_err(|_| RssClientError::Request)?;
        let final_url = response.url().clone();
        validate_remote_url(&final_url)?;
        if !response.status().is_success() {
            return Err(RssClientError::Http(response.status().as_u16()));
        }
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take(MAX_FEED_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| RssClientError::Request)?;
        if bytes.len() as u64 > MAX_FEED_BYTES {
            return Err(RssClientError::ResponseTooLarge);
        }
        let xml = decode_xml(&bytes).map_err(|_| RssClientError::InvalidEncoding)?;
        parse_podcast_feed(&xml, &final_url).map_err(Into::into)
    }
}

fn parse_remote_url(value: &str) -> Result<Url, RssClientError> {
    let url = Url::parse(value.trim()).map_err(|_| RssClientError::InvalidUrl)?;
    validate_remote_url(&url)?;
    Ok(url)
}

fn validate_remote_url(url: &Url) -> Result<(), RssClientError> {
    if matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
    {
        Ok(())
    } else {
        Err(RssClientError::InvalidUrl)
    }
}

#[cfg(test)]
mod tests {
    use apricot_media::{XmlTextError, decode_xml};

    use super::{RssClientError, parse_remote_url};

    #[test]
    fn rejects_local_credentialed_and_non_http_urls() {
        assert_eq!(
            parse_remote_url("file:///tmp/feed.xml").expect_err("file"),
            RssClientError::InvalidUrl
        );
        assert_eq!(
            parse_remote_url("https://user:secret@example.com/feed").expect_err("credentials"),
            RssClientError::InvalidUrl
        );
    }

    #[test]
    fn decodes_utf_boms_and_declared_legacy_encodings_without_replacement() {
        assert_eq!(
            decode_xml(&[0xEF, 0xBB, 0xBF, b'<', b'r', b's', b's', b'/', b'>']).expect("UTF-8 BOM"),
            "<rss/>"
        );
        let utf16 = "<?xml version=\"1.0\"?><rss/>"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        assert_eq!(
            decode_xml(&utf16).expect("UTF-16"),
            "<?xml version=\"1.0\"?><rss/>"
        );
        let windows_1252 = b"<?xml version='1.0' encoding='windows-1252'?><rss>\x80</rss>";
        assert!(
            decode_xml(windows_1252)
                .expect("Windows-1252")
                .contains('\u{20ac}')
        );
        assert_eq!(
            decode_xml(b"<?xml version='1.0'?><rss>\xff</rss>").expect_err("bad UTF-8"),
            XmlTextError::InvalidEncoding
        );
    }
}
