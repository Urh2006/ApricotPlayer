//! Audio bridge spike: LibreSpot player -> custom sink -> bounded PCM ring ->
//! libmpv `stream_cb` (rawaudio) -> Apricot's own mpv filter chain.
//!
//! Questions answered here (P0 item 5):
//! * exact seek/track fence without reading LibreSpot internals: the sink owns
//!   its own `PlayerEventChannel` and drains it with `try_recv` before each
//!   write. LibreSpot sends `Seeked`/`Playing` on the player thread before the
//!   first packet of the new position, so the first packet written after the
//!   event is the first new one;
//! * content clock = generation base + mpv `time-pos` (media time, so speed and
//!   pitch filters do not skew it);
//! * seek latency, speed/pitch through the existing audio_chain filter strings,
//!   buffered duration and backpressure.
//!
//! Output uses `ao=null` so the spike never makes sound on the user's machine.

use std::collections::{HashMap, VecDeque};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use libloading::Library;
use librespot_core::{session::Session, spotify_uri::SpotifyUri};
use librespot_playback::{
    audio_backend::{Sink, SinkResult},
    config::PlayerConfig,
    convert::Converter,
    decoder::AudioPacket,
    mixer::NoOpVolume,
    player::{Player, PlayerEvent, PlayerEventChannel},
};

const RATE: u64 = 44_100;
const BYTES_PER_FRAME: u64 = 4; // s16le stereo
/// Ring capacity: 200 ms of PCM.
const RING_BYTES: usize = (RATE * BYTES_PER_FRAME / 5) as usize;

// ---------- PCM generations ----------

struct Generation {
    data: Mutex<(VecDeque<u8>, bool)>, // (bytes, closed)
    cond: Condvar,
}

impl Generation {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            data: Mutex::new((VecDeque::with_capacity(RING_BYTES), false)),
            cond: Condvar::new(),
        })
    }

    /// Blocks while the ring is full (backpressure towards LibreSpot).
    fn push(&self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let mut guard = self.data.lock().unwrap();
            while guard.0.len() >= RING_BYTES && !guard.1 {
                guard = self.cond.wait(guard).unwrap();
            }
            if guard.1 {
                return;
            }
            let room = RING_BYTES - guard.0.len();
            let take = room.min(bytes.len());
            guard.0.extend(&bytes[..take]);
            bytes = &bytes[take..];
            self.cond.notify_all();
        }
    }

    fn close(&self) {
        self.data.lock().unwrap().1 = true;
        self.cond.notify_all();
    }

    fn buffered(&self) -> usize {
        self.data.lock().unwrap().0.len()
    }
}

#[derive(Default)]
struct Shared {
    generations: Mutex<HashMap<u64, Arc<Generation>>>,
    current: Mutex<Option<Arc<Generation>>>,
    next_id: AtomicU64,
    /// (generation id, base ms) to be loaded into mpv.
    pending_load: Mutex<Option<(u64, u32)>>,
    stale_packets_dropped: AtomicU64,
    fences: AtomicU64,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

fn shared() -> &'static Arc<Shared> {
    SHARED.get_or_init(Default::default)
}

impl Shared {
    fn start_generation(&self, base_ms: u32) -> Arc<Generation> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = Generation::new();
        if let Some(old) = self.current.lock().unwrap().replace(generation.clone()) {
            old.close();
        }
        self.generations
            .lock()
            .unwrap()
            .insert(id, generation.clone());
        *self.pending_load.lock().unwrap() = Some((id, base_ms));
        self.fences.fetch_add(1, Ordering::SeqCst);
        generation
    }
}

// ---------- LibreSpot sink ----------

struct BridgeSink {
    events: Arc<Mutex<Option<PlayerEventChannel>>>,
    started: bool,
}

impl BridgeSink {
    /// Drains LibreSpot events that were emitted before the packet now being
    /// written. A seek or a new playing position opens a new generation.
    fn fence(&mut self) {
        let mut guard = self.events.lock().unwrap();
        let Some(events) = guard.as_mut() else { return };
        while let Ok(event) = events.try_recv() {
            match event {
                PlayerEvent::Seeked { position_ms, .. } => {
                    shared().start_generation(position_ms);
                }
                PlayerEvent::Playing { position_ms, .. } if !self.started => {
                    self.started = true;
                    shared().start_generation(position_ms);
                }
                _ => {}
            }
        }
    }
}

