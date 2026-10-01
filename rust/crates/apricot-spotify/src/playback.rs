//! Local Spotify playback (`docs/SPOTIFY_PLAN.md` 6, 8): `LibreSpot` decodes and
//! owns the transport through its Connect device (Spirc); PCM goes to
//! Apricot's libmpv through [`apricot_playback::pcm_source`].
//!
//! Stream generations: every seek and every new track opens a new
//! generation. The sink owns its own `PlayerEventChannel` and drains it
//! before each write; `LibreSpot` sends `Seeked` and `Playing` on the player
//! thread before the first packet of the new position, so the first packet
//! written after such an event belongs to the new generation (P0 evidence 8).
//! A replaced generation is never ended by the source: mpv closes it when it
//! loads the next one, so it cannot report a false end of the item.

use std::{
    collections::VecDeque,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use apricot_core::{MediaItem, SpotifyEntityKind, SpotifyRef};
use apricot_playback::pcm_source::{PcmGeneration, PcmSource, PcmSourceEvent, PcmStream};
use librespot_connect::{
    LoadContextOptions, LoadRequest, LoadRequestOptions, Options as ConnectOptions, PlayingTrack,
    Spirc,
};
use librespot_metadata::audio::{AudioItem, UniqueFields};
use librespot_playback::{
    audio_backend::{Sink, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
    mixer::{Mixer, MixerConfig, NoOpVolume, VolumeGetter},
    player::{PlayerEvent, PlayerEventChannel},
};
use librespot_protocol::player::PlayerState;

/// 200 ms of s16le stereo 44.1 kHz: the bound between `LibreSpot` and mpv.
const RING_BYTES: usize = 44_100 * 4 / 5;

/// What the UI learns about the track the Connect device plays.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyTrack {
    pub uri: String,
    pub title: String,
    pub artists: String,
    pub album: String,
    pub duration_ms: u32,
}

impl SpotifyTrack {
    pub fn from_audio_item(item: &AudioItem) -> Self {
        let (artists, album) = match &item.unique_fields {
            UniqueFields::Track { artists, album, .. } => (
                artists
                    .iter()
                    .map(|artist| artist.name.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
                album.clone(),
            ),
            UniqueFields::Episode { show_name, .. } => (show_name.clone(), String::new()),
            UniqueFields::Local { artists, album, .. } => (
                artists.clone().unwrap_or_default(),
                album.clone().unwrap_or_default(),
            ),
        };
        Self {
            uri: item.uri.clone(),
            title: item.name.clone(),
            artists,
            album,
            duration_ms: item.duration_ms,
        }
    }

    /// The Apricot media item for this track (durable link, no stream URL).
    pub fn media_item(&self) -> MediaItem {
        let reference = SpotifyRef::parse(&self.uri);
        let kind = if reference
            .as_ref()
            .is_some_and(|reference| reference.kind == SpotifyEntityKind::Episode)
        {
            "spotify_episode"
        } else {
            "spotify_track"
        };
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("kind".to_owned(), serde_json::Value::from(kind));
        metadata.insert("source".to_owned(), serde_json::Value::from("spotify"));
        if !self.album.is_empty() {
            metadata.insert(
                "album".to_owned(),
                serde_json::Value::from(self.album.clone()),
            );
        }
        MediaItem {
            id: apricot_core::MediaId(self.uri.clone()),
            source: apricot_core::MediaSource::Spotify,
            kind: apricot_core::MediaKind::Audio,
            title: self.title.clone(),
            url: reference
                .and_then(|reference| reference.to_url())
                .and_then(|url| url::Url::parse(&url).ok()),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: self.artists.clone(),
            duration_seconds: (self.duration_ms > 0).then(|| f64::from(self.duration_ms) / 1000.0),
            metadata,
        }
    }
}

/// 0 to 100 percent as a Connect volume (0 to 65535).
pub fn volume_from_percent(percent: f64) -> u16 {
    let clamped = percent.clamp(0.0, 100.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let volume = (clamped / 100.0 * f64::from(u16::MAX)).round() as u16;
    volume
}

/// A Connect volume as 0 to 100 percent, whole percent.
pub fn percent_from_volume(volume: u16) -> f64 {
    (f64::from(volume) * 100.0 / f64::from(u16::MAX)).round()
}

/// Spotify repeat modes, cycled with R in the player.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepeatMode {
    Off,
    Context,
    Track,
}

/// Events for the Spotify controller of the UI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlaybackNotice {
    /// A new track started on this device; `requested` when Apricot asked
    /// for it, otherwise another device (Connect) did.
    Playing {
        track: SpotifyTrack,
        position_ms: u32,
        requested: bool,
    },
    /// The track cannot be played (market, rights); it is skipped by Spotify.
    Unavailable { uri: String },
    /// The confirmed queue, track, shuffle or repeat changed (here or on
    /// another device); an open queue view reloads.
    QueueChanged,
    /// The Connect devices of the account changed; an open device list
    /// reloads.
    DevicesChanged,
    /// Another device (the phone) set the volume of this device, 0 to 65535.
    Volume(u16),
}

struct Generation {
    /// The play request whose PCM the generation carries; a gapless
    /// transition moves it to the next request.
    request_id: AtomicU64,
    /// Bytes pushed so far: the stream position of a gapless boundary.
    written: AtomicU64,
    data: Mutex<(VecDeque<u8>, bool)>,
    cond: Condvar,
}

/// Bytes of s16le stereo 44.1 kHz per second.
const BYTES_PER_SECOND: u64 = 44_100 * 4;

impl Generation {
    fn new(request_id: u64) -> Arc<Self> {
        Arc::new(Self {
            request_id: AtomicU64::new(request_id),
            written: AtomicU64::new(0),
            data: Mutex::new((VecDeque::with_capacity(RING_BYTES), false)),
            cond: Condvar::new(),
        })
    }

    fn is_closed(&self) -> bool {
        self.data.lock().map_or(true, |guard| guard.1)
    }

    /// Blocks while the ring is full: the backpressure towards `LibreSpot`.
    fn push(&self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            let Ok(mut guard) = self.data.lock() else {
                return;
            };
            while guard.0.len() >= RING_BYTES && !guard.1 {
                let Ok(next) = self.cond.wait(guard) else {
                    return;
                };
                guard = next;
            }
            if guard.1 {
                return;
            }
            let take = (RING_BYTES - guard.0.len()).min(bytes.len());
            guard.0.extend(&bytes[..take]);
            self.written.fetch_add(take as u64, Ordering::SeqCst);
            bytes = &bytes[take..];
            self.cond.notify_all();
        }
    }

    fn close(&self) {
        if let Ok(mut guard) = self.data.lock() {
            guard.1 = true;
        }
        self.cond.notify_all();
    }
}

