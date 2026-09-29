//! Bounded `yt-dlp` adapter for the default `YouTube` backend. Every process is
//! short-lived and runs off the UI thread through [`YoutubeRuntime`].

use std::{
    cmp::Reverse,
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::soundcloud_search::{SoundcloudCollectionSearch, search_soundcloud_collections};
use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
use apricot_media::cookie_errors::{is_age_or_js_playback_error, is_cookie_auth_error};
use apricot_media::{
    MAX_YOUTUBE_METADATA_ITEMS, YoutubeBackend, YoutubeCapability, YoutubeCollectionKind,
    YoutubeCommand, YoutubeEngine, YoutubeEngineError, YoutubeErrorCode, YoutubeFormat,
    YoutubeFormatTracks, YoutubeFormatTransport, YoutubeResponsePayload, YoutubeRuntime,
    YoutubeRuntimeError, YoutubeSearchKind, YoutubeSessionConfig, YoutubeStreamPreference,
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
    popular_cache: Option<PopularCollectionCache>,
}

#[derive(Clone, Debug)]
struct PopularCollectionCache {
    target: String,
    items: Vec<MediaItem>,
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
            popular_cache: None,
        })
    }

    /// `yt-dlp --version`, for the diagnostic report.
    ///
    /// # Errors
    ///
    /// Returns process, timeout or invalid output errors.
    pub fn version(&self) -> Result<String, YtDlpError> {
        let output = run_executable(&self.executable, [OsString::from("--version")])?;
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

    /// Extracts subtitle metadata on demand, without downloading media or captions.
    /// Call on a worker after configuring the engine's proxy/cookie settings.
    ///
    /// # Errors
    /// Returns validation, process, timeout, or invalid JSON errors.
    pub fn transcript_metadata(
        &self,
        media_url: &str,
        languages: &[String],
    ) -> Result<Value, YtDlpError> {
        let arguments = self.transcript_arguments(media_url, languages)?;
        parse_json(self.run(&arguments)?)
    }

    /// Python `fetch_ytdlp_comments`: extracts the video's information with
    /// up to 20 comments and no download. Call on a worker after configuring
    /// the engine's proxy/cookie settings.
    ///
    /// # Errors
    /// Returns validation, process, timeout, or invalid JSON errors.
    pub fn comments_metadata(&self, media_url: &str) -> Result<Value, YtDlpError> {
        let arguments = self.comments_arguments(media_url)?;
        parse_json(self.run(&arguments)?)
    }

    fn comments_arguments(&self, media_url: &str) -> Result<Vec<OsString>, YtDlpError> {
        let url = Url::parse(media_url).map_err(|_| {
            YtDlpError::InvalidConfiguration("invalid comments source URL".to_owned())
        })?;
        if media_url.len() > MAX_MEDIA_URL_BYTES
            || !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(YtDlpError::InvalidConfiguration(
                "invalid comments source URL".to_owned(),
            ));
        }
        let mut arguments = self.base_arguments();
        arguments.extend(
            [
                "--no-playlist",
                "--skip-download",
                "--write-comments",
                "--extractor-args",
                "youtube:max_comments=20",
                "--ignore-no-formats-error",
                "--dump-single-json",
                "--",
                media_url,
            ]
            .map(OsString::from),
        );
        Ok(arguments)
    }

    /// Downloads subtitle sidecars only when the caller's direct fetch fails.
    ///
    /// # Errors
    /// Returns validation, temporary-directory, process or subtitle read errors.
    pub fn transcript_fallback(
        &self,
        media_url: &str,
        languages: &[String],
    ) -> Result<String, YtDlpError> {
        let directory = tempfile::Builder::new()
            .prefix("apricot-transcript-")
            .tempdir()
            .map_err(|error| YtDlpError::Request(error.to_string()))?;
        let arguments =
            self.transcript_download_arguments(media_url, languages, directory.path())?;
        checked_stdout(self.run(&arguments)?)?;
        read_downloaded_transcript(directory.path())
    }

    fn transcript_download_arguments(
        &self,
        media_url: &str,
        languages: &[String],
        directory: &Path,
    ) -> Result<Vec<OsString>, YtDlpError> {
        let mut arguments = self.transcript_arguments(media_url, languages)?;
        arguments.retain(|argument| argument != "--dump-single-json");
        // Keep every generated artifact in the private temporary directory.
        let target = arguments.split_off(arguments.len() - 2);
        arguments.extend([
            OsString::from("--output"),
            directory.join("caption.%(ext)s").into_os_string(),
        ]);
        arguments.extend(target);
        Ok(arguments)
    }

    fn transcript_arguments(
        &self,
        media_url: &str,
        languages: &[String],
    ) -> Result<Vec<OsString>, YtDlpError> {
        let url = Url::parse(media_url).map_err(|_| {
            YtDlpError::InvalidConfiguration("invalid transcript source URL".to_owned())
        })?;
        if media_url.len() > MAX_MEDIA_URL_BYTES
            || !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(YtDlpError::InvalidConfiguration(
                "invalid transcript source URL".to_owned(),
            ));
        }
        let languages = languages.join(",");
        if languages.len() > MAX_SEARCH_QUERY_BYTES || languages.contains('\0') {
            return Err(YtDlpError::InvalidConfiguration(
                "invalid transcript languages".to_owned(),
            ));
        }
        let mut arguments = self.base_arguments();
        arguments.extend(
            [
                "--no-playlist",
                "--skip-download",
                "--write-subs",
                "--write-auto-subs",
                "--sub-langs",
                &languages,
                "--sub-format",
                "vtt/srt/best",
                "--ignore-no-formats-error",
                "--dump-single-json",
                "--",
                media_url,
            ]
            .map(OsString::from),
        );
        Ok(arguments)
    }

    fn configure(&mut self, config: YoutubeSessionConfig) -> Result<(), YtDlpError> {
        validate_config(&config)?;
        if self.config != config {
            self.popular_cache = None;
        }
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

        // Python `soundcloud_search_entries`: playlists and users come from
        // the SoundCloud web API, not from a yt-dlp listing.
        let collection_search = match kind {
            YoutubeSearchKind::SoundcloudPlaylist => Some(SoundcloudCollectionSearch::Playlist),
            YoutubeSearchKind::SoundcloudUser => Some(SoundcloudCollectionSearch::User),
            _ => None,
        };
        if let Some(collection_search) = collection_search {
            let items = search_soundcloud_collections(
                query,
                collection_search,
                limit,
                self.config.proxy_url.as_deref(),
            )
            .map_err(|error| YtDlpError::Request(error.to_string()))?
            .iter()
            .filter_map(media_item_from_value)
            .filter(|item| search_kind_accepts(kind, item.kind))
            .collect();
            return Ok(YoutubeResponsePayload::SearchResults {
                items,
                continuation: None,
            });
        }

        let fetch_limit = search_fetch_limit(kind, limit);
        let target = search_target(query, kind, fetch_limit);
        // Python `youtube_search_results_with_shorts`: All and Video ask for
        // Shorts at the same time and mix them in only when they are already
        // there once the main results arrive.
        let shorts = matches!(kind, YoutubeSearchKind::All | YoutubeSearchKind::Video).then(|| {
            self.spawn_flat_items(
                youtube_shorts_search_url(query),
                Some(fetch_limit),
                &["--extractor-retries", "0"],
            )
        });
        let primary = self
            .flat_items(target, Some(fetch_limit), "search results")?
            .into_iter()
            .filter(|item| search_kind_accepts(kind, item.kind))
            .collect::<Vec<_>>();
        let limit = limit as usize;
        let items = match shorts.and_then(|receiver| receiver.try_recv().ok()) {
            Some(shorts) => interleave_youtube_results(primary, shorts.unwrap_or_default(), limit),
            None => primary.into_iter().take(limit).collect(),
        };
        Ok(YoutubeResponsePayload::SearchResults {
            items,
            continuation: None,
        })
    }

    /// Python `extract_flat_entries`: one flat `yt-dlp` listing as items.
    fn flat_items(
        &self,
        target: String,
        limit: Option<u32>,
        description: &str,
    ) -> Result<Vec<MediaItem>, YtDlpError> {
        let arguments = self.flat_arguments(target, limit, &[]);
        flat_items_from_output(self.run(&arguments)?, description)
    }

    /// Runs a flat listing on its own thread, like Python's Shorts requests.
    fn spawn_flat_items(
        &self,
        target: String,
        limit: Option<u32>,
        extra_arguments: &[&str],
    ) -> mpsc::Receiver<Result<Vec<MediaItem>, YtDlpError>> {
        let (sender, receiver) = mpsc::channel();
        let executable = self.executable.clone();
        let config = self.config.clone();
        let arguments = self.flat_arguments(target, limit, extra_arguments);
        thread::spawn(move || {
            let result =
                run_with_cookie_retry(&executable, &config, &arguments, CookieRetry::Extract)
                    .and_then(|output| flat_items_from_output(output, "Shorts"));
            let _ = sender.send(result);
        });
        receiver
    }

    fn flat_arguments(
        &self,
        target: String,
        limit: Option<u32>,
        extra_arguments: &[&str],
    ) -> Vec<OsString> {
        let mut arguments = self.base_arguments();
        arguments.extend([
            OsString::from("--flat-playlist"),
            OsString::from("--skip-download"),
        ]);
        arguments.extend(extra_arguments.iter().map(OsString::from));
        if let Some(limit) = limit {
            arguments.extend([
                OsString::from("--playlist-end"),
                OsString::from(limit.to_string()),
            ]);
        }
        arguments.extend([
            OsString::from("--dump-single-json"),
            OsString::from("--"),
            OsString::from(target),
        ]);
        arguments
    }

    fn resolve(
        &self,
        media_url: &str,
        preference: YoutubeStreamPreference,
    ) -> Result<YoutubeResponsePayload, YtDlpError> {
        validate_media_url(media_url)?;
        let mut arguments = self.base_arguments();
        arguments.extend([
            OsString::from("--no-playlist"),
            OsString::from("--skip-download"),
            OsString::from("--dump-single-json"),
            OsString::from("--"),
            OsString::from(media_url),
        ]);
        let root = parse_json(run_with_cookie_retry(
            &self.executable,
            &self.config,
            &arguments,
            CookieRetry::Playback { media_url },
        )?)?;
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

    fn metadata(&self, urls: &[String]) -> Result<YoutubeResponsePayload, YtDlpError> {
        if urls.is_empty() || urls.len() > MAX_YOUTUBE_METADATA_ITEMS {
            return Err(YtDlpError::InvalidConfiguration(
                "metadata requires one bounded batch of media URLs".to_owned(),
            ));
        }
        for url in urls {
            validate_youtube_url(url)?;
        }
        let mut arguments = self.base_arguments();
        arguments.extend([
            OsString::from("--no-playlist"),
            OsString::from("--skip-download"),
            OsString::from("--ignore-errors"),
            OsString::from("--dump-json"),
            OsString::from("--"),
        ]);
        arguments.extend(urls.iter().map(OsString::from));
        let items = parse_json_lines(self.run(&arguments)?)?
            .iter()
            .filter_map(media_item_from_value)
            .collect::<Vec<_>>();
        if items.is_empty() {
            return Err(YtDlpError::InvalidOutput(
                "metadata results were missing".to_owned(),
            ));
        }
        Ok(YoutubeResponsePayload::Hydrated { items })
    }

    fn collection(
        &mut self,
        collection_url: &str,
        kind: YoutubeCollectionKind,
        limit: Option<u32>,
    ) -> Result<YoutubeResponsePayload, YtDlpError> {
        if matches!(limit, Some(0)) {
            return Err(YtDlpError::InvalidConfiguration(
                "collection limit must be greater than zero".to_owned(),
            ));
        }
        let target = collection_target(collection_url, kind)?;
        if kind == YoutubeCollectionKind::ChannelPopular
            && let Some(cache) = self
                .popular_cache
                .as_ref()
                .filter(|cache| cache.target == target)
        {
            return Ok(collection_response(&cache.items, limit));
        }
        // YouTube currently ignores the legacy channel `sort=p` query in
        // yt-dlp. Popular therefore needs one complete flat scan before it can
        // be sorted correctly; the cache keeps later 20/40/60 loads cheap.
        let fetch_limit = limit.filter(|_| kind != YoutubeCollectionKind::ChannelPopular);
        // Python `youtube_channel_upload_results`: a channel's videos tab
        // mixes in its Shorts, fetched at the same time.
        let shorts =
            (kind == YoutubeCollectionKind::ChannelUploads && limit.is_some()).then(|| {
                let base = target.strip_suffix("/videos").unwrap_or(&target);
                self.spawn_flat_items(format!("{base}/shorts"), fetch_limit, &[])
            });
        let accepted = |items: Vec<MediaItem>| {
            items
                .into_iter()
                .filter(|item| collection_kind_accepts(kind, item.kind))
                .collect::<Vec<_>>()
        };
        let mut items =
            accepted(self.flat_items(target.clone(), fetch_limit, "collection entries")?);
        if let (Some(shorts), Some(limit)) = (shorts, limit) {
            let shorts = shorts.recv().ok().and_then(Result::ok).unwrap_or_default();
            items = interleave_youtube_results(items, accepted(shorts), limit as usize);
        }
        if kind == YoutubeCollectionKind::ChannelPopular {
            sort_popular_items(&mut items);
            self.popular_cache = Some(PopularCollectionCache {
                target,
                items: items.clone(),
            });
        }
        Ok(collection_response(&items, limit))
    }

    fn base_arguments(&self) -> Vec<OsString> {
        let mut arguments = vec![
            OsString::from("--ignore-config"),
            OsString::from("--no-plugin-dirs"),
            OsString::from("--no-warnings"),
            OsString::from("--no-progress"),
        ];
        if let Some(proxy) = self.config.proxy_url.as_deref() {
            arguments.push(OsString::from("--proxy"));
            arguments.push(OsString::from(proxy));
        }
        arguments
    }

    /// Python `ydl_extract_info`: first without cookies, then with them.
    fn run(&self, arguments: &[OsString]) -> Result<ProcessOutput, YtDlpError> {
        run_with_cookie_retry(
            &self.executable,
            &self.config,
            arguments,
            CookieRetry::Extract,
        )
    }
}

