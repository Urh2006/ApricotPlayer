//! Bounded `yt-dlp` adapter for the default `YouTube` backend. Every process is
//! short-lived and runs off the UI thread through [`YoutubeRuntime`].

use std::{
    cmp::Reverse,
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
use apricot_media::{
    YoutubeBackend, YoutubeCapability, YoutubeCollectionKind, YoutubeCommand, YoutubeEngine,
    YoutubeEngineError, YoutubeErrorCode, YoutubeFormat, YoutubeFormatTracks,
    YoutubeFormatTransport, YoutubeResponsePayload, YoutubeRuntime, YoutubeRuntimeError,
    YoutubeSearchKind, YoutubeSessionConfig, YoutubeStreamPreference,
};
use serde_json::{Map, Value};
use thiserror::Error;
use url::Url;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(45);
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_STDOUT_BYTES: usize = 32 * 1_024 * 1_024;
const MAX_STDERR_BYTES: usize = 1_024 * 1_024;
const MAX_SEARCH_QUERY_BYTES: usize = 1_024;
const MAX_MEDIA_URL_BYTES: usize = 16_384;
const MAX_COOKIE_HEADER_BYTES: usize = 131_072;
const MAX_COOKIE_FILE_BYTES: usize = 32_768;
const MAX_PROXY_URL_BYTES: usize = 2_048;

#[derive(Debug, Error)]
pub enum YtDlpError {
    #[error("invalid YouTube component configuration: {0}")]
    InvalidConfiguration(String),
    #[error("could not launch yt-dlp: {0}")]
    Launch(String),
    #[error("yt-dlp operation timed out")]
    Timeout,
    #[error("yt-dlp {0} exceeded its output limit")]
    OutputLimit(&'static str),
    #[error("yt-dlp returned invalid data: {0}")]
    InvalidOutput(String),
    #[error("yt-dlp request failed: {0}")]
    Request(String),
}

pub struct YtDlpYoutubeEngine {
    executable: PathBuf,
    config: YoutubeSessionConfig,
}

impl YtDlpYoutubeEngine {
    /// Creates a lazy adapter for one trusted standalone `yt-dlp` executable.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured executable is not a regular file.
    pub fn new(executable: &Path) -> Result<Self, YtDlpError> {
        if !executable.is_file() {
            return Err(YtDlpError::InvalidConfiguration(
                "the bundled yt-dlp executable was not found".to_owned(),
            ));
        }
        Ok(Self {
            executable: executable.to_owned(),
            config: YoutubeSessionConfig::default(),
        })
    }

    fn version(&self) -> Result<String, YtDlpError> {
        let output = self.run([OsString::from("--version")])?;
        checked_stdout(output).and_then(|bytes| {
            let version = String::from_utf8(bytes)
                .map_err(|error| YtDlpError::InvalidOutput(error.to_string()))?;
            let version = version.lines().next().unwrap_or_default().trim();
            if version.is_empty() {
                Err(YtDlpError::InvalidOutput(
                    "the version response was empty".to_owned(),
                ))
            } else {
                Ok(version.to_owned())
            }
        })
    }

    fn configure(&mut self, config: YoutubeSessionConfig) -> Result<(), YtDlpError> {
        validate_config(&config)?;
        self.config = config;
        Ok(())
    }

    fn search(
        &self,
        query: &str,
        kind: YoutubeSearchKind,
        limit: u32,
    ) -> Result<YoutubeResponsePayload, YtDlpError> {
        let query = query.trim();
        if query.is_empty() || query.len() > MAX_SEARCH_QUERY_BYTES {
            return Err(YtDlpError::InvalidConfiguration(
                "search query is empty or too long".to_owned(),
            ));
        }
        if limit == 0 {
            return Err(YtDlpError::InvalidConfiguration(
                "search limit must be greater than zero".to_owned(),
            ));
        }

        let fetch_limit = search_fetch_limit(kind, limit);
        let target = search_target(query, kind, fetch_limit);
        let mut arguments = self.base_arguments();
        arguments.extend([
            OsString::from("--flat-playlist"),
            OsString::from("--skip-download"),
            OsString::from("--playlist-end"),
            OsString::from(fetch_limit.to_string()),
            OsString::from("--dump-single-json"),
            OsString::from("--"),
            OsString::from(target),
        ]);
        let root = parse_json(self.run(arguments)?)?;
        let entries = root
            .get("entries")
            .and_then(Value::as_array)
            .ok_or_else(|| YtDlpError::InvalidOutput("search results were missing".to_owned()))?;
        let items = entries
            .iter()
            .filter_map(media_item_from_value)
            .filter(|item| search_kind_accepts(kind, item.kind))
            .take(limit as usize)
            .collect();
        Ok(YoutubeResponsePayload::SearchResults {
            items,
            continuation: None,
        })
    }

    fn resolve(
        &self,
        media_url: &str,
        preference: YoutubeStreamPreference,
    ) -> Result<YoutubeResponsePayload, YtDlpError> {
        validate_youtube_url(media_url)?;
        let mut arguments = self.base_arguments();
        arguments.extend([
            OsString::from("--no-playlist"),
            OsString::from("--skip-download"),
            OsString::from("--dump-single-json"),
            OsString::from("--"),
            OsString::from(media_url),
        ]);
        let root = parse_json(self.run(arguments)?)?;
        let item = media_item_from_value(&root).ok_or_else(|| {
            YtDlpError::InvalidOutput("resolved media metadata was incomplete".to_owned())
        })?;
        let is_live = is_live(&root);
        let mut formats = root
            .get("formats")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|format| youtube_format(format, is_live))
            .collect::<Vec<_>>();
        sort_formats(&mut formats, preference);
        if formats.is_empty() {
            return Err(YtDlpError::Request(
                "no playable formats were returned".to_owned(),
            ));
        }
        Ok(YoutubeResponsePayload::Resolved {
            item: Box::new(item),
            formats,
        })
    }

    fn collection(
        &self,
        collection_url: &str,
        kind: YoutubeCollectionKind,
        limit: u32,
    ) -> Result<YoutubeResponsePayload, YtDlpError> {
        if limit == 0 {
            return Err(YtDlpError::InvalidConfiguration(
                "collection limit must be greater than zero".to_owned(),
            ));
        }
        let target = collection_target(collection_url, kind)?;
        let mut arguments = self.base_arguments();
        arguments.extend([
            OsString::from("--flat-playlist"),
            OsString::from("--skip-download"),
            OsString::from("--playlist-end"),
            OsString::from(limit.to_string()),
            OsString::from("--dump-single-json"),
            OsString::from("--"),
            OsString::from(target),
        ]);
        let root = parse_json(self.run(arguments)?)?;
        let entries = root
            .get("entries")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                YtDlpError::InvalidOutput("collection entries were missing".to_owned())
            })?;
        let items = entries
            .iter()
            .filter_map(media_item_from_value)
            .filter(|item| collection_kind_accepts(kind, item.kind))
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .collect();
        Ok(YoutubeResponsePayload::SearchResults {
            items,
            continuation: None,
        })
    }

    fn base_arguments(&self) -> Vec<OsString> {
        let mut arguments = vec![
            OsString::from("--ignore-config"),
            OsString::from("--no-plugin-dirs"),
            OsString::from("--no-warnings"),
            OsString::from("--no-progress"),
        ];
        if let Some(path) = self.config.cookies_file.as_deref() {
            arguments.push(OsString::from("--cookies"));
            arguments.push(OsString::from(path));
        }
        if let Some(proxy) = self.config.proxy_url.as_deref() {
            arguments.push(OsString::from("--proxy"));
            arguments.push(OsString::from(proxy));
        }
        arguments
    }

    fn run<I, S>(&self, arguments: I) -> Result<ProcessOutput, YtDlpError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(&self.executable);
        command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let mut child = command
            .spawn()
            .map_err(|error| YtDlpError::Launch(error.to_string()))?;
        collect_process_output(&mut child, OPERATION_TIMEOUT)
    }
}

