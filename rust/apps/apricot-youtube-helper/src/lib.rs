use std::{cmp::Reverse, collections::BTreeMap, time::Duration};

use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
use apricot_media::{
    RUSTY_YTDL_REVISION, YOUTUBE_HELPER_PROTOCOL_VERSION, YoutubeCapability, YoutubeCommand,
    YoutubeErrorCode, YoutubeFormat, YoutubeFormatTracks, YoutubeFormatTransport,
    YoutubeHelperError, YoutubeRequest, YoutubeResponse, YoutubeResponsePayload, YoutubeSearchKind,
    YoutubeSessionConfig, YoutubeStreamPreference,
};
use rusty_ytdl::{
    RequestOptions, Video as ResolvableVideo, VideoError, VideoOptions,
    search::{SearchOptions, SearchResult, SearchType, YouTube},
};
use serde_json::Value;
use url::Url;

const MAX_SEARCH_QUERY_BYTES: usize = 1_024;
const MAX_MEDIA_URL_BYTES: usize = 16_384;
const MAX_COOKIE_HEADER_BYTES: usize = 131_072;
const MAX_COOKIE_FILE_BYTES: usize = 32_768;
const MAX_PROXY_URL_BYTES: usize = 2_048;
const NETWORK_OPERATION_TIMEOUT: Duration = Duration::from_secs(45);

pub struct YoutubeHelper {
    config: YoutubeSessionConfig,
    search: YouTube,
}

impl YoutubeHelper {
    /// Creates a helper with an anonymous reusable `YouTube` client.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be initialized.
    pub fn new() -> Result<Self, YoutubeHelperError> {
        Ok(Self {
            config: YoutubeSessionConfig::default(),
            search: YouTube::new().map_err(|error| map_backend_error(&error))?,
        })
    }

    pub async fn handle(&mut self, request: YoutubeRequest) -> YoutubeResponse {
        let request_id = request.request_id;
        if request.protocol_version != YOUTUBE_HELPER_PROTOCOL_VERSION {
            return YoutubeResponse::failure(
                request_id,
                YoutubeHelperError::new(
                    YoutubeErrorCode::ProtocolMismatch,
                    format!(
                        "Unsupported protocol version {}; expected {}",
                        request.protocol_version, YOUTUBE_HELPER_PROTOCOL_VERSION
                    ),
                    false,
                ),
            );
        }

        let is_network_operation = matches!(
            &request.command,
            YoutubeCommand::Search { .. } | YoutubeCommand::Resolve { .. }
        );
        let result = if is_network_operation {
            match tokio::time::timeout(
                NETWORK_OPERATION_TIMEOUT,
                self.handle_command(request.command),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(YoutubeHelperError::new(
                    YoutubeErrorCode::Unavailable,
                    "YouTube operation timed out",
                    true,
                )),
            }
        } else {
            self.handle_command(request.command).await
        };
        match result {
            Ok(payload) => YoutubeResponse::success(request_id, payload),
            Err(error) => YoutubeResponse::failure(request_id, error),
        }
    }

    async fn handle_command(
        &mut self,
        command: YoutubeCommand,
    ) -> Result<YoutubeResponsePayload, YoutubeHelperError> {
        match command {
            YoutubeCommand::Hello => Ok(YoutubeResponsePayload::Hello {
                helper_version: env!("CARGO_PKG_VERSION").to_owned(),
                backend_revision: RUSTY_YTDL_REVISION.to_owned(),
                capabilities: vec![
                    YoutubeCapability::Search,
                    YoutubeCapability::Resolve,
                    YoutubeCapability::Cookies,
                    YoutubeCapability::Proxy,
                    YoutubeCapability::LiveStreams,
                ],
            }),
            YoutubeCommand::Configure { config } => {
                validate_config(&config)?;
                let options = request_options(&config)?;
                let search = YouTube::new_with_options(&options)
                    .map_err(|error| map_backend_error(&error))?;
                self.config = config;
                self.search = search;
                Ok(YoutubeResponsePayload::Configured)
            }
            YoutubeCommand::Search {
                query,
                kind,
                limit,
                safe_search,
            } => self.search(query, kind, limit, safe_search).await,
            YoutubeCommand::Resolve { url, preference } => self.resolve(url, preference).await,
            YoutubeCommand::Shutdown => Ok(YoutubeResponsePayload::ShuttingDown),
        }
    }