/// One mpv handle on a generation. mpv may open the same generation more
/// than once (probing); closing a handle never ends the generation itself,
/// only the source does that at the real end of playback.
struct Reader {
    generation: Arc<Generation>,
    cancelled: AtomicBool,
}

impl PcmStream for Reader {
    fn read(&self, buffer: &mut [u8]) -> usize {
        let generation = &self.generation;
        let Ok(mut guard) = generation.data.lock() else {
            return 0;
        };
        while guard.0.is_empty() && !guard.1 && !self.cancelled.load(Ordering::SeqCst) {
            let Ok(next) = generation.cond.wait(guard) else {
                return 0;
            };
            guard = next;
        }
        if self.cancelled.load(Ordering::SeqCst) {
            return 0;
        }
        // Whole frames only.
        let take = guard.0.len().min(buffer.len()) & !3;
        for (slot, byte) in buffer.iter_mut().zip(guard.0.drain(..take)) {
            *slot = byte;
        }
        generation.cond.notify_all();
        take
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        // Take the lock so a waiting read sees the flag after the wake-up.
        let _guard = self.generation.data.lock();
        self.generation.cond.notify_all();
    }
}

type Notify = Arc<dyn Fn() + Send + Sync>;

/// Shared state of the sink, the event listener and the PCM source.
pub struct Shared {
    next_id: AtomicU64,
    generations: Mutex<Vec<(u64, Arc<Generation>)>>,
    current: Mutex<Option<(u64, Arc<Generation>)>>,
    pending: Mutex<Option<PcmGeneration>>,
    events: Mutex<VecDeque<PcmSourceEvent>>,
    notices: Mutex<VecDeque<PlaybackNotice>>,
    /// Apricot started playback; the next new track is its own start.
    requested: AtomicBool,
    track: Mutex<Option<SpotifyTrack>>,
    bitrate_kbps: u32,
    notify: Notify,
}

