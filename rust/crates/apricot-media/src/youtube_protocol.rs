//! Versioned protocol shared by `ApricotPlayer` and its independently replaceable
//! Rust `YouTube` helper. Messages are newline-delimited JSON with a hard size cap.

use apricot_core::MediaItem;
use serde::{Deserialize, Serialize};

pub const YOUTUBE_HELPER_PROTOCOL_VERSION: u32 = 3;
pub const MAX_YOUTUBE_MESSAGE_BYTES: usize = 1_048_576;
pub const RUSTY_YTDL_REVISION: &str = "b1c6eb7c83f0d6189f256ed5df50019a5803c734";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum YoutubeBackend {
    #[default]
    YtDlp,
    RustyYtdl,
}

impl YoutubeBackend {
    pub const fn setting_value(self) -> &'static str {
        match self {
            Self::YtDlp => "yt-dlp",
            Self::RustyYtdl => "rusty_ytdl",
        }
    }

    pub fn from_setting_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "rusty_ytdl" => Self::RustyYtdl,
            _ => Self::YtDlp,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum YoutubeSearchKind {
    All,
    Video,
    Playlist,
    Channel,
    Film,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum YoutubeStreamPreference {
    Automatic,
    PreferAudio,
    PreferVideo,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum YoutubeCapability {
    Search,
    PlaylistCollections,
    ChannelCollections,
    Resolve,
    Cookies,
    Proxy,
    LiveStreams,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum YoutubeCollectionKind {
    PlaylistVideos,
    ChannelVideos,
    ChannelPlaylists,
    ChannelStreams,
    ChannelPopular,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct YoutubeSessionConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookies_header: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookies_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct YoutubeRequest {
    pub protocol_version: u32,
    pub request_id: u64,
    pub command: YoutubeCommand,
}

impl YoutubeRequest {
    pub const fn new(request_id: u64, command: YoutubeCommand) -> Self {
        Self {
            protocol_version: YOUTUBE_HELPER_PROTOCOL_VERSION,
            request_id,
            command,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum YoutubeCommand {
    Hello,
    Configure {
        config: YoutubeSessionConfig,
    },
    Search {
        query: String,
        kind: YoutubeSearchKind,
        limit: u32,
        safe_search: bool,
    },
    Collection {
        url: String,
        kind: YoutubeCollectionKind,
        limit: u32,
    },
    CollectionAll {
        url: String,
        kind: YoutubeCollectionKind,
    },
    Resolve {
        url: String,
        preference: YoutubeStreamPreference,
    },
    Shutdown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct YoutubeResponse {
    pub protocol_version: u32,
    pub request_id: u64,
    #[serde(flatten)]
    pub payload: YoutubeResponsePayload,
}

impl YoutubeResponse {
    pub const fn success(request_id: u64, payload: YoutubeResponsePayload) -> Self {
        Self {
            protocol_version: YOUTUBE_HELPER_PROTOCOL_VERSION,
            request_id,
            payload,
        }
    }

    pub fn failure(request_id: u64, error: YoutubeHelperError) -> Self {
        Self::success(request_id, YoutubeResponsePayload::Error { error })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum YoutubeResponsePayload {
    Hello {
        helper_version: String,
        backend_revision: String,
        capabilities: Vec<YoutubeCapability>,
    },
    Configured,
    SearchResults {
        items: Vec<MediaItem>,
        continuation: Option<String>,
    },
    Resolved {
        item: Box<MediaItem>,
        formats: Vec<YoutubeFormat>,
    },
    ShuttingDown,
    Error {
        error: YoutubeHelperError,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct YoutubeFormat {
    pub itag: u64,
    pub url: String,
    pub mime_type: String,
    pub bitrate: u64,
    pub width: Option<u64>,
    pub height: Option<u64>,
    pub fps: Option<u64>,
    pub tracks: YoutubeFormatTracks,
    pub is_live: bool,
    pub transport: YoutubeFormatTransport,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct YoutubeFormatTracks {
    pub video: bool,
    pub audio: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum YoutubeFormatTransport {
    Direct,
    Hls,
    Dash,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct YoutubePlaybackSelection {
    pub primary_index: usize,
    pub external_audio_index: Option<usize>,
}

/// Selects the same broad playback shapes as the Python player: seek-friendly
/// HLS with separate audio when available, progressive A/V as the next choice,
/// and an audio-only fallback. The component's ordering remains the tie-breaker.
pub fn select_youtube_playback_formats(
    formats: &[YoutubeFormat],
    preference: YoutubeStreamPreference,
) -> Option<YoutubePlaybackSelection> {
    let audio_only = |format: &YoutubeFormat| format.tracks.audio && !format.tracks.video;
    let video_only = |format: &YoutubeFormat| format.tracks.video && !format.tracks.audio;
    let combined = |format: &YoutubeFormat| format.tracks.video && format.tracks.audio;
    let hls_audio = formats
        .iter()
        .position(|format| audio_only(format) && format.transport == YoutubeFormatTransport::Hls);

    if preference == YoutubeStreamPreference::PreferAudio {
        let primary_index = hls_audio
            .or_else(|| formats.iter().position(audio_only))
            .or_else(|| formats.iter().position(|format| format.tracks.audio))?;
        return Some(YoutubePlaybackSelection {
            primary_index,
            external_audio_index: None,
        });
    }

    let height_limit = if preference == YoutubeStreamPreference::PreferVideo {
        720
    } else {
        360
    };
    if let (Some(primary_index), Some(external_audio_index)) = (
        formats.iter().position(|format| {
            video_only(format)
                && format.transport == YoutubeFormatTransport::Hls
                && format.height.is_some_and(|height| height <= height_limit)
        }),
        hls_audio,
    ) {
        return Some(YoutubePlaybackSelection {
            primary_index,
            external_audio_index: Some(external_audio_index),
        });
    }

    for itag in if preference == YoutubeStreamPreference::PreferVideo {
        [22, 18]
    } else {
        [18, 22]
    } {
        if let Some(primary_index) = formats
            .iter()
            .position(|format| format.itag == itag && combined(format))
        {
            return Some(YoutubePlaybackSelection {
                primary_index,
                external_audio_index: None,
            });
        }
    }

    if let Some(primary_index) = formats.iter().position(|format| {
        combined(format)
            && (preference != YoutubeStreamPreference::PreferVideo
                || format.height.is_none_or(|height| height <= height_limit))
    }) {
        return Some(YoutubePlaybackSelection {
            primary_index,
            external_audio_index: None,
        });
    }

    if let Some(primary_index) = formats.iter().position(video_only) {
        let external_audio_index = hls_audio.or_else(|| formats.iter().position(audio_only));
        return Some(YoutubePlaybackSelection {
            primary_index,
            external_audio_index,
        });
    }

    formats
        .iter()
        .position(|format| format.tracks.audio)
        .map(|primary_index| YoutubePlaybackSelection {
            primary_index,
            external_audio_index: None,
        })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum YoutubeErrorCode {
    InvalidRequest,
    ProtocolMismatch,
    NotConfigured,
    Unavailable,
    Restricted,
    RateLimited,
    Internal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct YoutubeHelperError {
    pub code: YoutubeErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl YoutubeHelperError {
    pub fn new(code: YoutubeErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        YOUTUBE_HELPER_PROTOCOL_VERSION, YoutubeBackend, YoutubeCollectionKind, YoutubeCommand,
        YoutubeFormat, YoutubeFormatTracks, YoutubeFormatTransport, YoutubeRequest,
        YoutubeResponse, YoutubeResponsePayload, YoutubeSessionConfig, YoutubeStreamPreference,
        select_youtube_playback_formats,
    };

    #[test]
    fn backend_setting_values_are_stable_and_unknown_values_are_safe() {
        assert_eq!(YoutubeBackend::YtDlp.setting_value(), "yt-dlp");
        assert_eq!(YoutubeBackend::RustyYtdl.setting_value(), "rusty_ytdl");
        assert_eq!(
            YoutubeBackend::from_setting_value("RUSTY_YTDL"),
            YoutubeBackend::RustyYtdl
        );
        assert_eq!(
            YoutubeBackend::from_setting_value("future-backend"),
            YoutubeBackend::YtDlp
        );
    }

    #[test]
    fn protocol_round_trip_does_not_expose_absent_secrets() {
        let request = YoutubeRequest::new(
            42,
            YoutubeCommand::Configure {
                config: YoutubeSessionConfig::default(),
            },
        );
        let json = serde_json::to_string(&request).expect("serialize request");
        assert!(!json.contains("cookies_header"));
        assert!(!json.contains("cookies_file"));
        assert!(!json.contains("proxy_url"));

        let decoded: YoutubeRequest = serde_json::from_str(&json).expect("deserialize request");
        assert_eq!(decoded, request);
        assert_eq!(decoded.protocol_version, YOUTUBE_HELPER_PROTOCOL_VERSION);
    }

    #[test]
    fn response_payload_is_explicitly_tagged() {
        let response = YoutubeResponse::success(7, YoutubeResponsePayload::Configured);
        let json = serde_json::to_string(&response).expect("serialize response");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(value["status"], "configured");
    }

    #[test]
    fn collection_command_round_trips_with_an_explicit_kind_and_bound() {
        let request = YoutubeRequest::new(
            8,
            YoutubeCommand::Collection {
                url: "https://www.youtube.com/playlist?list=PL123".to_owned(),
                kind: YoutubeCollectionKind::PlaylistVideos,
                limit: 20,
            },
        );
        let encoded = serde_json::to_string(&request).expect("serialize collection");
        let decoded: YoutubeRequest =
            serde_json::from_str(&encoded).expect("deserialize collection");
        assert_eq!(decoded, request);
        assert!(encoded.contains("playlist_videos"));
        assert_eq!(decoded.protocol_version, 3);
    }

    #[test]
    fn complete_collection_command_has_no_fake_numeric_limit() {
        let request = YoutubeRequest::new(
            9,
            YoutubeCommand::CollectionAll {
                url: "https://www.youtube.com/playlist?list=PL123".to_owned(),
                kind: YoutubeCollectionKind::PlaylistVideos,
            },
        );
        let encoded = serde_json::to_string(&request).expect("serialize complete collection");
        let decoded: YoutubeRequest =
            serde_json::from_str(&encoded).expect("deserialize complete collection");
        assert_eq!(decoded, request);
        assert!(encoded.contains("collection_all"));
        assert!(!encoded.contains("limit"));
    }

    fn format(
        itag: u64,
        video: bool,
        audio: bool,
        height: Option<u64>,
        transport: YoutubeFormatTransport,
    ) -> YoutubeFormat {
        YoutubeFormat {
            itag,
            url: format!("https://media.test/{itag}"),
            mime_type: String::new(),
            bitrate: itag,
            width: None,
            height,
            fps: None,
            tracks: YoutubeFormatTracks { video, audio },
            is_live: false,
            transport,
        }
    }

    #[test]
    fn automatic_playback_prefers_seekable_low_video_with_separate_audio() {
        let formats = vec![
            format(137, true, false, Some(1080), YoutubeFormatTransport::Dash),
            format(95, true, false, Some(720), YoutubeFormatTransport::Hls),
            format(93, true, false, Some(360), YoutubeFormatTransport::Hls),
            format(234, false, true, None, YoutubeFormatTransport::Hls),
            format(18, true, true, Some(360), YoutubeFormatTransport::Direct),
        ];
        assert_eq!(
            select_youtube_playback_formats(&formats, YoutubeStreamPreference::Automatic),
            Some(super::YoutubePlaybackSelection {
                primary_index: 2,
                external_audio_index: Some(3),
            })
        );
    }

    #[test]
    fn playback_preferences_choose_video_or_audio_shapes() {
        let formats = vec![
            format(95, true, false, Some(720), YoutubeFormatTransport::Hls),
            format(234, false, true, None, YoutubeFormatTransport::Hls),
            format(18, true, true, Some(360), YoutubeFormatTransport::Direct),
        ];
        assert_eq!(
            select_youtube_playback_formats(&formats, YoutubeStreamPreference::PreferVideo),
            Some(super::YoutubePlaybackSelection {
                primary_index: 0,
                external_audio_index: Some(1),
            })
        );
        assert_eq!(
            select_youtube_playback_formats(&formats, YoutubeStreamPreference::PreferAudio),
            Some(super::YoutubePlaybackSelection {
                primary_index: 1,
                external_audio_index: None,
            })
        );
    }
}