    async fn search(
        &self,
        query: String,
        kind: YoutubeSearchKind,
        limit: u32,
        safe_search: bool,
    ) -> Result<YoutubeResponsePayload, YoutubeHelperError> {
        let query = query.trim();
        if query.is_empty() || query.len() > MAX_SEARCH_QUERY_BYTES {
            return Err(YoutubeHelperError::new(
                YoutubeErrorCode::InvalidRequest,
                "Search query is empty or too long",
                false,
            ));
        }
        if limit == 0 {
            return Err(YoutubeHelperError::new(
                YoutubeErrorCode::InvalidRequest,
                "Search limit must be greater than zero",
                false,
            ));
        }

        let options = SearchOptions {
            limit: u64::from(limit),
            search_type: search_type(kind),
            safe_search,
        };
        let results = self
            .search
            .search(query, Some(&options))
            .await
            .map_err(|error| map_backend_error(&error))?;
        Ok(YoutubeResponsePayload::SearchResults {
            items: results.into_iter().map(search_item).collect(),
            continuation: None,
        })
    }

    async fn resolve(
        &self,
        media_url: String,
        preference: YoutubeStreamPreference,
    ) -> Result<YoutubeResponsePayload, YoutubeHelperError> {
        if media_url.trim().is_empty() || media_url.len() > MAX_MEDIA_URL_BYTES {
            return Err(YoutubeHelperError::new(
                YoutubeErrorCode::InvalidRequest,
                "Media URL is empty or too long",
                false,
            ));
        }
        let options = VideoOptions {
            request_options: request_options(&self.config)?,
            ..VideoOptions::default()
        };
        let video = ResolvableVideo::new_with_options(&media_url, options)
            .map_err(|error| map_backend_error(&error))?;
        let info = video
            .get_info()
            .await
            .map_err(|error| map_backend_error(&error))?;
        let details = info.video_details;
        let mut formats: Vec<_> = info.formats.into_iter().map(format_item).collect();
        sort_formats(&mut formats, preference);

        let mut metadata = BTreeMap::new();
        metadata.insert("description".to_owned(), Value::String(details.description));
        metadata.insert("views".to_owned(), Value::String(details.view_count));
        metadata.insert("upload_date".to_owned(), Value::String(details.upload_date));
        metadata.insert(
            "publish_date".to_owned(),
            Value::String(details.publish_date),
        );
        metadata.insert(
            "chapters".to_owned(),
            serde_json::to_value(details.chapters).unwrap_or(Value::Array(Vec::new())),
        );

        let duration_seconds = details.length_seconds.parse::<f64>().ok();
        let item = MediaItem {
            id: MediaId(details.video_id),
            source: MediaSource::Youtube,
            kind: if details.is_live_content {
                MediaKind::LiveStream
            } else {
                MediaKind::Video
            },
            title: details.title,
            url: Url::parse(&details.video_url)
                .or_else(|_| Url::parse(&media_url))
                .ok(),
            local_path: None,
            channel: details.owner_channel_name,
            duration_seconds,
            metadata,
        };
        Ok(YoutubeResponsePayload::Resolved { item, formats })
    }
}

fn validate_config(config: &YoutubeSessionConfig) -> Result<(), YoutubeHelperError> {
    if config
        .cookies_header
        .as_ref()
        .is_some_and(|cookies| cookies.len() > MAX_COOKIE_HEADER_BYTES)
        || config
            .cookies_file
            .as_ref()
            .is_some_and(|path| path.len() > MAX_COOKIE_FILE_BYTES)
        || config
            .proxy_url
            .as_ref()
            .is_some_and(|proxy| proxy.len() > MAX_PROXY_URL_BYTES)
    {
        return Err(YoutubeHelperError::new(
            YoutubeErrorCode::InvalidRequest,
            "YouTube session configuration is too large",
            false,
        ));
    }
    Ok(())
}