/// When a failed yt-dlp run is repeated with the cookies file.
#[derive(Clone, Copy)]
enum CookieRetry<'a> {
    /// Python `ydl_extract_info(use_cookies=False)`: sign-in errors retry
    /// with the cookies file, then after `repair_cookies_for_error`.
    Extract,
    /// Python `resolve_stream_url`: sign-in, age and player errors retry
    /// once with `playback_cookies_file_for_url`, without a browser refresh.
    Playback { media_url: &'a str },
}

/// Python `ydl_options(use_cookies=True)`: the cookies file and, with it,
/// the cookie user agent.
fn cookie_arguments(config: &YoutubeSessionConfig, cookies_file: &str) -> Vec<OsString> {
    let mut arguments = vec![OsString::from("--cookies"), OsString::from(cookies_file)];
    if let Some(user_agent) = config
        .cookie_user_agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        arguments.push(OsString::from("--add-headers"));
        arguments.push(OsString::from(format!("User-Agent:{user_agent}")));
    }
    arguments
}

fn with_cookies(
    config: &YoutubeSessionConfig,
    cookies_file: &str,
    arguments: &[OsString],
) -> Vec<OsString> {
    let mut with_cookies = cookie_arguments(config, cookies_file);
    with_cookies.extend(arguments.iter().cloned());
    with_cookies
}

