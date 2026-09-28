//! Owned mpv process, command mapping, and bounded asynchronous event stream.

use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use apricot_core::MediaItem;
use serde_json::{Value, json};

use crate::{
    MpvIpcClient, PlaybackCommand, PlaybackEngine, PlaybackError, PlaybackEvent, PlaybackMediaInfo,
    mpv_ipc::{MAX_READ_CHUNK_BYTES, MAX_RESPONSE_BYTES, available_bytes},
};

const IPC_START_TIMEOUT: Duration = Duration::from_secs(4);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(1);
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(5);
const EVENT_CHANNEL_CAPACITY: usize = 128;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MpvCacheConfig {
    pub megabytes: u32,
}

impl MpvCacheConfig {
    #[must_use]
    pub fn normalized(self) -> Self {
        Self {
            megabytes: self.megabytes.clamp(128, 4_096),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MpvVideoMode {
    Detached,
    Embedded(isize),
    AudioOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialPlaybackState {
    Playing,
    Paused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepeatMode {
    Off,
    One,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MpvLaunchOptions {
    pub executable: PathBuf,
    pub library: Option<PathBuf>,
    pub working_directory: PathBuf,
    pub log_file: Option<PathBuf>,
    pub video_mode: MpvVideoMode,
    pub initial_volume: f64,
    pub volume_max: u16,
    pub initial_speed: f64,
    pub initial_pitch: f64,
    pub audio_pitch_correction: bool,
    pub initial_playback_state: InitialPlaybackState,
    pub initial_position_seconds: Option<f64>,
    pub repeat_mode: RepeatMode,
    pub gapless: bool,
    pub replay_gain: String,
    pub audio_device: Option<String>,
    pub audio_driver: Option<String>,
    pub cache: Option<MpvCacheConfig>,
    pub initial_audio_filter: Option<String>,
}

impl MpvLaunchOptions {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        let executable = executable.into();
        let working_directory = executable
            .parent()
            .map_or_else(PathBuf::new, Path::to_path_buf);
        Self {
            executable,
            library: None,
            working_directory,
            log_file: None,
            video_mode: MpvVideoMode::Detached,
            initial_volume: 100.0,
            volume_max: 100,
            initial_speed: 1.0,
            initial_pitch: 1.0,
            audio_pitch_correction: true,
            initial_playback_state: InitialPlaybackState::Playing,
            initial_position_seconds: None,
            repeat_mode: RepeatMode::Off,
            gapless: true,
            replay_gain: "no".to_owned(),
            audio_device: None,
            audio_driver: None,
            cache: Some(MpvCacheConfig { megabytes: 512 }),
            initial_audio_filter: None,
        }
    }
}

pub struct MpvProcessEngine {
    process: Child,
    client: MpvIpcClient,
    events: Receiver<PlaybackEvent>,
    monitor_stop: Arc<AtomicBool>,
    monitor: Option<JoinHandle<()>>,
    process_exit_reported: bool,
}

impl MpvProcessEngine {
    /// Starts an idle mpv process and its bounded event monitor.
    ///
    /// # Errors
    ///
    /// Returns an error when the executable is missing, process creation fails,
    /// or the JSON IPC endpoint does not become ready before the deadline.
    pub fn spawn(options: &MpvLaunchOptions) -> Result<Self, PlaybackError> {
        validate_launch_options(options)?;
        let pipe_path = crate::make_unique_ipc_path();
        let arguments = launch_arguments(options, &pipe_path);
        let mut command = Command::new(&options.executable);
        command
            .args(&arguments)
            .current_dir(&options.working_directory)
            .stdin(Stdio::null());
        configure_process_output(&mut command, options.log_file.as_deref())?;
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let mut process = command
            .spawn()
            .map_err(|error| PlaybackError::Operation(format!("could not start mpv: {error}")))?;
        let client = MpvIpcClient::new(pipe_path.clone());
        if let Err(error) = wait_for_ipc(&client, &mut process) {
            let _ = process.kill();
            let _ = process.wait();
            return Err(error);
        }
        let (event_sender, events) = sync_channel(EVENT_CHANNEL_CAPACITY);
        let monitor_stop = Arc::new(AtomicBool::new(false));
        let monitor_stop_clone = Arc::clone(&monitor_stop);
        let monitor = thread::Builder::new()
            .name("apricot-mpv-events".to_owned())
            .spawn(move || event_monitor(&pipe_path, &event_sender, &monitor_stop_clone))
            .map_err(|error| {
                let _ = client.request(json!(["quit"]), COMMAND_TIMEOUT);
                let _ = process.kill();
                let _ = process.wait();
                PlaybackError::Operation(format!("could not start mpv event monitor: {error}"))
            })?;
        Ok(Self {
            process,
            client,
            events,
            monitor_stop,
            monitor: Some(monitor),
            process_exit_reported: false,
        })
    }

    pub fn process_id(&self) -> u32 {
        self.process.id()
    }

    pub fn ipc_path(&self) -> &str {
        self.client.pipe_path()
    }
}

impl PlaybackEngine for MpvProcessEngine {
    fn execute(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError> {
        let command = match command {
            PlaybackCommand::Load {
                item,
                start_position_seconds,
            } => {
                let mut options = serde_json::Map::new();
                if let Some(url) = &item.external_audio_url {
                    options.insert("audio-file".to_owned(), Value::String(url.to_string()));
                }
                if let Some(position) = valid_start_position(start_position_seconds) {
                    options.insert("start".to_owned(), Value::String(position.to_string()));
                }
                json!(["loadfile", media_target(&item)?, "replace", -1, options])
            }
            PlaybackCommand::SetPaused(paused) => json!(["set_property", "pause", paused]),
            PlaybackCommand::SeekRelative { seconds, exact } => json!([
                "seek",
                seconds,
                if exact { "relative+exact" } else { "relative" }
            ]),
            PlaybackCommand::SeekAbsolute { seconds, exact } => json!([
                "seek",
                seconds.max(0.0),
                if exact { "absolute+exact" } else { "absolute" }
            ]),
            PlaybackCommand::SetVolume(volume) => {
                json!(["set_property", "volume", volume.max(0.0)])
            }
            PlaybackCommand::SetVolumeMax(volume_max) => {
                json!(["set_property", "volume-max", volume_max.clamp(1, 300)])
            }
            PlaybackCommand::SetSpeed(speed) => {
                json!(["set_property", "speed", speed.clamp(0.01, 100.0)])
            }
            PlaybackCommand::SetPitch(pitch) => {
                json!(["set_property", "pitch", pitch.clamp(0.01, 100.0)])
            }
            PlaybackCommand::SetAudioPitchCorrection(enabled) => {
                json!(["set_property", "audio-pitch-correction", enabled])
            }
            PlaybackCommand::SetRepeat(enabled) => {
                json!([
                    "set_property",
                    "loop-file",
                    if enabled { "inf" } else { "no" }
                ])
            }
            PlaybackCommand::SetAudioFilter(filter) => {
                json!(["set_property", "af", filter.unwrap_or_default()])
            }
            PlaybackCommand::AddAudioFilter(filter) => json!(["af", "add", filter]),
            PlaybackCommand::RemoveAudioFilter(reference) => json!(["af", "remove", reference]),
            PlaybackCommand::AudioFilterCommand {
                label,
                command,
                argument,
            } => json!(["af-command", label, command, argument]),
            PlaybackCommand::SetEqualizerFilter(_) | PlaybackCommand::SetPitchFilter(_) => {
                return Err(PlaybackError::Operation(
                    "audio filter updates are expanded by the playback worker".to_owned(),
                ));
            }
            PlaybackCommand::SetReplayGain(mode) => {
                json!(["set_property", "replaygain", mode])
            }
            PlaybackCommand::SetAudioDevice(device) => {
                json!(["set_property", "audio-device", device])
            }
            PlaybackCommand::Stop => json!(["stop"]),
        };
        request_success(&self.client, command, COMMAND_TIMEOUT)
    }

    fn poll_event(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError> {
        match self.events.try_recv() {
            Ok(event) => return Ok(Some(event)),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) if !self.process_exit_reported => {
                self.process_exit_reported = true;
                return Ok(Some(PlaybackEvent::Failed(
                    "mpv event monitor stopped".to_owned(),
                )));
            }
            Err(TryRecvError::Disconnected) => return Ok(None),
        }
        if !self.process_exit_reported
            && let Some(status) = self.process.try_wait().map_err(|error| {
                PlaybackError::Operation(format!("could not read mpv process status: {error}"))
            })?
        {
            self.process_exit_reported = true;
            return Ok(Some(PlaybackEvent::Failed(format!(
                "mpv exited unexpectedly with {status}"
            ))));
        }
        Ok(None)
    }
}

fn valid_start_position(position: Option<f64>) -> Option<f64> {
    position.filter(|value| value.is_finite() && *value >= 0.0)
}

impl Drop for MpvProcessEngine {
    fn drop(&mut self) {
        self.monitor_stop.store(true, Ordering::Release);
        let _ = self
            .client
            .request(json!(["quit"]), Duration::from_millis(500));
        let deadline = Instant::now() + Duration::from_millis(750);
        while Instant::now() < deadline {
            if self.process.try_wait().ok().flatten().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if self.process.try_wait().ok().flatten().is_none() {
            let _ = self.process.kill();
        }
        let _ = self.process.wait();
        if let Some(monitor) = self.monitor.take() {
            let _ = monitor.join();
        }
    }
}

fn validate_launch_options(options: &MpvLaunchOptions) -> Result<(), PlaybackError> {
    if !options.executable.is_file() {
        return Err(PlaybackError::Operation(format!(
            "mpv executable was not found at {}",
            options.executable.display()
        )));
    }
    if !options.working_directory.is_dir() {
        return Err(PlaybackError::Operation(format!(
            "mpv working directory was not found at {}",
            options.working_directory.display()
        )));
    }
    if !(1..=300).contains(&options.volume_max) {
        return Err(PlaybackError::Operation(
            "mpv volume maximum must be between 1 and 300".to_owned(),
        ));
    }
    Ok(())
}

fn launch_arguments(options: &MpvLaunchOptions, pipe_path: &str) -> Vec<OsString> {
    let volume_max = f64::from(options.volume_max);
    let mut arguments = vec![
        OsString::from("--no-config"),
        OsString::from(if matches!(options.video_mode, MpvVideoMode::Embedded(_)) {
            "--force-window=yes"
        } else {
            "--force-window=no"
        }),
        OsString::from(format!("--input-ipc-server={pipe_path}")),
        OsString::from("--idle=yes"),
        OsString::from("--keep-open=yes"),
        OsString::from(format!("--volume-max={}", options.volume_max)),
        OsString::from(format!(
            "--volume={}",
            options.initial_volume.clamp(0.0, volume_max)
        )),
        OsString::from(format!(
            "--pitch={}",
            options.initial_pitch.clamp(0.01, 100.0)
        )),
        OsString::from(format!(
            "--speed={}",
            options.initial_speed.clamp(0.01, 100.0)
        )),
        OsString::from(format!(
            "--audio-pitch-correction={}",
            if options.audio_pitch_correction {
                "yes"
            } else {
                "no"
            }
        )),
        OsString::from(format!(
            "--loop-file={}",
            if options.repeat_mode == RepeatMode::One {
                "inf"
            } else {
                "no"
            }
        )),
        OsString::from(format!(
            "--gapless-audio={}",
            if options.gapless { "yes" } else { "no" }
        )),
        OsString::from(format!("--replaygain={}", options.replay_gain)),
        OsString::from("--replaygain-clip=yes"),
        OsString::from("--term-playing-msg="),
        OsString::from("--msg-level=all=warn"),
    ];
    if let MpvVideoMode::Embedded(video_host) = options.video_mode {
        arguments.push(OsString::from(format!("--wid={video_host}")));
    }
    if options.video_mode == MpvVideoMode::AudioOnly {
        arguments.push(OsString::from("--vid=no"));
    }
    if options.initial_playback_state == InitialPlaybackState::Paused {
        arguments.push(OsString::from("--pause=yes"));
    }
    if let Some(device) = options
        .audio_device
        .as_deref()
        .filter(|device| !device.eq_ignore_ascii_case("auto"))
    {
        arguments.push(OsString::from(format!("--audio-device={device}")));
    }
    if let Some(driver) = options.audio_driver.as_deref() {
        arguments.push(OsString::from(format!("--ao={driver}")));
    }
    if let Some(cache) = options.cache.map(MpvCacheConfig::normalized) {
        let back_cache = (cache.megabytes / 2).clamp(64, cache.megabytes);
        arguments.extend([
            OsString::from("--cache=yes"),
            OsString::from("--cache-pause=yes"),
            OsString::from(format!("--demuxer-max-bytes={}MiB", cache.megabytes)),
            OsString::from(format!("--demuxer-max-back-bytes={back_cache}MiB")),
            OsString::from("--demuxer-readahead-secs=30"),
            OsString::from(
                "--stream-lavf-o=reconnect=1,reconnect_on_network_error=1,reconnect_delay_max=5",
            ),
        ]);
    } else {
        arguments.push(OsString::from("--cache=no"));
    }
    if let Some(filter) = options.initial_audio_filter.as_deref() {
        arguments.push(OsString::from(format!("--af={filter}")));
    }
    arguments
}

fn configure_process_output(
    command: &mut Command,
    log_file: Option<&Path>,
) -> Result<(), PlaybackError> {
    if let Some(path) = log_file {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                PlaybackError::Operation(format!("could not create mpv log directory: {error}"))
            })?;
        }
        let output = File::create(path).map_err(|error| {
            PlaybackError::Operation(format!("could not create mpv log: {error}"))
        })?;
        let errors = output.try_clone().map_err(|error| {
            PlaybackError::Operation(format!("could not clone mpv log handle: {error}"))
        })?;
        command
            .stdout(Stdio::from(output))
            .stderr(Stdio::from(errors));
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    Ok(())
}

fn wait_for_ipc(client: &MpvIpcClient, process: &mut Child) -> Result<(), PlaybackError> {
    let deadline = Instant::now() + IPC_START_TIMEOUT;
    loop {
        if let Some(status) = process.try_wait().map_err(|error| {
            PlaybackError::Operation(format!("could not read mpv startup status: {error}"))
        })? {
            return Err(PlaybackError::Operation(format!(
                "mpv exited during startup with {status}"
            )));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(PlaybackError::Timeout);
        }
        match client.request(
            json!(["get_property", "mpv-version"]),
            remaining.min(Duration::from_millis(250)),
        ) {
            Ok(response) if response_is_success(&response) => return Ok(()),
            Ok(_) | Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn request_success(
    client: &MpvIpcClient,
    command: Value,
    timeout: Duration,
) -> Result<(), PlaybackError> {
    let response = client.request(command, timeout)?;
    if response_is_success(&response) {
        Ok(())
    } else {
        Err(PlaybackError::Operation(format!(
            "mpv rejected command: {}",
            response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        )))
    }
}

fn response_is_success(response: &Value) -> bool {
    response.get("error").and_then(Value::as_str) == Some("success")
}

fn media_target(item: &MediaItem) -> Result<String, PlaybackError> {
    item.local_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| item.stream_url.as_ref().map(ToString::to_string))
        .or_else(|| item.url.as_ref().map(ToString::to_string))
        .ok_or_else(|| PlaybackError::Operation("media item has no playable target".to_owned()))
}

fn event_monitor(pipe_path: &str, sender: &SyncSender<PlaybackEvent>, stop: &AtomicBool) {
    if let Err(error) = event_monitor_inner(pipe_path, sender, stop) {
        let _ = sender.try_send(PlaybackEvent::Failed(error.to_string()));
    }
}

fn event_monitor_inner(
    pipe_path: &str,
    sender: &SyncSender<PlaybackEvent>,
    stop: &AtomicBool,
) -> Result<(), PlaybackError> {
    let deadline = Instant::now() + IPC_START_TIMEOUT;
    let mut pipe = open_event_pipe(pipe_path, deadline)?;
    for (observer_id, property) in [
        (1, "pause"),
        (2, "time-pos"),
        (3, "duration"),
        (4, "idle-active"),
        (5, "file-format"),
        (6, "video-codec"),
        (7, "video-params/w"),
        (8, "video-params/h"),
        (9, "audio-codec-name"),
        (10, "audio-bitrate"),
        (11, "audio-params/samplerate"),
        (12, "audio-params/channel-count"),
        (13, "audio-params/hr-channels"),
        (14, "chapter-list"),
        (15, "audio-device-list"),
    ] {
        let mut payload = serde_json::to_vec(&json!({
            "command": ["observe_property", observer_id, property]
        }))
        .map_err(|error| PlaybackError::InvalidData(error.to_string()))?;
        payload.push(b'\n');
        pipe.write_all(&payload).map_err(|error| {
            PlaybackError::Operation(format!("mpv event subscription failed: {error}"))
        })?;
    }
    pipe.flush().map_err(|error| {
        PlaybackError::Operation(format!("mpv event subscription flush failed: {error}"))
    })?;
    let mut buffer = Vec::new();
    let mut elapsed = 0.0;
    let mut duration = None;
    let mut media_info = PlaybackMediaInfo::default();
    while !stop.load(Ordering::Acquire) {
        let available = available_bytes(&pipe)?;
        if available == 0 {
            thread::sleep(EVENT_POLL_INTERVAL);
            continue;
        }
        let mut chunk = vec![0_u8; available.min(MAX_READ_CHUNK_BYTES)];
        let read = pipe
            .read(&mut chunk)
            .map_err(|error| PlaybackError::Operation(format!("mpv event read failed: {error}")))?;
        if read == 0 {
            thread::sleep(EVENT_POLL_INTERVAL);
            continue;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > MAX_RESPONSE_BYTES {
            return Err(PlaybackError::InvalidData(
                "mpv event buffer exceeded 1 MiB".to_owned(),
            ));
        }
        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=newline).collect();
            let line = &line[..line.len().saturating_sub(1)];
            if line.is_empty() {
                continue;
            }
            let event: Value = serde_json::from_slice(line)
                .map_err(|error| PlaybackError::InvalidData(error.to_string()))?;
            project_event(&event, sender, &mut elapsed, &mut duration, &mut media_info);
        }
    }
    Ok(())
}

fn open_event_pipe(path: &str, deadline: Instant) -> Result<File, PlaybackError> {
    loop {
        match OpenOptions::new().read(true).write(true).open(path) {
            Ok(pipe) => return Ok(pipe),
            Err(error) if Instant::now() >= deadline => {
                return Err(PlaybackError::Operation(format!(
                    "mpv event pipe was unavailable: {error}"
                )));
            }
            Err(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn project_event(
    event: &Value,
    sender: &SyncSender<PlaybackEvent>,
    elapsed: &mut f64,
    duration: &mut Option<f64>,
    media_info: &mut PlaybackMediaInfo,
) {
    match event.get("event").and_then(Value::as_str) {
        Some("start-file") => {
            *elapsed = 0.0;
            *duration = None;
            *media_info = PlaybackMediaInfo::default();
        }
        Some("file-loaded") => {
            let _ = sender.try_send(PlaybackEvent::Started);
        }
        Some("end-file") if event.get("reason").and_then(Value::as_str) == Some("eof") => {
            let _ = sender.try_send(PlaybackEvent::Ended);
        }
        Some("property-change") => match event.get("name").and_then(Value::as_str) {
            Some("chapter-list") => {
                media_info.chapters = event
                    .get("data")
                    .and_then(Value::as_array)
                    .map(|chapters| chapters.iter().take(10000).cloned().collect())
                    .unwrap_or_default();
                emit_media_info(sender, media_info);
            }
            Some("audio-device-list") => {
                let devices = event
                    .get("data")
                    .map(crate::audio_output_devices_from_json)
                    .unwrap_or_default();
                let _ = sender.try_send(PlaybackEvent::AudioDevices(devices));
            }
            Some("pause") => {
                if let Some(paused) = event.get("data").and_then(Value::as_bool) {
                    let _ = sender.try_send(PlaybackEvent::Paused(paused));
                }
            }
            Some("time-pos") => {
                if let Some(value) = event.get("data").and_then(Value::as_f64) {
                    *elapsed = value.max(0.0);
                    let _ = sender.try_send(PlaybackEvent::Position {
                        elapsed: *elapsed,
                        duration: *duration,
                    });
                }
            }
            Some("duration") => {
                *duration = event.get("data").and_then(Value::as_f64);
                let _ = sender.try_send(PlaybackEvent::Position {
                    elapsed: *elapsed,
                    duration: *duration,
                });
            }
            Some("file-format") => {
                media_info.container = event_string(event);
                emit_media_info(sender, media_info);
            }
            Some("video-codec") => {
                media_info.video_codec = event_string(event);
                emit_media_info(sender, media_info);
            }
            Some("video-params/w") => {
                media_info.width = event_u32(event);
                emit_media_info(sender, media_info);
            }
            Some("video-params/h") => {
                media_info.height = event_u32(event);
                emit_media_info(sender, media_info);
            }
            Some("audio-codec-name") => {
                media_info.audio_codec = event_string(event);
                emit_media_info(sender, media_info);
            }
            Some("audio-bitrate") => {
                media_info.audio_bitrate_bits_per_second = event
                    .get("data")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value > 0.0);
                emit_media_info(sender, media_info);
            }
            Some("audio-params/samplerate") => {
                media_info.sample_rate_hz = event_u32(event);
                emit_media_info(sender, media_info);
            }
            Some("audio-params/channel-count") => {
                media_info.channel_count = event_u32(event);
                emit_media_info(sender, media_info);
            }
            Some("audio-params/hr-channels") => {
                media_info.channel_layout = event_string(event);
                emit_media_info(sender, media_info);
            }
            _ => {}
        },
        _ => {}
    }
}

fn event_string(event: &Value) -> Option<String> {
    event
        .get("data")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(128).collect())
}

fn event_u32(event: &Value) -> Option<u32> {
    event
        .get("data")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn emit_media_info(sender: &SyncSender<PlaybackEvent>, media_info: &PlaybackMediaInfo) {
    let _ = sender.try_send(PlaybackEvent::MediaInfo(media_info.clone()));
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::Path};

    use apricot_core::{MediaId, MediaKind, MediaSource};

    use super::{MpvCacheConfig, MpvLaunchOptions, launch_arguments, media_target, project_event};
    use crate::{PlaybackEvent, PlaybackMediaInfo};

    #[test]
    fn launch_arguments_apply_volume_before_any_media_is_loaded() {
        let mut options = MpvLaunchOptions::new(Path::new(r"C:\mpv\mpv.exe"));
        options.initial_volume = 20.0;
        options.volume_max = 100;
        options.cache = Some(MpvCacheConfig { megabytes: 512 });
        let arguments = launch_arguments(&options, r"\\.\pipe\test");
        let arguments: Vec<_> = arguments
            .iter()
            .map(|argument| argument.to_string_lossy())
            .collect();
        assert!(arguments.iter().any(|argument| argument == "--volume=20"));
        assert!(
            arguments
                .iter()
                .any(|argument| argument == "--volume-max=100")
        );
        assert!(arguments.iter().any(|argument| argument == "--idle=yes"));
        assert!(!arguments.iter().any(|argument| argument == "--volume=300"));
    }

    #[test]
    fn local_media_target_wins_over_remote_metadata() {
        let item = apricot_core::MediaItem {
            id: MediaId("local".to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: "Track".to_owned(),
            url: Some("https://example.invalid/wrong".parse().expect("URL")),
            stream_url: None,
            external_audio_url: None,
            local_path: Some(r"C:\Music\Track.mp3".to_owned()),
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::default(),
        };
        assert_eq!(media_target(&item).expect("target"), r"C:\Music\Track.mp3");
    }

    #[test]
    fn resolved_stream_wins_over_the_durable_public_url() {
        let item = apricot_core::MediaItem {
            id: MediaId("remote".to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: "Video".to_owned(),
            url: Some("https://youtube.test/watch?v=remote".parse().expect("URL")),
            stream_url: Some("https://media.test/stream".parse().expect("URL")),
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::default(),
        };
        assert_eq!(
            media_target(&item).expect("target"),
            "https://media.test/stream"
        );
    }

    #[test]
    fn event_projection_preserves_position_and_duration() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let mut elapsed = 0.0;
        let mut duration = None;
        let mut media_info = PlaybackMediaInfo::default();
        project_event(
            &serde_json::json!({"event":"property-change","name":"duration","data":90.0}),
            &sender,
            &mut elapsed,
            &mut duration,
            &mut media_info,
        );
        project_event(
            &serde_json::json!({"event":"property-change","name":"time-pos","data":12.5}),
            &sender,
            &mut elapsed,
            &mut duration,
            &mut media_info,
        );
        assert_eq!(
            receiver.try_iter().last(),
            Some(PlaybackEvent::Position {
                elapsed: 12.5,
                duration: Some(90.0),
            })
        );
    }

    #[test]
    fn event_projection_accumulates_typed_media_information() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let mut elapsed = 0.0;
        let mut duration = None;
        let mut media_info = PlaybackMediaInfo::default();
        for event in [
            serde_json::json!({"event":"property-change","name":"file-format","data":"matroska"}),
            serde_json::json!({"event":"property-change","name":"audio-codec-name","data":"opus"}),
            serde_json::json!({"event":"property-change","name":"video-params/h","data":1080}),
        ] {
            project_event(
                &event,
                &sender,
                &mut elapsed,
                &mut duration,
                &mut media_info,
            );
        }
        assert_eq!(
            receiver.try_iter().last(),
            Some(PlaybackEvent::MediaInfo(PlaybackMediaInfo {
                container: Some("matroska".to_owned()),
                audio_codec: Some("opus".to_owned()),
                height: Some(1080),
                ..PlaybackMediaInfo::default()
            }))
        );
    }
}
