//! In-process libmpv qualification before selecting the Rust playback backend.

#![cfg_attr(windows, allow(unsafe_code, unsafe_op_in_unsafe_fn))]

#[cfg(windows)]
mod windows_spike {
    use std::{
        collections::BTreeMap,
        ffi::{CStr, CString, c_char, c_double, c_int, c_void},
        fs,
        io::{self, Read, Write},
        path::{Path, PathBuf},
        process::{Command as ProcessCommand, Stdio},
        ptr,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_playback::{
        LibMpvEngine, MpvLaunchOptions, MpvVideoMode, PlaybackCommand, PlaybackEngine,
        PlaybackEvent,
    };
    use libloading::Library;
    use windows::{
        Win32::{
            Foundation::HWND,
            UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP},
        },
        core::w,
    };

    const MPV_FORMAT_DOUBLE: c_int = 5;
    const MPV_EVENT_SHUTDOWN: c_int = 1;
    const MPV_EVENT_END_FILE: c_int = 7;
    const MPV_EVENT_FILE_LOADED: c_int = 8;
    const MPV_EVENT_VIDEO_RECONFIG: c_int = 17;

    type MpvHandle = c_void;
    type Create = unsafe extern "C" fn() -> *mut MpvHandle;
    type Initialize = unsafe extern "C" fn(*mut MpvHandle) -> c_int;
    type TerminateDestroy = unsafe extern "C" fn(*mut MpvHandle);
    type SetOptionString =
        unsafe extern "C" fn(*mut MpvHandle, *const c_char, *const c_char) -> c_int;
    type Command = unsafe extern "C" fn(*mut MpvHandle, *const *const c_char) -> c_int;
    type GetProperty =
        unsafe extern "C" fn(*mut MpvHandle, *const c_char, c_int, *mut c_void) -> c_int;
    type WaitEvent = unsafe extern "C" fn(*mut MpvHandle, c_double) -> *const MpvEvent;
    type ErrorString = unsafe extern "C" fn(c_int) -> *const c_char;

    #[repr(C)]
    struct MpvEvent {
        event_id: c_int,
        error: c_int,
        reply_userdata: u64,
        data: *mut c_void,
    }

    struct MpvApi {
        _library: Library,
        create: Create,
        initialize: Initialize,
        terminate_destroy: TerminateDestroy,
        set_option_string: SetOptionString,
        command: Command,
        get_property: GetProperty,
        wait_event: WaitEvent,
        error_string: ErrorString,
    }

    impl MpvApi {
        unsafe fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
            let library = Library::new(path)?;
            let create = *library.get::<Create>(b"mpv_create\0")?;
            let initialize = *library.get::<Initialize>(b"mpv_initialize\0")?;
            let terminate_destroy = *library.get::<TerminateDestroy>(b"mpv_terminate_destroy\0")?;
            let set_option_string = *library.get::<SetOptionString>(b"mpv_set_option_string\0")?;
            let command = *library.get::<Command>(b"mpv_command\0")?;
            let get_property = *library.get::<GetProperty>(b"mpv_get_property\0")?;
            let wait_event = *library.get::<WaitEvent>(b"mpv_wait_event\0")?;
            let error_string = *library.get::<ErrorString>(b"mpv_error_string\0")?;
            Ok(Self {
                _library: library,
                create,
                initialize,
                terminate_destroy,
                set_option_string,
                command,
                get_property,
                wait_event,
                error_string,
            })
        }

        unsafe fn check(&self, status: c_int, operation: &str) -> Result<(), String> {
            if status >= 0 {
                return Ok(());
            }
            let message = (self.error_string)(status);
            let detail = if message.is_null() {
                format!("error {status}")
            } else {
                CStr::from_ptr(message).to_string_lossy().into_owned()
            };
            Err(format!("{operation}: {detail}"))
        }