impl Shared {
    fn open_generation(&self, request_id: u64, base_ms: u32, duration_ms: Option<u32>) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        log::info!("generation {id} opened at {base_ms} ms for request {request_id}");
        let generation = Generation::new(request_id);
        if let Ok(mut all) = self.generations.lock() {
            // Keep only the few generations mpv may still open.
            all.retain(|(known, _)| *known + 4 > id);
            all.push((id, generation.clone()));
        }
        if let Ok(mut current) = self.current.lock() {
            *current = Some((id, generation));
        }
        if let Ok(mut pending) = self.pending.lock() {
            *pending = Some(PcmGeneration {
                id,
                base_ms,
                duration_ms,
            });
        }
    }

    /// Ends every generation: the session goes away (exit, account switch).
    /// A `LibreSpot` player thread waiting for ring space returns at once, so
    /// dropping the player cannot wait forever for a reader that is gone.
    pub fn close_all(&self) {
        if let Ok(all) = self.generations.lock() {
            for (_, generation) in all.iter() {
                generation.close();
            }
        }
        if let Ok(current) = self.current.lock()
            && let Some((_, generation)) = current.as_ref()
        {
            generation.close();
        }
    }

    fn push_event(&self, event: PcmSourceEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push_back(event);
        }
    }

    fn push_notice(&self, notice: PlaybackNotice) {
        if let Ok(mut notices) = self.notices.lock() {
            notices.push_back(notice);
        }
        (self.notify)();
    }

    pub fn take_notice(&self) -> Option<PlaybackNotice> {
        self.notices.lock().ok()?.pop_front()
    }
}

/// `LibreSpot` sink writing into the current generation.
pub struct BridgeSink {
    shared: Arc<Shared>,
    events: Arc<Mutex<Option<PlayerEventChannel>>>,
    request_id: Option<u64>,
    logged: bool,
    /// The decoder finished a track; the next one continues gaplessly.
    ended: bool,
    /// Length of the track `LibreSpot` announced last (`TrackChanged` comes
    /// right before its `Playing`), for the generation or boundary it opens.
    duration_ms: Option<u32>,
}

