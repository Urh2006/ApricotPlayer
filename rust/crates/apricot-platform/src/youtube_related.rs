//! Python `fetch_related_and_play_next`: downloads a `YouTube` watch page and
//! reads its related videos. Runs on a background thread.

use std::{io::Read, time::Duration};

use apricot_core::MediaItem;
use reqwest::{blocking::Client, redirect::Policy};
use thiserror::Error;
use url::Url;

/// Python `REMOTE_WEB_PAGE_MAX_BYTES`.
const MAX_PAGE_BYTES: u64 = 12_000_000;
/// Python sends a desktop browser user agent so the page carries its data.
const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Error, Eq, PartialEq)]
pub enum RelatedVideosError {
    #[error("the address is not a YouTube page")]
    UntrustedUrl,
    #[error("the YouTube page request failed")]
    Request,
    #[error("the YouTube page returned HTTP {0}")]
    Http(u16),
    #[error("the YouTube page exceeded the size limit")]
    TooLarge,
}

/// Downloads the watch page and returns its related videos in page order.
///
/// # Errors
///
/// Returns an error for non-`YouTube` addresses, failed or oversized requests.
pub fn fetch_related_videos(watch_url: &str) -> Result<Vec<MediaItem>, RelatedVideosError> {
    let url = Url::parse(watch_url.trim()).map_err(|_| RelatedVideosError::UntrustedUrl)?;
    validate_youtube_url(&url)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(Policy::limited(5))
        .user_agent(BROWSER_USER_AGENT)
        .build()
        .map_err(|_| RelatedVideosError::Request)?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|_| RelatedVideosError::Request)?;
    validate_youtube_url(response.url())?;
    if !response.status().is_success() {
        return Err(RelatedVideosError::Http(response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_PAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RelatedVideosError::Request)?;
    if bytes.len() as u64 > MAX_PAGE_BYTES {
        return Err(RelatedVideosError::TooLarge);
    }
    Ok(apricot_media::parse_related_videos(
        &String::from_utf8_lossy(&bytes),
    ))
}

/// Python `validate_trusted_https_url(url, {"youtube.com"})`.
fn validate_youtube_url(url: &Url) -> Result<(), RelatedVideosError> {
    let host = url
        .host_str()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && (host == "youtube.com" || host.ends_with(".youtube.com"))
    {
        Ok(())
    } else {
        Err(RelatedVideosError::UntrustedUrl)
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{RelatedVideosError, fetch_related_videos, validate_youtube_url};

    #[test]
    #[ignore = "needs network access to YouTube"]
    fn live_watch_page_has_related_videos() {
        let videos = fetch_related_videos("https://www.youtube.com/watch?v=jNQXAC9IVRw")
            .expect("watch page");
        assert!(!videos.is_empty());
        assert!(videos.iter().all(|video| video.youtube_video_id().is_some()));
    }

    #[test]
    fn only_https_youtube_pages_are_fetched() {
        for accepted in [
            "https://www.youtube.com/watch?v=abcdefghijk",
            "https://youtube.com/watch?v=abcdefghijk",
        ] {
            assert_eq!(
                validate_youtube_url(&Url::parse(accepted).expect("url")),
                Ok(())
            );
        }
        for rejected in [
            "http://www.youtube.com/watch?v=abcdefghijk",
            "https://notyoutube.com/watch?v=abcdefghijk",
            "https://youtube.com.example.org/watch",
        ] {
            assert_eq!(
                validate_youtube_url(&Url::parse(rejected).expect("url")),
                Err(RelatedVideosError::UntrustedUrl)
            );
        }
    }
}