        unsafe fn set_option(
            &self,
            handle: *mut MpvHandle,
            name: &str,
            value: &str,
        ) -> Result<(), Box<dyn std::error::Error>> {
            let name = CString::new(name)?;
            let value = CString::new(value)?;
            self.check(
                (self.set_option_string)(handle, name.as_ptr(), value.as_ptr()),
                "set option",
            )?;
            Ok(())
        }

        unsafe fn run_command(
            &self,
            handle: *mut MpvHandle,
            arguments: &[&str],
        ) -> Result<(), Box<dyn std::error::Error>> {
            let strings = arguments
                .iter()
                .map(|argument| CString::new(*argument))
                .collect::<Result<Vec<_>, _>>()?;
            let mut pointers = strings
                .iter()
                .map(|argument| argument.as_ptr())
                .collect::<Vec<_>>();
            pointers.push(ptr::null());
            self.check((self.command)(handle, pointers.as_ptr()), "mpv command")?;
            Ok(())
        }

        unsafe fn get_double(
            &self,
            handle: *mut MpvHandle,
            property: &str,
        ) -> Result<f64, Box<dyn std::error::Error>> {
            let property = CString::new(property)?;
            let mut value = 0.0_f64;
            self.check(
                (self.get_property)(
                    handle,
                    property.as_ptr(),
                    MPV_FORMAT_DOUBLE,
                    (&raw mut value).cast(),
                ),
                "get property",
            )?;
            Ok(value)
        }
    }

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("..");
        let dll = std::env::var_os("APRICOT_LIBMPV_DLL").map_or_else(
            || {
                repository
                    .join("rust")
                    .join(".cargo-local")
                    .join("libmpv")
                    .join("libmpv-2.dll")
            },
            PathBuf::from,
        );
        if !dll.is_file() {
            return Err(format!("libmpv was not found at {}", dll.display()).into());
        }
        let fixture = temporary_wav_path();
        write_silent_stereo_wav(&fixture, 44_100, 2)?;
        let video_fixture = temporary_video_path();
        write_video_fixture(&repository, &video_fixture)?;
        let result = unsafe { qualify(&dll, &fixture, &video_fixture) };
        if result.is_ok() {
            qualify_production_engine(&dll, &fixture, &video_fixture)?;
        }
        let _ = fs::remove_file(fixture);
        let _ = fs::remove_file(video_fixture);
        result
    }

    fn qualify_production_engine(
        dll: &Path,
        audio_fixture: &Path,
        video_fixture: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let window = unsafe { HiddenVideoWindow::create()? };
        let mut options = MpvLaunchOptions::new(dll);
        options.library = Some(dll.to_path_buf());
        options.video_mode = MpvVideoMode::Embedded(window.handle.0 as isize);
        options.audio_driver = Some("null".to_owned());
        options.initial_volume = 37.0;
        let mut engine = LibMpvEngine::load(&options)?;
        let first_started = Instant::now();
        engine.execute(PlaybackCommand::Load(Box::new(media_item(
            "audio",
            MediaKind::Audio,
            audio_fixture,
        ))))?;
        wait_for_engine_event(&mut engine, Duration::from_secs(3), |event| {
            matches!(event, PlaybackEvent::Started)
        })?;
        engine.execute(PlaybackCommand::SetVolume(42.0))?;
        engine.execute(PlaybackCommand::SetSpeed(1.25))?;
        engine.execute(PlaybackCommand::SetPitch(1.1))?;
        engine.execute(PlaybackCommand::SeekAbsolute {
            seconds: 0.5,
            exact: true,
        })?;
        println!(
            "LIBMPV_ENGINE_FIRST_LOAD_MS={:.3}",
            first_started.elapsed().as_secs_f64() * 1_000.0
        );

        engine.execute(PlaybackCommand::Load(Box::new(media_item(
            "video",
            MediaKind::Video,
            video_fixture,
        ))))?;
        wait_for_engine_event(&mut engine, Duration::from_secs(3), |event| {
            matches!(event, PlaybackEvent::Started)
        })?;
        println!("LIBMPV_PRODUCTION_ENGINE=PASS");
        Ok(())
    }

    fn media_item(id: &str, kind: MediaKind, path: &Path) -> MediaItem {
        MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Local,
            kind,
            title: id.to_owned(),
            url: None,
            local_path: Some(path.to_string_lossy().into_owned()),
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    fn wait_for_engine_event(
        engine: &mut LibMpvEngine,
        timeout: Duration,
        predicate: impl Fn(&PlaybackEvent) -> bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(event) = engine.poll_event()? {
                if let PlaybackEvent::Failed(error) = &event {
                    return Err(error.clone().into());
                }
                if predicate(&event) {
                    return Ok(());
                }
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        Err("expected libmpv engine event was not received".into())
    }

    unsafe fn qualify(
        dll: &Path,
        fixture: &Path,
        video_fixture: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let library_started = Instant::now();
        let api = MpvApi::load(dll)?;
        let handle = (api.create)();
        if handle.is_null() {
            return Err("mpv_create returned null".into());
        }
        let result = qualify_handle(&api, handle, fixture, video_fixture, library_started);
        (api.terminate_destroy)(handle);
        result
    }

    unsafe fn qualify_handle(
        api: &MpvApi,
        handle: *mut MpvHandle,
        fixture: &Path,
        video_fixture: &Path,
        library_started: Instant,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let video_window = HiddenVideoWindow::create()?;
        let window_id = video_window.handle.0 as usize;
        let window_id = window_id.to_string();
        for (name, value) in [
            ("config", "no"),
            ("terminal", "no"),
            ("wid", window_id.as_str()),
            ("ao", "null"),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("volume-max", "300"),
            ("volume", "37"),
            ("speed", "1.0"),
            ("pitch", "1.0"),
            ("gapless-audio", "yes"),
            ("replaygain", "no"),
        ] {
            api.set_option(handle, name, value)?;
        }
        api.check((api.initialize)(handle), "mpv_initialize")?;
        println!(
            "LIBMPV_INITIALIZE_MS={:.3}",
            library_started.elapsed().as_secs_f64() * 1_000.0
        );

        let fixture = fixture.to_string_lossy();
        let first_started = Instant::now();
        api.run_command(handle, &["loadfile", &fixture, "replace"])?;
        wait_for_event(api, handle, MPV_EVENT_FILE_LOADED, Duration::from_secs(3))?;
        println!(
            "LIBMPV_FIRST_LOAD_MS={:.3}",
            first_started.elapsed().as_secs_f64() * 1_000.0
        );
        api.run_command(handle, &["set", "volume", "42"])?;
        api.run_command(handle, &["set", "speed", "1.25"])?;
        api.run_command(handle, &["set", "pitch", "1.1"])?;
        api.run_command(handle, &["seek", "0.5", "absolute+exact"])?;
        api.run_command(
            handle,
            &[
                "af",
                "add",
                "@apricot_eq:lavfi=[volume=-3.0dB,equalizer=f=31:t=q:w=1.2:g=3.0,equalizer=f=1000:t=q:w=1.8:g=-2.0,alimiter=limit=0.95:attack=5:release=50]",
            ],
        )?;
        let volume = api.get_double(handle, "volume")?;
        if (volume - 42.0).abs() > 0.01 {
            return Err(format!("libmpv volume mismatch: {volume}").into());
        }

        let second_started = Instant::now();
        api.run_command(handle, &["loadfile", &fixture, "replace"])?;
        wait_for_event(api, handle, MPV_EVENT_FILE_LOADED, Duration::from_secs(3))?;
        println!(
            "LIBMPV_SECOND_LOAD_MS={:.3}",
            second_started.elapsed().as_secs_f64() * 1_000.0
        );
        println!("LIBMPV_SINGLE_HANDLE=PASS");

        let video_started = Instant::now();
        let video_fixture = video_fixture.to_string_lossy();
        api.run_command(handle, &["loadfile", &video_fixture, "replace"])?;
        wait_for_event(api, handle, MPV_EVENT_FILE_LOADED, Duration::from_secs(3))?;
        wait_for_event(
            api,
            handle,
            MPV_EVENT_VIDEO_RECONFIG,
            Duration::from_secs(3),
        )?;
        let width = api.get_double(handle, "width")?;
        let height = api.get_double(handle, "height")?;
        if width < 63.0 || height < 63.0 {
            return Err(format!("embedded video dimensions were {width}x{height}").into());
        }
        println!(
            "LIBMPV_EMBEDDED_VIDEO_MS={:.3}",
            video_started.elapsed().as_secs_f64() * 1_000.0
        );
        println!("LIBMPV_EMBEDDED_HWND=PASS");
        Ok(())
    }

    unsafe fn wait_for_event(
        api: &MpvApi,
        handle: *mut MpvHandle,
        expected: c_int,
        timeout: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let event = (api.wait_event)(handle, 0.05);
            if event.is_null() {
                return Err("mpv_wait_event returned null".into());
            }
            match (*event).event_id {
                value if value == expected => return Ok(()),
                MPV_EVENT_END_FILE if (*event).error < 0 => {
                    return Err(
                        format!("libmpv ended the file with error {}", (*event).error).into(),
                    );
                }
                MPV_EVENT_SHUTDOWN => return Err("libmpv shut down during qualification".into()),
                _ => {}
            }
        }
        Err("timed out waiting for libmpv file-loaded event".into())
    }

    fn temporary_wav_path() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!(
            "apricot-rust-libmpv-{}-{timestamp:x}.wav",
            std::process::id()
        ))
    }

    fn temporary_video_path() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!(
            "apricot-rust-libmpv-video-{}-{timestamp:x}.mp4",
            std::process::id()
        ))
    }

    fn write_video_fixture(
        repository: &Path,
        destination: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ffmpeg = repository.join("vendor").join("ffmpeg").join("ffmpeg.exe");
        let output = ProcessCommand::new(ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=64x64:d=1",
                "-an",
                "-c:v",
                "mpeg4",
                "-y",
            ])
            .arg(destination)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "FFmpeg fixture creation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into())
        }
    }

    struct HiddenVideoWindow {
        handle: HWND,
    }

    impl HiddenVideoWindow {
        unsafe fn create() -> windows::core::Result<Self> {
            let handle = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Apricot libmpv qualification"),
                WS_POPUP,
                0,
                0,
                64,
                64,
                None,
                None,
                None,
                None,
            )?;
            Ok(Self { handle })
        }
    }

    impl Drop for HiddenVideoWindow {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.handle);
            }
        }
    }

    fn write_silent_stereo_wav(path: &Path, sample_rate: u32, seconds: u32) -> io::Result<()> {
        let channels = 2_u16;
        let bits_per_sample = 16_u16;
        let bytes_per_sample = u32::from(bits_per_sample / 8);
        let data_size = sample_rate * seconds * u32::from(channels) * bytes_per_sample;
        let mut file = fs::File::create(path)?;
        file.write_all(b"RIFF")?;
        file.write_all(&(36 + data_size).to_le_bytes())?;
        file.write_all(b"WAVEfmt ")?;
        file.write_all(&16_u32.to_le_bytes())?;
        file.write_all(&1_u16.to_le_bytes())?;
        file.write_all(&channels.to_le_bytes())?;
        file.write_all(&sample_rate.to_le_bytes())?;
        let byte_rate = sample_rate * u32::from(channels) * bytes_per_sample;
        file.write_all(&byte_rate.to_le_bytes())?;
        let block_align = channels * (bits_per_sample / 8);
        file.write_all(&block_align.to_le_bytes())?;
        file.write_all(&bits_per_sample.to_le_bytes())?;
        file.write_all(b"data")?;
        file.write_all(&data_size.to_le_bytes())?;
        io::copy(&mut io::repeat(0).take(u64::from(data_size)), &mut file)?;
        file.flush()
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows_spike::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The libmpv spike currently qualifies the Windows backend only.");
}