fn search_target(query: &str, kind: YoutubeSearchKind, limit: u32) -> String {
    if kind == YoutubeSearchKind::Video {
        return format!("ytsearch{limit}:{query}");
    }
    let filter = match kind {
        YoutubeSearchKind::Playlist => Some("EgIQAw=="),
        YoutubeSearchKind::Channel => Some("EgIQAg=="),
        YoutubeSearchKind::All | YoutubeSearchKind::Film | YoutubeSearchKind::Video => None,
    };
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.append_pair("search_query", query);
    if let Some(filter) = filter {
        serializer.append_pair("sp", filter);
    }
    format!("https://www.youtube.com/results?{}", serializer.finish())
}

fn search_fetch_limit(kind: YoutubeSearchKind, requested: u32) -> u32 {
    match kind {
        YoutubeSearchKind::All | YoutubeSearchKind::Video => requested,
        // Filtered YouTube result pages can contain pinned entries of another
        // kind. Fetch a bounded cushion, then preserve the caller's visible
        // limit after typed filtering.
        YoutubeSearchKind::Playlist | YoutubeSearchKind::Channel | YoutubeSearchKind::Film => {
            requested.saturating_mul(2).clamp(20, 500)
        }
    }
}

impl YoutubeEngine for YtDlpYoutubeEngine {
    fn execute(
        &mut self,
        command: YoutubeCommand,
    ) -> Result<YoutubeResponsePayload, YoutubeEngineError> {
        let result = match command {
            YoutubeCommand::Hello => self.version().map(|version| YoutubeResponsePayload::Hello {
                helper_version: version.clone(),
                backend_revision: version,
                capabilities: vec![
                    YoutubeCapability::Search,
                    YoutubeCapability::PlaylistCollections,
                    YoutubeCapability::ChannelCollections,
                    YoutubeCapability::Resolve,
                    YoutubeCapability::Cookies,
                    YoutubeCapability::Proxy,
                    YoutubeCapability::LiveStreams,
                ],
            }),
            YoutubeCommand::Configure { config } => self
                .configure(config)
                .map(|()| YoutubeResponsePayload::Configured),
            YoutubeCommand::Search {
                query,
                kind,
                limit,
                safe_search: _,
            } => self.search(&query, kind, limit),
            YoutubeCommand::Collection { url, kind, limit } => self.collection(&url, kind, limit),
            YoutubeCommand::Resolve { url, preference } => self.resolve(&url, preference),
            YoutubeCommand::Shutdown => Ok(YoutubeResponsePayload::ShuttingDown),
        };
        result.map_err(|error| map_engine_error(&error, &self.config))
    }
}

