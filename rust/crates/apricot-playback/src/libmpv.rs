//! Safe playback-engine facade around the dynamically loaded libmpv C API.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{
    ffi::{CStr, CString, c_char, c_double, c_int, c_void},
    path::{Path, PathBuf},
    ptr,
};

use libloading::Library;

use crate::{
    InitialPlaybackState, MpvLaunchOptions, MpvVideoMode, PlaybackCommand, PlaybackEngine,
    PlaybackError, PlaybackEvent, PlaybackMediaInfo, RepeatMode,
};

const MPV_FORMAT_NONE: c_int = 0;
const MPV_FORMAT_STRING: c_int = 1;
const MPV_FORMAT_FLAG: c_int = 3;
const MPV_FORMAT_INT64: c_int = 4;
const MPV_FORMAT_DOUBLE: c_int = 5;
const MPV_EVENT_SHUTDOWN: c_int = 1;
const MPV_EVENT_END_FILE: c_int = 7;
const MPV_EVENT_FILE_LOADED: c_int = 8;
const MPV_EVENT_PROPERTY_CHANGE: c_int = 22;
const MPV_EVENT_QUEUE_OVERFLOW: c_int = 24;
const MPV_END_FILE_REASON_EOF: c_int = 0;
const MPV_END_FILE_REASON_ERROR: c_int = 4;

type MpvHandle = c_void;
type Create = unsafe extern "C" fn() -> *mut MpvHandle;
type Initialize = unsafe extern "C" fn(*mut MpvHandle) -> c_int;
type TerminateDestroy = unsafe extern "C" fn(*mut MpvHandle);
type SetOptionString = unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
type Command = unsafe extern "C" fn(*mut MpvHandle, *const *const c_char) -> c_int;
type ObserveProperty = unsafe extern "C" fn(*mut MpvHandle, u64, *const c_char, c_int) -> c_int;
type WaitEvent = unsafe extern "C" fn(*mut MpvHandle, c_double) -> *const MpvEvent;
type ErrorString = unsafe extern "C" fn(c_int) -> *const c_char;

#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct MpvEventProperty {
    name: *const c_char,
    format: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct MpvEventEndFile {
    reason: c_int,
    error: c_int,
    playlist_entry_id: i64,
    playlist_insert_id: i64,
    playlist_insert_num_entries: c_int,
}

struct MpvApi {
    _library: Library,
    create: Create,
    initialize: Initialize,
    terminate_destroy: TerminateDestroy,
    set_option_string: SetOptionString,
    command: Command,
    observe_property: ObserveProperty,
    wait_event: WaitEvent,
    error_string: ErrorString,
}

impl MpvApi {
    unsafe fn load(path: &Path) -> Result<Self, PlaybackError> {
        let library = Library::new(path).map_err(|error| {
            PlaybackError::Operation(format!(
                "could not load libmpv from {}: {error}",
                path.display()
            ))
        })?;
        let create = *library
            .get::<Create>(b"mpv_create\0")
            .map_err(|error| symbol_error(&error))?;
        let initialize = *library
            .get::<Initialize>(b"mpv_initialize\0")
            .map_err(|error| symbol_error(&error))?;
        let terminate_destroy = *library
            .get::<TerminateDestroy>(b"mpv_terminate_destroy\0")
            .map_err(|error| symbol_error(&error))?;
        let set_option_string = *library
            .get::<SetOptionString>(b"mpv_set_option_string\0")
            .map_err(|error| symbol_error(&error))?;
        let command = *library
            .get::<Command>(b"mpv_command\0")
            .map_err(|error| symbol_error(&error))?;
        let observe_property = *library
            .get::<ObserveProperty>(b"mpv_observe_property\0")
            .map_err(|error| symbol_error(&error))?;
        let wait_event = *library
            .get::<WaitEvent>(b"mpv_wait_event\0")
            .map_err(|error| symbol_error(&error))?;
        let error_string = *library
            .get::<ErrorString>(b"mpv_error_string\0")
            .map_err(|error| symbol_error(&error))?;
        Ok(Self {
            _library: library,
            create,
            initialize,
            terminate_destroy,
            set_option_string,
            command,
            observe_property,
            wait_event,
            error_string,
        })
    }

