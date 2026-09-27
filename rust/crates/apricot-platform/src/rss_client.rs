//! Bounded network retrieval for RSS and Atom podcast feeds.

use std::{io::Read, time::Duration};

use apricot_media::{PodcastFeedDocument, PodcastFeedParseError, decode_xml, parse_podcast_feed};
use reqwest::{Proxy, blocking::Client, redirect::Policy};
use thiserror::Error;
use url::Url;

const MAX_FEED_BYTES: u64 = 3_000_000;
const MAX_CHAPTER_BYTES: u64 = 1_000_000;

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
    #[error("podcast chapter response exceeded the 1 MB safety limit")]
    ChaptersTooLarge,
    #[error("podcast chapter response is not valid JSON")]
    InvalidChapters,
    #[error(transparent)]
    InvalidFeed(#[from] PodcastFeedParseError),
}

#[derive(Clone)]
pub struct RssClient {
    client: Client,
}

impl RssClient {
    /// Fetches Podcasting 2.0 chapter JSON without blocking the UI thread.
    /// The caller must run this blocking operation on its background worker.
    ///
    /// # Errors
    /// Rejects invalid URLs, failed requests, oversized bodies, and invalid JSON.
    pub fn fetch_chapters(&self, source: &str) -> Result<Vec<serde_json::Value>, RssClientError> {
        let source = parse_remote_url(source)?;
        let mut response = self
            .client
            .get(source)
            .send()
            .map_err(|_| RssClientError::Request)?;
        validate_remote_url(response.url())?;
        if !response.status().is_success() {
            return Err(RssClientError::Http(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_CHAPTER_BYTES)
        {
            return Err(RssClientError::ChaptersTooLarge);
        }
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take(MAX_CHAPTER_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| RssClientError::Request)?;
        if bytes.len() as u64 > MAX_CHAPTER_BYTES {
            return Err(RssClientError::ChaptersTooLarge);
        }
        parse_chapter_document(&bytes)
    }

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

fn parse_chapter_document(bytes: &[u8]) -> Result<Vec<serde_json::Value>, RssClientError> {
    let document: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| RssClientError::InvalidChapters)?;
    let array = document
        .as_array()
        .or_else(|| {
            document
                .get("chapters")
                .and_then(serde_json::Value::as_array)
        })
        .filter(|array| !array.is_empty())
        .or_else(|| document.get("items").and_then(serde_json::Value::as_array));
    Ok(array.cloned().unwrap_or_default())
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

    fn chapter_response(
        status: u16,
        body: Vec<u8>,
    ) -> Result<Vec<serde_json::Value>, RssClientError> {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::{Duration, Instant},
        };

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/chapters", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                assert!(Instant::now() < deadline, "chapter client did not connect");
                std::thread::sleep(Duration::from_millis(5));
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(30)))
                .unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).unwrap() > 0);
            write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            // The bounded reader can disconnect before an oversized body finishes.
            let _ = stream.write_all(&body);
        });
        let client = super::RssClient {
            client: reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
        };
        let result = client.fetch_chapters(&url);
        server.join().unwrap();
        result
    }

    #[test]
    fn chapter_http_fetch_checks_status_json_and_declared_body_size() {
        let chapters = chapter_response(
            200,
            br#"{"chapters":[{"startTime":0,"title":"Opening"}]}"#.to_vec(),
        )
        .unwrap();
        assert_eq!(chapters[0]["title"], "Opening");
        assert_eq!(
            chapter_response(403, b"Forbidden".to_vec()),
            Err(RssClientError::Http(403))
        );
        assert_eq!(
            chapter_response(200, b"<html>login</html>".to_vec()),
            Err(RssClientError::InvalidChapters)
        );
        assert_eq!(
            chapter_response(200, vec![b' '; 1_000_001]),
            Err(RssClientError::ChaptersTooLarge)
        );
    }

    #[test]
    fn accepts_podcast_chapter_document_shapes() {
        for bytes in [
            br#"[{"startTime":0,"title":"Intro"}]"#.as_slice(),
            br#"{"chapters":[{"startTime":0,"title":"Intro"}]}"#,
            br#"{"items":[{"startTime":0,"title":"Intro"}]}"#,
        ] {
            let result = super::parse_chapter_document(bytes).expect("chapter document");
            assert_eq!(result.len(), 1);
            assert_eq!(result[0]["title"], "Intro");
        }
        assert!(super::parse_chapter_document(b"[]").unwrap().is_empty());
        assert_eq!(
            super::parse_chapter_document(b"<html>login</html>"),
            Err(RssClientError::InvalidChapters)
        );
    }

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
