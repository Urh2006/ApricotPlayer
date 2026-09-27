//! Bounded sidecar lyrics loading. Call on a worker for remote/local slow disks.

use std::{fs::File, io::Read, path::Path};

const MAX_LOCAL_LYRICS_BYTES: u64 = 512_000;
const MAX_ONLINE_LYRICS_BYTES: u64 = 2_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LyricsSource {
    Local,
    Online,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FetchedLyrics {
    pub text: String,
    pub source: LyricsSource,
}

/// Resolves local sidecars before the optional online provider on a worker.
///
/// # Errors
/// Returns the online provider's error if enabled and no local text was found.
pub fn fetch_lyrics(
    local_path: Option<&Path>,
    query: &LyricsQuery,
    online_enabled: bool,
    proxy: Option<&str>,
) -> Result<Option<FetchedLyrics>, LyricsError> {
    fetch_with(local_path, online_enabled, || online_lyrics(query, proxy))
}

fn fetch_with(
    local_path: Option<&Path>,
    online_enabled: bool,
    online: impl FnOnce() -> Result<String, LyricsError>,
) -> Result<Option<FetchedLyrics>, LyricsError> {
    if let Some(path) = local_path {
        let text = local_lyrics(path);
        if !text.is_empty() {
            return Ok(Some(FetchedLyrics {
                text,
                source: LyricsSource::Local,
            }));
        }
    }
    if !online_enabled {
        return Ok(None);
    }
    let text = online()?;
    Ok((!text.is_empty()).then_some(FetchedLyrics {
        text,
        source: LyricsSource::Online,
    }))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LyricsQuery {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_seconds: u64,
}

impl LyricsQuery {
    /// Builds lookup metadata using the Python title cleanup rules.
    ///
    /// # Panics
    /// Panics only if the hard-coded, regression-tested regex is invalid.
    #[must_use]
    pub fn from_item(item: &apricot_core::MediaItem) -> Self {
        static DECORATION: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let decoration = DECORATION.get_or_init(|| regex::Regex::new(
            r"(?i)\s*[\(\[]\s*(official\s+)?(music\s+video|video|lyrics?|lyric\s+video|audio|visualizer|remaster(?:ed)?)\s*[\)\]]\s*"
        ).expect("static lyrics title pattern"));
        let metadata = |key: &str| {
            item.metadata
                .get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|text| !text.is_empty())
        };
        let title = metadata("track").unwrap_or(&item.title).trim();
        let mut artist = metadata("artist")
            .or_else(|| metadata("creator"))
            .unwrap_or_default()
            .trim()
            .to_owned();
        let title = if artist.is_empty()
            && let Some((left, right)) = title.split_once(" - ")
        {
            left.trim().clone_into(&mut artist);
            right.trim()
        } else {
            title
        };
        let title = decoration
            .replace_all(title, " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim_matches([' ', '-'])
            .to_owned();
        if artist.is_empty() {
            item.channel.trim().clone_into(&mut artist);
        }
        Self {
            title,
            artist,
            album: metadata("album").unwrap_or_default().trim().to_owned(),
            duration_seconds: duration_for_query(item.duration_seconds),
        }
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn duration_for_query(duration: Option<f64>) -> u64 {
    // Rust's saturating float conversion is safe after excluding invalid values.
    duration
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or_default() as u64
}

#[derive(Debug, thiserror::Error)]
pub enum LyricsError {
    #[error("lyrics request failed")]
    Request,
    #[error("lyrics response exceeded the 2 MB safety limit")]
    TooLarge,
    #[error("lyrics response was not valid JSON")]
    InvalidResponse,
}

fn trusted_lyrics_url(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("lrclib.net")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
}

fn query_url(query: &LyricsQuery) -> url::Url {
    let mut url = url::Url::parse("https://lrclib.net/api/get").expect("static lyrics endpoint");
    {
        let mut params = url.query_pairs_mut();
        params.append_pair("track_name", &query.title);
        for (key, value) in [("artist_name", &query.artist), ("album_name", &query.album)] {
            if !value.is_empty() {
                params.append_pair(key, value);
            }
        }
        if query.duration_seconds != 0 {
            params.append_pair("duration", &query.duration_seconds.to_string());
        }
    }
    url
}

/// Blocking worker operation; never invoke on the native UI thread.
///
/// # Errors
/// Reports transport errors, oversized responses, or malformed JSON.
pub fn online_lyrics(query: &LyricsQuery, proxy: Option<&str>) -> Result<String, LyricsError> {
    if query.title.trim().is_empty() {
        return Ok(String::new());
    }
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent("ApricotPlayer/2.0")
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !trusted_lyrics_url(attempt.url()) {
                attempt.error("untrusted lyrics redirect")
            } else {
                attempt.follow()
            }
        }));
    if let Some(proxy) = proxy.map(str::trim).filter(|proxy| !proxy.is_empty()) {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| LyricsError::Request)?);
    }
    let mut response = builder
        .build()
        .map_err(|_| LyricsError::Request)?
        .get(query_url(query))
        .send()
        .map_err(|_| LyricsError::Request)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(String::new());
    }
    if !response.status().is_success() || !trusted_lyrics_url(response.url()) {
        return Err(LyricsError::Request);
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_ONLINE_LYRICS_BYTES)
    {
        return Err(LyricsError::TooLarge);
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_ONLINE_LYRICS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| LyricsError::Request)?;
    if bytes.len() as u64 > MAX_ONLINE_LYRICS_BYTES {
        return Err(LyricsError::TooLarge);
    }
    lyrics_response(&bytes)
}

