//! Safe playback-engine facade around the dynamically loaded libmpv C API.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{
    ffi::{CStr, CString, c_char, c_double, c_int, c_void},
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant},
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
const MPV_FORMAT_NODE: c_int = 6;
const MPV_FORMAT_NODE_ARRAY: c_int = 7;
const MPV_FORMAT_NODE_MAP: c_int = 8;

#[repr(C)]
union NodeValue {
    string: *const c_char,
    integer: i64,
    double: f64,
    list: *const NodeList,
}

#[repr(C)]
struct Node {
    value: NodeValue,
    format: c_int,
}

#[repr(C)]
struct NodeList {
    count: c_int,
    values: *const Node,
    keys: *const *const c_char,
}

// libmpv owns these nodes until the next wait_event call. Copy only the shallow
// array/maps with the listed keys while that lifetime is active; never retain
// native pointers.
unsafe fn node_json(node: &Node, depth: usize, wanted: &[&str]) -> serde_json::Value {
    use serde_json::Value;
    if depth > 3 {
        return Value::Null;
    }
    match node.format {
        MPV_FORMAT_STRING if !node.value.string.is_null() => Value::String(
            CStr::from_ptr(node.value.string)
                .to_string_lossy()
                .chars()
                .take(4096)
                .collect(),
        ),
        MPV_FORMAT_DOUBLE => {
            serde_json::Number::from_f64(node.value.double).map_or(Value::Null, Value::Number)
        }
        MPV_FORMAT_INT64 => Value::from(node.value.integer),
        MPV_FORMAT_NODE_ARRAY | MPV_FORMAT_NODE_MAP if !node.value.list.is_null() => {
            let list = &*node.value.list;
            let Ok(count) = usize::try_from(list.count) else {
                return Value::Null;
            };
            if count > 10000 || (count > 0 && list.values.is_null()) {
                return Value::Null;
            }
            if count == 0 {
                return if node.format == MPV_FORMAT_NODE_ARRAY {
                    Value::Array(Vec::new())
                } else {
                    Value::Object(serde_json::Map::new())
                };
            }
            let values = std::slice::from_raw_parts(list.values, count);
            if node.format == MPV_FORMAT_NODE_ARRAY {
                return Value::Array(
                    values
                        .iter()
                        .map(|value| node_json(value, depth + 1, wanted))
                        .collect(),
                );
            }
            if list.keys.is_null() {
                return Value::Null;
            }
            let keys = std::slice::from_raw_parts(list.keys, count);
            let mut result = serde_json::Map::new();
            for (key, value) in keys.iter().zip(values) {
                if !key.is_null() {
                    let key = CStr::from_ptr(*key).to_string_lossy();
                    if wanted.contains(&key.as_ref()) {
                        result.insert(key.into_owned(), node_json(value, depth + 1, wanted));
                    }
                }
            }
            Value::Object(result)
        }
        _ => Value::Null,
    }
}
const MPV_EVENT_SHUTDOWN: c_int = 1;
const MPV_EVENT_LOG_MESSAGE: c_int = 2;
const MPV_EVENT_START_FILE: c_int = 6;
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
type GetProperty = unsafe extern "C" fn(*mut MpvHandle, *const c_char, c_int, *mut c_void) -> c_int;
type WaitEvent = unsafe extern "C" fn(*mut MpvHandle, c_double) -> *const MpvEvent;
type ErrorString = unsafe extern "C" fn(c_int) -> *const c_char;
type RequestLogMessages = unsafe extern "C" fn(*mut MpvHandle, *const c_char) -> c_int;
type StreamCbAddRo =
    unsafe extern "C" fn(*mut MpvHandle, *const c_char, *mut c_void, StreamOpen) -> c_int;
type StreamOpen = unsafe extern "C" fn(*mut c_void, *mut c_char, *mut StreamCbInfo) -> c_int;

/// mpv `mpv_stream_cb_info`.
#[repr(C)]
struct StreamCbInfo {
    cookie: *mut c_void,
    read_fn: Option<unsafe extern "C" fn(*mut c_void, *mut c_char, u64) -> i64>,
    seek_fn: Option<unsafe extern "C" fn(*mut c_void, i64) -> i64>,
    size_fn: Option<unsafe extern "C" fn(*mut c_void) -> i64>,
    close_fn: Option<unsafe extern "C" fn(*mut c_void)>,
    cancel_fn: Option<unsafe extern "C" fn(*mut c_void)>,
}

type PcmCookie = std::sync::Arc<dyn crate::pcm_source::PcmStream>;

/// mpv opens `apricot-pcm://<generation>` from its own thread.
unsafe extern "C" fn pcm_open(
    _user: *mut c_void,
    uri: *mut c_char,
    info: *mut StreamCbInfo,
) -> c_int {
    const MPV_ERROR_LOADING_FAILED: c_int = -13;
    if uri.is_null() || info.is_null() {
        return MPV_ERROR_LOADING_FAILED;
    }
    let uri = CStr::from_ptr(uri).to_string_lossy();
    let Some(id) = uri.rsplit('/').next().and_then(|id| id.parse::<u64>().ok()) else {
        return MPV_ERROR_LOADING_FAILED;
    };
    let Some(stream) = crate::pcm_source::pcm_source().and_then(|source| source.open(id)) else {
        return MPV_ERROR_LOADING_FAILED;
    };
    let cookie: Box<PcmCookie> = Box::new(stream);
    (*info).cookie = Box::into_raw(cookie).cast();
    (*info).read_fn = Some(pcm_read);
    (*info).seek_fn = None;
    (*info).size_fn = None;
    (*info).close_fn = Some(pcm_close);
    (*info).cancel_fn = Some(pcm_cancel);
    0
}

unsafe extern "C" fn pcm_read(cookie: *mut c_void, buffer: *mut c_char, size: u64) -> i64 {
    let stream = &*cookie.cast::<PcmCookie>();
    let size = usize::try_from(size).unwrap_or(usize::MAX).min(1 << 20);
    let buffer = std::slice::from_raw_parts_mut(buffer.cast::<u8>(), size);
    i64::try_from(stream.read(buffer)).unwrap_or(0)
}

unsafe extern "C" fn pcm_close(cookie: *mut c_void) {
    let stream = Box::from_raw(cookie.cast::<PcmCookie>());
    stream.cancel();
}

unsafe extern "C" fn pcm_cancel(cookie: *mut c_void) {
    (*cookie.cast::<PcmCookie>()).cancel();
}

/// Python `start_mpv` writes the terminal output of mpv to `mpv.log`. mpv
/// prints messages of level info and above on the terminal.
const TERMINAL_LOG_LEVEL: &str = "info";