fn collection_target(value: &str, kind: YoutubeCollectionKind) -> Result<String, YtDlpError> {
    validate_youtube_url(value)?;
    if kind == YoutubeCollectionKind::PlaylistVideos {
        return Ok(value.to_owned());
    }
    let mut url = Url::parse(value)
        .map_err(|_| YtDlpError::InvalidConfiguration("collection URL is invalid".to_owned()))?;
    let mut path = url.path().trim_end_matches('/').to_owned();
    for suffix in ["/videos", "/playlists", "/streams", "/shorts", "/featured"] {
        if path.to_ascii_lowercase().ends_with(suffix) {
            path.truncate(path.len() - suffix.len());
            break;
        }
    }
    let suffix = match kind {
        YoutubeCollectionKind::PlaylistVideos => unreachable!("handled above"),
        YoutubeCollectionKind::ChannelVideos | YoutubeCollectionKind::ChannelPopular => "/videos",
        YoutubeCollectionKind::ChannelPlaylists => "/playlists",
        YoutubeCollectionKind::ChannelStreams => "/streams",
    };
    url.set_path(&format!("{path}{suffix}"));
    url.set_query(None);
    url.set_fragment(None);
    if kind == YoutubeCollectionKind::ChannelPopular {
        url.query_pairs_mut()
            .append_pair("view", "0")
            .append_pair("sort", "p")
            .append_pair("flow", "grid");
    }
    Ok(url.into())
}

fn collection_kind_accepts(kind: YoutubeCollectionKind, item: MediaKind) -> bool {
    match kind {
        YoutubeCollectionKind::ChannelPlaylists => item == MediaKind::Playlist,
        YoutubeCollectionKind::PlaylistVideos
        | YoutubeCollectionKind::ChannelVideos
        | YoutubeCollectionKind::ChannelStreams
        | YoutubeCollectionKind::ChannelPopular => {
            matches!(item, MediaKind::Video | MediaKind::LiveStream)
        }
    }
}

/// Creates the selected backend lazily. Neither helper nor `yt-dlp` is started
/// during application startup.
///
/// # Errors
///
/// Returns an error only when the bounded runtime worker cannot be created.
pub fn spawn_youtube_runtime(
    backend: YoutubeBackend,
    components_directory: &Path,
) -> Result<YoutubeRuntime, YoutubeRuntimeError> {
    let executable = components_directory.join(component_executable(backend));
    YoutubeRuntime::spawn(Box::new(move || match backend {
        YoutubeBackend::YtDlp => YtDlpYoutubeEngine::new(&executable)
            .map(|engine| Box::new(engine) as Box<dyn YoutubeEngine>)
            .map_err(|error| map_engine_error(&error, &YoutubeSessionConfig::default())),
        YoutubeBackend::RustyYtdl => super::YoutubeHelperProcess::start(&executable)
            .map(|engine| Box::new(engine) as Box<dyn YoutubeEngine>)
            .map_err(|error| YoutubeEngineError::new(error.to_string(), true)),
    }))
}