impl Sink for BridgeSink {
    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        self.fence();
        let AudioPacket::Samples(samples) = packet else {
            return Ok(());
        };
        let Some(generation) = shared().current.lock().unwrap().clone() else {
            shared()
                .stale_packets_dropped
                .fetch_add(1, Ordering::SeqCst);
            return Ok(());
        };
        let pcm = converter.f64_to_s16(&samples);
        let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        generation.push(&bytes);
        Ok(())
    }
}

// ---------- libmpv FFI (only what the spike needs) ----------

type Handle = *mut c_void;

#[repr(C)]
struct StreamCbInfo {
    cookie: *mut c_void,
    read_fn: Option<unsafe extern "C" fn(*mut c_void, *mut c_char, u64) -> i64>,
    seek_fn: Option<unsafe extern "C" fn(*mut c_void, i64) -> i64>,
    size_fn: Option<unsafe extern "C" fn(*mut c_void) -> i64>,
    close_fn: Option<unsafe extern "C" fn(*mut c_void)>,
    cancel_fn: Option<unsafe extern "C" fn(*mut c_void)>,
}

type OpenFn = unsafe extern "C" fn(*mut c_void, *mut c_char, *mut StreamCbInfo) -> c_int;

struct Mpv {
    _lib: Library,
    handle: Handle,
    command: unsafe extern "C" fn(Handle, *mut *const c_char) -> c_int,
    get_property: unsafe extern "C" fn(Handle, *const c_char, c_int, *mut c_void) -> c_int,
    set_option_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
}

unsafe extern "C" fn stream_read(cookie: *mut c_void, buf: *mut c_char, nbytes: u64) -> i64 {
    let generation = unsafe { &*(cookie as *const Generation) };
    let mut guard = generation.data.lock().unwrap();
    while guard.0.is_empty() && !guard.1 {
        guard = generation.cond.wait(guard).unwrap();
    }
    if guard.0.is_empty() {
        return 0; // closed: EOF of this generation
    }
    let take = (nbytes as usize).min(guard.0.len());
    let out = unsafe { std::slice::from_raw_parts_mut(buf as *mut u8, take) };
    for (slot, byte) in out.iter_mut().zip(guard.0.drain(..take)) {
        *slot = byte;
    }
    generation.cond.notify_all();
    take as i64
}

unsafe extern "C" fn stream_close(cookie: *mut c_void) {
    let generation = unsafe { Arc::from_raw(cookie as *const Generation) };
    generation.close();
}

unsafe extern "C" fn stream_cancel(cookie: *mut c_void) {
    let generation = unsafe { &*(cookie as *const Generation) };
    generation.close();
}

unsafe extern "C" fn stream_open(
    _user: *mut c_void,
    uri: *mut c_char,
    info: *mut StreamCbInfo,
) -> c_int {
    let uri = unsafe { CStr::from_ptr(uri) }.to_string_lossy();
    let id: u64 = uri
        .rsplit('/')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let Some(generation) = shared().generations.lock().unwrap().remove(&id) else {
        return -13; // MPV_ERROR_LOADING_FAILED
    };
    unsafe {
        (*info).cookie = Arc::into_raw(generation) as *mut c_void;
        (*info).read_fn = Some(stream_read);
        (*info).seek_fn = None;
        (*info).size_fn = None;
        (*info).close_fn = Some(stream_close);
        (*info).cancel_fn = Some(stream_cancel);
    }
    0
}

