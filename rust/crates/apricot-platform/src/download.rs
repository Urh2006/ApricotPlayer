//! Structured, cancellable media downloads through the bundled standalone
//! `yt-dlp` component. The blocking adapter is intended to run on a worker
//! thread; it never invokes a command shell and streams bounded progress back
//! to the caller.

use std::{
    collections::HashSet,
    ffi::OsString,
    fs,
    io::{self, BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, SyncSender},
    },
    thread,
    time::Duration,
};

use thiserror::Error;
use url::Url;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const MAX_MEDIA_URL_BYTES: usize = 16_384;
const MAX_TEMPLATE_BYTES: usize = 4_096;
const MAX_OUTPUT_LINE_BYTES: usize = 128 * 1_024;
const MAX_DIAGNOSTIC_BYTES: usize = 1_024 * 1_024;
const OUTPUT_CHANNEL_CAPACITY: usize = 256;
const DEFAULT_FILENAME_TEMPLATE: &str = "%(title)s.%(ext)s";
const PROGRESS_PREFIX: &str = "APRICOT_PROGRESS:";
const PROCESS_PREFIX: &str = "APRICOT_PROCESS:";
const FINISHED_PREFIX: &str = "APRICOT_FINISHED:";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadMode {
    Audio,
    Video,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VideoDownloadFormat {
    Mp4,
    BestAvailable,
    Mp4SingleFile,
    Smallest,
}

impl VideoDownloadFormat {
    pub fn from_setting(value: &str) -> Self {
        match value.trim() {
            "best-any" => Self::BestAvailable,
            "mp4-single" => Self::Mp4SingleFile,
            "smallest" => Self::Smallest,
            _ => Self::Mp4,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct DownloadOptions {
    pub audio_format: String,
    pub audio_quality: String,
    pub video_format: VideoDownloadFormat,
    pub max_video_height: u32,
    pub quiet: bool,
    pub keep_playlist_order: bool,
    pub filename_template: String,
    pub write_thumbnail: bool,
    pub write_description: bool,
    pub write_info_json: bool,
    pub write_subtitles: bool,
    pub write_automatic_subtitles: bool,
    pub subtitle_languages: Vec<String>,
    pub embed_metadata: bool,
    pub embed_thumbnail: bool,
    pub restrict_filenames: bool,
    pub concurrent_fragments: u32,
    pub retries: u32,
    pub socket_timeout_seconds: u32,
    pub rate_limit: Option<String>,
    pub proxy_url: Option<String>,
    pub cookies_file: Option<PathBuf>,
    pub ffmpeg_location: Option<PathBuf>,
    pub download_archive: Option<PathBuf>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            audio_format: "mp3".to_owned(),
            audio_quality: "0".to_owned(),
            video_format: VideoDownloadFormat::Mp4,
            max_video_height: 1_080,
            quiet: false,
            keep_playlist_order: true,
            filename_template: DEFAULT_FILENAME_TEMPLATE.to_owned(),
            write_thumbnail: false,
            write_description: false,
            write_info_json: false,
            write_subtitles: false,
            write_automatic_subtitles: false,
            subtitle_languages: vec!["sl".to_owned(), "en".to_owned()],
            embed_metadata: true,
            embed_thumbnail: false,
            restrict_filenames: false,
            concurrent_fragments: 4,
            retries: 10,
            socket_timeout_seconds: 20,
            rate_limit: None,
            proxy_url: None,
            cookies_file: None,
            ffmpeg_location: None,
            download_archive: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadRequest {
    pub url: String,
    pub title: String,
    pub mode: DownloadMode,
    pub output_directory: PathBuf,
    pub target_path: Option<PathBuf>,
    pub allow_playlist: bool,
    pub options: DownloadOptions,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadPhase {
    Downloading,
    Processing,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DownloadEvent {
    Progress {
        phase: DownloadPhase,
        title: String,
        percent: Option<f64>,
        playlist_index: Option<u32>,
        playlist_count: Option<u32>,
    },
    ItemFailed {
        message: String,
    },
    FileFinished {
        title: String,
        path: PathBuf,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DownloadSummary {
    pub files: Vec<PathBuf>,
    pub item_failures: Vec<String>,
}

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("invalid download configuration: {0}")]
    InvalidConfiguration(String),
    #[error("could not create the download folder: {0}")]
    CreateFolder(String),
    #[error("could not launch yt-dlp: {0}")]
    Launch(String),
    #[error("download cancelled")]
    Cancelled,
    #[error("yt-dlp output exceeded its safety limit")]
    OutputLimit,
    #[error("yt-dlp download failed: {0}")]
    Request(String),
}

#[derive(Clone, Debug)]
pub struct YtDlpDownloader {
    executable: PathBuf,
}

impl YtDlpDownloader {
    /// Creates a downloader for the trusted bundled component.
    ///
    /// # Errors
    ///
    /// Returns an error when the executable is missing or not a regular file.
    pub fn new(executable: &Path) -> Result<Self, DownloadError> {
        if !executable.is_file() {
            return Err(DownloadError::InvalidConfiguration(
                "the bundled yt-dlp executable was not found".to_owned(),
            ));
        }
        Ok(Self {
            executable: executable.to_owned(),
        })
    }

    /// Downloads one item or one source-owned collection. The caller must run
    /// this method away from the UI thread.
    ///
    /// # Errors
    ///
    /// Returns a validation, process, cancellation, or yt-dlp request error.
    pub fn download<F>(
        &self,
        request: &DownloadRequest,
        cancelled: &Arc<AtomicBool>,
        mut emit: F,
    ) -> Result<DownloadSummary, DownloadError>
    where
        F: FnMut(DownloadEvent),
    {
        validate_request(request)?;
        fs::create_dir_all(&request.output_directory)
            .map_err(|error| DownloadError::CreateFolder(error.to_string()))?;
        let arguments = build_arguments(request)?;
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
            .map_err(|error| DownloadError::Launch(error.to_string()))?;
        #[cfg(windows)]
        let _kill_on_close_job = process_job::KillOnCloseJob::attach(&child);
        collect_download(&mut child, request, cancelled, &mut emit)
    }
}

fn validate_request(request: &DownloadRequest) -> Result<(), DownloadError> {
    let value = request.url.trim();
    if value.is_empty() || value.len() > MAX_MEDIA_URL_BYTES {
        return Err(DownloadError::InvalidConfiguration(
            "the media URL is empty or too long".to_owned(),
        ));
    }
    let url = Url::parse(value)
        .map_err(|_| DownloadError::InvalidConfiguration("the media URL is invalid".to_owned()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(DownloadError::InvalidConfiguration(
            "only HTTP and HTTPS media URLs can be downloaded".to_owned(),
        ));
    }
    if !request.output_directory.is_absolute() {
        return Err(DownloadError::InvalidConfiguration(
            "the download folder must be an absolute path".to_owned(),
        ));
    }
    if request
        .target_path
        .as_ref()
        .is_some_and(|path| !path.is_absolute())
    {
        return Err(DownloadError::InvalidConfiguration(
            "the selected download path must be absolute".to_owned(),
        ));
    }
    validate_options(&request.options)
}

fn validate_options(options: &DownloadOptions) -> Result<(), DownloadError> {
    if !matches!(
        options.audio_format.trim().to_ascii_lowercase().as_str(),
        "mp3" | "m4a" | "opus" | "wav" | "flac"
    ) {
        return Err(DownloadError::InvalidConfiguration(
            "the selected audio format is unsupported".to_owned(),
        ));
    }
    if options.filename_template.len() > MAX_TEMPLATE_BYTES {
        return Err(DownloadError::InvalidConfiguration(
            "the filename template is too long".to_owned(),
        ));
    }
    if let Some(path) = &options.cookies_file
        && !path.is_file()
    {
        return Err(DownloadError::InvalidConfiguration(
            "the configured cookies file was not found".to_owned(),
        ));
    }
    if let Some(path) = &options.ffmpeg_location
        && !path.exists()
    {
        return Err(DownloadError::InvalidConfiguration(
            "the configured FFmpeg location was not found".to_owned(),
        ));
    }
    if let Some(proxy) = options.proxy_url.as_deref() {
        let parsed = Url::parse(proxy).map_err(|_| {
            DownloadError::InvalidConfiguration("the proxy URL is invalid".to_owned())
        })?;
        if !matches!(
            parsed.scheme(),
            "http" | "https" | "socks4" | "socks4a" | "socks5" | "socks5h"
        ) {
            return Err(DownloadError::InvalidConfiguration(
                "the proxy URL uses an unsupported scheme".to_owned(),
            ));
        }
    }
    Ok(())
}

fn build_arguments(request: &DownloadRequest) -> Result<Vec<OsString>, DownloadError> {
    let options = &request.options;
    let mut arguments = vec![
        "--ignore-config".into(),
        "--no-plugin-dirs".into(),
        "--newline".into(),
        "--progress".into(),
        "--no-colors".into(),
        "--progress-template".into(),
        progress_template("download", PROGRESS_PREFIX, "downloading").into(),
        "--progress-template".into(),
        progress_template("postprocess", PROCESS_PREFIX, "processing").into(),
        "--print".into(),
        format!("after_move:{FINISHED_PREFIX}%(title)j\t%(filepath)j").into(),
        "--output".into(),
        output_template(request)?.into_os_string(),
        "--concurrent-fragments".into(),
        match request.mode {
            DownloadMode::Audio => options.concurrent_fragments.max(1),
            DownloadMode::Video => options.concurrent_fragments.max(8),
        }
        .to_string()
        .into(),
        "--retries".into(),
        options.retries.to_string().into(),
        "--socket-timeout".into(),
        options.socket_timeout_seconds.max(1).to_string().into(),
    ];
    arguments.push(if options.quiet {
        "--quiet".into()
    } else {
        "--no-quiet".into()
    });
    arguments.push(if request.allow_playlist {
        "--yes-playlist".into()
    } else {
        "--no-playlist".into()
    });
    if request.allow_playlist {
        arguments.extend([
            "--ignore-errors".into(),
            "--skip-unavailable-fragments".into(),
        ]);
    }
    append_feature_arguments(&mut arguments, options);
    append_location_arguments(&mut arguments, options);
    append_mode_arguments(&mut arguments, request.mode, options);
    arguments.extend(["--".into(), request.url.trim().into()]);
    Ok(arguments)
}

fn append_feature_arguments(arguments: &mut Vec<OsString>, options: &DownloadOptions) {
    for (enabled, flag) in [
        (options.write_thumbnail, "--write-thumbnail"),
        (options.write_description, "--write-description"),
        (options.write_info_json, "--write-info-json"),
        (options.write_subtitles, "--write-subs"),
        (options.write_automatic_subtitles, "--write-auto-subs"),
        (options.embed_metadata, "--embed-metadata"),
        (options.embed_thumbnail, "--embed-thumbnail"),
        (options.restrict_filenames, "--restrict-filenames"),
    ] {
        if enabled {
            arguments.push(flag.into());
        }
    }
    if (options.write_subtitles || options.write_automatic_subtitles)
        && !options.subtitle_languages.is_empty()
    {
        arguments.extend([
            "--sub-langs".into(),
            options.subtitle_languages.join(",").into(),
        ]);
    }
}

fn append_location_arguments(arguments: &mut Vec<OsString>, options: &DownloadOptions) {
    push_optional(arguments, "--limit-rate", options.rate_limit.as_deref());
    push_optional(arguments, "--proxy", options.proxy_url.as_deref());
    push_optional_path(arguments, "--cookies", options.cookies_file.as_deref());
    push_optional_path(
        arguments,
        "--ffmpeg-location",
        options.ffmpeg_location.as_deref(),
    );
    push_optional_path(
        arguments,
        "--download-archive",
        options.download_archive.as_deref(),
    );
}

fn append_mode_arguments(
    arguments: &mut Vec<OsString>,
    mode: DownloadMode,
    options: &DownloadOptions,
) {
    match mode {
        DownloadMode::Audio => arguments.extend([
            "--format".into(),
            "bestaudio/best".into(),
            "--extract-audio".into(),
            "--audio-format".into(),
            options.audio_format.trim().to_ascii_lowercase().into(),
            "--audio-quality".into(),
            options.audio_quality.trim().into(),
        ]),
        DownloadMode::Video => {
            arguments.extend([
                "--format".into(),
                video_format_selector(options.video_format, options.max_video_height).into(),
                "--http-chunk-size".into(),
                "10M".into(),
                "--buffer-size".into(),
                "1M".into(),
                "--progress-delta".into(),
                "0.5".into(),
            ]);
            if matches!(
                options.video_format,
                VideoDownloadFormat::Mp4
                    | VideoDownloadFormat::Mp4SingleFile
                    | VideoDownloadFormat::Smallest
            ) {
                arguments.extend(["--merge-output-format".into(), "mp4".into()]);
            }
        }
    }
}

fn progress_template(kind: &str, prefix: &str, phase: &str) -> String {
    format!(
        "{kind}:{prefix}{phase}\t%(progress._percent_str)j\t%(info.title)j\t%(info.playlist_index)j\t%(info.playlist_count)j"
    )
}

fn output_template(request: &DownloadRequest) -> Result<PathBuf, DownloadError> {
    if let Some(target) = &request.target_path {
        if request.mode == DownloadMode::Audio {
            let parent = target.parent().ok_or_else(|| {
                DownloadError::InvalidConfiguration("the selected path has no parent".to_owned())
            })?;
            let stem = target.file_stem().ok_or_else(|| {
                DownloadError::InvalidConfiguration("the selected path has no filename".to_owned())
            })?;
            return Ok(parent.join(format!("{}.%(ext)s", stem.to_string_lossy())));
        }
        return Ok(target.clone());
    }
    let mut template = safe_filename_template(&request.options.filename_template).to_owned();
    if request.allow_playlist
        && request.options.keep_playlist_order
        && !template.contains("%(playlist_index)")
    {
        template.insert_str(0, "%(playlist_index)s - ");
    }
    Ok(request.output_directory.join(template))
}

fn safe_filename_template(value: &str) -> &str {
    let template = value.trim();
    if template.is_empty()
        || template.as_bytes().contains(&0)
        || template.starts_with(['/', '\\'])
        || template.contains(':')
        || template
            .replace('\\', "/")
            .split('/')
            .filter(|component| !component.is_empty())
            .any(invalid_template_component)
    {
        DEFAULT_FILENAME_TEMPLATE
    } else {
        template
    }
}

fn invalid_template_component(component: &str) -> bool {
    matches!(component, "." | "..")
        || component.trim_end_matches([' ', '.']) != component
        || is_windows_reserved_stem(component)
}

fn is_windows_reserved_stem(component: &str) -> bool {
    let stem = component
        .split_once('.')
        .map_or(component, |(stem, _)| stem)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn video_format_selector(format: VideoDownloadFormat, height: u32) -> String {
    let limit = if height == 0 {
        String::new()
    } else {
        format!("[height<={height}]")
    };
    match format {
        VideoDownloadFormat::BestAvailable => {
            format!("bestvideo{limit}+bestaudio/best{limit}/best")
        }
        VideoDownloadFormat::Mp4SingleFile => format!(
            "best[ext=mp4][vcodec!=none][acodec!=none]{limit}/best[ext=mp4][vcodec!=none][acodec!=none]/best{limit}/best"
        ),
        VideoDownloadFormat::Smallest => format!(
            "worst[ext=mp4][vcodec!=none][acodec!=none]{limit}/worst[ext=mp4][vcodec!=none][acodec!=none]/worst{limit}/worst"
        ),
        VideoDownloadFormat::Mp4 => format!(
            "best[ext=mp4][vcodec!=none][acodec!=none]{limit}/best[ext=mp4][vcodec!=none][acodec!=none]/bestvideo[ext=mp4]{limit}+bestaudio[ext=m4a]/bestvideo{limit}+bestaudio/best{limit}/best"
        ),
    }
}

fn push_optional(arguments: &mut Vec<OsString>, flag: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        arguments.extend([flag.into(), value.into()]);
    }
}

fn push_optional_path(arguments: &mut Vec<OsString>, flag: &str, value: Option<&Path>) {
    if let Some(value) = value {
        arguments.extend([flag.into(), value.as_os_str().to_owned()]);
    }
}

enum ProcessLine {
    Text { stderr: bool, value: String },
    Overflow,
    Done,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BoundedLineRead {
    Line,
    Overflow,
    End,
}

fn collect_download<F>(
    child: &mut Child,
    request: &DownloadRequest,
    cancelled: &Arc<AtomicBool>,
    emit: &mut F,
) -> Result<DownloadSummary, DownloadError>
where
    F: FnMut(DownloadEvent),
{
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| DownloadError::Launch("stdout pipe was not created".to_owned()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| DownloadError::Launch("stderr pipe was not created".to_owned()))?;
    let (sender, receiver) = mpsc::sync_channel(OUTPUT_CHANNEL_CAPACITY);
    let stdout_reader = spawn_line_reader(stdout, false, sender.clone());
    let stderr_reader = spawn_line_reader(stderr, true, sender);
    let mut summary = DownloadSummary::default();
    let mut diagnostics = String::new();
    let mut seen_failures = HashSet::new();
    let mut status: Option<ExitStatus> = None;
    let mut readers_done = 0;
    let mut terminal_error = None;
    while status.is_none() || readers_done < 2 {
        if cancelled.load(Ordering::Acquire) {
            stop_process_tree(child);
            terminal_error = Some(DownloadError::Cancelled);
            break;
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(next_status) => status = next_status,
                Err(error) => {
                    stop_process_tree(child);
                    terminal_error = Some(DownloadError::Request(error.to_string()));
                    break;
                }
            }
        }
        match receiver.recv_timeout(POLL_INTERVAL) {
            Ok(ProcessLine::Text { stderr, value }) => {
                handle_process_line(
                    &value,
                    stderr,
                    request,
                    &mut summary,
                    &mut diagnostics,
                    &mut seen_failures,
                    emit,
                );
            }
            Ok(ProcessLine::Overflow) => {
                stop_process_tree(child);
                terminal_error = Some(DownloadError::OutputLimit);
                break;
            }
            Ok(ProcessLine::Done) => readers_done += 1,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => readers_done = 2,
        }
    }
    drop(receiver);
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
    if let Some(error) = terminal_error {
        return Err(error);
    }
    if status.is_some_and(|status| status.success()) {
        return Ok(summary);
    }
    let error = redact_diagnostic(
        diagnostics.trim(),
        request.options.cookies_file.as_deref(),
        request.options.proxy_url.as_deref(),
    );
    Err(DownloadError::Request(if error.is_empty() {
        "the component exited without an error message".to_owned()
    } else {
        error
    }))
}

fn spawn_line_reader<R: Read + Send + 'static>(
    reader: R,
    stderr: bool,
    sender: SyncSender<ProcessLine>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut bytes = Vec::new();
        loop {
            match read_bounded_line(&mut reader, &mut bytes, MAX_OUTPUT_LINE_BYTES) {
                Ok(BoundedLineRead::End) => break,
                Ok(BoundedLineRead::Overflow) => {
                    if sender.send(ProcessLine::Overflow).is_err() {
                        return;
                    }
                }
                Ok(BoundedLineRead::Line) => {
                    let value = String::from_utf8_lossy(&bytes)
                        .trim_end_matches(['\r', '\n'])
                        .to_owned();
                    if sender.send(ProcessLine::Text { stderr, value }).is_err() {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.send(ProcessLine::Text {
                        stderr: true,
                        value: error.to_string(),
                    });
                    break;
                }
            }
        }
        let _ = sender.send(ProcessLine::Done);
    })
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    output: &mut Vec<u8>,
    maximum_bytes: usize,
) -> io::Result<BoundedLineRead> {
    output.clear();
    let mut overflowed = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(if overflowed {
                BoundedLineRead::Overflow
            } else if output.is_empty() {
                BoundedLineRead::End
            } else {
                BoundedLineRead::Line
            });
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if !overflowed {
            if output.len().saturating_add(consumed) <= maximum_bytes {
                output.extend_from_slice(&available[..consumed]);
            } else {
                output.clear();
                overflowed = true;
            }
        }
        reader.consume(consumed);
        if newline.is_some() {
            return Ok(if overflowed {
                BoundedLineRead::Overflow
            } else {
                BoundedLineRead::Line
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_process_line<F>(
    line: &str,
    stderr: bool,
    request: &DownloadRequest,
    summary: &mut DownloadSummary,
    diagnostics: &mut String,
    seen_failures: &mut HashSet<String>,
    emit: &mut F,
) where
    F: FnMut(DownloadEvent),
{
    if let Some(event) = parse_progress_line(line, request) {
        if let DownloadEvent::FileFinished { path, .. } = &event {
            summary.files.push(path.clone());
        }
        emit(event);
        return;
    }
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return;
    }
    if stderr && diagnostics.len() < MAX_DIAGNOSTIC_BYTES {
        if !diagnostics.is_empty() {
            diagnostics.push('\n');
        }
        let remaining = MAX_DIAGNOSTIC_BYTES.saturating_sub(diagnostics.len());
        push_bounded_text(diagnostics, trimmed, remaining);
    }
    if request.allow_playlist
        && trimmed.to_ascii_lowercase().contains("error:")
        && seen_failures.insert(trimmed.to_owned())
    {
        summary.item_failures.push(trimmed.to_owned());
        emit(DownloadEvent::ItemFailed {
            message: trimmed.to_owned(),
        });
    }
}

fn push_bounded_text(target: &mut String, value: &str, maximum_bytes: usize) {
    if value.len() <= maximum_bytes {
        target.push_str(value);
        return;
    }
    let boundary = value
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= maximum_bytes)
        .last()
        .unwrap_or_default();
    target.push_str(&value[..boundary]);
}

fn parse_progress_line(line: &str, request: &DownloadRequest) -> Option<DownloadEvent> {
    if let Some(value) = line.find(PROGRESS_PREFIX).map(|index| &line[index..]) {
        return parse_progress_fields(value, PROGRESS_PREFIX, DownloadPhase::Downloading, request);
    }
    if let Some(value) = line.find(PROCESS_PREFIX).map(|index| &line[index..]) {
        return parse_progress_fields(value, PROCESS_PREFIX, DownloadPhase::Processing, request);
    }
    let value = line.find(FINISHED_PREFIX).map(|index| &line[index..])?;
    let mut fields = value[FINISHED_PREFIX.len()..].splitn(2, '\t');
    let title = decode_json_text(fields.next().unwrap_or_default())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| request.title.clone());
    let path = decode_json_text(fields.next().unwrap_or_default())?;
    Some(DownloadEvent::FileFinished {
        title,
        path: PathBuf::from(path),
    })
}

fn parse_progress_fields(
    value: &str,
    prefix: &str,
    phase: DownloadPhase,
    request: &DownloadRequest,
) -> Option<DownloadEvent> {
    let mut fields = value[prefix.len()..].splitn(5, '\t');
    let _reported_phase = fields.next()?;
    let percent = decode_json_text(fields.next().unwrap_or_default())
        .and_then(|value| {
            value
                .trim()
                .trim_end_matches('%')
                .trim()
                .parse::<f64>()
                .ok()
        })
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 100.0));
    let title = decode_json_text(fields.next().unwrap_or_default())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| request.title.clone());
    let playlist_index = decode_json_u32(fields.next().unwrap_or_default());
    let playlist_count = decode_json_u32(fields.next().unwrap_or_default());
    Some(DownloadEvent::Progress {
        phase,
        title,
        percent,
        playlist_index,
        playlist_count,
    })
}

fn decode_json_text(value: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .filter(|value| value != "NA")
}

fn decode_json_u32(value: &str) -> Option<u32> {
    let value = serde_json::from_str::<serde_json::Value>(value).ok()?;
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .or_else(|| value.as_str()?.parse().ok())
}

fn redact_diagnostic(value: &str, cookies: Option<&Path>, proxy: Option<&str>) -> String {
    let mut redacted = value.to_owned();
    if let Some(path) = cookies {
        redacted = redacted.replace(&path.to_string_lossy().to_string(), "[cookies file]");
    }
    if let Some(proxy) = proxy.filter(|value| !value.is_empty()) {
        redacted = redacted.replace(proxy, "[proxy]");
    }
    redacted
}

#[cfg(windows)]
mod process_job {
    #![allow(unsafe_code)]

    use std::{ffi::c_void, mem::size_of, os::windows::io::AsRawHandle, process::Child};

    use windows::{
        Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            },
        },
        core::PCWSTR,
    };

    pub struct KillOnCloseJob(HANDLE);

    impl KillOnCloseJob {
        pub fn attach(child: &Child) -> Option<Self> {
            // SAFETY: The unnamed job handle is process-owned, the information
            // buffer has the exact documented layout, and the child handle stays
            // valid for the duration of this call.
            unsafe {
                let job = CreateJobObjectW(None, PCWSTR::null()).ok()?;
                let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                if SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&raw const limits).cast::<c_void>(),
                    u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                        .expect("job limit structure size fits in u32"),
                )
                .is_err()
                    || AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())).is_err()
                {
                    let _ = CloseHandle(job);
                    return None;
                }
                Some(Self(job))
            }
        }
    }

    impl Drop for KillOnCloseJob {
        fn drop(&mut self) {
            // SAFETY: This type uniquely owns the valid handle returned by
            // CreateJobObjectW and closes it exactly once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(windows)]
fn stop_process_tree(child: &mut Child) {
    let mut command = Command::new("taskkill.exe");
    command
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    let _ = command.status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(windows))]
fn stop_process_tree(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, path::PathBuf};

    use super::{
        BoundedLineRead, DownloadEvent, DownloadMode, DownloadOptions, DownloadPhase,
        DownloadRequest, VideoDownloadFormat, build_arguments, output_template,
        parse_progress_line, read_bounded_line, safe_filename_template, video_format_selector,
    };

    fn request(mode: DownloadMode) -> DownloadRequest {
        DownloadRequest {
            url: "https://example.com/media".to_owned(),
            title: "Example".to_owned(),
            mode,
            output_directory: PathBuf::from(r"C:\Downloads\ApricotPlayer\music"),
            target_path: None,
            allow_playlist: false,
            options: DownloadOptions::default(),
        }
    }

    fn arguments(request: &DownloadRequest) -> Vec<String> {
        build_arguments(request)
            .expect("arguments")
            .into_iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn audio_uses_best_source_then_exact_transcode_settings() {
        let mut request = request(DownloadMode::Audio);
        request.options.audio_format = "flac".to_owned();
        request.options.audio_quality = "320".to_owned();
        let args = arguments(&request);
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--format", "bestaudio/best"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--audio-format", "flac"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--audio-quality", "320"])
        );
        assert!(args.contains(&"--extract-audio".to_owned()));
    }

    #[test]
    fn video_selectors_match_python_fallback_order() {
        assert_eq!(
            video_format_selector(VideoDownloadFormat::BestAvailable, 720),
            "bestvideo[height<=720]+bestaudio/best[height<=720]/best"
        );
        assert_eq!(
            video_format_selector(VideoDownloadFormat::Mp4SingleFile, 0),
            "best[ext=mp4][vcodec!=none][acodec!=none]/best[ext=mp4][vcodec!=none][acodec!=none]/best/best"
        );
        let request = request(DownloadMode::Video);
        let args = arguments(&request);
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--concurrent-fragments", "8"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--merge-output-format", "mp4"])
        );
    }

    #[test]
    fn collection_preserves_order_and_continues_after_item_errors() {
        let mut request = request(DownloadMode::Audio);
        request.allow_playlist = true;
        let args = arguments(&request);
        assert!(args.contains(&"--yes-playlist".to_owned()));
        assert!(args.contains(&"--ignore-errors".to_owned()));
        assert!(args.contains(&"--skip-unavailable-fragments".to_owned()));
        assert!(
            output_template(&request)
                .expect("template")
                .to_string_lossy()
                .contains("%(playlist_index)s - %(title)s.%(ext)s")
        );
    }

    #[test]
    fn unsafe_filename_templates_fall_back_without_escaping_folder() {
        for unsafe_value in [
            r"..\%(title)s.%(ext)s",
            r"C:\outside\%(title)s.%(ext)s",
            "/outside/%(title)s.%(ext)s",
            "CON.%(ext)s",
            "folder. /file",
        ] {
            assert_eq!(safe_filename_template(unsafe_value), "%(title)s.%(ext)s");
        }
        assert_eq!(
            safe_filename_template("%(channel)s/%(title)s.%(ext)s"),
            "%(channel)s/%(title)s.%(ext)s"
        );
    }

    #[test]
    fn ask_each_time_audio_path_keeps_selected_stem_and_transcoded_extension() {
        let mut request = request(DownloadMode::Audio);
        request.target_path = Some(PathBuf::from(r"C:\Chosen\My episode.mp3"));
        assert_eq!(
            output_template(&request).expect("template"),
            PathBuf::from(r"C:\Chosen\My episode.%(ext)s")
        );
    }

    #[test]
    fn progress_and_finished_lines_are_typed() {
        let request = request(DownloadMode::Audio);
        let progress = parse_progress_line(
            "APRICOT_PROGRESS:downloading\t\" 42.5%\"\t\"Episode\"\t2\t10",
            &request,
        );
        assert_eq!(
            progress,
            Some(DownloadEvent::Progress {
                phase: DownloadPhase::Downloading,
                title: "Episode".to_owned(),
                percent: Some(42.5),
                playlist_index: Some(2),
                playlist_count: Some(10),
            })
        );
        assert_eq!(
            parse_progress_line(
                "APRICOT_FINISHED:\"Episode\"\t\"C:\\\\Music\\\\Episode.mp3\"",
                &request,
            ),
            Some(DownloadEvent::FileFinished {
                title: "Episode".to_owned(),
                path: PathBuf::from(r"C:\Music\Episode.mp3"),
            })
        );
    }

    #[test]
    fn process_lines_are_rejected_without_unbounded_buffering() {
        let mut reader = Cursor::new(b"123456789\nnext\n");
        let mut line = Vec::new();
        assert_eq!(
            read_bounded_line(&mut reader, &mut line, 8).expect("overflow result"),
            BoundedLineRead::Overflow
        );
        assert!(line.is_empty());
        assert_eq!(
            read_bounded_line(&mut reader, &mut line, 8).expect("next line"),
            BoundedLineRead::Line
        );
        assert_eq!(line, b"next\n");
        assert_eq!(
            read_bounded_line(&mut reader, &mut line, 8).expect("end"),
            BoundedLineRead::End
        );
    }
}