fn request_options(config: &YoutubeSessionConfig) -> Result<RequestOptions, YoutubeHelperError> {
    let proxy = config
        .proxy_url
        .as_deref()
        .map(rusty_ytdl::reqwest::Proxy::all)
        .transpose()
        .map_err(|error| {
            YoutubeHelperError::new(
                YoutubeErrorCode::InvalidRequest,
                format!("Proxy URL is invalid: {error}"),
                false,
            )
        })?;
    Ok(RequestOptions {
        cookies: config.cookies_header.clone(),
        proxy,
        ..RequestOptions::default()
    })
}

const fn search_type(kind: YoutubeSearchKind) -> SearchType {
    match kind {
        YoutubeSearchKind::All => SearchType::All,
        YoutubeSearchKind::Video => SearchType::Video,
        YoutubeSearchKind::Playlist => SearchType::Playlist,
        YoutubeSearchKind::Channel => SearchType::Channel,
        YoutubeSearchKind::Film => SearchType::Film,
    }
}

fn search_item(result: SearchResult) -> MediaItem {
    match result {
        SearchResult::Video(video) => {
            let mut metadata = BTreeMap::new();
            metadata.insert("description".to_owned(), Value::String(video.description));
            metadata.insert("views".to_owned(), Value::from(video.views));
            if let Some(uploaded_at) = video.uploaded_at {
                metadata.insert("uploaded_at".to_owned(), Value::String(uploaded_at));
            }
            MediaItem {
                id: MediaId(video.id),
                source: MediaSource::Youtube,
                kind: MediaKind::Video,
                title: video.title,
                url: Url::parse(&video.url).ok(),
                local_path: None,
                channel: video.channel.name,
                duration_seconds: video.duration.to_string().parse::<f64>().ok(),
                metadata,
            }
        }
        SearchResult::Playlist(playlist) => {
            let mut metadata = BTreeMap::new();
            metadata.insert("views".to_owned(), Value::from(playlist.views));
            metadata.insert("video_count".to_owned(), Value::from(playlist.videos.len()));
            if let Some(last_update) = playlist.last_update {
                metadata.insert("last_update".to_owned(), Value::String(last_update));
            }
            MediaItem {
                id: MediaId(playlist.id),
                source: MediaSource::Youtube,
                kind: MediaKind::Playlist,
                title: playlist.name,
                url: Url::parse(&playlist.url).ok(),
                local_path: None,
                channel: playlist.channel.name,
                duration_seconds: None,
                metadata,
            }
        }
        SearchResult::Channel(channel) => {
            let mut metadata = BTreeMap::new();
            metadata.insert("verified".to_owned(), Value::Bool(channel.verified));
            metadata.insert("subscribers".to_owned(), Value::from(channel.subscribers));
            MediaItem {
                id: MediaId(channel.id),
                source: MediaSource::Youtube,
                kind: MediaKind::Channel,
                title: channel.name,
                url: Url::parse(&channel.url).ok(),
                local_path: None,
                channel: String::new(),
                duration_seconds: None,
                metadata,
            }
        }
    }
}

fn format_item(format: rusty_ytdl::VideoFormat) -> YoutubeFormat {
    let mime_type = serde_json::to_value(&format.mime_type)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    YoutubeFormat {
        itag: format.itag,
        url: format.url,
        mime_type,
        bitrate: format.bitrate,
        width: format.width,
        height: format.height,
        fps: format.fps,
        tracks: YoutubeFormatTracks {
            video: format.has_video,
            audio: format.has_audio,
        },
        is_live: format.is_live,
        transport: if format.is_hls {
            YoutubeFormatTransport::Hls
        } else if format.is_dash_mpd {
            YoutubeFormatTransport::Dash
        } else {
            YoutubeFormatTransport::Direct
        },
    }
}