#[repr(C)]
struct MpvEventLogMessage {
    prefix: *const c_char,
    _level: *const c_char,
    text: *const c_char,
    _log_level: c_int,
}

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
struct MpvEventStartFile {
    playlist_entry_id: i64,
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
    get_property: GetProperty,
    wait_event: WaitEvent,
    error_string: ErrorString,
    request_log_messages: RequestLogMessages,
    stream_cb_add_ro: Option<StreamCbAddRo>,
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
        let get_property = *library
            .get::<GetProperty>(b"mpv_get_property\0")
            .map_err(|error| symbol_error(&error))?;
        let wait_event = *library
            .get::<WaitEvent>(b"mpv_wait_event\0")
            .map_err(|error| symbol_error(&error))?;
        let error_string = *library
            .get::<ErrorString>(b"mpv_error_string\0")
            .map_err(|error| symbol_error(&error))?;
        let request_log_messages = *library
            .get::<RequestLogMessages>(b"mpv_request_log_messages\0")
            .map_err(|error| symbol_error(&error))?;
        let stream_cb_add_ro = library
            .get::<StreamCbAddRo>(b"mpv_stream_cb_add_ro\0")
            .ok()
            .map(|symbol| *symbol);
        Ok(Self {
            _library: library,
            create,
            initialize,
            terminate_destroy,
            set_option_string,
            command,
            observe_property,
            get_property,
            wait_event,
            error_string,
            request_log_messages,
            stream_cb_add_ro,
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

    unsafe fn property_i64(&self, handle: *mut MpvHandle, property: &str) -> Option<i64> {
        let property = c_string(property, "libmpv property name").ok()?;
        let mut value: i64 = 0;
        let status = (self.get_property)(
            handle,
            property.as_ptr(),
            MPV_FORMAT_INT64,
            (&raw mut value).cast(),
        );
        (status >= 0).then_some(value)
    }

    unsafe fn property_flag(&self, handle: *mut MpvHandle, property: &str) -> Option<bool> {
        let property = c_string(property, "libmpv property name").ok()?;
        let mut value: c_int = 0;
        let status = (self.get_property)(
            handle,
            property.as_ptr(),
            MPV_FORMAT_FLAG,
            (&raw mut value).cast(),
        );
        (status >= 0).then_some(value != 0)
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
    /// Python `mpv.log`: rewritten for every started item.
    log_path: Option<PathBuf>,
    log: Option<File>,
    /// Playlist entry of the latest `loadfile`. libmpv may still hold events of
    /// the replaced file; they must not be reported as the new item's events.
    expected_entry: Option<i64>,
    current_entry: Option<i64>,
    /// With `keep-open=yes` mpv pauses at the end instead of ending the file,
    /// so the end is reported once from `eof-reached` (Python `player_monitor_worker`).
    ended_reported: bool,
    /// The item plays as PCM from the installed [`crate::pcm_source`].
    pcm: Option<PcmPlayback>,
}

/// A PCM item: mpv positions are relative to the loaded generation.
struct PcmPlayback {
    base_ms: u32,
    /// Stream time where the current track of the generation starts.
    offset: f64,
    started: bool,
    loaded: bool,
    /// Gapless boundaries not yet reached: stream seconds, base ms and the
    /// length of the next track, applied when it becomes audible.
    boundaries: std::collections::VecDeque<(f64, u32, Option<f64>)>,
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
            .and_then(|()| subscribe(&api, handle))
            .and_then(|()| register_pcm_protocol(&api, handle))
            .and_then(|()| {
                if options.log_file.is_none() {
                    return Ok(());
                }
                let level = c_string(TERMINAL_LOG_LEVEL, "libmpv log level")?;
                api.check(
                    (api.request_log_messages)(handle, level.as_ptr()),
                    "could not request libmpv log messages",
                )
            });
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
            log_path: options.log_file.clone(),
            log: None,
            expected_entry: None,
            current_entry: None,
            ended_reported: false,
            pcm: None,
        })
    }

    fn handle(&self) -> *mut MpvHandle {
        self.handle as *mut MpvHandle
    }