fn run_with_cookie_retry(
    executable: &Path,
    config: &YoutubeSessionConfig,
    arguments: &[OsString],
    retry: CookieRetry<'_>,
) -> Result<ProcessOutput, YtDlpError> {
    let output = run_executable(executable, arguments)?;
    if output.status.success() {
        return Ok(output);
    }
    let error = process_error_text(&output);
    let cookies_file = config
        .cookies_file
        .as_deref()
        .filter(|path| Path::new(path).is_file());
    match retry {
        CookieRetry::Extract => {
            if !is_cookie_auth_error(&error) {
                return Ok(output);
            }
            let mut retry_error = error;
            let mut last = output;
            if let Some(cookies_file) = cookies_file {
                let output =
                    run_executable(executable, with_cookies(config, cookies_file, arguments))?;
                if output.status.success() {
                    return Ok(output);
                }
                retry_error = process_error_text(&output);
                last = output;
                if !is_cookie_auth_error(&retry_error) {
                    return Ok(last);
                }
            }
            match crate::browser_cookies::repair_cookies_for_error(&retry_error) {
                Some(repaired) => run_executable(
                    executable,
                    with_cookies(config, &repaired.to_string_lossy(), arguments),
                ),
                None => Ok(last),
            }
        }
        CookieRetry::Playback { media_url } => {
            let retryable = is_cookie_auth_error(&error) || is_age_or_js_playback_error(&error);
            let cookies_file = cookies_file
                .filter(|_| !is_youtube_media_url(media_url) || config.cookies_have_youtube_login);
            match cookies_file {
                Some(cookies_file) if retryable => {
                    run_executable(executable, with_cookies(config, cookies_file, arguments))
                }
                _ => Ok(output),
            }
        }
    }
}

/// Python `is_youtube_url` for the playback cookie rule.
fn is_youtube_media_url(media_url: &str) -> bool {
    Url::parse(media_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| {
            ["youtube.com", "youtu.be", "youtube-nocookie.com"]
                .iter()
                .any(|root| host == *root || host.ends_with(&format!(".{root}")))
        })
}

fn run_executable<I, S>(executable: &Path, arguments: I) -> Result<ProcessOutput, YtDlpError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(executable);
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

fn flat_items_from_output(
    output: ProcessOutput,
    description: &str,
) -> Result<Vec<MediaItem>, YtDlpError> {
    let root = parse_json(output)?;
    let entries = root
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| YtDlpError::InvalidOutput(format!("{description} were missing")))?;
    Ok(entries.iter().filter_map(media_item_from_value).collect())
}

/// Python `youtube_shorts_search_url`.
fn youtube_shorts_search_url(query: &str) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.append_pair("search_query", query);
    serializer.append_pair("sp", "EgIQCQ==");
    format!("https://www.youtube.com/results?{}", serializer.finish())
}