fn lyrics_response(bytes: &[u8]) -> Result<String, LyricsError> {
    let payload: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(bytes))
        .map_err(|_| LyricsError::InvalidResponse)?;
    Ok(["syncedLyrics", "plainLyrics"]
        .iter()
        .find_map(|key| {
            payload
                .get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|text| !text.is_empty())
        })
        .unwrap_or_default()
        .trim()
        .to_owned())
}

/// Uses the same first-readable-sidecar ordering as the Python player.
#[must_use]
pub fn local_lyrics(media: &Path) -> String {
    let mut lyrics_name = media.file_stem().unwrap_or_default().to_os_string();
    lyrics_name.push(".lyrics.txt");
    for path in [
        media.with_extension("lrc"),
        media.with_extension("txt"),
        media.with_file_name(lyrics_name),
    ] {
        let Ok(file) = File::open(path) else { continue };
        let Ok(metadata) = file.metadata() else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_LOCAL_LYRICS_BYTES {
            continue;
        }
        // Bound the actual read too: a sidecar can grow after metadata was read.
        let mut bytes = Vec::new();
        if file
            .take(MAX_LOCAL_LYRICS_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() as u64 > MAX_LOCAL_LYRICS_BYTES
        {
            continue;
        }
        return String::from_utf8_lossy(&bytes).trim().to_owned();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "opt-in live LRCLIB request; requires network and service availability"]
    fn live_lrclib_returns_displayable_lyrics() {
        let text = online_lyrics(
            &LyricsQuery {
                title: "Never Gonna Give You Up".into(),
                artist: "Rick Astley".into(),
                ..Default::default()
            },
            None,
        )
        .expect("live lyrics request");
        assert!(!text.trim().is_empty(), "provider returned no lyrics");
        let document = apricot_media::lyrics::LyricsDocument::parse(&text, "Online");
        assert!(document.text.lines().count() > 2, "displayable lyric lines");
    }

    #[test]
    fn local_text_and_disabled_online_setting_never_contact_provider() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("song.mp3");
        std::fs::write(media.with_extension("lrc"), "local").unwrap();
        let result = fetch_with(Some(&media), true, || panic!("unexpected online request"))
            .unwrap()
            .unwrap();
        assert_eq!(result.source, LyricsSource::Local);
        assert_eq!(result.text, "local");
        assert_eq!(
            fetch_with(None, false, || panic!("online disabled")).unwrap(),
            None
        );
    }

    #[test]
    fn missing_local_text_uses_online_only_when_enabled() {
        let result = fetch_with(None, true, || Ok("online".into()))
            .unwrap()
            .unwrap();
        assert_eq!(result.source, LyricsSource::Online);
        assert_eq!(fetch_with(None, true, || Ok(String::new())).unwrap(), None);
        assert!(fetch_with(None, true, || Err(LyricsError::Request)).is_err());
    }

    #[test]
    fn search_terms_follow_track_artist_and_title_cleanup_precedence() {
        let mut item =
            apricot_core::MediaItem::from_direct_link("https://example.com/song").unwrap();
        item.title = "Artist - Song (Official Music Video) [Lyrics]".into();
        item.channel = "Uploader".into();
        item.duration_seconds = Some(123.9);
        let query = LyricsQuery::from_item(&item);
        assert_eq!(query.artist, "Artist");
        assert_eq!(query.title, "Song");
        assert_eq!(query.duration_seconds, 123);
        item.metadata
            .insert("track".into(), " Real Song [Remastered] ".into());
        item.metadata
            .insert("artist".into(), " Actual Artist ".into());
        item.metadata.insert("album".into(), " Album ".into());
        let query = LyricsQuery::from_item(&item);
        assert_eq!(query.title, "Real Song");
        assert_eq!(query.artist, "Actual Artist");
        assert_eq!(query.album, "Album");
    }

    #[test]
    fn title_cleanup_preserves_meaningful_bracketed_text() {
        let mut item =
            apricot_core::MediaItem::from_direct_link("https://example.com/song").unwrap();
        item.title = "Song (Live in London)".into();
        item.channel = "Band".into();
        item.duration_seconds = Some(f64::INFINITY);
        let query = LyricsQuery::from_item(&item);
        assert_eq!(query.title, item.title);
        assert_eq!(query.artist, "Band");
        assert_eq!(query.duration_seconds, 0);
    }

    #[test]
    fn online_response_prefers_synced_and_falls_back_to_plain() {
        assert_eq!(
            lyrics_response(br#"{"syncedLyrics":"[00:01.00] timed", "plainLyrics":"plain"}"#)
                .unwrap(),
            "[00:01.00] timed"
        );
        assert_eq!(
            lyrics_response(br#"{"syncedLyrics":null, "plainLyrics":" plain "}"#).unwrap(),
            "plain"
        );
        assert_eq!(lyrics_response(br#"{"instrumental":true}"#).unwrap(), "");
        assert!(lyrics_response(b"<html>error</html>").is_err());
    }

    #[test]
    fn query_encodes_metadata_and_redirects_cannot_leave_trusted_https_host() {
        let url = query_url(&LyricsQuery {
            title: "A & B?".into(),
            duration_seconds: 120,
            ..Default::default()
        });
        let pairs: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs["track_name"], "A & B?");
        assert_eq!(pairs["duration"], "120");
        assert!(!pairs.contains_key("artist_name"));
        assert!(trusted_lyrics_url(&url));
        for url in [
            "http://lrclib.net/api/get",
            "https://lrclib.net.evil.test/",
            "https://user@lrclib.net/",
            "https://lrclib.net:444/",
        ] {
            assert!(!trusted_lyrics_url(&url::Url::parse(url).unwrap()));
        }
    }

    #[test]
    fn prefers_lrc_then_text_then_named_lyrics() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("song.mp3");
        assert_eq!(local_lyrics(&media), "");
        std::fs::write(dir.path().join("song.lyrics.txt"), "named").unwrap();
        assert_eq!(local_lyrics(&media), "named");
        std::fs::write(dir.path().join("song.txt"), "plain").unwrap();
        assert_eq!(local_lyrics(&media), "plain");
        std::fs::write(dir.path().join("song.lrc"), " [00:00.00] timed\n").unwrap();
        assert_eq!(local_lyrics(&media), "[00:00.00] timed");
    }

    #[test]
    fn skips_directories_and_oversize_files_and_replaces_invalid_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("song.flac");
        std::fs::create_dir(dir.path().join("song.lrc")).unwrap();
        std::fs::write(dir.path().join("song.txt"), vec![b'x'; 512_001]).unwrap();
        std::fs::write(dir.path().join("song.lyrics.txt"), b"line \xff").unwrap();
        assert_eq!(local_lyrics(&media), "line \u{fffd}");
    }

    #[test]
    fn readable_empty_preferred_sidecar_retains_python_fallback_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let media = dir.path().join("song.wav");
        std::fs::write(dir.path().join("song.lrc"), " \n").unwrap();
        std::fs::write(dir.path().join("song.txt"), "not selected").unwrap();
        assert_eq!(local_lyrics(&media), "");
    }
}