    unsafe fn next_event(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError> {
        if let Some(event) = self.poll_pcm()? {
            return Ok(Some(event));
        }
        let event = (self.api.wait_event)(self.handle(), 0.0);
        if event.is_null() {
            return Err(PlaybackError::InvalidData(
                "libmpv returned a null event".to_owned(),
            ));
        }
        match (*event).event_id {
            MPV_EVENT_LOG_MESSAGE => {
                self.write_log_message((*event).data);
                Ok(None)
            }
            MPV_EVENT_START_FILE => {
                let data = (*event).data;
                if !data.is_null() {
                    self.current_entry =
                        Some((*data.cast::<MpvEventStartFile>()).playlist_entry_id);
                    self.ended_reported = false;
                }
                Ok(None)
            }
            MPV_EVENT_FILE_LOADED if self.stale() => Ok(None),
            MPV_EVENT_FILE_LOADED => {
                // A PCM item reports its start once, not for every generation.
                if let Some(pcm) = self.pcm.as_mut() {
                    if pcm.started {
                        return Ok(None);
                    }
                    pcm.started = true;
                }
                Ok(Some(PlaybackEvent::Started))
            }
            MPV_EVENT_END_FILE => {
                if self.pcm.is_some() {
                    let end = &*(*event).data.cast::<MpvEventEndFile>();
                    log::info!(
                        "pcm end-file reason {} entry {} expected {:?}",
                        end.reason,
                        end.playlist_entry_id,
                        self.expected_entry
                    );
                }
                self.project_end_file((*event).data)
            }
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

    /// Diagnostics only: a failing log file never stops playback.
    unsafe fn write_log_message(&mut self, data: *mut c_void) {
        let Some(log) = self.log.as_mut() else {
            return;
        };
        if data.is_null() {
            return;
        }
        let message = &*data.cast::<MpvEventLogMessage>();
        let text = |pointer: *const c_char| {
            if pointer.is_null() {
                String::new()
            } else {
                CStr::from_ptr(pointer).to_string_lossy().into_owned()
            }
        };
        let line = terminal_log_line(&text(message.prefix), &text(message.text));
        let _ = log.write_all(line.as_bytes());
    }

    fn restart_log(&mut self) {
        self.log = self
            .log_path
            .as_deref()
            .and_then(|path| File::create(path).ok());
    }

    /// True while libmpv still reports events of a replaced file.
    fn stale(&self) -> bool {
        self.expected_entry
            .is_some_and(|expected| self.current_entry != Some(expected))
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
        if self
            .expected_entry
            .is_some_and(|expected| expected != end.playlist_entry_id)
        {
            return Ok(None);
        }
        match end.reason {
            MPV_END_FILE_REASON_EOF if self.ended_reported => Ok(None),
            MPV_END_FILE_REASON_EOF => Ok(Some(PlaybackEvent::Ended)),
            MPV_END_FILE_REASON_ERROR => Ok(Some(PlaybackEvent::Failed(format!(
                "libmpv playback failed: {}",
                self.api.error_detail(end.error)
            )))),
            _ => Ok(None),
        }
    }

    #[allow(clippy::too_many_lines)]
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
        let name = CStr::from_ptr(property.name).to_bytes();
        if self.stale() && !matches!(name, b"pause" | b"audio-device-list") {
            return Ok(None);
        }
        match name {
            b"chapter-list" if property.format == MPV_FORMAT_NODE => {
                self.media_info.chapters =
                    node_json(&*property.data.cast::<Node>(), 0, &["time", "title"])
                        .as_array()
                        .cloned()
                        .unwrap_or_default();
                Ok(Some(self.media_info_event()))
            }
            b"audio-device-list" if property.format == MPV_FORMAT_NODE => {
                let list = node_json(&*property.data.cast::<Node>(), 0, &["name", "description"]);
                Ok(Some(PlaybackEvent::AudioDevices(
                    crate::audio_output_devices_from_json(&list),
                )))
            }
            b"pause" if property.format == MPV_FORMAT_FLAG => {
                let paused = *property.data.cast::<c_int>() != 0;
                if self.pcm.is_some() {
                    log::info!(
                        "pcm pause {paused}, eof {:?}",
                        self.api.property_flag(self.handle(), "eof-reached")
                    );
                }
                // The pause mpv applies at the end is the end, not a user pause.
                if paused
                    && !self.stale()
                    && self.api.property_flag(self.handle(), "eof-reached") == Some(true)
                {
                    return Ok(self.report_end());
                }
                Ok(Some(PlaybackEvent::Paused(paused)))
            }
            b"eof-reached" if property.format == MPV_FORMAT_FLAG => {
                if self.pcm.is_some() {
                    log::info!("pcm eof-reached {}", *property.data.cast::<c_int>() != 0);
                }
                // mpv sets eof-reached when decoding ends, while the audio output
                // still plays its buffer. The end is the pause mpv applies after
                // that, or eof-reached arriving while the player is already paused.
                if *property.data.cast::<c_int>() != 0
                    && self.api.property_flag(self.handle(), "eof-reached") == Some(true)
                {
                    if self.api.property_flag(self.handle(), "pause") == Some(true) {
                        Ok(self.report_end())
                    } else {
                        Ok(None)
                    }
                } else {
                    if *property.data.cast::<c_int>() == 0 {
                        self.ended_reported = false;
                    }
                    Ok(None)
                }
            }
            b"time-pos" if property.format == MPV_FORMAT_DOUBLE => {
                let stream_time = (*property.data.cast::<f64>()).max(0.0);
                if let Some(pcm) = self.pcm.as_mut() {
                    // Gapless: the next track becomes the item when it is heard.
                    while pcm
                        .boundaries
                        .front()
                        .is_some_and(|(at, _, _)| stream_time >= *at)
                    {
                        if let Some((at, base_ms, duration)) = pcm.boundaries.pop_front() {
                            pcm.offset = at;
                            pcm.base_ms = base_ms;
                            if duration.is_some() {
                                self.duration = duration;
                            }
                        }
                    }
                }
                let (base, offset) = self.pcm.as_ref().map_or((0.0, 0.0), |pcm| {
                    (f64::from(pcm.base_ms) / 1000.0, pcm.offset)
                });
                self.elapsed = base + (stream_time - offset).max(0.0);
                Ok(Some(self.position_event()))
            }
            // A PCM stream has no length and no codec; the item's own
            // duration and the source's format stay.
            b"duration" | b"audio-codec-name" | b"audio-bitrate" | b"file-format"
                if self.pcm.is_some() =>
            {
                Ok(None)
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

    /// Loads a new PCM generation and forwards the source's own events.
    unsafe fn poll_pcm(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError> {
        if self.pcm.is_none() {
            return Ok(None);
        }
        let Some(source) = crate::pcm_source::pcm_source() else {
            self.pcm = None;
            return Ok(Some(PlaybackEvent::Failed(
                "the PCM source is gone".to_owned(),
            )));
        };
        if let Some(generation) = source.take_generation() {
            let arguments = [
                "loadfile".to_owned(),
                format!("{}://{}", crate::pcm_source::PCM_PROTOCOL, generation.id),
                "replace".to_owned(),
                "-1".to_owned(),
                crate::pcm_source::pcm_file_options(),
            ];
            self.api.run_command(self.handle(), &arguments)?;
            self.expected_entry = self.api.property_i64(self.handle(), "playlist/0/id");
            log::info!(
                "pcm loadfile generation {} entry {:?}",
                generation.id,
                self.expected_entry
            );
            self.ended_reported = false;
            if let Some(pcm) = self.pcm.as_mut() {
                pcm.base_ms = generation.base_ms;
                pcm.offset = 0.0;
                pcm.boundaries.clear();
                pcm.loaded = true;
            }
            // A generation of a new track carries that track's length.
            if let Some(duration_ms) = generation.duration_ms.filter(|ms| *ms > 0) {
                self.duration = Some(f64::from(duration_ms) / 1000.0);
            }
            self.elapsed = f64::from(generation.base_ms) / 1000.0;
            let (codec, bitrate) = source.format();
            self.media_info.audio_codec = Some(codec);
            // Spotify streams are Ogg Vorbis; the PCM itself has no container.
            self.media_info.container = Some("ogg".to_owned());
            self.media_info.audio_bitrate_bits_per_second = bitrate;
            return Ok(Some(self.position_event()));
        }
        match source.poll_event() {
            Some(crate::pcm_source::PcmSourceEvent::Paused(paused)) => {
                self.api.run_command(
                    self.handle(),
                    &[
                        "set".to_owned(),
                        "pause".to_owned(),
                        yes_no(paused).to_owned(),
                    ],
                )?;
                Ok(None)
            }
            Some(crate::pcm_source::PcmSourceEvent::Failed(reason)) => {
                Ok(Some(PlaybackEvent::Failed(reason)))
            }
            Some(crate::pcm_source::PcmSourceEvent::Boundary {
                at_ms,
                base_ms,
                duration_ms,
            }) => {
                if let Some(pcm) = self.pcm.as_mut() {
                    #[allow(clippy::cast_precision_loss)]
                    pcm.boundaries.push_back((
                        at_ms as f64 / 1000.0,
                        base_ms,
                        duration_ms
                            .filter(|ms| *ms > 0)
                            .map(|ms| f64::from(ms) / 1000.0),
                    ));
                }
                Ok(None)
            }
            None => Ok(None),
        }
    }

    /// Transport commands of a PCM item go to its source; the rest (volume,
    /// speed, filters, device) stay with mpv. Returns `true` when handled.
    unsafe fn execute_pcm(&mut self, command: &PlaybackCommand) -> Result<bool, PlaybackError> {
        if let PlaybackCommand::Load {
            item,
            start_position_seconds,
        } = command
        {
            if let Some(source) = crate::pcm_source::pcm_source()
                && self.pcm.take().is_some()
                && !crate::pcm_source::is_pcm_item(item)
            {
                source.stop();
            }
            if !crate::pcm_source::is_pcm_item(item) {
                return Ok(false);
            }
            let source = crate::pcm_source::pcm_source()
                .ok_or_else(|| PlaybackError::Operation("Spotify is not connected".to_owned()))?;
            let position_ms = start_position_seconds
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map_or(0, seconds_to_ms);
            let paused = self.api.property_flag(self.handle(), "pause") == Some(true);
            let attach = item
                .metadata
                .get("spotify_attach")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            self.api.run_command(self.handle(), &["stop".to_owned()])?;
            self.expected_entry = None;
            self.duration = item.duration_seconds;
            self.elapsed = f64::from(position_ms) / 1000.0;
            source
                .start(item, position_ms, paused, attach)
                .map_err(PlaybackError::Operation)?;
            self.pcm = Some(PcmPlayback {
                base_ms: position_ms,
                offset: 0.0,
                started: false,
                loaded: false,
                boundaries: std::collections::VecDeque::new(),
            });
            return Ok(true);
        }
        let Some(pcm) = self.pcm.as_ref() else {
            return Ok(false);
        };
        let Some(source) = crate::pcm_source::pcm_source() else {
            return Ok(false);
        };
        let clamp = |seconds: f64| {
            let upper = self.duration.unwrap_or(f64::MAX).max(0.0);
            seconds_to_ms(seconds.clamp(0.0, upper))
        };
        match command {
            PlaybackCommand::SeekAbsolute { seconds, .. } => {
                source.seek(clamp(*seconds));
                Ok(true)
            }
            PlaybackCommand::SeekRelative { seconds, .. } => {
                let _ = pcm;
                source.seek(clamp(self.elapsed + seconds));
                Ok(true)
            }
            PlaybackCommand::SetPaused(paused) => {
                source.set_paused(*paused);
                // Keep draining PCM until the decoder acknowledges the pause.
                // Pausing mpv first can fill the bounded ring and block the
                // decoder thread before it handles any further transport command.
                Ok(true)
            }
            PlaybackCommand::Stop => {
                source.stop();
                self.pcm = None;
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    fn report_end(&mut self) -> Option<PlaybackEvent> {
        if self.ended_reported {
            return None;
        }
        self.ended_reported = true;
        Some(PlaybackEvent::Ended)
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

/// Python `audio_output_device_options` runs `mpv --audio-device=help`. The
/// libmpv equivalent is a short-lived idle client that reports
/// `audio-device-list` without a window and without playing anything.
///
/// # Errors
///
/// Returns an error when libmpv cannot be loaded or initialized, or when it
/// does not report the device list before `timeout`.
pub fn probe_audio_output_devices(
    library: &Path,
    audio_driver: Option<&str>,
    timeout: Duration,
) -> Result<Vec<crate::AudioOutputDevice>, PlaybackError> {
    if !library.is_file() {
        return Err(PlaybackError::Operation(format!(
            "libmpv was not found at {}",
            library.display()
        )));
    }
    // SAFETY: The probe owns its client handle and destroys it before returning.
    unsafe {
        let api = MpvApi::load(library)?;
        let handle = (api.create)();
        if handle.is_null() {
            return Err(PlaybackError::Operation(
                "libmpv could not create a player instance".to_owned(),
            ));
        }
        let result = probe_devices(&api, handle, audio_driver, timeout);
        (api.terminate_destroy)(handle);
        result
    }
}

unsafe fn probe_devices(
    api: &MpvApi,
    handle: *mut MpvHandle,
    audio_driver: Option<&str>,
    timeout: Duration,
) -> Result<Vec<crate::AudioOutputDevice>, PlaybackError> {
    for (name, value) in [
        ("config", "no"),
        ("terminal", "no"),
        ("idle", "yes"),
        ("vo", "null"),
    ] {
        api.set_option(handle, name, value)?;
    }
    if let Some(driver) = audio_driver {
        api.set_option(handle, "ao", driver)?;
    }
    api.check((api.initialize)(handle), "libmpv initialization failed")?;
    api.observe(handle, 1, "audio-device-list", MPV_FORMAT_NODE)?;
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(PlaybackError::Timeout);
        }
        let event = (api.wait_event)(handle, remaining.as_secs_f64());
        if event.is_null() {
            return Err(PlaybackError::InvalidData(
                "libmpv returned a null event".to_owned(),
            ));
        }
        match (*event).event_id {
            MPV_EVENT_PROPERTY_CHANGE if !(*event).data.is_null() => {
                let property = &*(*event).data.cast::<MpvEventProperty>();
                if property.format == MPV_FORMAT_NODE && !property.data.is_null() {
                    let list =
                        node_json(&*property.data.cast::<Node>(), 0, &["name", "description"]);
                    return Ok(crate::audio_output_devices_from_json(&list));
                }
            }
            MPV_EVENT_SHUTDOWN => {
                return Err(PlaybackError::Operation(
                    "libmpv shut down during the device probe".to_owned(),
                ));
            }
            _ => {}
        }
    }
}

impl PlaybackEngine for LibMpvEngine {
    fn execute(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError> {
        if unsafe { self.execute_pcm(&command)? } {
            if matches!(&command, PlaybackCommand::Load { .. }) {
                self.media_info = PlaybackMediaInfo::default();
                self.restart_log();
            }
            return Ok(());
        }
        if matches!(&command, PlaybackCommand::Load { .. }) {
            self.elapsed = 0.0;
            self.duration = None;
            self.media_info = PlaybackMediaInfo::default();
            self.restart_log();
        }
        let load = matches!(&command, PlaybackCommand::Load { .. });
        let arguments = command_arguments(command)?;
        unsafe {
            self.api.run_command(self.handle(), &arguments)?;
            if load {
                // `loadfile replace` leaves only the new entry in the playlist.
                self.expected_entry = self.api.property_i64(self.handle(), "playlist/0/id");
                self.ended_reported = false;
            }
        }
        Ok(())
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
            "audio-pitch-correction",
            yes_no(options.audio_pitch_correction).to_owned(),
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

/// mpv prints the messages of its main module without a prefix and those of
/// other modules as `[module] text`.
fn terminal_log_line(prefix: &str, text: &str) -> String {
    let mut line = if prefix.is_empty() || prefix == "cplayer" {
        text.to_owned()
    } else {
        format!("[{prefix}] {text}")
    };
    if !line.ends_with('\n') {
        line.push('\n');
    }
    line
}

fn configure_optional_values(values: &mut Vec<(&'static str, String)>, options: &MpvLaunchOptions) {
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

/// Whole milliseconds of a non-negative position, saturated at `u32::MAX`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn seconds_to_ms(seconds: f64) -> u32 {
    (seconds.max(0.0) * 1000.0).round().min(f64::from(u32::MAX)) as u32
}

/// `apricot-pcm://` for PCM sources; an old libmpv without stream callbacks
/// simply cannot play them.
unsafe fn register_pcm_protocol(api: &MpvApi, handle: *mut MpvHandle) -> Result<(), PlaybackError> {
    let Some(add) = api.stream_cb_add_ro else {
        return Ok(());
    };
    let protocol = c_string(crate::pcm_source::PCM_PROTOCOL, "PCM protocol")?;
    api.check(
        add(handle, protocol.as_ptr(), ptr::null_mut(), pcm_open),
        "could not register the PCM protocol",
    )
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
        (13, "chapter-list", MPV_FORMAT_NODE),
        (14, "audio-device-list", MPV_FORMAT_NODE),
        (15, "eof-reached", MPV_FORMAT_FLAG),
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
            options.extend(http_header_options(&item));
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
        PlaybackCommand::SetAudioPitchCorrection(enabled) => vec![
            "set".to_owned(),
            "audio-pitch-correction".to_owned(),
            yes_no(enabled).to_owned(),
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
        command @ (PlaybackCommand::AddAudioFilter(_)
        | PlaybackCommand::RemoveAudioFilter(_)
        | PlaybackCommand::AudioFilterCommand { .. }
        | PlaybackCommand::SetEqualizerFilter(_)
        | PlaybackCommand::SetPitchFilter(_)) => audio_filter_arguments(command)?,
        PlaybackCommand::SetReplayGain(mode) => {
            vec!["set".to_owned(), "replaygain".to_owned(), mode]
        }
        PlaybackCommand::SetAudioDevice(device) => {
            vec!["set".to_owned(), "audio-device".to_owned(), device]
        }
        PlaybackCommand::Stop => vec!["stop".to_owned()],
    };
    Ok(arguments)
}

/// mpv `af` and `af-command` arguments. The equalizer and pitch updates are
/// expanded into these by the playback worker before they reach the engine.
fn audio_filter_arguments(command: PlaybackCommand) -> Result<Vec<String>, PlaybackError> {
    match command {
        PlaybackCommand::AddAudioFilter(filter) => {
            Ok(vec!["af".to_owned(), "add".to_owned(), filter])
        }
        PlaybackCommand::RemoveAudioFilter(reference) => {
            Ok(vec!["af".to_owned(), "remove".to_owned(), reference])
        }
        PlaybackCommand::AudioFilterCommand {
            label,
            command,
            argument,
        } => Ok(vec!["af-command".to_owned(), label, command, argument]),
        _ => Err(PlaybackError::Operation(
            "audio filter updates are expanded by the playback worker".to_owned(),
        )),
    }
}

/// Python `start_mpv` with stream headers: `--user-agent`, `--referrer`
/// and the other headers as HTTP header fields, from the item's
/// `http_headers` metadata.
fn http_header_options(item: &apricot_core::MediaItem) -> Vec<String> {
    let Some(headers) = item
        .metadata
        .get("http_headers")
        .and_then(serde_json::Value::as_object)
    else {
        return Vec::new();
    };
    let quoted = |value: &str| format!("%{}%{value}", value.len());
    let mut options = Vec::new();
    let mut fields = Vec::new();
    for (name, value) in headers {
        let Some(value) = value.as_str().filter(|value| !value.is_empty()) else {
            continue;
        };
        match name.to_ascii_lowercase().as_str() {
            "user-agent" => options.push(format!("user-agent={}", quoted(value))),
            "referer" => options.push(format!("referrer={}", quoted(value))),
            _ => fields.push(format!("{name}: {value}")),
        }
    }
    if !fields.is_empty() {
        options.push(format!("http-header-fields={}", quoted(&fields.join(","))));
    }
    options
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

    use super::{command_arguments, http_header_options, library_path, terminal_log_line};
    use crate::{MpvLaunchOptions, PlaybackCommand};

    fn chapter_fixtures() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let folder = tempfile::tempdir().expect("fixture folder");
        let metadata = folder.path().join("chapters.ffmeta");
        std::fs::write(&metadata, ";FFMETADATA1\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=10000\ntitle=Opening\n[CHAPTER]\nTIMEBASE=1/1000\nSTART=10000\nEND=20000\ntitle=Second chapter\n").expect("chapter metadata");
        let chapter_media = folder.path().join("chapters.mka");
        let plain_media = folder.path().join("plain.wav");
        for (output, with_chapters) in [(&chapter_media, true), (&plain_media, false)] {
            use std::os::windows::process::CommandExt;
            let mut command = std::process::Command::new(
                std::env::var_os("APRICOT_TEST_FFMPEG").expect("FFmpeg"),
            );
            command.creation_flags(0x0800_0000);
            command.args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
            ]);
            if with_chapters {
                command.arg("-i").arg(&metadata).args([
                    "-map_metadata",
                    "1",
                    "-map_chapters",
                    "1",
                    "-c:a",
                    "flac",
                ]);
            }
            command.args(["-t", "20"]).arg(output);
            assert!(command.status().expect("FFmpeg fixture").success());
        }
        (folder, chapter_media, plain_media)
    }

    #[test]
    #[ignore = "requires APRICOT_TEST_MPV"]
    fn real_libmpv_probe_lists_auto_first() {
        let options = MpvLaunchOptions::new(std::path::PathBuf::from(
            std::env::var_os("APRICOT_TEST_MPV").expect("mpv path"),
        ));
        let devices = super::probe_audio_output_devices(
            &library_path(&options),
            None,
            std::time::Duration::from_secs(5),
        )
        .expect("device probe");
        assert_eq!(
            devices.first().map(|device| device.name.as_str()),
            Some("auto")
        );
    }

    #[test]
    #[ignore = "requires APRICOT_TEST_MPV and APRICOT_TEST_FFMPEG"]
    fn real_libmpv_reports_embedded_chapters() {
        use crate::{PlaybackEngine, PlaybackEvent};
        let (_folder, chapter_media, plain_media) = chapter_fixtures();
        let mut options = MpvLaunchOptions::new(std::path::PathBuf::from(
            std::env::var_os("APRICOT_TEST_MPV").expect("mpv path"),
        ));
        options.audio_driver = Some("null".to_owned());
        options.video_mode = crate::MpvVideoMode::AudioOnly;
        options.initial_playback_state = crate::InitialPlaybackState::Paused;
        let mut engine = super::LibMpvEngine::load(&options).expect("load real library");
        let item = MediaItem {
            id: MediaId("chapter-fixture".to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: "Chapter fixture".to_owned(),
            local_path: Some(chapter_media.to_string_lossy().into_owned()),
            url: None,
            stream_url: None,
            external_audio_url: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        };
        engine
            .execute(PlaybackCommand::Load {
                item: Box::new(item.clone()),
                start_position_seconds: None,
            })
            .expect("load fixture");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match engine.poll_event().expect("poll real library") {
                Some(PlaybackEvent::MediaInfo(info)) if !info.chapters.is_empty() => {
                    assert_eq!(info.chapters.len(), 2);
                    assert_eq!(info.chapters[0]["title"], "Opening");
                    assert_eq!(info.chapters[1]["time"], 10.0);
                    break;
                }
                Some(PlaybackEvent::Failed(error)) => panic!("fixture playback failed: {error}"),
                _ => {}
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no embedded chapter event"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let mut replacement = item;
        replacement.id = MediaId("no-chapters-fixture".to_owned());
        replacement.local_path = Some(plain_media.to_string_lossy().into_owned());
        engine
            .execute(PlaybackCommand::Load {
                item: Box::new(replacement),
                start_position_seconds: None,
            })
            .expect("replace fixture");
        assert!(engine.media_info.chapters.is_empty());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut started = false;
        while std::time::Instant::now() < deadline {
            match engine.poll_event().expect("replacement event") {
                Some(PlaybackEvent::Started) => started = true,
                Some(PlaybackEvent::MediaInfo(info)) if started => {
                    assert!(info.chapters.is_empty());
                    return;
                }
                Some(PlaybackEvent::Failed(error)) => panic!("replacement failed: {error}"),
                _ => {}
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("replacement did not provide fresh metadata");
    }

    #[test]
    fn chapter_nodes_are_copied_without_retaining_native_pointers() {
        use super::{
            MPV_FORMAT_DOUBLE, MPV_FORMAT_NODE_ARRAY, MPV_FORMAT_NODE_MAP, MPV_FORMAT_STRING, Node,
            NodeList, NodeValue,
        };
        let title = std::ffi::CString::new("Opening").unwrap();
        let keys = [c"time".as_ptr(), c"title".as_ptr()];
        let values = [
            Node {
                value: NodeValue { double: 12.5 },
                format: MPV_FORMAT_DOUBLE,
            },
            Node {
                value: NodeValue {
                    string: title.as_ptr(),
                },
                format: MPV_FORMAT_STRING,
            },
        ];
        let map = NodeList {
            count: 2,
            values: values.as_ptr(),
            keys: keys.as_ptr(),
        };
        let child = Node {
            value: NodeValue {
                list: &raw const map,
            },
            format: MPV_FORMAT_NODE_MAP,
        };
        let list = NodeList {
            count: 1,
            values: &raw const child,
            keys: std::ptr::null(),
        };
        let root = Node {
            value: NodeValue {
                list: &raw const list,
            },
            format: MPV_FORMAT_NODE_ARRAY,
        };
        // SAFETY: All pointers reference live local allocations for the complete call.
        let copied = unsafe { super::node_json(&root, 0, &["time", "title"]) };
        drop(title);
        assert_eq!(copied, serde_json::json!([{"time":12.5,"title":"Opening"}]));
    }

    #[test]
    fn chapter_nodes_reject_invalid_lengths_before_dereferencing_values() {
        let list = super::NodeList {
            count: -1,
            values: std::ptr::null(),
            keys: std::ptr::null(),
        };
        let root = super::Node {
            value: super::NodeValue {
                list: &raw const list,
            },
            format: super::MPV_FORMAT_NODE_ARRAY,
        };
        // SAFETY: The list is live and rejected before its null values pointer is accessed.
        assert!(unsafe { super::node_json(&root, 0, &["time", "title"]) }.is_null());
    }

    #[test]
    fn terminal_log_lines_match_mpv_terminal_output() {
        assert_eq!(
            terminal_log_line("cplayer", " (+) Audio --aid=1 (opus 2ch 48000Hz)\n"),
            " (+) Audio --aid=1 (opus 2ch 48000Hz)\n"
        );
        assert_eq!(
            terminal_log_line("ffmpeg/demuxer", "error reading header"),
            "[ffmpeg/demuxer] error reading header\n"
        );
    }

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

    #[test]
    #[ignore = "requires APRICOT_TEST_MPV and APRICOT_TEST_FFMPEG"]
    fn real_libmpv_replaces_only_the_equalizer_and_pitch_filters() {
        use crate::{AudioFilterState, PlaybackEngine, PlaybackEvent, SpeedAudioMode};
        let (_folder, _chapter_media, plain_media) = chapter_fixtures();
        let mut options = MpvLaunchOptions::new(std::path::PathBuf::from(
            std::env::var_os("APRICOT_TEST_MPV").expect("mpv path"),
        ));
        options.audio_driver = Some("null".to_owned());
        options.video_mode = crate::MpvVideoMode::AudioOnly;
        options.initial_audio_filter = crate::audio_filter_chain(
            SpeedAudioMode::Scaletempo2,
            Some("@apricot_eq:lavfi=[equalizer=f=31:t=q:w=1.7:g=3.0]"),
            crate::PitchMode::Rubberband,
            1.0,
        );
        let mut engine = super::LibMpvEngine::load(&options).expect("load real library");
        engine
            .execute(PlaybackCommand::Load {
                item: Box::new(MediaItem {
                    id: MediaId("tone".to_owned()),
                    source: MediaSource::Local,
                    kind: MediaKind::Audio,
                    title: "Tone".to_owned(),
                    local_path: Some(plain_media.to_string_lossy().into_owned()),
                    url: None,
                    stream_url: None,
                    external_audio_url: None,
                    channel: String::new(),
                    duration_seconds: None,
                    metadata: BTreeMap::new(),
                }),
                start_position_seconds: None,
            })
            .expect("load fixture");
        let wait_until_playing = |engine: &mut super::LibMpvEngine, after: f64| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                match engine.poll_event().expect("poll real library") {
                    Some(PlaybackEvent::Position { elapsed, .. }) if elapsed > after => {
                        return true;
                    }
                    Some(PlaybackEvent::Failed(error)) => panic!("playback failed: {error}"),
                    _ => std::thread::sleep(std::time::Duration::from_millis(5)),
                }
            }
            false
        };
        assert!(wait_until_playing(&mut engine, 0.1), "fixture plays");
        let mut filters = AudioFilterState::from_chain(options.initial_audio_filter.as_deref());
        for graph in [
            "lavfi=[equalizer=f=62:t=q:w=2:g=4.0]",
            "lavfi=[volume=-6.0dB,equalizer=f=1000:t=q:w=2:g=6.0,alimiter=limit=0.95:attack=5:release=80]",
        ] {
            filters
                .execute(
                    &mut engine,
                    PlaybackCommand::SetEqualizerFilter(Some(graph.to_owned())),
                )
                .expect("equalizer replacement");
        }
        filters
            .execute(&mut engine, PlaybackCommand::SetPitchFilter(Some(1.1)))
            .expect("pitch filter added");
        let pitch_command = |pitch: &str| PlaybackCommand::AudioFilterCommand {
            label: "apricot_pitch".to_owned(),
            command: "set-pitch".to_owned(),
            argument: pitch.to_owned(),
        };
        assert!(
            engine.execute(pitch_command("1.2000")).is_ok(),
            "the Rubberband filter accepts set-pitch in place"
        );
        filters
            .execute(&mut engine, PlaybackCommand::SetPitchFilter(Some(1.3)))
            .expect("pitch adjusted");
        filters
            .execute(&mut engine, PlaybackCommand::SetEqualizerFilter(None))
            .expect("equalizer cleared");
        filters
            .execute(&mut engine, PlaybackCommand::SetPitchFilter(None))
            .expect("pitch cleared");
        assert!(
            engine.execute(pitch_command("1.0000")).is_err(),
            "the pitch filter is gone"
        );
        assert!(
            engine
                .execute(PlaybackCommand::AddAudioFilter(
                    "@apricot_eq:not_a_real_filter".to_owned()
                ))
                .is_err(),
            "af add reports invalid filters"
        );
        assert!(
            wait_until_playing(&mut engine, 0.5),
            "playback continues after the filter updates"
        );
    }

    #[test]
    #[ignore = "requires APRICOT_TEST_MPV and APRICOT_TEST_FFMPEG"]
    fn real_libmpv_accepts_python_speed_pitch_and_equalizer_chains() {
        use crate::{
            PitchMode, PlaybackEngine, PlaybackEvent, SpeedAudioMode, audio_filter_chain,
            mpv_pitch_property,
        };
        let (_folder, _chapter_media, plain_media) = chapter_fixtures();
        let equalizer = "@apricot_eq:lavfi=[volume=-3.0dB,equalizer=f=31:t=q:w=1.7:g=3.0,alimiter=limit=0.95:attack=5:release=80]";
        for speed_mode in [
            "Rubberband high quality",
            "High quality scaletempo2",
            "mpv default scaletempo2",
            "Classic scaletempo",
        ] {
            for pitch_mode in [
                "Independent pitch - highest quality (mpv built-in)",
                "Independent pitch - advanced (Rubberband)",
                "Linked pitch and speed - pitch keys change both",
            ] {
                let speed_mode = SpeedAudioMode::from_setting(speed_mode);
                let pitch_mode = PitchMode::from_setting(pitch_mode);
                let mut options = MpvLaunchOptions::new(std::path::PathBuf::from(
                    std::env::var_os("APRICOT_TEST_MPV").expect("mpv path"),
                ));
                options.audio_driver = Some("null".to_owned());
                options.video_mode = crate::MpvVideoMode::AudioOnly;
                options.initial_speed = 1.5;
                options.audio_pitch_correction = speed_mode.audio_pitch_correction();
                options.initial_audio_filter =
                    audio_filter_chain(speed_mode, Some(equalizer), pitch_mode, 1.0);
                let mut engine = super::LibMpvEngine::load(&options).expect("load real library");
                engine
                    .execute(PlaybackCommand::Load {
                        item: Box::new(MediaItem {
                            id: MediaId("tone".to_owned()),
                            source: MediaSource::Local,
                            kind: MediaKind::Audio,
                            title: "Tone".to_owned(),
                            local_path: Some(plain_media.to_string_lossy().into_owned()),
                            url: None,
                            stream_url: None,
                            external_audio_url: None,
                            channel: String::new(),
                            duration_seconds: None,
                            metadata: BTreeMap::new(),
                        }),
                        start_position_seconds: None,
                    })
                    .expect("load fixture");
                let pitch = 1.12;
                engine
                    .execute(PlaybackCommand::SetAudioPitchCorrection(true))
                    .expect("pitch correction");
                engine
                    .execute(PlaybackCommand::SetPitch(mpv_pitch_property(
                        pitch_mode, pitch,
                    )))
                    .expect("pitch property");
                engine
                    .execute(PlaybackCommand::SetAudioFilter(audio_filter_chain(
                        speed_mode,
                        Some(equalizer),
                        pitch_mode,
                        pitch,
                    )))
                    .expect("pitch filter chain");
                engine
                    .execute(PlaybackCommand::SetSpeed(0.75))
                    .expect("speed change");
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                let mut advanced = false;
                while std::time::Instant::now() < deadline {
                    match engine.poll_event().expect("poll real library") {
                        Some(PlaybackEvent::Position { elapsed, .. }) if elapsed > 0.3 => {
                            advanced = true;
                            break;
                        }
                        Some(PlaybackEvent::Failed(error)) => {
                            panic!("{speed_mode:?}/{pitch_mode:?} failed: {error}")
                        }
                        _ => std::thread::sleep(std::time::Duration::from_millis(5)),
                    }
                }
                assert!(advanced, "{speed_mode:?}/{pitch_mode:?} did not play");
                assert!(
                    engine
                        .execute(PlaybackCommand::SetAudioFilter(Some(
                            "@apricot_speed:not_a_real_filter".to_owned()
                        )))
                        .is_err(),
                    "invalid filters must be rejected so the chain check is meaningful"
                );
            }
        }
    }

    #[test]
    fn stream_headers_become_per_file_options() {
        let mut item = MediaItem {
            id: MediaId("1".to_owned()),
            source: MediaSource::Audiovault,
            kind: MediaKind::Movie,
            title: "Movie".to_owned(),
            url: None,
            stream_url: Some("https://media.test/movie.mp3".parse().expect("URL")),
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        };
        assert!(http_header_options(&item).is_empty());
        item.metadata.insert(
            "http_headers".to_owned(),
            serde_json::json!({
                "User-Agent": "ApricotPlayer/2",
                "Referer": "https://direct.audiovault.net",
                "Cookie": "a=1; b=2"
            }),
        );
        assert_eq!(
            http_header_options(&item),
            [
                "user-agent=%15%ApricotPlayer/2",
                "referrer=%29%https://direct.audiovault.net",
                "http-header-fields=%16%Cookie: a=1; b=2",
            ]
        );
    }

    fn short_fixture(folder: &Path) -> MediaItem {
        use std::os::windows::process::CommandExt;
        let wav = folder.join("short.wav");
        let mut command =
            std::process::Command::new(std::env::var_os("APRICOT_TEST_FFMPEG").expect("FFmpeg"));
        command.creation_flags(0x0800_0000);
        command
            .args([
                "-nostdin",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-t",
                "1",
            ])
            .arg(&wav);
        assert!(command.status().expect("FFmpeg fixture").success());
        MediaItem {
            id: MediaId("short-fixture".to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: "Short fixture".to_owned(),
            local_path: Some(wav.to_string_lossy().into_owned()),
            url: None,
            stream_url: None,
            external_audio_url: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    fn short_engine(paused: bool) -> super::LibMpvEngine {
        let mut options = MpvLaunchOptions::new(std::path::PathBuf::from(
            std::env::var_os("APRICOT_TEST_MPV").expect("mpv path"),
        ));
        options.audio_driver = Some("null".to_owned());
        options.video_mode = crate::MpvVideoMode::AudioOnly;
        if paused {
            options.initial_playback_state = crate::InitialPlaybackState::Paused;
        }
        super::LibMpvEngine::load(&options).expect("load real library")
    }

    #[test]
    #[ignore = "requires APRICOT_TEST_MPV"]
    fn real_libmpv_defers_pcm_pause_and_rejects_old_eof_notification() {
        use crate::{
            PlaybackEngine,
            pcm_source::{PcmGeneration, PcmSource, PcmSourceEvent, PcmStream, set_pcm_source},
        };
        use std::sync::{Arc, Mutex};
        #[derive(Default)]
        struct Source {
            acknowledged: Mutex<Option<bool>>,
        }
        impl PcmSource for Source {
            fn start(&self, _: &MediaItem, _: u32, _: bool, _: bool) -> Result<(), String> {
                Ok(())
            }
            fn set_paused(&self, _: bool) {}
            fn seek(&self, _: u32) {}
            fn stop(&self) {}
            fn take_generation(&self) -> Option<PcmGeneration> {
                None
            }
            fn open(&self, _: u64) -> Option<Arc<dyn PcmStream>> {
                None
            }
            fn poll_event(&self) -> Option<PcmSourceEvent> {
                self.acknowledged
                    .lock()
                    .unwrap()
                    .take()
                    .map(PcmSourceEvent::Paused)
            }
            fn format(&self) -> (String, Option<f64>) {
                ("Test PCM".into(), None)
            }
        }
        let source = Arc::new(Source::default());
        set_pcm_source(Some(source.clone()));
        let mut engine = short_engine(false);
        engine.pcm = Some(super::PcmPlayback {
            base_ms: 0,
            offset: 0.0,
            started: true,
            loaded: true,
            boundaries: std::collections::VecDeque::default(),
        });
        engine.execute(PlaybackCommand::SetPaused(true)).unwrap();
        // SAFETY: The engine owns this initialized client for the whole test.
        assert_eq!(
            unsafe { engine.api.property_flag(engine.handle(), "pause") },
            Some(false),
            "mpv must drain the ring until the decoder acknowledges pause"
        );
        *source.acknowledged.lock().unwrap() = Some(true);
        // SAFETY: Same live client, no external event data.
        unsafe {
            engine.poll_pcm().unwrap();
        }
        assert_eq!(
            unsafe { engine.api.property_flag(engine.handle(), "pause") },
            Some(true)
        );
        let mut old_eof: std::ffi::c_int = 1;
        let mut property = super::MpvEventProperty {
            name: c"eof-reached".as_ptr(),
            format: super::MPV_FORMAT_FLAG,
            data: (&raw mut old_eof).cast(),
        };
        // A queued EOF of the previous entry arrives after live EOF cleared.
        assert_ne!(
            unsafe { engine.api.property_flag(engine.handle(), "eof-reached") },
            Some(true)
        );
        let event = unsafe { engine.project_property((&raw mut property).cast()).unwrap() };
        set_pcm_source(None);
        assert!(
            event.is_none(),
            "old EOF must not finish the replacement: {event:?}"
        );
    }

    fn collect_events(
        engine: &mut super::LibMpvEngine,
        duration: std::time::Duration,
    ) -> Vec<crate::PlaybackEvent> {
        use crate::{PlaybackEngine, PlaybackEvent};
        let deadline = std::time::Instant::now() + duration;
        let mut events = Vec::new();
        while std::time::Instant::now() < deadline {
            match engine.poll_event().expect("poll real library") {
                Some(
                    PlaybackEvent::MediaInfo(_)
                    | PlaybackEvent::AudioDevices(_)
                    | PlaybackEvent::Position { .. },
                ) => {}
                Some(event) => events.push(event),
                None => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
        events
    }

    /// With `keep-open=yes` mpv only pauses at the end; the engine must report
    /// the end once (Python `player_monitor_worker` polls `eof-reached`).
    #[test]
    #[ignore = "requires APRICOT_TEST_MPV and APRICOT_TEST_FFMPEG"]
    fn real_libmpv_reports_the_natural_end_once_instead_of_a_pause() {
        use crate::{PlaybackEngine, PlaybackEvent};
        let folder = tempfile::tempdir().expect("fixture folder");
        let item = short_fixture(folder.path());
        let mut engine = short_engine(false);
        engine
            .execute(PlaybackCommand::Load {
                item: Box::new(item),
                start_position_seconds: None,
            })
            .expect("load fixture");
        let events = collect_events(&mut engine, std::time::Duration::from_millis(2500));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, PlaybackEvent::Ended))
                .count(),
            1,
            "{events:?}"
        );
        assert!(!events.contains(&PlaybackEvent::Paused(true)), "{events:?}");
        // Python `restart_current_playback`: seek to the start, then unpause.
        engine
            .execute(PlaybackCommand::SeekAbsolute {
                seconds: 0.0,
                exact: true,
            })
            .expect("seek to start");
        engine
            .execute(PlaybackCommand::SetPaused(false))
            .expect("unpause");
        let events = collect_events(&mut engine, std::time::Duration::from_millis(2500));
        assert_eq!(
            events,
            vec![PlaybackEvent::Paused(false), PlaybackEvent::Ended],
            "the restarted item plays to its end again"
        );
    }

    #[test]
    #[ignore = "requires APRICOT_TEST_MPV and APRICOT_TEST_FFMPEG"]
    fn real_libmpv_starting_paused_reports_the_pause_before_started() {
        use crate::{PlaybackEngine, PlaybackEvent};
        let folder = tempfile::tempdir().expect("fixture folder");
        let item = short_fixture(folder.path());
        let mut engine = short_engine(true);
        engine
            .execute(PlaybackCommand::Load {
                item: Box::new(item),
                start_position_seconds: None,
            })
            .expect("load fixture");
        let events = collect_events(&mut engine, std::time::Duration::from_millis(1500));
        assert_eq!(
            events,
            vec![PlaybackEvent::Paused(true), PlaybackEvent::Started]
        );
    }

    #[test]
    #[ignore = "requires APRICOT_TEST_MPV and APRICOT_TEST_FFMPEG"]
    fn real_libmpv_does_not_report_the_failure_of_a_replaced_file() {
        use crate::{PlaybackEngine, PlaybackEvent};
        let folder = tempfile::tempdir().expect("fixture folder");
        let item = short_fixture(folder.path());
        let mut missing = item.clone();
        missing.local_path = Some(
            folder
                .path()
                .join("missing.wav")
                .to_string_lossy()
                .into_owned(),
        );
        let mut engine = short_engine(false);
        engine
            .execute(PlaybackCommand::Load {
                item: Box::new(missing),
                start_position_seconds: None,
            })
            .expect("load missing file");
        // The failure of the missing file is queued but not read yet.
        std::thread::sleep(std::time::Duration::from_millis(300));
        engine
            .execute(PlaybackCommand::Load {
                item: Box::new(item),
                start_position_seconds: None,
            })
            .expect("load fixture");
        let events = collect_events(&mut engine, std::time::Duration::from_millis(1500));
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, PlaybackEvent::Failed(_))),
            "{events:?}"
        );
        assert!(events.contains(&PlaybackEvent::Started), "{events:?}");
    }

    /// A generated two-second tone through `apricot-pcm://`: one start, the
    /// item position across a seek (new generation), and one natural end.
    #[test]
    #[ignore = "requires APRICOT_TEST_MPV"]
    #[allow(clippy::too_many_lines)]
    fn real_libmpv_plays_a_pcm_source_across_a_seek_to_its_end() {
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicU64, Ordering},
        };

        use crate::{
            PlaybackEngine, PlaybackEvent,
            pcm_source::{PcmGeneration, PcmSource, PcmSourceEvent, PcmStream, set_pcm_source},
        };

        const LENGTH_MS: u32 = 2000;
        struct Stream {
            id: u64,
            current: Arc<AtomicU64>,
            remaining: Mutex<usize>,
            cancelled: AtomicBool,
        }
        impl PcmStream for Stream {
            fn read(&self, buffer: &mut [u8]) -> usize {
                loop {
                    if self.cancelled.load(Ordering::SeqCst) {
                        return 0;
                    }
                    if self.current.load(Ordering::SeqCst) == self.id {
                        break;
                    }
                    // A replaced generation waits until mpv closes it.
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                let mut remaining = self.remaining.lock().unwrap();
                let take = (*remaining).min(buffer.len()) & !3;
                buffer[..take].fill(0);
                *remaining -= take;
                take
            }
            fn cancel(&self) {
                self.cancelled.store(true, Ordering::SeqCst);
            }
        }
        #[derive(Default)]
        struct Source {
            current: Arc<AtomicU64>,
            pending: Mutex<Option<PcmGeneration>>,
        }
        impl Source {
            fn begin(&self, base_ms: u32) {
                let id = self.current.fetch_add(1, Ordering::SeqCst) + 1;
                *self.pending.lock().unwrap() = Some(PcmGeneration {
                    id,
                    base_ms,
                    duration_ms: None,
                });
            }
        }
        impl PcmSource for Source {
            fn start(
                &self,
                _: &MediaItem,
                position_ms: u32,
                _: bool,
                _: bool,
            ) -> Result<(), String> {
                self.begin(position_ms);
                Ok(())
            }
            fn set_paused(&self, _: bool) {}
            fn seek(&self, position_ms: u32) {
                self.begin(position_ms);
            }
            fn stop(&self) {}
            fn take_generation(&self) -> Option<PcmGeneration> {
                self.pending.lock().unwrap().take()
            }
            fn open(&self, id: u64) -> Option<Arc<dyn PcmStream>> {
                let base = if id == 1 { 0 } else { 1500 };
                let bytes = usize::try_from((LENGTH_MS - base) * 441 / 10 * 4).unwrap();
                Some(Arc::new(Stream {
                    id,
                    current: self.current.clone(),
                    remaining: Mutex::new(bytes),
                    cancelled: AtomicBool::new(false),
                }))
            }
            fn poll_event(&self) -> Option<PcmSourceEvent> {
                None
            }
            fn format(&self) -> (String, Option<f64>) {
                ("Test PCM".to_owned(), None)
            }
        }
        set_pcm_source(Some(Arc::new(Source::default())));
        let item = MediaItem {
            id: MediaId("spotify:track:4u7EnebtmKWzUH433cf5Qv".to_owned()),
            source: MediaSource::Spotify,
            kind: MediaKind::Audio,
            title: "Tone".to_owned(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: Some(2.0),
            metadata: BTreeMap::new(),
        };
        let mut engine = short_engine(false);
        engine
            .execute(PlaybackCommand::Load {
                item: Box::new(item),
                start_position_seconds: None,
            })
            .expect("load PCM item");
        let mut positions = Vec::new();
        let mut events = Vec::new();
        let mut poll = |engine: &mut super::LibMpvEngine, millis: u64| {
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(millis);
            while std::time::Instant::now() < deadline {
                match engine.poll_event().expect("poll") {
                    Some(PlaybackEvent::Position { elapsed, duration }) => {
                        positions.push((elapsed, duration));
                    }
                    Some(PlaybackEvent::MediaInfo(_) | PlaybackEvent::AudioDevices(_)) => {}
                    Some(event) => events.push(event),
                    None => std::thread::sleep(std::time::Duration::from_millis(5)),
                }
            }
        };
        poll(&mut engine, 700);
        engine
            .execute(PlaybackCommand::SeekAbsolute {
                seconds: 1.5,
                exact: true,
            })
            .expect("seek");
        poll(&mut engine, 1500);
        set_pcm_source(None);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, PlaybackEvent::Started))
                .count(),
            1,
            "{events:?}"
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, PlaybackEvent::Ended))
                .count(),
            1,
            "{events:?}"
        );
        assert!(positions.iter().all(|(_, duration)| *duration == Some(2.0)));
        let before_seek = positions
            .iter()
            .filter(|(e, _)| *e > 0.1 && *e < 1.4)
            .count();
        let after_seek = positions.iter().filter(|(e, _)| *e >= 1.5).count();
        assert!(before_seek > 0 && after_seek > 0, "{positions:?}");
    }

    /// Like `LibreSpot` at the end of a track: a generation opened twice by
    /// mpv (probe), its data ends and the source closes it while mpv waits.
    #[test]
    #[ignore = "requires APRICOT_TEST_MPV"]
    #[allow(clippy::too_many_lines)]
    fn real_libmpv_reports_the_end_when_the_source_closes_a_waiting_generation() {
        use std::sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicBool, Ordering},
        };

        use crate::{
            PlaybackEngine, PlaybackEvent,
            pcm_source::{PcmGeneration, PcmSource, PcmSourceEvent, PcmStream, set_pcm_source},
        };

        struct Shared {
            data: Mutex<(usize, bool)>,
            cond: Condvar,
        }
        struct Reader {
            shared: Arc<Shared>,
            cancelled: AtomicBool,
        }
        impl PcmStream for Reader {
            fn read(&self, buffer: &mut [u8]) -> usize {
                let mut guard = self.shared.data.lock().unwrap();
                while guard.0 == 0 && !guard.1 && !self.cancelled.load(Ordering::SeqCst) {
                    guard = self.shared.cond.wait(guard).unwrap();
                }
                if self.cancelled.load(Ordering::SeqCst) {
                    return 0;
                }
                let take = guard.0.min(buffer.len()) & !3;
                buffer[..take].fill(0);
                guard.0 -= take;
                take
            }
            fn cancel(&self) {
                self.cancelled.store(true, Ordering::SeqCst);
                let _guard = self.shared.data.lock();
                self.shared.cond.notify_all();
            }
        }
        struct Source {
            shared: Arc<Shared>,
            pending: Mutex<Option<PcmGeneration>>,
        }
        impl PcmSource for Source {
            fn start(&self, _: &MediaItem, _: u32, _: bool, _: bool) -> Result<(), String> {
                *self.pending.lock().unwrap() = Some(PcmGeneration {
                    id: 1,
                    base_ms: 0,
                    duration_ms: None,
                });
                Ok(())
            }
            fn set_paused(&self, _: bool) {}
            fn seek(&self, _: u32) {}
            fn stop(&self) {}
            fn take_generation(&self) -> Option<PcmGeneration> {
                self.pending.lock().unwrap().take()
            }
            fn open(&self, _: u64) -> Option<Arc<dyn PcmStream>> {
                Some(Arc::new(Reader {
                    shared: self.shared.clone(),
                    cancelled: AtomicBool::new(false),
                }))
            }
            fn poll_event(&self) -> Option<PcmSourceEvent> {
                None
            }
            fn format(&self) -> (String, Option<f64>) {
                ("Test PCM".to_owned(), None)
            }
        }
        // One second of PCM, then the source closes the generation.
        let shared = Arc::new(Shared {
            data: Mutex::new((44_100 * 4, false)),
            cond: Condvar::new(),
        });
        set_pcm_source(Some(Arc::new(Source {
            shared: shared.clone(),
            pending: Mutex::new(None),
        })));
        let item = MediaItem {
            id: MediaId("spotify:track:4u7EnebtmKWzUH433cf5Qv".to_owned()),
            source: MediaSource::Spotify,
            kind: MediaKind::Audio,
            title: "Tone".to_owned(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: Some(1.0),
            metadata: BTreeMap::new(),
        };
        let mut engine = short_engine(false);
        engine
            .execute(PlaybackCommand::Load {
                item: Box::new(item),
                start_position_seconds: None,
            })
            .expect("load PCM item");
        let closer = {
            let shared = shared.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                shared.data.lock().unwrap().1 = true;
                shared.cond.notify_all();
            })
        };
        let events = collect_events(&mut engine, std::time::Duration::from_millis(3500));
        closer.join().unwrap();
        set_pcm_source(None);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, PlaybackEvent::Ended))
                .count(),
            1,
            "{events:?}"
        );
    }
}
