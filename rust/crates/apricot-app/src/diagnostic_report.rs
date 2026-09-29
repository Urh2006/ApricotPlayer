//! Python `DiagnosticsMixin`: the plain-text diagnostic report copied with
//! Ctrl+Alt+Shift+D, with URLs, secrets and user folders redacted.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::LazyLock,
};

use apricot_core::audio::EQUALIZER_BANDS;
use regex::{Captures, Regex};

use crate::{Application, PlaybackPhase, SearchPhase, SessionToggle, YoutubeCollectionPhase};

/// Python `DIAGNOSTIC_LOG_TAIL_MAX_BYTES`.
pub const LOG_TAIL_MAX_BYTES: u64 = 256 * 1024;
/// Python `diagnostic_file_tail(line_count=50)`.
pub const LOG_TAIL_LINES: usize = 50;
const MAX_VALUE_CHARS: usize = 1_000;

static URL_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)https?://[^\s"'<>]+"#).expect("valid regex"));
static SECRET_FIELD_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)\b(cookie|authorization|proxy-authorization|x-api-key|youtube_data_api_key)\s*[:=]\s*[^\r\n]+",
    )
    .expect("valid regex")
});

/// One value of a report line, formatted like Python `diagnostic_format_value`.
#[derive(Clone, Debug, PartialEq)]
pub enum DiagnosticValue {
    Missing,
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(String),
    /// Python `diagnostic_url_summary` of a stream address.
    UrlSummary(String),
}

impl From<bool> for DiagnosticValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for DiagnosticValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<usize> for DiagnosticValue {
    fn from(value: usize) -> Self {
        Self::Integer(i64::try_from(value).unwrap_or(i64::MAX))
    }
}

impl From<u32> for DiagnosticValue {
    fn from(value: u32) -> Self {
        Self::Integer(i64::from(value))
    }
}

impl From<f64> for DiagnosticValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<&str> for DiagnosticValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<String> for DiagnosticValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&Path> for DiagnosticValue {
    fn from(value: &Path) -> Self {
        Self::Text(value.display().to_string())
    }
}

impl<T: Into<Self>> From<Option<T>> for DiagnosticValue {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Missing, Into::into)
    }
}

/// A report section: its heading line and its `Label: value` lines.
#[derive(Clone, Debug, PartialEq)]
pub struct DiagnosticSection {
    heading: String,
    lines: Vec<(String, DiagnosticValue)>,
}

impl DiagnosticSection {
    pub fn new(heading: impl Into<String>) -> Self {
        Self {
            heading: heading.into(),
            lines: Vec::new(),
        }
    }

    #[must_use]
    pub fn line(mut self, label: &str, value: impl Into<DiagnosticValue>) -> Self {
        self.push(label, value);
        self
    }

    pub fn push(&mut self, label: &str, value: impl Into<DiagnosticValue>) {
        self.lines.push((label.to_owned(), value.into()));
    }

    /// Replaces the value of an existing line, for values that are read off the
    /// window thread.
    pub fn set(&mut self, label: &str, value: impl Into<DiagnosticValue>) {
        if let Some(line) = self.lines.iter_mut().find(|(name, _)| name == label) {
            line.1 = value.into();
        }
    }
}

/// Python `diagnostic_redact_text` with the user folders of this computer.
#[derive(Clone, Debug, Default)]
pub struct DiagnosticRedactor {
    /// Case-insensitive folder patterns and their placeholders, longest
    /// folder first.
    roots: Vec<(Regex, String)>,
}

impl DiagnosticRedactor {
    /// Python reads `APPDATA`, `LOCALAPPDATA`, `USERPROFILE` (or the home
    /// folder) and `TEMP`.
    pub fn from_environment() -> Self {
        let variable = |name: &str| std::env::var(name).unwrap_or_default();
        Self::with_roots([
            (variable("APPDATA"), "%APPDATA%"),
            (variable("LOCALAPPDATA"), "%LOCALAPPDATA%"),
            (variable("USERPROFILE"), "%USERPROFILE%"),
            (variable("TEMP"), "%TEMP%"),
        ])
    }