impl BridgeSink {
    /// Opens a generation for every new track and every seek.
    fn fence(&mut self) {
        let Ok(mut guard) = self.events.lock() else {
            return;
        };
        let Some(events) = guard.as_mut() else { return };
        while let Ok(event) = events.try_recv() {
            match event {
                PlayerEvent::EndOfTrack { .. } => self.ended = true,
                PlayerEvent::TrackChanged { audio_item } => {
                    self.duration_ms = Some(audio_item.duration_ms);
                }
                PlayerEvent::Seeked {
                    play_request_id,
                    position_ms,
                    ..
                } => {
                    self.ended = false;
                    self.request_id = Some(play_request_id);
                    self.shared
                        .open_generation(play_request_id, position_ms, None);
                }
                PlayerEvent::Playing {
                    play_request_id,
                    position_ms,
                    ..
                } if self.request_id != Some(play_request_id) => {
                    self.request_id = Some(play_request_id);
                    let continuing = self
                        .shared
                        .current
                        .lock()
                        .ok()
                        .and_then(|current| current.as_ref().map(|(_, g)| g.clone()))
                        .filter(|generation| self.ended && !generation.is_closed());
                    self.ended = false;
                    if let Some(generation) = continuing {
                        // Gapless: the next track follows in the same stream.
                        generation
                            .request_id
                            .store(play_request_id, Ordering::SeqCst);
                        let at_ms =
                            generation.written.load(Ordering::SeqCst) * 1000 / BYTES_PER_SECOND;
                        log::info!("gapless boundary at {at_ms} ms of the stream");
                        self.shared.push_event(PcmSourceEvent::Boundary {
                            at_ms,
                            base_ms: position_ms,
                            duration_ms: self.duration_ms.take(),
                        });
                    } else {
                        self.shared.open_generation(
                            play_request_id,
                            position_ms,
                            self.duration_ms.take(),
                        );
                    }
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
        let generation = self
            .shared
            .current
            .lock()
            .ok()
            .and_then(|current| current.as_ref().map(|(_, generation)| generation.clone()));
        if let Some(generation) = generation {
            if !self.logged {
                self.logged = true;
                log::info!("first PCM written");
            }
            let pcm = converter.f64_to_s16(&samples);
            let bytes: Vec<u8> = pcm.iter().flat_map(|sample| sample.to_le_bytes()).collect();
            generation.push(&bytes);
        }
        Ok(())
    }
}

/// Connect volume is kept, not applied: Apricot's own volume (mpv) is the
/// only gain stage, so there is no double volume (plan 8).
#[derive(Default)]
pub struct KeptVolumeMixer {
    volume: AtomicU64,
}

impl Mixer for KeptVolumeMixer {
    fn open(_config: MixerConfig) -> Result<Self, librespot_core::Error> {
        Ok(Self::default())
    }

    fn volume(&self) -> u16 {
        u16::try_from(self.volume.load(Ordering::SeqCst)).unwrap_or(u16::MAX)
    }

    fn set_volume(&self, volume: u16) {
        self.volume.store(u64::from(volume), Ordering::SeqCst);
    }

    fn get_soft_volume(&self) -> Box<dyn VolumeGetter + Send> {
        Box::new(NoOpVolume)
    }
}

/// The PCM source Apricot's player uses for Spotify items.
pub struct SpotifyPlayback {
    shared: Arc<Shared>,
    spirc: Mutex<Option<Spirc>>,
    /// Playback needs Premium; known once the session attributes arrived.
    premium: AtomicBool,
}

impl SpotifyPlayback {
    pub fn new(bitrate_kbps: u32, notify: Notify) -> (Arc<Self>, Arc<Shared>) {
        let shared = Arc::new(Shared {
            next_id: AtomicU64::new(0),
            generations: Mutex::default(),
            current: Mutex::default(),
            pending: Mutex::default(),
            events: Mutex::default(),
            notices: Mutex::default(),
            requested: AtomicBool::new(false),
            track: Mutex::default(),
            bitrate_kbps,
            notify,
        });
        (
            Arc::new(Self {
                shared: shared.clone(),
                spirc: Mutex::new(None),
                premium: AtomicBool::new(false),
            }),
            shared,
        )
    }

    pub fn sink(shared: Arc<Shared>, events: Arc<Mutex<Option<PlayerEventChannel>>>) -> BridgeSink {
        BridgeSink {
            shared,
            events,
            request_id: None,
            logged: false,
            ended: false,
            duration_ms: None,
        }
    }

    /// The confirmed Connect state of this device (patched `librespot-connect`).
    pub fn player_state(&self) -> Option<PlayerState> {
        self.spirc
            .lock()
            .ok()?
            .as_ref()
            .map(|spirc| spirc.player_state().borrow().clone())
    }

    pub fn next(&self) {
        self.with_spirc(|spirc| {
            let _ = spirc.next();
        });
    }

    pub fn previous(&self) {
        self.with_spirc(|spirc| {
            let _ = spirc.prev();
        });
    }

    pub fn set_shuffle(&self, shuffle: bool) {
        self.with_spirc(|spirc| {
            let _ = spirc.shuffle(shuffle);
        });
    }

    /// Repeat off, the context, or the current track (plan 5.3).
    pub fn set_repeat(&self, mode: RepeatMode) {
        self.with_spirc(|spirc| {
            let _ = spirc.repeat(mode == RepeatMode::Context);
            let _ = spirc.repeat_track(mode == RepeatMode::Track);
        });
    }

    pub fn add_to_queue(&self, uri: String) {
        self.with_spirc(|spirc| {
            let _ = spirc.add_to_queue(uri);
        });
    }

    /// Applies `edit` to the confirmed queue. Edits name exact occurrences
    /// (UIDs); `false` when the occurrence is gone or nothing changes, and the
    /// view then reloads. The revision guards against a remote change that
    /// arrives between reading the state and applying the edit.
    pub fn edit_queue(&self, edit: &crate::queue::QueueEdit) -> bool {
        let Some(state) = self.player_state() else {
            return false;
        };
        let Some(next_tracks) = crate::queue::apply(&state.next_tracks, edit) else {
            return false;
        };
        let mut sent = false;
        self.with_spirc(|spirc| {
            sent = spirc
                .set_queue(next_tracks, state.queue_revision.clone())
                .is_ok();
        });
        sent
    }

    /// Plays an upcoming track now: a manually added one is moved first and
    /// skipped to, a context track starts its context at that occurrence
    /// (the manual queue stays, plan D16).
    pub fn play_queue_entry(&self, uid: &str) -> bool {
        let Some(state) = self.player_state() else {
            return false;
        };
        let Some(track) = state.next_tracks.iter().find(|track| track.uid == uid) else {
            return false;
        };
        if track.provider == "queue" {
            let edit = crate::queue::QueueEdit::ToFront(uid.to_owned());
            if !self.edit_queue(&edit) {
                return false;
            }
            self.next();
            return true;
        }
        if state.context_uri.is_empty() {
            return false;
        }
        let request = LoadRequest::from_context_uri(
            state.context_uri.clone(),
            LoadRequestOptions {
                start_playing: true,
                seek_to: 0,
                context_options: Some(LoadContextOptions::Options(ConnectOptions {
                    shuffle: crate::queue::shuffle(&state),
                    repeat: crate::queue::repeat_mode(&state) == RepeatMode::Context,
                    repeat_track: crate::queue::repeat_mode(&state) == RepeatMode::Track,
                })),
                playing_track: Some(PlayingTrack::Uid(uid.to_owned())),
            },
        );
        let mut sent = false;
        self.with_spirc(|spirc| {
            sent = spirc.load(request).is_ok();
        });
        sent
    }

    /// The Connect devices of the account (patched `librespot-connect`).
    pub fn connect_devices(&self) -> Option<librespot_connect::ConnectDevices> {
        self.spirc
            .lock()
            .ok()?
            .as_ref()
            .map(|spirc| spirc.devices().borrow().clone())
    }

    /// Apricot's volume (0 to 100 percent) as the volume of its Connect
    /// device, so the phone shows it. Spotify reports it back unchanged.
    pub fn set_volume(&self, percent: f64) {
        let volume = volume_from_percent(percent);
        self.with_spirc(|spirc| {
            let _ = spirc.set_volume(volume);
        });
    }

    /// Announces volumes another device set for this one.
    pub async fn watch_remote_volume(
        shared: Arc<Shared>,
        mut volumes: tokio::sync::watch::Receiver<Option<u16>>,
    ) {
        while volumes.changed().await.is_ok() {
            let volume = *volumes.borrow_and_update();
            if let Some(volume) = volume {
                shared.push_notice(PlaybackNotice::Volume(volume));
            }
        }
    }

    /// Announces device list changes to the UI, at most a few times a second.
    pub async fn watch_devices(
        shared: Arc<Shared>,
        mut devices: tokio::sync::watch::Receiver<librespot_connect::ConnectDevices>,
    ) {
        let mut previous = None;
        while devices.changed().await.is_ok() {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let key = {
                let connect = devices.borrow_and_update();
                let list: Vec<_> = connect
                    .devices
                    .iter()
                    .map(|device| {
                        (
                            device.device_id.clone(),
                            device.name.clone(),
                            device.is_offline,
                            device.can_play,
                        )
                    })
                    .collect();
                (connect.active_device_id.clone(), list)
            };
            if previous.as_ref() != Some(&key) {
                previous = Some(key);
                shared.push_notice(PlaybackNotice::DevicesChanged);
            }
        }
    }

    /// Announces changes of the confirmed state to the UI, at most a few
    /// times a second: a burst of updates becomes one reload.
    pub async fn watch_state(
        shared: Arc<Shared>,
        mut states: tokio::sync::watch::Receiver<PlayerState>,
    ) {
        let mut last = None;
        while states.changed().await.is_ok() {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let key = {
                let state = states.borrow_and_update();
                (
                    state.queue_revision.clone(),
                    state.track.as_ref().map(|track| track.uid.clone()),
                    state.context_uri.clone(),
                    crate::queue::shuffle(&state),
                    crate::queue::repeat_mode(&state),
                    state.is_playing || state.is_paused,
                )
            };
            if last.as_ref() != Some(&key) {
                last = Some(key);
                shared.push_notice(PlaybackNotice::QueueChanged);
            }
        }
    }

    pub fn set_premium(&self, premium: bool) {
        self.premium.store(premium, Ordering::SeqCst);
    }

    pub fn set_spirc(&self, spirc: Option<Spirc>) {
        if let Ok(mut slot) = self.spirc.lock()
            && let Some(old) = std::mem::replace(&mut *slot, spirc)
        {
            let _ = old.shutdown();
        }
    }

    fn with_spirc(&self, action: impl FnOnce(&Spirc)) {
        if let Ok(slot) = self.spirc.lock()
            && let Some(spirc) = slot.as_ref()
        {
            action(spirc);
        }
    }

    /// Player events that are not fences: pause state, track changes, the
    /// end of playback and unavailable tracks. Runs on the Spotify runtime.
    pub async fn listen(shared: Arc<Shared>, mut events: PlayerEventChannel) {
        let mut paused = false;
        let mut request_id = None;
        while let Some(event) = events.recv().await {
            if !matches!(event, PlayerEvent::PositionChanged { .. }) {
                let name = format!("{event:?}");
                log::info!(
                    "player event {}",
                    name.split([' ', '{', '(']).next().unwrap_or_default()
                );
            }
            match event {
                PlayerEvent::TrackChanged { audio_item } => {
                    if let Ok(mut track) = shared.track.lock() {
                        *track = Some(SpotifyTrack::from_audio_item(&audio_item));
                    }
                }
                PlayerEvent::Playing {
                    play_request_id,
                    position_ms,
                    ..
                } => {
                    if paused {
                        paused = false;
                        shared.push_event(PcmSourceEvent::Paused(false));
                    }
                    if request_id != Some(play_request_id) {
                        request_id = Some(play_request_id);
                        let track = shared.track.lock().ok().and_then(|track| track.clone());
                        if let Some(track) = track {
                            let requested = shared.requested.swap(false, Ordering::SeqCst);
                            shared.push_notice(PlaybackNotice::Playing {
                                track,
                                position_ms,
                                requested,
                            });
                        }
                    }
                }
                PlayerEvent::Paused { .. } => {
                    if !paused {
                        paused = true;
                        shared.push_event(PcmSourceEvent::Paused(true));
                    }
                }
                PlayerEvent::Stopped {
                    play_request_id, ..
                } => {
                    // The end of playback: the generation of that request
                    // ends, so mpv reports the item's end once.
                    if let Ok(current) = shared.current.lock()
                        && let Some((_, generation)) = current.as_ref()
                        && generation.request_id.load(Ordering::SeqCst) == play_request_id
                    {
                        log::info!("request {play_request_id} stopped; its generation ends");
                        generation.close();
                    }
                }
                PlayerEvent::Unavailable { track_id, .. } => {
                    shared.push_notice(PlaybackNotice::Unavailable {
                        uri: track_id.to_uri().unwrap_or_default(),
                    });
                }
                _ => {}
            }
        }
    }
}

impl PcmSource for SpotifyPlayback {
    fn start(
        &self,
        item: &MediaItem,
        position_ms: u32,
        paused: bool,
        attach: bool,
    ) -> Result<(), String> {
        if !self.premium.load(Ordering::SeqCst) {
            return Err("spotify_premium_required".to_owned());
        }
        let current = self
            .shared
            .current
            .lock()
            .ok()
            .and_then(|current| current.as_ref().map(|(id, _)| *id));
        if attach {
            // The Connect device already plays it: load its current generation.
            if let Some(id) = current {
                let base_ms = position_ms;
                if let Ok(mut pending) = self.shared.pending.lock()
                    && pending.is_none()
                {
                    let duration_ms = self
                        .shared
                        .track
                        .lock()
                        .ok()
                        .and_then(|track| track.as_ref().map(|track| track.duration_ms));
                    *pending = Some(PcmGeneration {
                        id,
                        base_ms,
                        duration_ms,
                    });
                }
            }
            return Ok(());
        }
        let uri = SpotifyRef::parse(&item.id.0)
            .or_else(|| {
                item.url
                    .as_ref()
                    .and_then(|url| SpotifyRef::parse(url.as_str()))
            })
            .map(|reference| reference.to_uri())
            .ok_or_else(|| "spotify_unplayable".to_owned())?;
        self.shared.requested.store(true, Ordering::SeqCst);
        log::info!("start requested at {position_ms} ms, paused {paused}");
        let text = |key: &str| {
            item.metadata
                .get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        let request = if let Some(context) = text("spotify_context") {
            // The exact occurrence when known, otherwise the track itself.
            let playing_track = text("spotify_uid")
                .map(PlayingTrack::Uid)
                .or(Some(PlayingTrack::Uri(uri)));
            let shuffle = item
                .metadata
                .get("spotify_shuffle")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            LoadRequest::from_context_uri(
                context,
                LoadRequestOptions {
                    start_playing: !paused,
                    seek_to: position_ms,
                    context_options: Some(LoadContextOptions::Options(ConnectOptions {
                        shuffle,
                        ..ConnectOptions::default()
                    })),
                    playing_track,
                },
            )
        } else {
            LoadRequest::from_tracks(
                vec![uri],
                LoadRequestOptions {
                    start_playing: !paused,
                    seek_to: position_ms,
                    ..LoadRequestOptions::default()
                },
            )
        };
        let mut result = Err("spotify_not_connected".to_owned());
        self.with_spirc(|spirc| {
            result = spirc
                .activate()
                .and_then(|()| spirc.load(request))
                .map_err(|_| "spotify_not_connected".to_owned());
        });
        result
    }

    fn set_paused(&self, paused: bool) {
        self.with_spirc(|spirc| {
            let _ = if paused { spirc.pause() } else { spirc.play() };
        });
    }

    fn seek(&self, position_ms: u32) {
        self.with_spirc(|spirc| {
            let _ = spirc.set_position_ms(position_ms);
        });
    }

    fn stop(&self) {
        self.with_spirc(|spirc| {
            let _ = spirc.pause();
        });
        if let Ok(mut pending) = self.shared.pending.lock() {
            *pending = None;
        }
    }

    fn take_generation(&self) -> Option<PcmGeneration> {
        self.shared.pending.lock().ok()?.take()
    }

    fn open(&self, id: u64) -> Option<Arc<dyn PcmStream>> {
        log::info!("mpv opens generation {id}");
        let all = self.shared.generations.lock().ok()?;
        all.iter()
            .find(|(known, _)| *known == id)
            .map(|(_, generation)| {
                Arc::new(Reader {
                    generation: generation.clone(),
                    cancelled: AtomicBool::new(false),
                }) as Arc<dyn PcmStream>
            })
    }

    fn poll_event(&self) -> Option<PcmSourceEvent> {
        self.shared.events.lock().ok()?.pop_front()
    }

    fn format(&self) -> (String, Option<f64>) {
        (
            "vorbis".to_owned(),
            Some(f64::from(self.shared.bitrate_kbps) * 1000.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shared() -> Arc<Shared> {
        SpotifyPlayback::new(320, Arc::new(|| {})).1
    }

    #[test]
    fn a_replaced_generation_is_not_ended_by_the_source() {
        let shared = shared();
        shared.open_generation(1, 0, None);
        let first = shared.current.lock().unwrap().clone().unwrap().1;
        first.push(&[1, 2, 3, 4]);
        shared.open_generation(1, 60_000, None);
        // The old stream still has its data and is not closed.
        let mut buffer = [0_u8; 8];
        let reader = Reader {
            generation: first.clone(),
            cancelled: AtomicBool::new(false),
        };
        assert_eq!(reader.read(&mut buffer), 4);
        assert!(!first.data.lock().unwrap().1);
        let pending = shared.pending.lock().unwrap().take().unwrap();
        assert_eq!(pending.base_ms, 60_000);
        assert_eq!(pending.id, 2);
    }

    #[test]
    fn closing_ends_reads_after_the_buffered_data() {
        let generation = Generation::new(7);
        generation.push(&[0; 8]);
        let probe = Reader {
            generation: generation.clone(),
            cancelled: AtomicBool::new(false),
        };
        probe.cancel();
        let reader = Reader {
            generation: generation.clone(),
            cancelled: AtomicBool::new(false),
        };
        generation.close();
        let mut buffer = [0_u8; 6];
        assert_eq!(probe.read(&mut buffer), 0, "a closed handle reads nothing");
        assert_eq!(reader.read(&mut buffer), 4, "whole frames only");
        assert_eq!(reader.read(&mut buffer), 4, "another handle still reads");
        assert_eq!(reader.read(&mut buffer), 0);
    }

    #[test]
    fn closing_the_session_releases_a_writer_waiting_for_ring_space() {
        let shared = shared();
        shared.open_generation(1, 0, None);
        let generation = shared.current.lock().unwrap().clone().unwrap().1;
        generation.push(&vec![0; RING_BYTES]);
        // Nobody reads: the player thread would wait here forever.
        let writer = std::thread::spawn(move || generation.push(&[0; 4]));
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!writer.is_finished());
        shared.close_all();
        writer.join().unwrap();
    }

    #[test]
    fn whole_percent_volumes_survive_the_round_trip() {
        for percent in 0_u16..=100 {
            let back = percent_from_volume(volume_from_percent(f64::from(percent)));
            assert!((back - f64::from(percent)).abs() < f64::EPSILON);
        }
        assert_eq!(volume_from_percent(250.0), u16::MAX);
    }

    #[test]
    fn free_accounts_cannot_start_playback() {
        let (playback, _) = SpotifyPlayback::new(160, Arc::new(|| {}));
        let item = SpotifyTrack {
            uri: "spotify:track:4u7EnebtmKWzUH433cf5Qv".into(),
            title: "T".into(),
            artists: "A".into(),
            album: String::new(),
            duration_ms: 1000,
        }
        .media_item();
        assert_eq!(
            playback.start(&item, 0, false, false),
            Err("spotify_premium_required".to_owned())
        );
    }

    #[test]
    fn media_items_keep_the_durable_link_only() {
        let item = SpotifyTrack {
            uri: "spotify:track:4u7EnebtmKWzUH433cf5Qv".into(),
            title: "Title".into(),
            artists: "Artist".into(),
            album: "Album".into(),
            duration_ms: 354_000,
        }
        .media_item();
        assert_eq!(item.source, apricot_core::MediaSource::Spotify);
        assert_eq!(
            item.url.as_ref().map(url::Url::as_str),
            Some("https://open.spotify.com/track/4u7EnebtmKWzUH433cf5Qv")
        );
        assert_eq!(item.stream_url, None);
        assert_eq!(item.duration_seconds, Some(354.0));
        assert_eq!(item.channel, "Artist");
    }
}