    unsafe fn check(&self, status: c_int, operation: &str) -> Result<(), PlaybackError> {
        if status >= 0 {
            return Ok(());
        }
        Err(PlaybackError::Operation(format!(
            "{operation}: {}",
            self.error_detail(status)
        )))
    }

    unsafe fn error_detail(&self, status: c_int) -> String {
        let message = (self.error_string)(status);
        if message.is_null() {
            format!("libmpv error {status}")
        } else {
            CStr::from_ptr(message).to_string_lossy().into_owned()
        }
    }

    unsafe fn set_option(
        &self,
        handle: *mut MpvHandle,
        name: &str,
        value: &str,
    ) -> Result<(), PlaybackError> {
        let name = c_string(name, "libmpv option name")?;
        let value = c_string(value, "libmpv option value")?;
        self.check(
            (self.set_option_string)(handle, name.as_ptr(), value.as_ptr()),
            "could not configure libmpv",
        )
    }

    unsafe fn run_command(
        &self,
        handle: *mut MpvHandle,
        arguments: &[String],
    ) -> Result<(), PlaybackError> {
        let strings = arguments
            .iter()
            .map(|argument| c_string(argument, "libmpv command argument"))
            .collect::<Result<Vec<_>, _>>()?;
        let mut pointers = strings
            .iter()
            .map(|value| value.as_ptr())
            .collect::<Vec<_>>();
        pointers.push(ptr::null());
        self.check(
            (self.command)(handle, pointers.as_ptr()),
            "libmpv rejected a playback command",
        )
    }

    unsafe fn observe(
        &self,
        handle: *mut MpvHandle,
        id: u64,
        property: &str,
        format: c_int,
    ) -> Result<(), PlaybackError> {
        let property = c_string(property, "libmpv property name")?;
        self.check(
            (self.observe_property)(handle, id, property.as_ptr(), format),
            "could not observe a libmpv property",
        )
    }
}

fn symbol_error(error: &libloading::Error) -> PlaybackError {
    PlaybackError::Operation(format!("libmpv has an incompatible client API: {error}"))
}

fn c_string(value: &str, label: &str) -> Result<CString, PlaybackError> {
    CString::new(value)
        .map_err(|_| PlaybackError::InvalidData(format!("{label} contains a null byte")))
}

/// One libmpv client instance owned by the playback worker thread.
pub struct LibMpvEngine {
    api: MpvApi,
    handle: usize,
    elapsed: f64,
    duration: Option<f64>,
    media_info: PlaybackMediaInfo,
    shutdown_reported: bool,
}

impl LibMpvEngine {
    /// Loads the client DLL, creates one idle player, and subscribes to the
    /// properties needed by the application state model.
    ///
    /// # Errors
    ///
    /// Returns an error when the DLL is absent or incompatible, an option is
    /// rejected, or libmpv initialization fails.
    pub fn load(options: &MpvLaunchOptions) -> Result<Self, PlaybackError> {
        let library_path = library_path(options);
        if !library_path.is_file() {
            return Err(PlaybackError::Operation(format!(
                "libmpv was not found at {}",
                library_path.display()
            )));
        }
        unsafe { Self::load_inner(options, &library_path) }
    }

    unsafe fn load_inner(
        options: &MpvLaunchOptions,
        library_path: &Path,
    ) -> Result<Self, PlaybackError> {
        validate_options(options)?;
        let api = MpvApi::load(library_path)?;
        let handle = (api.create)();
        if handle.is_null() {
            return Err(PlaybackError::Operation(
                "libmpv could not create a player instance".to_owned(),
            ));
        }
        let result = configure(&api, handle, options)
            .and_then(|()| api.check((api.initialize)(handle), "libmpv initialization failed"))
            .and_then(|()| subscribe(&api, handle));
        if let Err(error) = result {
            (api.terminate_destroy)(handle);
            return Err(error);
        }
        Ok(Self {
            api,
            handle: handle as usize,
            elapsed: 0.0,
            duration: None,
            media_info: PlaybackMediaInfo::default(),
            shutdown_reported: false,
        })
    }

    fn handle(&self) -> *mut MpvHandle {
        self.handle as *mut MpvHandle
    }