const fn component_executable(backend: YoutubeBackend) -> &'static str {
    match backend {
        YoutubeBackend::YtDlp => "yt-dlp.exe",
        YoutubeBackend::RustyYtdl => "apricot-youtube-helper.exe",
    }
}

fn validate_config(config: &YoutubeSessionConfig) -> Result<(), YtDlpError> {
    if config
        .cookies_header
        .as_ref()
        .is_some_and(|value| value.len() > MAX_COOKIE_HEADER_BYTES)
        || config
            .cookies_file
            .as_ref()
            .is_some_and(|value| value.len() > MAX_COOKIE_FILE_BYTES)
        || config
            .proxy_url
            .as_ref()
            .is_some_and(|value| value.len() > MAX_PROXY_URL_BYTES)
    {
        return Err(YtDlpError::InvalidConfiguration(
            "session configuration is too large".to_owned(),
        ));
    }
    if let Some(path) = config.cookies_file.as_deref()
        && !Path::new(path).is_file()
    {
        return Err(YtDlpError::InvalidConfiguration(
            "the configured cookies file was not found".to_owned(),
        ));
    }
    if let Some(proxy) = config.proxy_url.as_deref() {
        let parsed = Url::parse(proxy)
            .map_err(|_| YtDlpError::InvalidConfiguration("the proxy URL is invalid".to_owned()))?;
        if !matches!(
            parsed.scheme(),
            "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h"
        ) {
            return Err(YtDlpError::InvalidConfiguration(
                "the proxy URL uses an unsupported scheme".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_youtube_url(value: &str) -> Result<(), YtDlpError> {
    if value.trim().is_empty() || value.len() > MAX_MEDIA_URL_BYTES {
        return Err(YtDlpError::InvalidConfiguration(
            "media URL is empty or too long".to_owned(),
        ));
    }
    let url = Url::parse(value)
        .map_err(|_| YtDlpError::InvalidConfiguration("media URL is invalid".to_owned()))?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if !matches!(url.scheme(), "http" | "https")
        || !(host == "youtu.be"
            || host == "youtube.com"
            || host.ends_with(".youtube.com")
            || host == "youtube-nocookie.com"
            || host.ends_with(".youtube-nocookie.com"))
    {
        return Err(YtDlpError::InvalidConfiguration(
            "only YouTube URLs are accepted by this backend".to_owned(),
        ));
    }
    Ok(())
}

struct ProcessOutput {
    status: ExitStatus,
    stdout: BoundedBytes,
    stderr: BoundedBytes,
}

struct BoundedBytes {
    bytes: Vec<u8>,
    overflowed: bool,
}

fn collect_process_output(
    child: &mut Child,
    timeout: Duration,
) -> Result<ProcessOutput, YtDlpError> {
    let Some(stdout) = child.stdout.take() else {
        stop_child(child);
        return Err(YtDlpError::Launch("stdout pipe was not created".to_owned()));
    };
    let Some(stderr) = child.stderr.take() else {
        stop_child(child);
        return Err(YtDlpError::Launch("stderr pipe was not created".to_owned()));
    };
    let stdout_reader = thread::spawn(move || read_bounded(stdout, MAX_STDOUT_BYTES));
    let stderr_reader = thread::spawn(move || read_bounded(stderr, MAX_STDERR_BYTES));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                stop_child(child);
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(YtDlpError::Timeout);
            }
            Err(error) => {
                stop_child(child);
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(YtDlpError::Launch(error.to_string()));
            }
        }
    };
    let stdout = join_reader(stdout_reader)?;
    let stderr = join_reader(stderr_reader)?;
    if stdout.overflowed {
        return Err(YtDlpError::OutputLimit("standard output"));
    }
    if stderr.overflowed {
        return Err(YtDlpError::OutputLimit("error output"));
    }
    Ok(ProcessOutput {
        status,
        stdout,
        stderr,
    })
}

fn stop_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_bounded(mut reader: impl Read, limit: usize) -> io::Result<BoundedBytes> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1_024));
    let mut overflowed = false;
    let mut buffer = [0_u8; 8 * 1_024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(bytes.len());
        let retained = remaining.min(count);
        bytes.extend_from_slice(&buffer[..retained]);
        overflowed |= retained < count;
    }
    Ok(BoundedBytes { bytes, overflowed })
}