/// Python `interleave_youtube_results`: up to four main results, then one
/// Short, skipping anything already listed.
fn interleave_youtube_results(
    primary: Vec<MediaItem>,
    shorts: Vec<MediaItem>,
    limit: usize,
) -> Vec<MediaItem> {
    fn identity(item: &MediaItem) -> String {
        let id = item.id.0.trim();
        let is_video_id = id.chars().count() == 11
            && id
                .chars()
                .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-'));
        if is_video_id {
            id.to_owned()
        } else {
            item.url
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default()
        }
    }
    fn append_next(
        items: &mut std::iter::Peekable<std::vec::IntoIter<MediaItem>>,
        merged: &mut Vec<MediaItem>,
        seen: &mut std::collections::HashSet<String>,
    ) -> bool {
        for item in items.by_ref() {
            let identity = identity(&item);
            if identity.is_empty() || !seen.insert(identity) {
                continue;
            }
            merged.push(item);
            return true;
        }
        false
    }
    let mut merged = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut primary = primary.into_iter().peekable();
    let mut shorts = shorts.into_iter().peekable();
    while merged.len() < limit && (primary.peek().is_some() || shorts.peek().is_some()) {
        for _slot in 0..4 {
            if merged.len() >= limit || !append_next(&mut primary, &mut merged, &mut seen) {
                break;
            }
        }
        if merged.len() >= limit {
            break;
        }
        if !append_next(&mut shorts, &mut merged, &mut seen) && primary.peek().is_none() {
            break;
        }
    }
    merged
}

fn read_downloaded_transcript(directory: &Path) -> Result<String, YtDlpError> {
    let files =
        std::fs::read_dir(directory).map_err(|error| YtDlpError::Request(error.to_string()))?;
    let mut candidates = Vec::new();
    for entry in files.flatten() {
        let path = entry.path();
        let extension = path.extension().and_then(OsStr::to_str).unwrap_or_default();
        if !["vtt", "srt"]
            .iter()
            .any(|expected| extension.eq_ignore_ascii_case(expected))
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file()
            || metadata.len() > 5_000_000
            || entry.file_type().map_or(true, |kind| kind.is_symlink())
        {
            continue;
        }
        candidates.push((
            metadata
                .modified()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            path,
        ));
    }
    candidates.sort_by_key(|(modified, _)| Reverse(*modified));
    for (_, path) in candidates {
        let Ok(file) = std::fs::File::open(path) else {
            continue;
        };
        let mut bytes = Vec::new();
        if file.take(5_000_001).read_to_end(&mut bytes).is_err() || bytes.len() > 5_000_000 {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if !text.trim().is_empty() {
            return Ok(text);
        }
    }
    Ok(String::new())
}

fn search_target(query: &str, kind: YoutubeSearchKind, limit: u32) -> String {
    if kind == YoutubeSearchKind::Video {
        return format!("ytsearch{limit}:{query}");
    }
    // Python `soundcloud_search_entries` for the Track type.
    if kind.is_soundcloud() {
        return format!("scsearch{limit}:{query}");
    }
    let filter = match kind {
        YoutubeSearchKind::Playlist => Some("EgIQAw=="),
        YoutubeSearchKind::Channel => Some("EgIQAg=="),
        YoutubeSearchKind::All
        | YoutubeSearchKind::Film
        | YoutubeSearchKind::Video
        | YoutubeSearchKind::SoundcloudTrack
        | YoutubeSearchKind::SoundcloudPlaylist
        | YoutubeSearchKind::SoundcloudUser => None,
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
        YoutubeSearchKind::All
        | YoutubeSearchKind::Video
        | YoutubeSearchKind::SoundcloudTrack
        | YoutubeSearchKind::SoundcloudPlaylist
        | YoutubeSearchKind::SoundcloudUser => requested,
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
                    YoutubeCapability::Metadata,
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
            YoutubeCommand::Collection { url, kind, limit } => {
                self.collection(&url, kind, Some(limit))
            }
            YoutubeCommand::CollectionAll { url, kind } => self.collection(&url, kind, None),
            YoutubeCommand::Metadata { urls } => self.metadata(&urls),
            YoutubeCommand::Resolve { url, preference } => self.resolve(&url, preference),
            YoutubeCommand::Shutdown => Ok(YoutubeResponsePayload::ShuttingDown),
        };
        result.map_err(|error| map_engine_error(&error, &self.config))
    }
}

fn collection_target(value: &str, kind: YoutubeCollectionKind) -> Result<String, YtDlpError> {
    if kind == YoutubeCollectionKind::PlaylistVideos {
        // SoundCloud sets open like YouTube playlists in Python.
        validate_media_url(value)?;
        return Ok(value.to_owned());
    }
    if kind == YoutubeCollectionKind::SoundcloudArtistTracks {
        validate_media_url(value)?;
        if !is_soundcloud_url(value) {
            return Err(YtDlpError::InvalidConfiguration(
                "artist tracks need a SoundCloud URL".to_owned(),
            ));
        }
        return Ok(format!("{}/tracks", value.trim().trim_end_matches('/')));
    }
    validate_youtube_url(value)?;
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
        YoutubeCollectionKind::PlaylistVideos | YoutubeCollectionKind::SoundcloudArtistTracks => {
            unreachable!("handled above")
        }
        YoutubeCollectionKind::ChannelVideos
        | YoutubeCollectionKind::ChannelUploads
        | YoutubeCollectionKind::ChannelPopular => "/videos",
        YoutubeCollectionKind::ChannelPlaylists => "/playlists",
        YoutubeCollectionKind::ChannelStreams => "/streams",
    };
    url.set_path(&format!("{path}{suffix}"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.into())
}

fn collection_response(items: &[MediaItem], limit: Option<u32>) -> YoutubeResponsePayload {
    let limit = limit
        .and_then(|limit| usize::try_from(limit).ok())
        .unwrap_or(usize::MAX);
    YoutubeResponsePayload::SearchResults {
        items: items.iter().take(limit).cloned().collect(),
        continuation: None,
    }
}