    unsafe fn next_event(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError> {
        let event = (self.api.wait_event)(self.handle(), 0.0);
        if event.is_null() {
            return Err(PlaybackError::InvalidData(
                "libmpv returned a null event".to_owned(),
            ));
        }
        match (*event).event_id {
            MPV_EVENT_FILE_LOADED => Ok(Some(PlaybackEvent::Started)),
            MPV_EVENT_END_FILE => self.project_end_file((*event).data),
            MPV_EVENT_PROPERTY_CHANGE => self.project_property((*event).data),
            MPV_EVENT_QUEUE_OVERFLOW => Ok(Some(PlaybackEvent::Failed(
                "libmpv event queue overflowed".to_owned(),
            ))),
            MPV_EVENT_SHUTDOWN if !self.shutdown_reported => {
                self.shutdown_reported = true;
                Ok(Some(PlaybackEvent::Failed(
                    "libmpv shut down unexpectedly".to_owned(),
                )))
            }
            _ => Ok(None),
        }
    }

    unsafe fn project_end_file(
        &self,
        data: *mut c_void,
    ) -> Result<Option<PlaybackEvent>, PlaybackError> {
        if data.is_null() {
            return Err(PlaybackError::InvalidData(
                "libmpv end-file event had no data".to_owned(),
            ));
        }
        let end = &*data.cast::<MpvEventEndFile>();
        match end.reason {
            MPV_END_FILE_REASON_EOF => Ok(Some(PlaybackEvent::Ended)),
            MPV_END_FILE_REASON_ERROR => Ok(Some(PlaybackEvent::Failed(format!(
                "libmpv playback failed: {}",
                self.api.error_detail(end.error)
            )))),
            _ => Ok(None),
        }
    }

    unsafe fn project_property(
        &mut self,
        data: *mut c_void,
    ) -> Result<Option<PlaybackEvent>, PlaybackError> {
        if data.is_null() {
            return Err(PlaybackError::InvalidData(
                "libmpv property event had no data".to_owned(),
            ));
        }
        let property = &*data.cast::<MpvEventProperty>();
        if property.name.is_null() || property.format == MPV_FORMAT_NONE || property.data.is_null()
        {
            return Ok(None);
        }
        match CStr::from_ptr(property.name).to_bytes() {
            b"pause" if property.format == MPV_FORMAT_FLAG => {
                let paused = *property.data.cast::<c_int>() != 0;
                Ok(Some(PlaybackEvent::Paused(paused)))
            }
            b"time-pos" if property.format == MPV_FORMAT_DOUBLE => {
                self.elapsed = (*property.data.cast::<f64>()).max(0.0);
                Ok(Some(self.position_event()))
            }
            b"duration" if property.format == MPV_FORMAT_DOUBLE => {
                self.duration = Some((*property.data.cast::<f64>()).max(0.0));
                Ok(Some(self.position_event()))
            }
            b"file-format" if property.format == MPV_FORMAT_STRING => {
                self.media_info.container = property_string(property.data);
                Ok(Some(self.media_info_event()))
            }
            b"video-codec" if property.format == MPV_FORMAT_STRING => {
                self.media_info.video_codec = property_string(property.data);
                Ok(Some(self.media_info_event()))
            }
            b"video-params/w" if property.format == MPV_FORMAT_INT64 => {
                self.media_info.width = property_u32(property.data);
                Ok(Some(self.media_info_event()))
            }
            b"video-params/h" if property.format == MPV_FORMAT_INT64 => {
                self.media_info.height = property_u32(property.data);
                Ok(Some(self.media_info_event()))
            }
            b"audio-codec-name" if property.format == MPV_FORMAT_STRING => {
                self.media_info.audio_codec = property_string(property.data);
                Ok(Some(self.media_info_event()))
            }
            b"audio-bitrate" if property.format == MPV_FORMAT_DOUBLE => {
                let bitrate = *property.data.cast::<f64>();
                self.media_info.audio_bitrate_bits_per_second =
                    (bitrate.is_finite() && bitrate > 0.0).then_some(bitrate);
                Ok(Some(self.media_info_event()))
            }
            b"audio-params/samplerate" if property.format == MPV_FORMAT_INT64 => {
                self.media_info.sample_rate_hz = property_u32(property.data);
                Ok(Some(self.media_info_event()))
            }
            b"audio-params/channel-count" if property.format == MPV_FORMAT_INT64 => {
                self.media_info.channel_count = property_u32(property.data);
                Ok(Some(self.media_info_event()))
            }
            b"audio-params/hr-channels" if property.format == MPV_FORMAT_STRING => {
                self.media_info.channel_layout = property_string(property.data);
                Ok(Some(self.media_info_event()))
            }
            _ => Ok(None),
        }
    }

    fn position_event(&self) -> PlaybackEvent {
        PlaybackEvent::Position {
            elapsed: self.elapsed,
            duration: self.duration,
        }
    }

    fn media_info_event(&self) -> PlaybackEvent {
        PlaybackEvent::MediaInfo(self.media_info.clone())
    }
}

impl PlaybackEngine for LibMpvEngine {
    fn execute(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError> {
        if matches!(&command, PlaybackCommand::Load { .. }) {
            self.elapsed = 0.0;
            self.duration = None;
            self.media_info = PlaybackMediaInfo::default();
        }
        let arguments = command_arguments(command)?;
        unsafe { self.api.run_command(self.handle(), &arguments) }
    }

    fn poll_event(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError> {
        unsafe { self.next_event() }
    }
}

impl Drop for LibMpvEngine {
    fn drop(&mut self) {
        unsafe {
            (self.api.terminate_destroy)(self.handle());
        }
    }
}

fn library_path(options: &MpvLaunchOptions) -> PathBuf {
    if let Some(path) = &options.library {
        return path.clone();
    }
    if options
        .executable
        .extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("dll"))
    {
        return options.executable.clone();
    }
    options.executable.parent().map_or_else(
        || PathBuf::from("libmpv-2.dll"),
        |parent| parent.join("libmpv-2.dll"),
    )
}

fn validate_options(options: &MpvLaunchOptions) -> Result<(), PlaybackError> {
    if !(1..=300).contains(&options.volume_max) {
        return Err(PlaybackError::Operation(
            "libmpv volume maximum must be between 1 and 300".to_owned(),
        ));
    }
    Ok(())
}

unsafe fn configure(
    api: &MpvApi,
    handle: *mut MpvHandle,
    options: &MpvLaunchOptions,
) -> Result<(), PlaybackError> {
    let volume_max = f64::from(options.volume_max);
    let mut values = vec![
        ("config", "no".to_owned()),
        ("terminal", "no".to_owned()),
        ("idle", "yes".to_owned()),
        ("keep-open", "yes".to_owned()),
        ("volume-max", options.volume_max.to_string()),
        (
            "volume",
            options.initial_volume.clamp(0.0, volume_max).to_string(),
        ),
        (
            "speed",
            options.initial_speed.clamp(0.01, 100.0).to_string(),
        ),
        (
            "pitch",
            options.initial_pitch.clamp(0.01, 100.0).to_string(),
        ),
        (
            "pause",
            yes_no(options.initial_playback_state == InitialPlaybackState::Paused).to_owned(),
        ),
        (
            "loop-file",
            if options.repeat_mode == RepeatMode::One {
                "inf"
            } else {
                "no"
            }
            .to_owned(),
        ),
        ("gapless-audio", yes_no(options.gapless).to_owned()),
        ("replaygain", options.replay_gain.clone()),
        ("replaygain-clip", "yes".to_owned()),
    ];
    configure_video(&mut values, options.video_mode);
    configure_optional_values(&mut values, options);
    configure_cache(&mut values, options);
    for (name, value) in values {
        api.set_option(handle, name, &value)?;
    }
    Ok(())
}

fn configure_video(values: &mut Vec<(&'static str, String)>, mode: MpvVideoMode) {
    match mode {
        MpvVideoMode::Detached => values.push(("force-window", "no".to_owned())),
        MpvVideoMode::Embedded(window) => {
            values.push(("force-window", "no".to_owned()));
            values.push(("wid", window.to_string()));
        }
        MpvVideoMode::AudioOnly => {
            values.push(("force-window", "no".to_owned()));
            values.push(("vid", "no".to_owned()));
        }
    }
}

fn configure_optional_values(values: &mut Vec<(&'static str, String)>, options: &MpvLaunchOptions) {
    if let Some(path) = &options.log_file {
        values.push(("log-file", path.to_string_lossy().into_owned()));
    }
    if let Some(device) = options
        .audio_device
        .as_deref()
        .filter(|device| !device.eq_ignore_ascii_case("auto"))
    {
        values.push(("audio-device", device.to_owned()));
    }
    if let Some(driver) = &options.audio_driver {
        values.push(("ao", driver.clone()));
    }
    if let Some(filter) = &options.initial_audio_filter {
        values.push(("af", filter.clone()));
    }
}

fn configure_cache(values: &mut Vec<(&'static str, String)>, options: &MpvLaunchOptions) {
    if let Some(cache) = options.cache.map(crate::MpvCacheConfig::normalized) {
        let back_cache = (cache.megabytes / 2).clamp(64, cache.megabytes);
        values.extend([
            ("cache", "yes".to_owned()),
            ("cache-pause", "yes".to_owned()),
            ("demuxer-max-bytes", format!("{}MiB", cache.megabytes)),
            ("demuxer-max-back-bytes", format!("{back_cache}MiB")),
            ("demuxer-readahead-secs", "30".to_owned()),
            (
                "stream-lavf-o",
                "reconnect=1,reconnect_on_network_error=1,reconnect_delay_max=5".to_owned(),
            ),
        ]);
    } else {
        values.push(("cache", "no".to_owned()));
    }
}

unsafe fn subscribe(api: &MpvApi, handle: *mut MpvHandle) -> Result<(), PlaybackError> {
    for (id, name, format) in [
        (1, "pause", MPV_FORMAT_FLAG),
        (2, "time-pos", MPV_FORMAT_DOUBLE),
        (3, "duration", MPV_FORMAT_DOUBLE),
        (4, "file-format", MPV_FORMAT_STRING),
        (5, "video-codec", MPV_FORMAT_STRING),
        (6, "video-params/w", MPV_FORMAT_INT64),
        (7, "video-params/h", MPV_FORMAT_INT64),
        (8, "audio-codec-name", MPV_FORMAT_STRING),
        (9, "audio-bitrate", MPV_FORMAT_DOUBLE),
        (10, "audio-params/samplerate", MPV_FORMAT_INT64),
        (11, "audio-params/channel-count", MPV_FORMAT_INT64),
        (12, "audio-params/hr-channels", MPV_FORMAT_STRING),
    ] {
        api.observe(handle, id, name, format)?;
    }
    Ok(())
}

unsafe fn property_string(data: *mut c_void) -> Option<String> {
    let value = *data.cast::<*const c_char>();
    if value.is_null() {
        return None;
    }
    let value = CStr::from_ptr(value).to_string_lossy();
    let value = value.trim();
    (!value.is_empty()).then(|| value.chars().take(128).collect())
}

unsafe fn property_u32(data: *mut c_void) -> Option<u32> {
    u32::try_from(*data.cast::<i64>())
        .ok()
        .filter(|value| *value > 0)
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn command_arguments(command: PlaybackCommand) -> Result<Vec<String>, PlaybackError> {
    let arguments = match command {
        PlaybackCommand::Load {
            item,
            start_position_seconds,
        } => {
            let mut arguments = vec![
                "loadfile".to_owned(),
                media_target(&item)?,
                "replace".to_owned(),
            ];
            let mut options = Vec::new();
            if let Some(audio_url) = &item.external_audio_url {
                let audio_url = audio_url.to_string();
                options.push(format!("audio-file=%{}%{audio_url}", audio_url.len()));
            }
            if let Some(position) =
                start_position_seconds.filter(|value| value.is_finite() && *value >= 0.0)
            {
                options.push(format!("start={position}"));
            }
            if !options.is_empty() {
                arguments.extend(["-1".to_owned(), options.join(",")]);
            }
            arguments
        }
        PlaybackCommand::SetPaused(paused) => {
            vec![
                "set".to_owned(),
                "pause".to_owned(),
                yes_no(paused).to_owned(),
            ]
        }
        PlaybackCommand::SeekRelative { seconds, exact } => vec![
            "seek".to_owned(),
            seconds.to_string(),
            if exact { "relative+exact" } else { "relative" }.to_owned(),
        ],
        PlaybackCommand::SeekAbsolute { seconds, exact } => vec![
            "seek".to_owned(),
            seconds.max(0.0).to_string(),
            if exact { "absolute+exact" } else { "absolute" }.to_owned(),
        ],
        PlaybackCommand::SetVolume(volume) => vec![
            "set".to_owned(),
            "volume".to_owned(),
            volume.max(0.0).to_string(),
        ],
        PlaybackCommand::SetVolumeMax(volume_max) => vec![
            "set".to_owned(),
            "volume-max".to_owned(),
            volume_max.clamp(1, 300).to_string(),
        ],
        PlaybackCommand::SetSpeed(speed) => vec![
            "set".to_owned(),
            "speed".to_owned(),
            speed.clamp(0.01, 100.0).to_string(),
        ],
        PlaybackCommand::SetPitch(pitch) => vec![
            "set".to_owned(),
            "pitch".to_owned(),
            pitch.clamp(0.01, 100.0).to_string(),
        ],
        PlaybackCommand::SetRepeat(enabled) => vec![
            "set".to_owned(),
            "loop-file".to_owned(),
            if enabled { "inf" } else { "no" }.to_owned(),
        ],
        PlaybackCommand::SetAudioFilter(filter) => vec![
            "set".to_owned(),
            "af".to_owned(),
            filter.unwrap_or_default(),
        ],
        PlaybackCommand::Stop => vec!["stop".to_owned()],
    };
    Ok(arguments)
}

fn media_target(item: &apricot_core::MediaItem) -> Result<String, PlaybackError> {
    item.local_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| item.stream_url.as_ref().map(ToString::to_string))
        .or_else(|| item.url.as_ref().map(ToString::to_string))
        .ok_or_else(|| PlaybackError::Operation("media item has no playable target".to_owned()))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::Path};

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};

    use super::{command_arguments, library_path};
    use crate::{MpvLaunchOptions, PlaybackCommand};

    #[test]
    fn library_defaults_to_the_runtime_directory() {
        let options = MpvLaunchOptions::new(Path::new(r"C:\Apricot\mpv\mpv.exe"));
        assert_eq!(
            library_path(&options),
            Path::new(r"C:\Apricot\mpv\libmpv-2.dll")
        );
    }

    #[test]
    fn command_mapping_prefers_a_local_path() {
        let item = MediaItem {
            id: MediaId("track".to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: "Track".to_owned(),
            url: Some("https://example.invalid/wrong".parse().expect("URL")),
            stream_url: None,
            external_audio_url: None,
            local_path: Some(r"C:\Music\Track.mp3".to_owned()),
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        };
        assert_eq!(
            command_arguments(PlaybackCommand::Load {
                item: Box::new(item),
                start_position_seconds: None,
            })
            .expect("command"),
            ["loadfile", r"C:\Music\Track.mp3", "replace"]
        );
    }

    #[test]
    fn command_mapping_keeps_public_link_separate_from_resolved_video_and_audio() {
        let item = MediaItem {
            id: MediaId("video".to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: "Video".to_owned(),
            url: Some("https://youtube.test/watch?v=video".parse().expect("URL")),
            stream_url: Some("https://media.test/video".parse().expect("URL")),
            external_audio_url: Some("https://media.test/audio".parse().expect("URL")),
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        };
        assert_eq!(
            command_arguments(PlaybackCommand::Load {
                item: Box::new(item),
                start_position_seconds: None,
            })
            .expect("command"),
            [
                "loadfile",
                "https://media.test/video",
                "replace",
                "-1",
                "audio-file=%24%https://media.test/audio",
            ]
        );
    }

    #[test]
    fn load_command_applies_exact_initial_position_with_other_file_options() {
        let item = MediaItem {
            id: MediaId("video".to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: "Video".to_owned(),
            url: Some("https://youtube.test/watch?v=video".parse().expect("URL")),
            stream_url: Some("https://media.test/video".parse().expect("URL")),
            external_audio_url: Some("https://media.test/audio".parse().expect("URL")),
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        };
        assert_eq!(
            command_arguments(PlaybackCommand::Load {
                item: Box::new(item),
                start_position_seconds: Some(12.3),
            })
            .expect("command"),
            [
                "loadfile",
                "https://media.test/video",
                "replace",
                "-1",
                "audio-file=%24%https://media.test/audio,start=12.3",
            ]
        );
    }
}