fn join_reader(
    handle: thread::JoinHandle<io::Result<BoundedBytes>>,
) -> Result<BoundedBytes, YtDlpError> {
    handle
        .join()
        .map_err(|_| YtDlpError::Launch("output reader stopped unexpectedly".to_owned()))?
        .map_err(|error| YtDlpError::Launch(error.to_string()))
}

fn checked_stdout(output: ProcessOutput) -> Result<Vec<u8>, YtDlpError> {
    if output.status.success() {
        return Ok(output.stdout.bytes);
    }
    let message = String::from_utf8_lossy(&output.stderr.bytes);
    let message = message
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("yt-dlp exited without an error message")
        .trim();
    Err(YtDlpError::Request(message.to_owned()))
}

fn parse_json(output: ProcessOutput) -> Result<Value, YtDlpError> {
    let bytes = checked_stdout(output)?;
    serde_json::from_slice(&bytes).map_err(|error| YtDlpError::InvalidOutput(error.to_string()))
}

fn media_item_from_value(value: &Value) -> Option<MediaItem> {
    let object = value.as_object()?;
    let title = string(object, "title")?.trim().to_owned();
    if title.is_empty() {
        return None;
    }
    let raw_url = first_string(object, &["webpage_url", "original_url", "url"]);
    let kind = media_kind(object, raw_url);
    let id = string(object, "id")
        .filter(|value| !value.is_empty())
        .or(raw_url)
        .unwrap_or(&title)
        .to_owned();
    let url = media_url(kind, &id, raw_url);
    let mut metadata = BTreeMap::new();
    for key in [
        "description",
        "view_count",
        "upload_date",
        "timestamp",
        "release_timestamp",
        "channel_id",
        "uploader_id",
        "channel_follower_count",
        "playlist_count",
        "verified",
        "thumbnail",
        "chapters",
        "live_status",
    ] {
        if let Some(value) = object.get(key).filter(|value| !value.is_null()) {
            metadata.insert(key.to_owned(), value.clone());
        }
    }
    insert_alias(&mut metadata, object, "views", &["view_count"]);
    insert_alias(
        &mut metadata,
        object,
        "subscribers",
        &["channel_follower_count"],
    );
    insert_alias(&mut metadata, object, "video_count", &["playlist_count"]);
    insert_alias(
        &mut metadata,
        object,
        "verified",
        &["channel_is_verified", "uploader_is_verified"],
    );
    insert_alias(&mut metadata, object, "uploaded_at", &["upload_date"]);
    Some(MediaItem {
        id: MediaId(id),
        source: MediaSource::Youtube,
        kind,
        title,
        url,
        stream_url: None,
        external_audio_url: None,
        local_path: None,
        channel: first_string(object, &["channel", "uploader", "channel_name"])
            .unwrap_or_default()
            .to_owned(),
        duration_seconds: number(object, "duration"),
        metadata,
    })
}

fn insert_alias(
    metadata: &mut BTreeMap<String, Value>,
    object: &Map<String, Value>,
    destination: &str,
    sources: &[&str],
) {
    if let Some(value) = sources
        .iter()
        .find_map(|source| object.get(*source))
        .filter(|value| !value.is_null())
    {
        metadata.insert(destination.to_owned(), value.clone());
    }
}

fn media_kind(object: &Map<String, Value>, raw_url: Option<&str>) -> MediaKind {
    if is_live_object(object) {
        return MediaKind::LiveStream;
    }
    let item_type = string(object, "_type").unwrap_or_default();
    let extractor = string(object, "ie_key").unwrap_or_default();
    let url = raw_url.unwrap_or_default().to_ascii_lowercase();
    if item_type == "playlist" || url.contains("/playlist") || url.contains("list=") {
        MediaKind::Playlist
    } else if extractor.eq_ignore_ascii_case("YoutubeTab")
        && (url.contains("/channel/") || url.contains("/@") || url.contains("/user/"))
    {
        MediaKind::Channel
    } else {
        MediaKind::Video
    }
}

fn media_url(kind: MediaKind, id: &str, raw_url: Option<&str>) -> Option<Url> {
    if let Some(url) = raw_url.and_then(|value| Url::parse(value).ok()) {
        return Some(url);
    }
    let canonical = match kind {
        MediaKind::Playlist => format!("https://www.youtube.com/playlist?list={id}"),
        MediaKind::Channel => format!("https://www.youtube.com/channel/{id}"),
        _ => format!("https://www.youtube.com/watch?v={id}"),
    };
    Url::parse(&canonical).ok()
}