fn sort_formats(formats: &mut [YoutubeFormat], preference: YoutubeStreamPreference) {
    formats.sort_by_key(|format| {
        let suitability = match preference {
            YoutubeStreamPreference::Automatic => {
                u8::from(!(format.tracks.audio && format.tracks.video))
            }
            YoutubeStreamPreference::PreferAudio => {
                u8::from(!format.tracks.audio || format.tracks.video)
            }
            YoutubeStreamPreference::PreferVideo => u8::from(!format.tracks.video),
        };
        (suitability, Reverse(format.bitrate))
    });
}

fn map_backend_error(error: &VideoError) -> YoutubeHelperError {
    let message = error.to_string();
    let lower = message.to_ascii_lowercase();
    let (code, retryable) = if lower.contains("429") || lower.contains("too many requests") {
        (YoutubeErrorCode::RateLimited, true)
    } else if lower.contains("private")
        || lower.contains("age")
        || lower.contains("restricted")
        || lower.contains("premium")
    {
        (YoutubeErrorCode::Restricted, false)
    } else if matches!(
        *error,
        VideoError::VideoNotFound | VideoError::VideoSourceNotFound
    ) {
        (YoutubeErrorCode::Unavailable, false)
    } else {
        (YoutubeErrorCode::Unavailable, true)
    };
    YoutubeHelperError::new(code, message, retryable)
}

#[cfg(test)]
mod tests {
    use super::{YoutubeHelper, sort_formats};
    use apricot_media::{
        YoutubeCommand, YoutubeFormat, YoutubeFormatTracks, YoutubeFormatTransport, YoutubeRequest,
        YoutubeResponsePayload, YoutubeSessionConfig, YoutubeStreamPreference,
    };

    fn format(has_video: bool, has_audio: bool, bitrate: u64) -> YoutubeFormat {
        YoutubeFormat {
            itag: bitrate,
            url: String::new(),
            mime_type: String::new(),
            bitrate,
            width: None,
            height: None,
            fps: None,
            tracks: YoutubeFormatTracks {
                video: has_video,
                audio: has_audio,
            },
            is_live: false,
            transport: YoutubeFormatTransport::Direct,
        }
    }

    #[tokio::test]
    async fn hello_identifies_exact_backend_revision() {
        let mut helper = YoutubeHelper::new().expect("helper");
        let response = helper
            .handle(YoutubeRequest::new(9, YoutubeCommand::Hello))
            .await;
        assert_eq!(response.request_id, 9);
        assert!(matches!(
            response.payload,
            YoutubeResponsePayload::Hello { backend_revision, .. }
                if backend_revision == apricot_media::RUSTY_YTDL_REVISION
        ));
    }

    #[tokio::test]
    async fn configure_rebuilds_client_without_echoing_secrets() {
        let mut helper = YoutubeHelper::new().expect("helper");
        let response = helper
            .handle(YoutubeRequest::new(
                10,
                YoutubeCommand::Configure {
                    config: YoutubeSessionConfig {
                        cookies_header: Some("PREF=test".to_owned()),
                        cookies_file: None,
                        proxy_url: None,
                    },
                },
            ))
            .await;
        assert!(matches!(
            response.payload,
            YoutubeResponsePayload::Configured
        ));
        assert!(
            !serde_json::to_string(&response)
                .expect("json")
                .contains("PREF=test")
        );
    }

    #[test]
    fn format_preference_changes_order_without_discarding_fallbacks() {
        let mut formats = vec![
            format(true, true, 100),
            format(false, true, 200),
            format(true, false, 300),
        ];
        sort_formats(&mut formats, YoutubeStreamPreference::PreferAudio);
        assert!(!formats[0].tracks.video && formats[0].tracks.audio);
        assert_eq!(formats.len(), 3);
    }
}