    pub fn with_roots<I, S>(roots: I) -> Self
    where
        I: IntoIterator<Item = (S, &'static str)>,
        S: Into<String>,
    {
        let mut roots = roots
            .into_iter()
            .map(|(root, placeholder)| (root.into(), placeholder.to_owned()))
            .filter(|(root, _)| !root.is_empty())
            .collect::<Vec<_>>();
        // Python sorts the dictionary by folder length, longest first; equal
        // folders collapse to the last placeholder like dictionary keys.
        roots.reverse();
        let mut unique: Vec<(String, String)> = Vec::new();
        for (root, placeholder) in roots {
            if !unique.iter().any(|(existing, _)| *existing == root) {
                unique.push((root, placeholder));
            }
        }
        unique.sort_by_key(|(root, _)| std::cmp::Reverse(root.chars().count()));
        let roots = unique
            .into_iter()
            .filter_map(|(root, placeholder)| {
                Regex::new(&format!("(?i){}", regex::escape(&root)))
                    .ok()
                    .map(|pattern| (pattern, placeholder))
            })
            .collect();
        Self { roots }
    }

    pub fn redact_text(&self, text: &str) -> String {
        let redacted =
            URL_PATTERN.replace_all(text, |captures: &Captures<'_>| redact_url(&captures[0]));
        let mut redacted = SECRET_FIELD_PATTERN
            .replace_all(&redacted, |captures: &Captures<'_>| {
                format!("{}: <redacted>", &captures[1])
            })
            .into_owned();
        for (pattern, placeholder) in &self.roots {
            redacted = pattern
                .replace_all(&redacted, regex::NoExpand(placeholder))
                .into_owned();
        }
        redacted
    }

    /// Python `diagnostic_format_value`.
    pub fn format_value(&self, value: &DiagnosticValue) -> String {
        let text = match value {
            DiagnosticValue::Bool(value) => return if *value { "yes" } else { "no" }.to_owned(),
            DiagnosticValue::Missing => return "none".to_owned(),
            DiagnosticValue::Float(value) => return format_float(*value),
            DiagnosticValue::Integer(value) => value.to_string(),
            DiagnosticValue::Text(value) => value.clone(),
            DiagnosticValue::UrlSummary(url) => self.url_summary(url),
        };
        let text = self.redact_text(&text.replace("\r\n", "\n").replace('\r', "\n"));
        if text.chars().count() > MAX_VALUE_CHARS {
            let cut = text.chars().take(MAX_VALUE_CHARS).collect::<String>();
            return format!("{}...", cut.trim_end());
        }
        text
    }

    /// Python `diagnostic_line`.
    pub fn line(&self, label: &str, value: &DiagnosticValue) -> String {
        format!("{label}: {}", self.format_value(value))
    }

    /// Python `diagnostic_url_summary`: a stream address without its query.
    pub fn url_summary(&self, url: &str) -> String {
        let url = url.trim();
        if url.is_empty() {
            return "none".to_owned();
        }
        let parts = UrlParts::parse(url);
        if !matches!(parts.scheme.as_str(), "http" | "https") {
            return self.redact_text(url);
        }
        let mut path = if parts.path.is_empty() {
            "/".to_owned()
        } else {
            parts.path.clone()
        };
        if path.chars().count() > 100 {
            path = format!("{}...", path.chars().take(97).collect::<String>());
        }
        format!(
            "{}://{}{path} (query={}, length={})",
            parts.scheme,
            parts.host_and_port(false),
            if parts.query.is_some() { "yes" } else { "no" },
            url.chars().count()
        )
    }

    /// Python `build_diagnostic_report`: the sections, then the log tails.
    pub fn report(&self, sections: &[DiagnosticSection], logs: &[(&str, &Path)]) -> String {
        let mut blocks = sections
            .iter()
            .map(|section| {
                std::iter::once(section.heading.clone())
                    .chain(
                        section
                            .lines
                            .iter()
                            .map(|(label, value)| self.line(label, value)),
                    )
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .collect::<Vec<_>>();
        for (title, path) in logs {
            blocks.push(match self.file_tail(path, LOG_TAIL_LINES) {
                Some(tail) => format!("## {title}\n{tail}"),
                None => format!("## {title}\nnot available"),
            });
        }
        let report = blocks
            .into_iter()
            .filter(|block| !block.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        format!("{}\n", report.trim())
    }

    /// Python `diagnostic_file_tail`: the last lines of at most 256 KiB,
    /// redacted; `None` when the file is missing, unreadable or empty.
    pub fn file_tail(&self, path: &Path, line_count: usize) -> Option<String> {
        let mut file = File::open(path).ok()?;
        let size = file.seek(SeekFrom::End(0)).ok()?;
        let start = size.saturating_sub(LOG_TAIL_MAX_BYTES);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut raw = Vec::new();
        file.take(LOG_TAIL_MAX_BYTES).read_to_end(&mut raw).ok()?;
        if start > 0 {
            raw = raw
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or_else(Vec::new, |index| raw[index + 1..].to_vec());
        }
        let text = String::from_utf8_lossy(&raw);
        let lines = python_splitlines(&text);
        let tail = lines[lines.len().saturating_sub(line_count)..].join("\n");
        let tail = self.redact_text(&tail).trim().to_owned();
        (!tail.is_empty()).then_some(tail)
    }
}

/// Python `diagnostic_equalizer_gains`: every band, missing ones as 0.0.
pub fn equalizer_gains_text<'a>(
    bands: impl IntoIterator<Item = &'a str>,
    gains: &std::collections::BTreeMap<String, f64>,
) -> String {
    bands
        .into_iter()
        .map(|band| {
            format!(
                "{band} Hz={}",
                python_float_repr(gains.get(band).copied().unwrap_or(0.0))
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Python `f"{value:.3f}".rstrip("0").rstrip(".")`.
fn format_float(value: f64) -> String {
    let text = format!("{value:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Python `str(float)` for the plain values of equalizer gains.
fn python_float_repr(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

fn python_splitlines(text: &str) -> Vec<&str> {
    let mut lines = text.split('\n').collect::<Vec<_>>();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
        .into_iter()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect()
}

/// Python `diagnostic_redact_url`: scheme, host, port and path; the query and
/// fragment become `...` and credentials are dropped.
pub fn redact_url(url: &str) -> String {
    let parts = UrlParts::parse(url);
    let mut result = String::new();
    if !parts.scheme.is_empty() {
        result.push_str(&parts.scheme);
        result.push(':');
    }
    if parts.has_authority {
        result.push_str("//");
        result.push_str(&parts.host_and_port(true));
    }
    result.push_str(&parts.path);
    if parts
        .query
        .as_deref()
        .is_some_and(|query| !query.is_empty())
    {
        result.push_str("?...");
    }
    if parts
        .fragment
        .as_deref()
        .is_some_and(|fragment| !fragment.is_empty())
    {
        result.push_str("#...");
    }
    result
}

/// The parts of Python `urllib.parse.urlparse` that the report needs.
#[derive(Debug, Default)]
struct UrlParts {
    scheme: String,
    has_authority: bool,
    host: String,
    port: Option<u16>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

impl UrlParts {
    fn parse(url: &str) -> Self {
        let mut parts = Self::default();
        let mut rest = url;
        if let Some((scheme, after)) = rest.split_once(':')
            && !scheme.is_empty()
            && scheme
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "+-.".contains(character))
        {
            parts.scheme = scheme.to_ascii_lowercase();
            rest = after;
        }
        if let Some((before, fragment)) = rest.split_once('#') {
            parts.fragment = Some(fragment.to_owned());
            rest = before;
        }
        if let Some((before, query)) = rest.split_once('?') {
            parts.query = Some(query.to_owned());
            rest = before;
        }
        if let Some(after) = rest.strip_prefix("//") {
            parts.has_authority = true;
            let end = after.find('/').unwrap_or(after.len());
            let netloc = &after[..end];
            after[end..].clone_into(&mut parts.path);
            let host_port = netloc.rsplit_once('@').map_or(netloc, |(_, host)| host);
            let (host, port) = if let Some(bracketed) = host_port.strip_prefix('[') {
                let (host, after_host) = bracketed.split_once(']').unwrap_or((bracketed, ""));
                (host, after_host.strip_prefix(':'))
            } else {
                match host_port.split_once(':') {
                    Some((host, port)) => (host, Some(port)),
                    None => (host_port, None),
                }
            };
            parts.host = host.to_ascii_lowercase();
            parts.port = port
                .and_then(|port| port.parse().ok())
                .filter(|port| *port != 0);
        } else {
            rest.clone_into(&mut parts.path);
        }
        parts
    }

    fn host_and_port(&self, bracket_ipv6: bool) -> String {
        let host = if bracket_ipv6 && self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        match self.port {
            Some(port) => format!("{host}:{port}"),
            None => host,
        }
    }
}

/// What only the window knows, and the paths and versions of this run.
#[allow(
    clippy::struct_excessive_bools,
    reason = "plain report values, one per Python line"
)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagnosticEnvironment {
    pub app_version: String,
    pub app_label: String,
    pub frozen_build: bool,
    pub executable: String,
    pub working_directory: String,
    pub platform: String,
    pub ytdlp_version: String,
    pub mpv_path: String,
    pub ffmpeg_path: String,
    /// Python `player_is_active`: a running libmpv player.
    pub player_active: bool,
    pub resolve_pending: bool,
    pub in_player_screen: bool,
    pub current_index: Option<usize>,
    pub visible_results: usize,
    pub process_id: u32,
}

/// The label of the application line that is filled off the window thread.
pub const YTDLP_LINE: &str = "yt-dlp";

/// Python `build_diagnostic_report` sections, from the Rust state that holds
/// the same information.
pub fn diagnostic_sections(
    application: &Application,
    environment: &DiagnosticEnvironment,
) -> Vec<DiagnosticSection> {
    vec![
        app_section(application, environment),
        player_section(application, environment),
        audio_section(application),
        current_item_section(application),
        queue_section(application, environment),
        settings_section(application),
    ]
}

fn app_section(
    application: &Application,
    environment: &DiagnosticEnvironment,
) -> DiagnosticSection {
    let settings_file = application.settings_file();
    let app_folder = settings_file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    DiagnosticSection::new("# ApricotPlayer diagnostic report")
        .line(
            "Generated",
            chrono::Local::now()
                .format("%Y-%m-%dT%H:%M:%S%:z")
                .to_string(),
        )
        .line("App version", environment.app_version.as_str())
        .line("App label", environment.app_label.as_str())
        .line(
            "Update channel",
            application.settings().update_channel.as_str(),
        )
        .line("Frozen build", environment.frozen_build)
        .line("Executable", environment.executable.as_str())
        .line("Working directory", environment.working_directory.as_str())
        .line("Settings file", settings_file.as_path())
        .line("App data folder", app_folder.as_path())
        .line("Platform", environment.platform.as_str())
        .line(YTDLP_LINE, environment.ytdlp_version.as_str())
        .line("mpv path", environment.mpv_path.as_str())
        .line("FFmpeg path", environment.ffmpeg_path.as_str())
}

fn player_section(
    application: &Application,
    environment: &DiagnosticEnvironment,
) -> DiagnosticSection {
    let session = application.player_session();
    let phase = session.phase();
    let toggles = session.enabled_toggles();
    let active = environment.player_active;
    let mut section = DiagnosticSection::new("## Player")
        .line("Active", active)
        .line("Kind", if active { "mpv" } else { "" })
        .line("Control mode", active)
        .line("Session open", session.is_open())
        .line(
            "Playback pending",
            phase == PlaybackPhase::Starting || environment.resolve_pending,
        )
        .line("In player screen", environment.in_player_screen)
        .line("Return screen", application.player_return_screen())
        .line(
            "Fullscreen session",
            toggles.contains(&SessionToggle::Fullscreen),
        )
        .line("Paused", phase == PlaybackPhase::Paused)
        .line("Ended", phase == PlaybackPhase::Ended)
        .line(
            "Process PID",
            if active {
                DiagnosticValue::from(environment.process_id)
            } else {
                DiagnosticValue::from("")
            },
        );
    if active {
        let audio = session.audio();
        let boosted = toggles.contains(&SessionToggle::VolumeBoost)
            || audio.is_some_and(|audio| audio.volume > 100.0);
        let info = session.media_info();
        let mut params = Vec::new();
        if let Some(rate) = info.sample_rate_hz {
            params.push(format!("'samplerate': {rate}"));
        }
        if let Some(channels) = info.channel_count {
            params.push(format!("'channel-count': {channels}"));
        }
        if let Some(layout) = info.channel_layout.as_deref() {
            params.push(format!("'hr-channels': '{layout}'"));
        }
        section.push("mpv pause", phase == PlaybackPhase::Paused);
        section.push("mpv time-pos", session.position_seconds());
        section.push("mpv duration", session.duration_seconds());
        section.push("mpv volume", audio.map(|audio| audio.volume));
        section.push("mpv volume-max", if boosted { 300_i64 } else { 100 });
        section.push("mpv speed", audio.map(|audio| audio.speed));
        section.push("mpv pitch", audio.map(|audio| audio.pitch));
        section.push(
            "mpv audio-device",
            audio
                .map(|audio| audio.output_device.trim())
                .filter(|device| !device.is_empty())
                .unwrap_or("auto"),
        );
        section.push(
            "mpv audio-params",
            (!params.is_empty()).then(|| format!("{{{}}}", params.join(", "))),
        );
    }
    section
}

fn audio_section(application: &Application) -> DiagnosticSection {
    let settings = application.settings();
    let session = application.player_session();
    let audio = session.audio();
    let toggles = session.enabled_toggles();
    let session_equalizer = audio.and_then(|audio| audio.equalizer.as_ref());
    let equalizer_enabled = session_equalizer
        .map_or(settings.global_equalizer_enabled, |equalizer| {
            equalizer.enabled
        });
    let gains = session_equalizer
        .map(|equalizer| &equalizer.gains)
        .filter(|gains| !gains.is_empty())
        .unwrap_or(&settings.global_equalizer_gains);
    let session_device = audio
        .map(|audio| audio.output_device.clone())
        .unwrap_or_default();
    let effective_preset = crate::equalizer::EqualizerSettings::from_document(settings)
        .effective_preset(&application.equalizer_device_key());
    DiagnosticSection::new("## Audio state")
        .line("Default volume", settings.default_volume)
        .line("Session volume", audio.map(|audio| audio.volume))
        .line(
            "Volume boost enabled",
            toggles.contains(&SessionToggle::VolumeBoost),
        )
        .line("Volume boost by default", settings.volume_boost_by_default)
        .line(
            "Bass boost enabled",
            toggles.contains(&SessionToggle::BassBoost),
        )
        .line("Equalizer enabled", equalizer_enabled)
        .line("Equalizer preset", effective_preset)
        .line("Equalizer range", settings.equalizer_db_range)
        .line(
            "Equalizer clipping protection",
            settings.equalizer_clipping_protection,
        )
        .line(
            "Equalizer device presets",
            settings.equalizer_device_presets.len(),
        )
        .line(
            "Equalizer gains",
            equalizer_gains_text(EQUALIZER_BANDS.iter().map(|band| band.id), gains),
        )
        .line(
            "Configured output device",
            settings.audio_output_device.as_str(),
        )
        .line("Session output device", session_device.as_str())
        .line(
            "Current output device",
            if session.is_open() {
                session_device.as_str()
            } else {
                ""
            },
        )
        .line("ReplayGain mode", settings.replaygain_mode.as_str())
        .line("Gapless playback", settings.gapless_playback)
        .line("Speed audio mode", settings.speed_audio_mode.as_str())
        .line(
            "Speed/pitch hold delay ms",
            settings.speed_pitch_hold_delay_ms,
        )
        .line(
            "Speed/pitch hold interval ms",
            settings.speed_pitch_hold_interval_ms,
        )
        .line("Repeat", toggles.contains(&SessionToggle::Repeat))
        .line("Shuffle", toggles.contains(&SessionToggle::Shuffle))
        .line("Autoplay next setting", settings.autoplay_next)
        .line(
            "Autoplay next session",
            toggles.contains(&SessionToggle::AutoplayNext),
        )
}

fn current_item_section(application: &Application) -> DiagnosticSection {
    let item = application.player_session().current_item();
    let value = item.map(apricot_storage::media_item_to_python_value);
    let text = |key: &str| {
        value
            .as_ref()
            .and_then(|value| value.get(key))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let duration = Some(text("duration"))
        .filter(|duration| !duration.is_empty())
        .unwrap_or_else(|| format_duration(item.and_then(|item| item.duration_seconds)));
    let url = Some(text("webpage_url"))
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| text("url"));
    let stream_url = item
        .and_then(|item| item.stream_url.as_ref())
        .map(ToString::to_string)
        .unwrap_or_default();
    DiagnosticSection::new("## Current item")
        .line("Title", text("title"))
        .line("Kind", text("kind"))
        .line("Type", text("type"))
        .line("Channel", text("channel"))
        .line("Duration", duration)
        .line("URL", url)
        .line(
            "Local path",
            item.and_then(|item| item.local_path.clone())
                .unwrap_or_default(),
        )
        .line("Stream URL", DiagnosticValue::UrlSummary(stream_url))
        // Rust gives mpv the yt-dlp formats without extra request headers.
        .line("Stream header names", "none")
}

fn queue_section(
    application: &Application,
    environment: &DiagnosticEnvironment,
) -> DiagnosticSection {
    let search = application.search_session();
    let collection = application.youtube_collection();
    let last_session = application.last_player_session();
    let all_results =
        collection.map_or(search.items().len(), |collection| collection.items().len());
    let return_results = if collection.is_some() {
        search.items().len()
    } else {
        0
    };
    let loading_more = search.phase() == SearchPhase::LoadingMore
        || collection
            .is_some_and(|collection| collection.phase() == YoutubeCollectionPhase::LoadingMore);
    DiagnosticSection::new("## Results and queue")
        .line("Current index", environment.current_index)
        .line("Return index", Option::<i64>::None)
        .line("Visible results", environment.visible_results)
        .line("All results", all_results)
        .line("Return results", return_results)
        .line("Return all results", return_results)
        .line("Player sequence count", application.player_sequence_len())
        .line("Playback queue count", application.playback_queue().len())
        .line(
            "Last player session",
            last_session.map_or("", |session| session.title.as_str()),
        )
        .line(
            "Last session sequence count",
            last_session.map_or(0, |session| session.sequence.len()),
        )
        .line(
            "Dynamic fetch enabled",
            collection.map_or(
                search.is_dynamic(),
                crate::YoutubeCollectionSession::is_dynamic,
            ),
        )
        .line("Loading more results", loading_more)
        .line(
            "Collection URL",
            collection.map_or("", |collection| collection.url()),
        )
        .line(
            "Collection fully loaded",
            collection.is_some_and(|collection| !collection.can_load_more()),
        )
}

fn settings_section(application: &Application) -> DiagnosticSection {
    let settings = application.settings();
    DiagnosticSection::new("## Key settings")
        .line("Language", settings.language.as_str())
        .line("Results limit", settings.results_limit)
        .line("Background playback", settings.enable_background_playback)
        .line("Close to tray", settings.close_to_tray)
        .line("Stream cache", settings.enable_stream_cache)
        .line("Stream URL cache", settings.enable_stream_url_cache)
        .line(
            "Stream URL cache minutes",
            settings.stream_url_cache_minutes,
        )
        .line("Cache folder", settings.cache_folder.as_str())
        .line("Cache size MB", settings.cache_size_mb)
        .line("Cookies file configured", !settings.cookies_file.is_empty())
        .line("Cookies file", settings.cookies_file.as_str())
        .line("Cookies source file", settings.cookies_source_file.as_str())
        .line(
            "Cookies source signature configured",
            !settings.cookies_source_signature.is_empty(),
        )
        .line("Cookies source refresh error", "")
        .line("Cookies browser", settings.cookies_from_browser.as_str())
        .line(
            "Cookies browser profile",
            settings.cookies_browser_profile.as_str(),
        )
        .line(
            "YouTube API key configured",
            !settings.youtube_data_api_key.is_empty(),
        )
        .line(
            "Player command configured",
            !settings.player_command.is_empty(),
        )
        .line("FFmpeg configured", !settings.ffmpeg_location.is_empty())
        .line("Auto update app", settings.auto_update_app)
        .line("Auto update yt-dlp", settings.auto_update_ytdlp)
}

/// Python `format_duration`: empty for a missing or zero duration.
fn format_duration(seconds: Option<f64>) -> String {
    let Some(seconds) = seconds.filter(|seconds| seconds.is_finite() && *seconds >= 1.0) else {
        return String::new();
    };
    let seconds = std::time::Duration::from_secs_f64(seconds).as_secs();
    let (hours, minutes, seconds) = (seconds / 3_600, (seconds % 3_600) / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor() -> DiagnosticRedactor {
        DiagnosticRedactor::with_roots([
            (r"C:\Users\Urh\AppData\Roaming", "%APPDATA%"),
            (r"C:\Users\Urh\AppData\Local", "%LOCALAPPDATA%"),
            (r"C:\Users\Urh", "%USERPROFILE%"),
            (r"C:\Users\Urh\AppData\Local\Temp", "%TEMP%"),
        ])
    }

    #[test]
    fn redacts_tokens_and_user_paths_like_python_security_test() {
        let redacted = redactor().redact_text(
            "Cookie: secret-value\nAuthorization: Bearer token\nc:\\users\\urh\\Music\\song.mp3\n\
             https://example.com/media?sig=secret#access-token\n\
             http://proxy-user:proxy-password@proxy.example:8080/media",
        );
        for secret in [
            "secret-value",
            "Bearer token",
            r"c:\users\urh",
            "sig=secret",
            "access-token",
            "proxy-user",
            "proxy-password",
        ] {
            assert!(
                !redacted.to_lowercase().contains(&secret.to_lowercase()),
                "{secret}"
            );
        }
        assert_eq!(
            redacted,
            "Cookie: <redacted>\nAuthorization: <redacted>\n%USERPROFILE%\\Music\\song.mp3\n\
             https://example.com/media?...#...\nhttp://proxy.example:8080/media"
        );
    }

    #[test]
    fn longest_user_folder_wins() {
        let redactor = redactor();
        assert_eq!(
            redactor.redact_text(r"C:\Users\Urh\AppData\Local\Temp\a.txt and C:\Users\Urh\AppData\Roaming\ApricotPlayer2Beta"),
            r"%TEMP%\a.txt and %APPDATA%\ApricotPlayer2Beta"
        );
    }

    #[test]
    fn values_are_formatted_like_python() {
        let redactor = redactor();
        assert_eq!(redactor.format_value(&true.into()), "yes");
        assert_eq!(redactor.format_value(&false.into()), "no");
        assert_eq!(redactor.format_value(&Option::<f64>::None.into()), "none");
        assert_eq!(redactor.format_value(&100.0.into()), "100");
        assert_eq!(redactor.format_value(&1.25.into()), "1.25");
        assert_eq!(redactor.format_value(&0.123_456.into()), "0.123");
        assert_eq!(redactor.format_value(&7_usize.into()), "7");
        assert_eq!(redactor.format_value(&"a\r\nb\rc".into()), "a\nb\nc");
        let long = "x".repeat(1_200);
        assert_eq!(
            redactor.format_value(&long.into()),
            format!("{}...", "x".repeat(1_000))
        );
    }

    #[test]
    fn stream_url_summary_hides_the_query() {
        let redactor = redactor();
        assert_eq!(redactor.url_summary(""), "none");
        assert_eq!(
            redactor.url_summary("https://rr1.googlevideo.com/videoplayback?sig=abc&expire=1"),
            "https://rr1.googlevideo.com/videoplayback (query=yes, length=58)"
        );
        assert_eq!(
            redactor.url_summary("http://Host.Example:8080"),
            "http://host.example:8080/ (query=no, length=24)"
        );
        let long_path = format!("https://a.b/{}", "p".repeat(120));
        assert_eq!(
            redactor.url_summary(&long_path),
            format!("https://a.b/{}... (query=no, length=132)", "p".repeat(96))
        );
        assert_eq!(
            redactor.url_summary(r"C:\Users\Urh\Music\a.mp3"),
            r"%USERPROFILE%\Music\a.mp3"
        );
    }

    #[test]
    fn equalizer_gains_list_every_band() {
        let gains = [("60".to_owned(), 3.5), ("1000".to_owned(), -2.0)].into();
        assert_eq!(
            equalizer_gains_text(["60", "230", "1000"], &gains),
            "60 Hz=3.5, 230 Hz=0.0, 1000 Hz=-2.0"
        );
    }

    #[test]
    fn report_joins_sections_and_log_tails() {
        let directory = tempfile::tempdir().expect("temp dir");
        let log = directory.path().join("mpv.log");
        let lines = (1..=60)
            .map(|index| format!("line {index} https://x.test/a?token=1"))
            .collect::<Vec<_>>()
            .join("\r\n");
        std::fs::write(&log, lines).expect("log");
        let empty = directory.path().join("updater.log");
        std::fs::write(&empty, "").expect("empty log");
        let mut app = DiagnosticSection::new("# ApricotPlayer diagnostic report")
            .line("App version", "2.0.0")
            .line("yt-dlp", "pending");
        app.set("yt-dlp", "2026.09.01");
        let player = DiagnosticSection::new("## Player").line("Active", false);
        let report = redactor().report(
            &[app, player],
            &[
                ("mpv.log", log.as_path()),
                ("updater.log", empty.as_path()),
                ("missing.log", directory.path().join("nope").as_path()),
            ],
        );
        let expected_tail = (11..=60)
            .map(|index| format!("line {index} https://x.test/a?..."))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            report,
            format!(
                "# ApricotPlayer diagnostic report\nApp version: 2.0.0\nyt-dlp: 2026.09.01\n\n\
                 ## Player\nActive: no\n\n## mpv.log\n{expected_tail}\n\n\
                 ## updater.log\nnot available\n\n## missing.log\nnot available\n"
            )
        );
    }

    #[test]
    fn sections_follow_python_labels_and_open_player_state() {
        use std::collections::BTreeMap;

        use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
        use apricot_storage::{SettingsDocument, SettingsPaths};

        use crate::{MainMenuAvailability, SettingsController};

        let root = tempfile::tempdir().expect("temp dir");
        let paths =
            SettingsPaths::for_app_data(&root.path().join("beta"), &root.path().join("stable"));
        let mut application = Application::new(
            SettingsController::load(paths, SettingsDocument::default()),
            MainMenuAvailability::default(),
        );
        let environment = DiagnosticEnvironment {
            app_version: "2.0.0".to_owned(),
            player_active: true,
            process_id: 42,
            ..DiagnosticEnvironment::default()
        };
        let closed = diagnostic_sections(
            &application,
            &DiagnosticEnvironment {
                player_active: false,
                ..environment.clone()
            },
        );
        assert_eq!(closed.len(), 6);
        assert!(
            !closed[1]
                .lines
                .iter()
                .any(|(label, _)| label == "mpv volume")
        );
        application.start_player_item(MediaItem {
            id: MediaId("song".to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: "Song".to_owned(),
            url: None,
            stream_url: Some(
                "https://media.example/song.mp3?token=secret"
                    .parse()
                    .expect("URL"),
            ),
            external_audio_url: None,
            local_path: Some(r"C:\Music\song.mp3".to_owned()),
            channel: String::new(),
            duration_seconds: Some(125.0),
            metadata: BTreeMap::new(),
        });
        let sections = diagnostic_sections(&application, &environment);
        let headings = sections
            .iter()
            .map(|section| section.heading.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            headings,
            [
                "# ApricotPlayer diagnostic report",
                "## Player",
                "## Audio state",
                "## Current item",
                "## Results and queue",
                "## Key settings"
            ]
        );
        let report = DiagnosticRedactor::default().report(&sections, &[]);
        for line in [
            "App version: 2.0.0",
            "Active: yes",
            "Kind: mpv",
            "Session open: yes",
            "Process PID: 42",
            "mpv volume-max: 100",
            "Title: Song",
            "Kind: local_file",
            "Duration: 2:05",
            r"Local path: C:\Music\song.mp3",
            "Stream URL: https://media.example/song.mp3 (query=yes, length=43)",
            "Stream header names: none",
            "Return index: none",
            "Cookies file configured: no",
            "Auto update yt-dlp: yes",
        ] {
            assert!(
                report.lines().any(|candidate| candidate == line),
                "{line}
{report}"
            );
        }
        assert!(!report.contains("secret"));
    }

    #[test]
    fn large_log_tail_starts_at_a_whole_line() {
        let directory = tempfile::tempdir().expect("temp dir");
        let log = directory.path().join("big.log");
        let mut text = "a".repeat(300 * 1024);
        text.push_str("\nfirst whole\nlast\n");
        std::fs::write(&log, text).expect("log");
        assert_eq!(
            redactor().file_tail(&log, 50).as_deref(),
            Some("first whole\nlast")
        );
    }
}