fn youtube_format(value: &Value, root_is_live: bool) -> Option<YoutubeFormat> {
    let object = value.as_object()?;
    let url = string(object, "url")?;
    let parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let video = string(object, "vcodec").is_some_and(|codec| codec != "none");
    let audio = string(object, "acodec").is_some_and(|codec| codec != "none");
    if !video && !audio {
        return None;
    }
    let protocol = string(object, "protocol")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let transport = if protocol.contains("m3u8") {
        YoutubeFormatTransport::Hls
    } else if protocol.contains("dash") {
        YoutubeFormatTransport::Dash
    } else {
        YoutubeFormatTransport::Direct
    };
    let bitrate_kbps = number(object, "tbr")
        .or_else(|| number(object, "abr"))
        .unwrap_or_default();
    Some(YoutubeFormat {
        itag: string(object, "format_id")
            .and_then(|value| value.parse().ok())
            .unwrap_or_default(),
        url: url.to_owned(),
        mime_type: format_mime_type(object, video, audio),
        bitrate: saturating_u64(bitrate_kbps * 1_000.0),
        width: unsigned(object, "width"),
        height: unsigned(object, "height"),
        fps: number(object, "fps").map(saturating_u64),
        tracks: YoutubeFormatTracks { video, audio },
        is_live: root_is_live || bool_value(object, "is_live"),
        transport,
    })
}

fn format_mime_type(object: &Map<String, Value>, video: bool, audio: bool) -> String {
    if let Some(mime) = string(object, "mime_type") {
        return mime.to_owned();
    }
    let extension = if video {
        first_string(object, &["video_ext", "ext"])
    } else if audio {
        first_string(object, &["audio_ext", "ext"])
    } else {
        None
    }
    .unwrap_or("unknown");
    if video {
        format!("video/{extension}")
    } else {
        format!("audio/{extension}")
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

const fn search_kind_accepts(kind: YoutubeSearchKind, media_kind: MediaKind) -> bool {
    match kind {
        YoutubeSearchKind::All => true,
        YoutubeSearchKind::Video | YoutubeSearchKind::Film => {
            matches!(media_kind, MediaKind::Video | MediaKind::LiveStream)
        }
        YoutubeSearchKind::Playlist => matches!(media_kind, MediaKind::Playlist),
        YoutubeSearchKind::Channel => matches!(media_kind, MediaKind::Channel),
    }
}

fn is_live(value: &Value) -> bool {
    value.as_object().is_some_and(is_live_object)
}

fn is_live_object(object: &Map<String, Value>) -> bool {
    bool_value(object, "is_live")
        || matches!(
            string(object, "live_status"),
            Some("is_live" | "is_upcoming")
        )
}

fn string<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn first_string<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| string(object, key))
}

fn number(object: &Map<String, Value>, key: &str) -> Option<f64> {
    object.get(key).and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn unsigned(object: &Map<String, Value>, key: &str) -> Option<u64> {
    object.get(key).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
    })
}