fn sort_popular_items(items: &mut [MediaItem]) {
    items.sort_by(|left, right| {
        popular_numeric_value(right, "view_count")
            .cmp(&popular_numeric_value(left, "view_count"))
            .then_with(|| popular_recency_value(right).cmp(&popular_recency_value(left)))
            .then_with(|| {
                right
                    .title
                    .to_ascii_lowercase()
                    .cmp(&left.title.to_ascii_lowercase())
            })
    });
}

fn popular_recency_value(item: &MediaItem) -> u64 {
    ["timestamp", "release_timestamp", "upload_date"]
        .iter()
        .find_map(|key| {
            let value = popular_numeric_value(item, key);
            (value > 0).then_some(value)
        })
        .unwrap_or(0)
}

fn popular_numeric_value(item: &MediaItem, key: &str) -> u64 {
    let Some(value) = item.metadata.get(key) else {
        return 0;
    };
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
        .or_else(|| {
            value
                .as_str()
                .map(|value| value.replace([',', ' '], ""))
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or(0)
}

fn collection_kind_accepts(kind: YoutubeCollectionKind, item: MediaKind) -> bool {
    match kind {
        YoutubeCollectionKind::ChannelPlaylists => item == MediaKind::Playlist,
        YoutubeCollectionKind::PlaylistVideos
        | YoutubeCollectionKind::ChannelVideos
        | YoutubeCollectionKind::ChannelUploads
        | YoutubeCollectionKind::ChannelStreams
        | YoutubeCollectionKind::ChannelPopular
        | YoutubeCollectionKind::SoundcloudArtistTracks => {
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

/// Media this backend resolves and lists: `YouTube` and, as in Python's
/// search provider, `SoundCloud`.
fn validate_media_url(value: &str) -> Result<(), YtDlpError> {
    if is_soundcloud_url(value) && value.len() <= MAX_MEDIA_URL_BYTES {
        return Ok(());
    }
    validate_youtube_url(value)
}

fn is_soundcloud_url(value: &str) -> bool {
    Url::parse(value.trim()).is_ok_and(|url| {
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        matches!(url.scheme(), "http" | "https")
            && (host == "soundcloud.com" || host.ends_with(".soundcloud.com"))
    })
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
    Err(YtDlpError::Request(process_error_text(&output)))
}

fn process_error_text(output: &ProcessOutput) -> String {
    let message = String::from_utf8_lossy(&output.stderr.bytes);
    message
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("yt-dlp exited without an error message")
        .trim()
        .to_owned()
}

fn parse_json(output: ProcessOutput) -> Result<Value, YtDlpError> {
    let bytes = checked_stdout(output)?;
    serde_json::from_slice(&bytes).map_err(|error| YtDlpError::InvalidOutput(error.to_string()))
}

fn parse_json_lines(output: ProcessOutput) -> Result<Vec<Value>, YtDlpError> {
    let bytes = checked_stdout(output)?;
    let mut values = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        values.push(
            serde_json::from_slice(line)
                .map_err(|error| YtDlpError::InvalidOutput(error.to_string()))?,
        );
    }
    Ok(values)
}

fn media_item_from_value(value: &Value) -> Option<MediaItem> {
    let object = value.as_object()?;
    let title = first_string(object, &["title", "username"])?
        .trim()
        .to_owned();
    if title.is_empty() {
        return None;
    }
    let raw_url = first_string(
        object,
        &["webpage_url", "permalink_url", "original_url", "url"],
    );
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
        "track",
        "artist",
        "creator",
        "album",
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
    // Python `normalize_entry` counts a SoundCloud set by `track_count`.
    insert_alias(
        &mut metadata,
        object,
        "playlist_count",
        &["track_count", "playlist_count"],
    );
    insert_alias(&mut metadata, object, "video_count", &["playlist_count"]);
    insert_alias(
        &mut metadata,
        object,
        "verified",
        &["channel_is_verified", "uploader_is_verified"],
    );
    insert_alias(&mut metadata, object, "uploaded_at", &["upload_date"]);
    // Python `normalize_entry`: a SoundCloud web API playlist names its owner
    // in a nested `user`.
    let user = object.get("user").and_then(Value::as_object);
    if let Some(owner_url) = user.and_then(|user| string(user, "permalink_url")) {
        metadata
            .entry("channel_url".to_owned())
            .or_insert_with(|| Value::String(owner_url.to_owned()));
    }
    let source = if is_soundcloud_entry(object, raw_url) {
        MediaSource::Soundcloud
    } else {
        MediaSource::Youtube
    };
    Some(MediaItem {
        id: MediaId(id),
        source,
        kind,
        title,
        url,
        stream_url: None,
        external_audio_url: None,
        local_path: None,
        channel: first_string(object, &["uploader", "channel", "channel_name"])
            .or_else(|| user.and_then(|user| string(user, "username")))
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

/// Python `normalize_entry`: an entry is from `SoundCloud` when its extractor
/// or URL says so.
fn is_soundcloud_entry(object: &Map<String, Value>, raw_url: Option<&str>) -> bool {
    first_string(object, &["ie_key", "extractor_key"])
        .is_some_and(|extractor| extractor.to_ascii_lowercase().contains("soundcloud"))
        || raw_url.is_some_and(|url| url.to_ascii_lowercase().contains("soundcloud"))
}

fn media_kind(object: &Map<String, Value>, raw_url: Option<&str>) -> MediaKind {
    let item_type = string(object, "_type").unwrap_or_default();
    let url = raw_url.unwrap_or_default().to_ascii_lowercase();
    if is_soundcloud_entry(object, raw_url) {
        let entry_kind = string(object, "kind").unwrap_or_default();
        return if entry_kind == "user" {
            MediaKind::Channel
        } else if entry_kind == "playlist" || item_type == "playlist" || url.contains("/sets/") {
            MediaKind::Playlist
        } else {
            MediaKind::Video
        };
    }
    if is_live_object(object) {
        return MediaKind::LiveStream;
    }
    let extractor = string(object, "ie_key").unwrap_or_default();
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
        YoutubeSearchKind::Video | YoutubeSearchKind::Film | YoutubeSearchKind::SoundcloudTrack => {
            matches!(media_kind, MediaKind::Video | MediaKind::LiveStream)
        }
        YoutubeSearchKind::Playlist | YoutubeSearchKind::SoundcloudPlaylist => {
            matches!(media_kind, MediaKind::Playlist)
        }
        YoutubeSearchKind::Channel | YoutubeSearchKind::SoundcloudUser => {
            matches!(media_kind, MediaKind::Channel)
        }
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
mod cookie_retry_tests {
    use super::*;

    #[test]
    fn cookies_come_first_with_the_cookie_user_agent() {
        let config = YoutubeSessionConfig {
            cookie_user_agent: Some(" Mozilla/5.0 test ".to_owned()),
            ..YoutubeSessionConfig::default()
        };
        let arguments = with_cookies(
            &config,
            r"C:\app\cookies.txt",
            &[OsString::from("--"), OsString::from("https://youtu.be/x")],
        );
        assert_eq!(
            arguments,
            [
                "--cookies",
                r"C:\app\cookies.txt",
                "--add-headers",
                "User-Agent:Mozilla/5.0 test",
                "--",
                "https://youtu.be/x",
            ]
            .map(OsString::from)
        );
        assert_eq!(
            cookie_arguments(&YoutubeSessionConfig::default(), "c.txt"),
            ["--cookies", "c.txt"].map(OsString::from)
        );
    }

    #[test]
    fn base_arguments_never_carry_cookies() {
        let directory = tempfile::tempdir().expect("dir");
        let executable = directory.path().join("yt-dlp.exe");
        let cookies = directory.path().join("cookies.txt");
        std::fs::write(&executable, b"").expect("exe");
        std::fs::write(&cookies, b"# HTTP Cookie File\n").expect("cookies");
        let mut engine = YtDlpYoutubeEngine::new(&executable).expect("engine");
        engine
            .configure(YoutubeSessionConfig {
                cookies_file: Some(cookies.to_string_lossy().into_owned()),
                ..YoutubeSessionConfig::default()
            })
            .expect("configure");
        assert!(
            !engine
                .base_arguments()
                .contains(&OsString::from("--cookies"))
        );
    }

    #[test]
    fn playback_cookie_rule_knows_youtube_hosts() {
        assert!(is_youtube_media_url("https://www.youtube.com/watch?v=x"));
        assert!(is_youtube_media_url("https://youtu.be/x"));
        assert!(!is_youtube_media_url("https://soundcloud.com/a/b"));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn subtitle_fallback_keeps_outputs_local_and_skips_unusable_files() {
        let directory = tempfile::tempdir().unwrap();
        let engine = super::YtDlpYoutubeEngine::new(&std::env::current_exe().unwrap()).unwrap();
        let args = engine
            .transcript_download_arguments(
                "https://example.test/video",
                &["en".to_owned()],
                directory.path(),
            )
            .unwrap();
        assert!(!args.iter().any(|arg| arg == "--dump-single-json"));
        assert!(args.iter().any(|arg| arg == "--skip-download"));
        assert!(args.contains(&directory.path().join("caption.%(ext)s").into_os_string()));
        std::fs::write(directory.path().join("caption.mp4"), "not subtitles").unwrap();
        std::fs::write(directory.path().join("empty.srt"), "  ").unwrap();
        std::fs::write(directory.path().join("caption.en.vtt"), "WEBVTT\n").unwrap();
        assert_eq!(
            super::read_downloaded_transcript(directory.path()).unwrap(),
            "WEBVTT\n"
        );
    }
    #[test]
    fn transcript_arguments_are_on_demand_metadata_only_and_option_safe() {
        let engine = super::YtDlpYoutubeEngine::new(&std::env::current_exe().unwrap()).unwrap();
        let source = "https://example.test/watch?id=one&other=two";
        let args = engine
            .transcript_arguments(source, &["sl".to_owned(), "en".to_owned()])
            .unwrap();
        let args: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
        assert!(args.iter().any(|arg| arg == "--dump-single-json"));
        assert!(args.iter().any(|arg| arg == "--skip-download"));
        assert!(args.iter().any(|arg| arg == "sl,en"));
        assert_eq!(&args[args.len() - 2..], ["--", source]);
        for invalid in [
            "file:///C:/secret",
            "--exec=bad",
            "https://user:password@example.test/video",
        ] {
            assert!(engine.transcript_arguments(invalid, &[]).is_err());
        }
    }
    #[test]
    fn comment_arguments_fetch_twenty_comments_without_download() {
        let engine = super::YtDlpYoutubeEngine::new(&std::env::current_exe().unwrap()).unwrap();
        let source = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        let args = engine.comments_arguments(source).unwrap();
        let args: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
        for expected in [
            "--write-comments",
            "youtube:max_comments=20",
            "--skip-download",
            "--no-playlist",
            "--dump-single-json",
        ] {
            assert!(args.iter().any(|arg| arg == expected), "{expected}");
        }
        assert_eq!(&args[args.len() - 2..], ["--", source]);
        for invalid in [
            "file:///C:/secret",
            "--exec=bad",
            "https://user:pw@example.test/v",
        ] {
            assert!(engine.comments_arguments(invalid).is_err());
        }
    }
    #[test]
    fn music_metadata_survives_extraction_for_lyrics_lookup() {
        let item = super::media_item_from_value(&serde_json::json!({
            "id": "track-id",
            "title": "Promotional upload title",
            "webpage_url": "https://www.youtube.com/watch?v=track-id",
            "channel": "Label Channel",
            "track": "Actual Song",
            "artist": "Recording Artist",
            "creator": "Composer",
            "album": "Actual Album",
            "duration": 180
        }))
        .unwrap();
        let query = crate::lyrics::LyricsQuery::from_item(&item);
        assert_eq!(query.title, "Actual Song");
        assert_eq!(query.artist, "Recording Artist");
        assert_eq!(query.album, "Actual Album");
        assert_eq!(query.duration_seconds, 180);
        assert_eq!(item.metadata["creator"], "Composer");
    }

    use super::{
        BoundedBytes, YtDlpYoutubeEngine, collection_kind_accepts, collection_response,
        collection_target, component_executable, interleave_youtube_results, media_item_from_value,
        popular_numeric_value, read_bounded, sanitize_error, search_fetch_limit,
        search_kind_accepts, search_target, sort_formats, sort_popular_items, validate_media_url,
        youtube_format, youtube_shorts_search_url,
    };
    use apricot_core::{MediaItem, MediaKind, MediaSource};
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

    fn video(id: &str) -> MediaItem {
        media_item_from_value(&json!({
            "id": id, "title": id, "url": format!("https://www.youtube.com/watch?v={id}")
        }))
        .expect("video")
    }

    fn ids(items: &[MediaItem]) -> Vec<&str> {
        items.iter().map(|item| item.id.0.as_str()).collect()
    }

    #[test]
    fn shorts_are_mixed_in_after_every_four_results_like_python() {
        let primary = [
            "aaaaaaaaaa1",
            "aaaaaaaaaa2",
            "aaaaaaaaaa3",
            "aaaaaaaaaa4",
            "aaaaaaaaaa5",
        ]
        .map(video)
        .to_vec();
        // The duplicate Short is skipped, as Python's `seen` set does.
        let shorts = ["aaaaaaaaaa2", "sssssssssS1", "sssssssssS2"]
            .map(video)
            .to_vec();
        let merged = interleave_youtube_results(primary.clone(), shorts.clone(), 20);
        assert_eq!(
            ids(&merged),
            [
                "aaaaaaaaaa1",
                "aaaaaaaaaa2",
                "aaaaaaaaaa3",
                "aaaaaaaaaa4",
                "sssssssssS1",
                "aaaaaaaaaa5",
                "sssssssssS2",
            ]
        );
        assert_eq!(
            ids(&interleave_youtube_results(primary, shorts, 5)),
            [
                "aaaaaaaaaa1",
                "aaaaaaaaaa2",
                "aaaaaaaaaa3",
                "aaaaaaaaaa4",
                "sssssssssS1",
            ]
        );
        assert_eq!(
            youtube_shorts_search_url("open ai"),
            "https://www.youtube.com/results?search_query=open+ai&sp=EgIQCQ%3D%3D"
        );
    }

    #[test]
    fn soundcloud_entries_keep_python_provider_kinds_and_urls() {
        let track = media_item_from_value(&json!({
            "_type": "url", "ie_key": "Soundcloud", "id": "88335161",
            "title": "Get Lucky", "uploader": "DJ KB",
            "webpage_url": "https://soundcloud.com/eldjkb/get-lucky",
            "url": "https://api.soundcloud.com/tracks/soundcloud%3Atracks%3A88335161",
            "duration": 246.4, "view_count": 21_220_721
        }))
        .expect("track");
        assert_eq!(track.source, MediaSource::Soundcloud);
        assert_eq!(track.kind, MediaKind::Video);
        assert_eq!(
            track.url.as_ref().map(ToString::to_string).as_deref(),
            Some("https://soundcloud.com/eldjkb/get-lucky")
        );
        assert_eq!(track.channel, "DJ KB");
        let set = media_item_from_value(&json!({
            "id": "1", "title": "Set", "track_count": 12,
            "permalink_url": "https://soundcloud.com/artist/sets/best"
        }))
        .expect("set");
        assert_eq!(set.source, MediaSource::Soundcloud);
        assert_eq!(set.kind, MediaKind::Playlist);
        assert_eq!(set.metadata["playlist_count"], 12);
        let api_set = media_item_from_value(&json!({
            "kind": "playlist", "id": 3, "title": "Web set", "track_count": 4,
            "permalink_url": "https://soundcloud.com/owner/sets/web",
            "user": {"username": "Owner", "permalink_url": "https://soundcloud.com/owner"}
        }))
        .expect("api set");
        assert_eq!(api_set.kind, MediaKind::Playlist);
        assert_eq!(api_set.channel, "Owner");
        assert_eq!(
            api_set.metadata["channel_url"],
            "https://soundcloud.com/owner"
        );
        let user = media_item_from_value(&json!({
            "kind": "user", "id": "2", "username": "Artist",
            "permalink_url": "https://soundcloud.com/artist"
        }))
        .expect("user");
        assert_eq!(user.kind, MediaKind::Channel);
        assert_eq!(user.title, "Artist");
        assert!(search_kind_accepts(
            YoutubeSearchKind::SoundcloudTrack,
            track.kind
        ));
        assert!(search_kind_accepts(
            YoutubeSearchKind::SoundcloudUser,
            user.kind
        ));
        assert!(!search_kind_accepts(
            YoutubeSearchKind::SoundcloudTrack,
            set.kind
        ));
    }

    #[test]
    fn soundcloud_targets_follow_python_search_and_artist_tracks() {
        assert_eq!(
            search_target("daft punk", YoutubeSearchKind::SoundcloudTrack, 20),
            "scsearch20:daft punk"
        );
        assert_eq!(
            collection_target(
                "https://soundcloud.com/daftpunkofficialmusic/",
                YoutubeCollectionKind::SoundcloudArtistTracks,
            )
            .expect("artist tracks"),
            "https://soundcloud.com/daftpunkofficialmusic/tracks"
        );
        let set = "https://soundcloud.com/artist/sets/best";
        assert_eq!(
            collection_target(set, YoutubeCollectionKind::PlaylistVideos).expect("set"),
            set
        );
        assert!(
            collection_target(
                "https://www.youtube.com/@creator",
                YoutubeCollectionKind::SoundcloudArtistTracks,
            )
            .is_err()
        );
        assert!(
            collection_target(
                "https://soundcloud.com/artist",
                YoutubeCollectionKind::ChannelVideos,
            )
            .is_err()
        );
        assert_eq!(
            collection_target(
                "https://www.youtube.com/@creator",
                YoutubeCollectionKind::ChannelUploads,
            )
            .expect("uploads"),
            "https://www.youtube.com/@creator/videos"
        );
        assert!(validate_media_url("https://soundcloud.com/artist/track").is_ok());
        assert!(validate_media_url("https://notsoundcloud.com/artist/track").is_err());
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
            "https://www.youtube.com/channel/UC123/videos"
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
    fn popular_channel_items_are_globally_sorted_before_limiting() {
        let mut items = vec![
            media_item_from_value(&json!({
                "id": "new", "title": "Newest", "view_count": 998,
                "timestamp": 300
            }))
            .expect("newest"),
            media_item_from_value(&json!({
                "id": "top", "title": "All-time top", "view_count": 37_000_000,
                "timestamp": 100
            }))
            .expect("top"),
            media_item_from_value(&json!({
                "id": "middle", "title": "Middle", "view_count": "3,300",
                "timestamp": 200
            }))
            .expect("middle"),
        ];
        sort_popular_items(&mut items);
        let YoutubeResponsePayload::SearchResults { items, .. } =
            collection_response(&items, Some(2))
        else {
            panic!("collection results");
        };
        assert_eq!(
            items
                .iter()
                .map(|item| item.id.0.as_str())
                .collect::<Vec<_>>(),
            ["top", "middle"]
        );
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
            ..YoutubeSessionConfig::default()
        };
        let message = sanitize_error(
            "failed C:\\private\\cookies.txt through http://name:secret@proxy.test",
            &config,
        );
        assert_eq!(message, "failed [cookies file] through [proxy]");
    }

    #[test]
    #[ignore = "requires live SoundCloud, YouTube and APRICOT_YTDLP"]
    fn live_soundcloud_and_shorts_follow_python() {
        let executable = std::env::var_os("APRICOT_YTDLP").expect("APRICOT_YTDLP");
        let mut engine =
            YtDlpYoutubeEngine::new(std::path::Path::new(&executable)).expect("standalone yt-dlp");
        let tracks = match engine
            .execute(YoutubeCommand::Search {
                query: "daft punk".to_owned(),
                kind: YoutubeSearchKind::SoundcloudTrack,
                limit: 5,
                safe_search: false,
            })
            .expect("SoundCloud search")
        {
            YoutubeResponsePayload::SearchResults { items, .. } => items,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(tracks.len(), 5);
        assert!(
            tracks
                .iter()
                .all(|item| item.source == MediaSource::Soundcloud)
        );
        for (kind, media_kind) in [
            (YoutubeSearchKind::SoundcloudPlaylist, MediaKind::Playlist),
            (YoutubeSearchKind::SoundcloudUser, MediaKind::Channel),
        ] {
            let items = match engine
                .execute(YoutubeCommand::Search {
                    query: "daft punk".to_owned(),
                    kind,
                    limit: 5,
                    safe_search: false,
                })
                .expect("SoundCloud web API search")
            {
                YoutubeResponsePayload::SearchResults { items, .. } => items,
                other => panic!("unexpected {other:?}"),
            };
            assert_eq!(items.len(), 5, "{kind:?}");
            assert!(
                items
                    .iter()
                    .all(|item| item.source == MediaSource::Soundcloud
                        && item.kind == media_kind
                        && item.url.is_some())
            );
        }
        let resolved = engine
            .execute(YoutubeCommand::Resolve {
                url: tracks[0].url.as_ref().expect("track URL").to_string(),
                preference: YoutubeStreamPreference::PreferAudio,
            })
            .expect("SoundCloud resolve");
        assert!(matches!(
            resolved,
            YoutubeResponsePayload::Resolved { item, formats }
                if item.source == MediaSource::Soundcloud && !formats.is_empty()
        ));
        let artist = engine
            .execute(YoutubeCommand::Collection {
                url: "https://soundcloud.com/daftpunkofficialmusic".to_owned(),
                kind: YoutubeCollectionKind::SoundcloudArtistTracks,
                limit: 3,
            })
            .expect("artist tracks");
        assert!(matches!(
            artist,
            YoutubeResponsePayload::SearchResults { items, .. } if items.len() == 3
        ));
        let uploads = match engine
            .execute(YoutubeCommand::Collection {
                url: "https://www.youtube.com/@YouTube".to_owned(),
                kind: YoutubeCollectionKind::ChannelUploads,
                limit: 10,
            })
            .expect("channel uploads")
        {
            YoutubeResponsePayload::SearchResults { items, .. } => items,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(uploads.len(), 10);
        assert!(
            uploads.iter().any(|item| item
                .url
                .as_ref()
                .is_some_and(|url| url.path().starts_with("/shorts/"))),
            "the videos tab mixes in Shorts"
        );
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
        let metadata = engine
            .execute(YoutubeCommand::Metadata {
                urls: vec![
                    "https://www.youtube.com/watch?v=jNQXAC9IVRw".to_owned(),
                    "https://www.youtube.com/watch?v=aqz-KE-bpKQ".to_owned(),
                ],
            })
            .expect("live metadata");
        assert!(matches!(
            metadata,
            YoutubeResponsePayload::Hydrated { items }
                if items.len() == 2
                    && items.iter().all(|item| {
                        item.metadata.contains_key("view_count")
                            && item.metadata.contains_key("upload_date")
                    })
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

    #[test]
    #[ignore = "requires live YouTube and APRICOT_YTDLP"]
    fn live_popular_channel_is_global_and_cumulative() {
        let executable = std::env::var_os("APRICOT_YTDLP").expect("APRICOT_YTDLP");
        let mut engine =
            YtDlpYoutubeEngine::new(std::path::Path::new(&executable)).expect("standalone yt-dlp");
        let url = "https://www.youtube.com/@OpenAI".to_owned();
        let first = engine
            .execute(YoutubeCommand::Collection {
                url: url.clone(),
                kind: YoutubeCollectionKind::ChannelPopular,
                limit: 5,
            })
            .expect("first popular page");
        let second = engine
            .execute(YoutubeCommand::Collection {
                url,
                kind: YoutubeCollectionKind::ChannelPopular,
                limit: 10,
            })
            .expect("second popular page");
        let YoutubeResponsePayload::SearchResults {
            items: first_items, ..
        } = first
        else {
            panic!("first collection results");
        };
        let YoutubeResponsePayload::SearchResults {
            items: second_items,
            ..
        } = second
        else {
            panic!("second collection results");
        };
        assert_eq!(first_items.len(), 5);
        assert_eq!(second_items.len(), 10);
        assert_eq!(first_items, second_items[..5]);
        assert!(second_items.windows(2).all(|pair| {
            popular_numeric_value(&pair[0], "view_count")
                >= popular_numeric_value(&pair[1], "view_count")
        }));
    }
}
