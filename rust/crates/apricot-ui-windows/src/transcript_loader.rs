//! Worker orchestration shared by the native transcript dialog and its tests.

use apricot_app::transcript::{
    TranscriptEntry, TranscriptSource, language_candidates, local_transcript, parse_transcript,
    select_track,
};
use apricot_core::MediaItem;
use apricot_media::{YoutubeCommand, YoutubeEngine, YoutubeSessionConfig};
use apricot_platform::{
    YtDlpYoutubeEngine,
    transcript::{TranscriptError, fetch_text},
};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct LoadedTranscript {
    pub entries: Vec<TranscriptEntry>,
    pub source_key: &'static str,
}

/// Python raises either `transcript_rate_limited` or the provider error,
/// shown through `transcript_failed` as `friendly_error(exc)`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TranscriptFailure {
    RateLimited,
    Failed(String),
}

impl TranscriptFailure {
    fn from_message(message: &str) -> Self {
        let lowered = message.to_lowercase();
        if lowered.contains("http error 429") || lowered.contains("too many requests") {
            Self::RateLimited
        } else {
            Self::Failed(message.to_owned())
        }
    }
}

/// Runs only on a worker; never adds extraction to the normal playback path.
///
/// # Errors
/// Returns the provider failure, distinguishing rate limiting like Python.
pub fn load(
    item: &MediaItem,
    executable: &Path,
    config: YoutubeSessionConfig,
    languages: &str,
    user_agent: &str,
) -> Result<LoadedTranscript, TranscriptFailure> {
    if let Some(path) = item.local_path.as_deref() {
        let entries = local_transcript(Path::new(path), languages);
        return Ok(LoadedTranscript {
            entries,
            source_key: "transcript_source_local",
        });
    }
    let Some(source) = source_url(item) else {
        return Ok(LoadedTranscript {
            entries: Vec::new(),
            source_key: "",
        });
    };
    let proxy = config.proxy_url.clone();
    let mut engine = YtDlpYoutubeEngine::new(executable)
        .map_err(|error| TranscriptFailure::from_message(&error.to_string()))?;
    engine
        .execute(YoutubeCommand::Configure { config })
        .map_err(|error| TranscriptFailure::from_message(&error.to_string()))?;
    let candidates = language_candidates(languages);
    let info = engine
        .transcript_metadata(&source, &candidates)
        .map_err(|error| TranscriptFailure::from_message(&error.to_string()))?;
    let Some((track, kind)) = select_track(&info, languages) else {
        return Ok(LoadedTranscript {
            entries: Vec::new(),
            source_key: "",
        });
    };
    let text = direct_or_fallback(
        || fetch_text(track, &info, &source, user_agent, proxy.as_deref()),
        || {
            engine
                .transcript_fallback(&source, &candidates)
                .map_err(|error| TranscriptFailure::from_message(&error.to_string()))
        },
    )?;
    Ok(LoadedTranscript {
        entries: parse_transcript(&text),
        source_key: match kind {
            TranscriptSource::Subtitles => "transcript_source_subtitles",
            TranscriptSource::AutomaticCaptions => "transcript_source_auto_captions",
        },
    })
}

/// Python `fetch_transcript_entries`: the yt-dlp download is the fallback,
/// and an empty fallback re-raises the direct error.
fn direct_or_fallback(
    direct: impl FnOnce() -> Result<String, TranscriptError>,
    fallback: impl FnOnce() -> Result<String, TranscriptFailure>,
) -> Result<String, TranscriptFailure> {
    match direct() {
        Ok(text) => Ok(text),
        Err(error) => match fallback() {
            Ok(text) if text.is_empty() => Err(if matches!(error, TranscriptError::RateLimited) {
                TranscriptFailure::RateLimited
            } else {
                TranscriptFailure::Failed(error.to_string())
            }),
            result => result,
        },
    }
}

fn source_url(item: &MediaItem) -> Option<String> {
    for candidate in ["webpage_url", "original_url", "watch_url"]
        .iter()
        .filter_map(|key| item.metadata.get(*key).and_then(serde_json::Value::as_str))
        .chain(item.url.as_ref().map(url::Url::as_str))
    {
        let Ok(url) = url::Url::parse(candidate.trim()) else {
            continue;
        };
        let host = url.host_str().unwrap_or_default();
        if matches!(url.scheme(), "http" | "https")
            && !host.is_empty()
            && host != "googlevideo.com"
            && !host.ends_with(".googlevideo.com")
        {
            return Some(url.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn successful_direct_fetch_never_runs_fallback() {
        assert_eq!(
            direct_or_fallback(
                || Ok("WEBVTT".to_owned()),
                || panic!("fallback must not run")
            )
            .unwrap(),
            "WEBVTT"
        );
    }
    #[test]
    fn direct_failure_uses_fallback_and_preserves_rate_limit() {
        assert_eq!(
            direct_or_fallback(|| Err(TranscriptError::Request), || Ok("SRT".to_owned())).unwrap(),
            "SRT"
        );
        assert_eq!(
            direct_or_fallback(|| Err(TranscriptError::RateLimited), || Ok(String::new())),
            Err(TranscriptFailure::RateLimited)
        );
        assert_eq!(
            direct_or_fallback(
                || Err(TranscriptError::Request),
                || Err(TranscriptFailure::RateLimited)
            ),
            Err(TranscriptFailure::RateLimited)
        );
        assert_eq!(
            direct_or_fallback(|| Err(TranscriptError::Request), || Ok(String::new())),
            Err(TranscriptFailure::Failed(
                TranscriptError::Request.to_string()
            ))
        );
    }

    #[test]
    fn provider_errors_keep_their_text_unless_rate_limited() {
        assert_eq!(
            TranscriptFailure::from_message("ERROR: HTTP Error 429: Too Many Requests"),
            TranscriptFailure::RateLimited
        );
        assert_eq!(
            TranscriptFailure::from_message("ERROR: Video unavailable"),
            TranscriptFailure::Failed("ERROR: Video unavailable".to_owned())
        );
    }
    #[test]
    fn source_uses_durable_url_not_resolved_stream() {
        let mut item = MediaItem::from_direct_link("https://example.test/watch").unwrap();
        item.stream_url = Some(url::Url::parse("https://cdn.googlevideo.com/expired").unwrap());
        assert_eq!(
            source_url(&item).as_deref(),
            Some("https://example.test/watch")
        );
        item.url = item.stream_url.clone();
        assert!(source_url(&item).is_none());
        item.metadata.insert(
            "webpage_url".to_owned(),
            serde_json::json!("https://example.test/original"),
        );
        assert_eq!(
            source_url(&item).as_deref(),
            Some("https://example.test/original")
        );
    }
}