fn bool_value(object: &Map<String, Value>, key: &str) -> bool {
    object.get(key).and_then(Value::as_bool).unwrap_or(false)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn saturating_u64(value: f64) -> u64 {
    if !value.is_finite() || value <= 0.0 {
        0
    } else if value >= 18_446_744_073_709_551_615.0 {
        u64::MAX
    } else {
        value.round() as u64
    }
}

fn map_engine_error(error: &YtDlpError, config: &YoutubeSessionConfig) -> YoutubeEngineError {
    let raw = error.to_string();
    let message = sanitize_error(&raw, config);
    let lower = raw.to_ascii_lowercase();
    let code = if matches!(error, YtDlpError::InvalidConfiguration(_)) {
        YoutubeErrorCode::InvalidRequest
    } else if lower.contains("429") || lower.contains("too many requests") {
        YoutubeErrorCode::RateLimited
    } else if lower.contains("private")
        || lower.contains("age-restricted")
        || lower.contains("sign in")
        || lower.contains("members-only")
        || lower.contains("premium")
    {
        YoutubeErrorCode::Restricted
    } else if matches!(
        error,
        YtDlpError::InvalidOutput(_) | YtDlpError::OutputLimit(_)
    ) {
        YoutubeErrorCode::Internal
    } else {
        YoutubeErrorCode::Unavailable
    };
    let restart_required = matches!(error, YtDlpError::Launch(_));
    YoutubeEngineError::backend(code, message, restart_required)
}

fn sanitize_error(message: &str, config: &YoutubeSessionConfig) -> String {
    let mut sanitized = message.replace(['\r', '\n', '\t'], " ");
    if let Some(path) = config.cookies_file.as_deref() {
        sanitized = sanitized.replace(path, "[cookies file]");
    }
    if let Some(proxy) = config.proxy_url.as_deref() {
        sanitized = sanitized.replace(proxy, "[proxy]");
    }
    sanitized.truncate(sanitized.floor_char_boundary(2_048));
    sanitized
}

#[cfg(test)]
mod tests {
    use super::{
        BoundedBytes, YtDlpYoutubeEngine, collection_kind_accepts, collection_target,
        component_executable, media_item_from_value, read_bounded, sanitize_error,
        search_fetch_limit, search_kind_accepts, search_target, sort_formats, youtube_format,
    };
    use apricot_core::MediaKind;
    use apricot_media::{
        YoutubeBackend, YoutubeCollectionKind, YoutubeCommand, YoutubeEngine,
        YoutubeResponsePayload, YoutubeSearchKind, YoutubeSessionConfig, YoutubeStreamPreference,
    };
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn backend_selection_uses_packaged_component_names() {
        assert_eq!(component_executable(YoutubeBackend::YtDlp), "yt-dlp.exe");
        assert_eq!(
            component_executable(YoutubeBackend::RustyYtdl),
            "apricot-youtube-helper.exe"
        );
    }

    #[test]
    fn mixed_search_entries_keep_their_real_kinds() {
        let video = media_item_from_value(&json!({
            "_type": "url", "ie_key": "Youtube", "id": "abc", "title": "Video",
            "url": "https://www.youtube.com/watch?v=abc", "duration": 12.5,
            "channel": "Channel", "view_count": 42
        }))
        .expect("video");
        let channel = media_item_from_value(&json!({
            "_type": "url", "ie_key": "YoutubeTab", "id": "UC123", "title": "Creator",
            "url": "https://www.youtube.com/channel/UC123"
        }))
        .expect("channel");
        let playlist = media_item_from_value(&json!({
            "_type": "playlist", "id": "PL123", "title": "Mix"
        }))
        .expect("playlist");
        assert_eq!(video.kind, MediaKind::Video);
        assert_eq!(video.metadata["views"], 42);
        assert_eq!(channel.kind, MediaKind::Channel);
        assert_eq!(playlist.kind, MediaKind::Playlist);
        assert!(search_kind_accepts(YoutubeSearchKind::Video, video.kind));
        assert!(!search_kind_accepts(YoutubeSearchKind::Video, channel.kind));
    }

    #[test]
    fn search_targets_match_python_type_semantics() {
        assert_eq!(
            search_target("open ai", YoutubeSearchKind::Video, 20),
            "ytsearch20:open ai"
        );
        assert_eq!(
            search_target("open ai", YoutubeSearchKind::All, 20),
            "https://www.youtube.com/results?search_query=open+ai"
        );
        assert_eq!(
            search_target("open ai", YoutubeSearchKind::Playlist, 20),
            "https://www.youtube.com/results?search_query=open+ai&sp=EgIQAw%3D%3D"
        );
        assert_eq!(
            search_target("open ai", YoutubeSearchKind::Channel, 20),
            "https://www.youtube.com/results?search_query=open+ai&sp=EgIQAg%3D%3D"
        );
        assert_eq!(search_fetch_limit(YoutubeSearchKind::Playlist, 1), 20);
        assert_eq!(search_fetch_limit(YoutubeSearchKind::Channel, 20), 40);
        assert_eq!(search_fetch_limit(YoutubeSearchKind::Film, 250), 500);
        assert_eq!(search_fetch_limit(YoutubeSearchKind::All, 20), 20);
    }

    #[test]
    fn collection_targets_preserve_playlists_and_normalize_channel_tabs() {
        let playlist = "https://www.youtube.com/playlist?list=PL123";
        assert_eq!(
            collection_target(playlist, YoutubeCollectionKind::PlaylistVideos)
                .expect("playlist target"),
            playlist
        );
        assert_eq!(
            collection_target(
                "https://www.youtube.com/@creator/playlists?view=1",
                YoutubeCollectionKind::ChannelVideos,
            )
            .expect("video tab"),
            "https://www.youtube.com/@creator/videos"
        );
        assert_eq!(
            collection_target(
                "https://www.youtube.com/channel/UC123/streams",
                YoutubeCollectionKind::ChannelPopular,
            )
            .expect("popular tab"),
            "https://www.youtube.com/channel/UC123/videos?view=0&sort=p&flow=grid"
        );
        assert!(
            collection_target(
                "https://example.com/channel/UC123",
                YoutubeCollectionKind::ChannelVideos,
            )
            .is_err()
        );
        assert!(collection_kind_accepts(
            YoutubeCollectionKind::ChannelPlaylists,
            MediaKind::Playlist
        ));
        assert!(!collection_kind_accepts(
            YoutubeCollectionKind::ChannelPlaylists,
            MediaKind::Video
        ));
    }

    #[test]
    fn resolved_formats_are_typed_and_preference_sorted() {
        let combined = youtube_format(
            &json!({
                "format_id": "18", "url": "https://example.test/18",
                "vcodec": "avc1", "acodec": "mp4a", "protocol": "https",
                "tbr": 500.0, "width": 640, "height": 360, "fps": 30,
                "video_ext": "mp4"
            }),
            false,
        )
        .expect("combined");
        let audio = youtube_format(
            &json!({
                "format_id": "140", "url": "https://example.test/140",
                "vcodec": "none", "acodec": "mp4a", "protocol": "https",
                "abr": 129.5, "audio_ext": "m4a"
            }),
            false,
        )
        .expect("audio");
        let mut formats = vec![combined, audio];
        sort_formats(&mut formats, YoutubeStreamPreference::PreferAudio);
        assert_eq!(formats[0].itag, 140);
        assert_eq!(formats[0].bitrate, 129_500);
        assert_eq!(formats[0].mime_type, "audio/m4a");
    }

    #[test]
    fn reader_drains_but_retains_only_the_limit() {
        let BoundedBytes { bytes, overflowed } =
            read_bounded(Cursor::new(vec![b'x'; 32]), 8).expect("read");
        assert_eq!(bytes.len(), 8);
        assert!(overflowed);
    }

    #[test]
    fn errors_do_not_expose_configured_secrets() {
        let config = YoutubeSessionConfig {
            cookies_header: None,
            cookies_file: Some("C:\\private\\cookies.txt".to_owned()),
            proxy_url: Some("http://name:secret@proxy.test".to_owned()),
        };
        let message = sanitize_error(
            "failed C:\\private\\cookies.txt through http://name:secret@proxy.test",
            &config,
        );
        assert_eq!(message, "failed [cookies file] through [proxy]");
    }

    #[test]
    #[ignore = "requires live YouTube and APRICOT_YTDLP"]
    fn live_standalone_backend_searches_resolves_and_reads_a_playlist() {
        let executable = std::env::var_os("APRICOT_YTDLP").expect("APRICOT_YTDLP");
        let mut engine =
            YtDlpYoutubeEngine::new(std::path::Path::new(&executable)).expect("standalone yt-dlp");
        let search = engine
            .execute(YoutubeCommand::Search {
                query: "OpenAI".to_owned(),
                kind: YoutubeSearchKind::All,
                limit: 3,
                safe_search: false,
            })
            .expect("live search");
        assert!(matches!(
            search,
            YoutubeResponsePayload::SearchResults { items, .. } if !items.is_empty()
        ));
        let resolve = engine
            .execute(YoutubeCommand::Resolve {
                url: "https://www.youtube.com/watch?v=jNQXAC9IVRw".to_owned(),
                preference: YoutubeStreamPreference::Automatic,
            })
            .expect("live resolve");
        assert!(matches!(
            resolve,
            YoutubeResponsePayload::Resolved { item, formats }
                if item.title == "Me at the zoo" && !formats.is_empty()
        ));

        let playlist_search = engine
            .execute(YoutubeCommand::Search {
                query: "OpenAI".to_owned(),
                kind: YoutubeSearchKind::Playlist,
                limit: 1,
                safe_search: false,
            })
            .expect("live playlist search");
        let playlist_url = match playlist_search {
            YoutubeResponsePayload::SearchResults { items, .. } => items
                .into_iter()
                .next()
                .and_then(|item| item.url)
                .expect("playlist result URL"),
            payload => panic!("unexpected playlist search payload: {payload:?}"),
        };
        let collection = engine
            .execute(YoutubeCommand::Collection {
                url: playlist_url.to_string(),
                kind: YoutubeCollectionKind::PlaylistVideos,
                limit: 3,
            })
            .expect("live playlist collection");
        assert!(matches!(
            collection,
            YoutubeResponsePayload::SearchResults { items, .. }
                if !items.is_empty()
                    && items.iter().all(|item| matches!(item.kind, MediaKind::Video | MediaKind::LiveStream))
        ));
    }
}