impl Mpv {
    fn load(dll: &str) -> Result<Self> {
        unsafe {
            let lib = Library::new(dll)?;
            let create: unsafe extern "C" fn() -> Handle = *lib.get(b"mpv_create\0")?;
            let initialize: unsafe extern "C" fn(Handle) -> c_int =
                *lib.get(b"mpv_initialize\0")?;
            let set_option_string = *lib.get(b"mpv_set_option_string\0")?;
            let command = *lib.get(b"mpv_command\0")?;
            let get_property = *lib.get(b"mpv_get_property\0")?;
            let add_ro: unsafe extern "C" fn(Handle, *const c_char, *mut c_void, OpenFn) -> c_int =
                *lib.get(b"mpv_stream_cb_add_ro\0")?;
            let handle = create();
            let mpv = Self {
                _lib: lib,
                handle,
                command,
                get_property,
                set_option_string,
            };
            for (key, value) in [
                ("vid", "no"),
                ("ao", "null"),
                ("cache", "no"),
                ("demuxer-readahead-secs", "0.1"),
                ("audio-buffer", "0.1"),
                ("keep-open", "yes"),
                ("demuxer-rawaudio-format", "s16le"),
                ("demuxer-rawaudio-rate", "44100"),
                ("demuxer-rawaudio-channels", "2"),
                ("demuxer", "rawaudio"),
            ] {
                mpv.set_option(key, value)?;
            }
            if initialize(handle) < 0 {
                return Err(anyhow!("mpv_initialize failed"));
            }
            let proto = CString::new("apricotspotify")?;
            if add_ro(handle, proto.as_ptr(), std::ptr::null_mut(), stream_open) < 0 {
                return Err(anyhow!("mpv_stream_cb_add_ro failed"));
            }
            Ok(mpv)
        }
    }

    fn set_option(&self, key: &str, value: &str) -> Result<()> {
        let (k, v) = (CString::new(key)?, CString::new(value)?);
        let status = unsafe { (self.set_option_string)(self.handle, k.as_ptr(), v.as_ptr()) };
        if status < 0 {
            Err(anyhow!("option {key}={value}: {status}"))
        } else {
            Ok(())
        }
    }

    fn cmd(&self, args: &[&str]) -> Result<()> {
        let owned: Vec<CString> = args.iter().map(|a| CString::new(*a).unwrap()).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        let status = unsafe { (self.command)(self.handle, ptrs.as_mut_ptr()) };
        if status < 0 {
            Err(anyhow!("mpv command {args:?}: {status}"))
        } else {
            Ok(())
        }
    }

    fn time_pos(&self) -> Option<f64> {
        let name = CString::new("time-pos").ok()?;
        let mut value = 0f64;
        let status = unsafe {
            (self.get_property)(
                self.handle,
                name.as_ptr(),
                5,
                &mut value as *mut f64 as *mut c_void,
            )
        };
        (status >= 0).then_some(value)
    }
}

// ---------- scenario ----------

pub async fn run(session: Session, uri: &str, dll: &str) -> Result<()> {
    let mpv = Mpv::load(dll)?;
    let track = SpotifyUri::from_uri(uri).map_err(|e| anyhow!("uri: {e}"))?;
    let events_for_sink: Arc<Mutex<Option<PlayerEventChannel>>> = Arc::default();
    let sink_events = events_for_sink.clone();
    let config = PlayerConfig {
        position_update_interval: Some(Duration::from_millis(500)),
        ..Default::default()
    };
    let player = Player::new(config, session, Box::new(NoOpVolume), move || {
        Box::new(BridgeSink {
            events: sink_events,
            started: false,
        }) as Box<dyn Sink>
    });
    // Registered before the first command, so the sink sees every event.
    *events_for_sink.lock().unwrap() = Some(player.get_player_event_channel());
    let mut events = player.get_player_event_channel();

    let started = Instant::now();
    let base = Arc::new(Mutex::new((0u64, 0u32))); // (generation, base_ms)
    let reported = Arc::new(Mutex::new(0u32));
    let stop = Arc::new(AtomicBool::new(false));
    {
        let reported = reported.clone();
        let stop = stop.clone();
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                if let PlayerEvent::PositionChanged { position_ms, .. }
                | PlayerEvent::Seeked { position_ms, .. } = event
                {
                    *reported.lock().unwrap() = position_ms;
                }
                if stop.load(Ordering::SeqCst) {
                    break;
                }
            }
        });
    }
    player.load(track, true, 0);

    let mut first_audio: Option<u128> = None;
    let mut seek_requested: Option<Instant> = None;
    let mut last_print = Instant::now();
    let mut step = 0;
    while started.elapsed() < Duration::from_secs(40) {
        if let Some((id, base_ms)) = shared().pending_load.lock().unwrap().take() {
            mpv.cmd(&["loadfile", &format!("apricotspotify://{id}")])?;
            *base.lock().unwrap() = (id, base_ms);
            println!(
                "{:>6} ms  mpv loadfile generation {id} base {base_ms} ms",
                started.elapsed().as_millis()
            );
        }
        let elapsed = started.elapsed();
        let tp = mpv.time_pos();
        if first_audio.is_none() && tp.is_some_and(|t| t > 0.0) {
            first_audio = Some(elapsed.as_millis());
            println!(
                "{:>6} ms  first audio progressing in mpv",
                elapsed.as_millis()
            );
        }
        if let (Some(requested), Some(t)) = (seek_requested, tp)
            && base.lock().unwrap().1 >= 59_000
            && t > 0.0
        {
            println!(
                "{:>6} ms  seek audible after {} ms",
                elapsed.as_millis(),
                requested.elapsed().as_millis()
            );
            seek_requested = None;
        }
        match step {
            0 if elapsed >= Duration::from_secs(8) => {
                step = 1;
                println!(
                    "{:>6} ms  >>> set speed 2.0 (scaletempo2 as audio_chain Mpv mode)",
                    elapsed.as_millis()
                );
                mpv.cmd(&[
                    "af",
                    "set",
                    "@apricot_speed:scaletempo2=search-interval=50:window-size=20:max-speed=8.0",
                ])?;
                mpv.cmd(&["set", "speed", "2.0"])?;
            }
            1 if elapsed >= Duration::from_secs(16) => {
                step = 2;
                println!(
                    "{:>6} ms  >>> speed 1.0 + rubberband pitch 1.12 + equalizer",
                    elapsed.as_millis()
                );
                mpv.cmd(&["set", "speed", "1.0"])?;
                mpv.cmd(&["af", "set", "@apricot_pitch:rubberband=transients=smooth:formant=preserved:pitch=quality:engine=finer:pitch-scale=1.1200,@apricot_eq:lavfi=[equalizer=f=1000:t=o:w=1:g=6]"])?;
            }
            2 if elapsed >= Duration::from_secs(22) => {
                step = 3;
                println!(
                    "{:>6} ms  >>> LibreSpot seek to 60000 ms",
                    elapsed.as_millis()
                );
                seek_requested = Some(Instant::now());
                player.seek(60_000);
            }
            3 if elapsed >= Duration::from_secs(30) => {
                step = 4;
                println!("{:>6} ms  >>> pause 3 s", elapsed.as_millis());
                player.pause();
                mpv.cmd(&["set", "pause", "yes"])?;
            }
            4 if elapsed >= Duration::from_secs(33) => {
                step = 5;
                println!("{:>6} ms  >>> resume", elapsed.as_millis());
                mpv.cmd(&["set", "pause", "no"])?;
                player.play();
            }
            _ => {}
        }
        if last_print.elapsed() >= Duration::from_secs(1) {
            last_print = Instant::now();
            let (id, base_ms) = *base.lock().unwrap();
            let buffered = shared()
                .current
                .lock()
                .unwrap()
                .as_ref()
                .map(|g| g.buffered())
                .unwrap_or(0);
            let content = tp.map(|t| base_ms as f64 + t * 1000.0);
            println!(
                "{:>6} ms  gen {id} time-pos {:>7.3}  content {:>8.0} ms  librespot {:>6} ms  ring {:>3} ms",
                elapsed.as_millis(),
                tp.unwrap_or(-1.0),
                content.unwrap_or(-1.0),
                *reported.lock().unwrap(),
                buffered as u64 * 1000 / (RATE * BYTES_PER_FRAME),
            );
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    stop.store(true, Ordering::SeqCst);
    player.stop();
    println!(
        "first_audio_ms={first_audio:?} fences={} stale_dropped={}",
        shared().fences.load(Ordering::SeqCst),
        shared().stale_packets_dropped.load(Ordering::SeqCst)
    );
    let _ = mpv.cmd(&["quit"]);
    Ok(())
}
