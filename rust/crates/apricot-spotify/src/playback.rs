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

/// Distinguishes failure of the active load from a future preload or an old request.
#[derive(Default)]
struct AvailabilityNotices {
    active: Option<(u64, librespot_core::SpotifyUri, u64)>,
}

impl AvailabilityNotices {
    fn current_track_failed(&mut self, event: &PlayerEvent, revision: u64) -> bool {
        match event {
            PlayerEvent::Loading {
                play_request_id,
                track_id,
                ..
            }
            | PlayerEvent::Playing {
                play_request_id,
                track_id,
                ..
            }
            | PlayerEvent::Paused {
                play_request_id,
                track_id,
                ..
            } => {
                if !self
                    .active
                    .as_ref()
                    .is_some_and(|(request, uri, _)| request == play_request_id && uri == track_id)
                {
                    self.active = Some((*play_request_id, track_id.clone(), revision));
                }
            }
            PlayerEvent::Unavailable {
                play_request_id,
                track_id,
            } if self
                .active
                .as_ref()
                .is_some_and(|(request, uri, active_revision)| {
                    request == play_request_id && uri == track_id && *active_revision == revision
                }) =>
            {
                self.active = None;
                return true;
            }
            PlayerEvent::Stopped {
                play_request_id, ..
            } if self
                .active
                .as_ref()
                .is_some_and(|(request, _, _)| request == play_request_id) =>
            {
                self.active = None;
            }
            _ => {}
        }
        false
    }
}

/// 200 ms of s16le stereo 44.1 kHz: the bound between `LibreSpot` and mpv.
const RING_BYTES: usize = 44_100 * 4 / 5;

/// What the UI learns about the track the Connect device plays.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyTrack {
    pub uri: String,
    pub title: String,
    pub artists: String,
    pub artist_uri: String,
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
            artist_uri: match &item.unique_fields {
                UniqueFields::Track { artists, .. } => artists
                    .first()
                    .map(|artist| artist.id.to_string())
                    .unwrap_or_default(),
                _ => String::new(),
            },
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
        if self.artist_uri.starts_with("spotify:artist:") {
            metadata.insert(
                "spotify_artist_uri".to_owned(),
                serde_json::Value::from(self.artist_uri.clone()),
            );
        }
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
    fn push(&self, mut bytes: &[u8]) -> usize {
        let total = bytes.len();
        while !bytes.is_empty() {
            let Ok(mut guard) = self.data.lock() else {
                return total - bytes.len();
            };
            while guard.0.len() >= RING_BYTES && !guard.1 {
                let Ok(next) = self.cond.wait(guard) else {
                    return total - bytes.len();
                };
                guard = next;
            }
            if guard.1 {
                return total - bytes.len();
            }
            let take = (RING_BYTES - guard.0.len()).min(bytes.len());
            guard.0.extend(&bytes[..take]);
            self.written.fetch_add(take as u64, Ordering::SeqCst);
            bytes = &bytes[take..];
            self.cond.notify_all();
        }
        total
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
    notices: Mutex<VecDeque<(u64, PlaybackNotice)>>,
    manual_revision: AtomicU64,
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

    /// mpv has stopped consuming this stream. Release a decoder waiting for
    /// ring space before it can handle the next transport command.
    fn close_current(&self) {
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
        self.push_notice_at(self.manual_revision.load(Ordering::SeqCst), notice);
    }

    fn push_notice_at(&self, revision: u64, notice: PlaybackNotice) {
        if let Ok(mut notices) = self.notices.lock() {
            notices.push_back((revision, notice));
        }
        (self.notify)();
    }

    /// Manual navigation invalidates old callbacks, while automatic skips retain
    /// their genuine failure notice even after the following song has started.
    fn manual_navigation(&self) {
        self.manual_revision.fetch_add(1, Ordering::SeqCst);
    }

    pub fn take_notice(&self) -> Option<PlaybackNotice> {
        let mut notices = self.notices.lock().ok()?;
        while let Some((revision, notice)) = notices.pop_front() {
            if matches!(
                notice,
                PlaybackNotice::Unavailable { .. } | PlaybackNotice::Playing { .. }
            ) && revision != self.manual_revision.load(Ordering::SeqCst)
            {
                continue;
            }
            return Some(notice);
        }
        None
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
                } if self.request_id != Some(play_request_id)
                    || self.shared.current.lock().is_ok_and(|current| {
                        current
                            .as_ref()
                            .is_some_and(|(_, generation)| generation.is_closed())
                    }) =>
                {
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
        self.write_counted(packet, converter).map(|_| ())
    }

    fn write_counted(
        &mut self,
        packet: AudioPacket,
        converter: &mut Converter,
    ) -> SinkResult<usize> {
        self.fence();
        let AudioPacket::Samples(samples) = packet else {
            return Ok(0);
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
            return Ok(generation.push(&bytes) / 2);
        }
        Ok(0)
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
    /// The playlist that plays in smart shuffle, and the number of the next
    /// recommendation's UID.
    smart: Mutex<Option<String>>,
    next_smart_uid: AtomicU64,
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
            manual_revision: AtomicU64::new(0),
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
                smart: Mutex::new(None),
                next_smart_uid: AtomicU64::new(1),
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
        self.shared.manual_navigation();
        self.with_spirc(|spirc| {
            let _ = spirc.next();
        });
    }

    pub fn previous(&self) {
        self.shared.manual_navigation();
        self.with_spirc(|spirc| {
            let _ = spirc.prev();
        });
    }

    pub fn set_shuffle(&self, shuffle: bool) {
        // Beta 6 check (plan 4a.3): which context Spotify reports when the
        // user changes shuffle, e.g. Liked Songs from the menu or Library.
        log::info!(
            "shuffle {shuffle} in context {}",
            self.player_state()
                .map(|state| state.context_uri)
                .unwrap_or_default()
        );
        self.with_spirc(|spirc| {
            let _ = spirc.shuffle(shuffle);
        });
    }

    /// The playlist in smart shuffle, if any.
    pub fn smart_shuffle(&self) -> Option<String> {
        self.smart.lock().ok()?.clone()
    }

    /// Turns smart shuffle on for `playlist`, or off with `None`; off removes
    /// the recommendations from the upcoming tracks.
    pub fn set_smart_shuffle(&self, playlist: Option<String>) {
        log::info!("smart shuffle {playlist:?}");
        let off = playlist.is_none();
        if let Ok(mut smart) = self.smart.lock() {
            *smart = playlist;
        }
        if !off {
            return;
        }
        let Some(state) = self.player_state() else {
            return;
        };
        if let Some(next_tracks) = crate::smart_shuffle::without_smart(&state.next_tracks) {
            self.with_spirc(|spirc| {
                let _ = spirc.set_queue(next_tracks, state.queue_revision.clone());
            });
        }
    }

    /// Mixes recommendations into the upcoming tracks; `false` when the
    /// queue could not take them.
    pub fn add_smart_tracks(&self, tracks: &[crate::smart_shuffle::SmartTrack]) -> bool {
        let Some(state) = self.player_state() else {
            return false;
        };
        let first = self
            .next_smart_uid
            .fetch_add(tracks.len() as u64, Ordering::Relaxed);
        let next_tracks = crate::smart_shuffle::interleave(&state.next_tracks, tracks, first);
        if next_tracks.len() == state.next_tracks.len() {
            return false;
        }
        let mut sent = false;
        self.with_spirc(|spirc| {
            sent = spirc
                .set_queue(next_tracks, state.queue_revision.clone())
                .is_ok();
        });
        sent
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

    /// Plays an upcoming track now: a manual track or smart recommendation
    /// is moved first and skipped to; a context track starts at its occurrence
    /// (the manual queue stays, plan D16).
    pub fn play_queue_entry(&self, uid: &str) -> bool {
        let Some(state) = self.player_state() else {
            return false;
        };
        let Some(track) = state.next_tracks.iter().find(|track| track.uid == uid) else {
            return false;
        };
        if track.provider == "queue" || crate::smart_shuffle::is_smart(track) {
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
        let mut availability = AvailabilityNotices::default();
        while let Some(event) = events.recv().await {
            if !matches!(event, PlayerEvent::PositionChanged { .. }) {
                let name = format!("{event:?}");
                log::info!(
                    "player event {}",
                    name.split([' ', '{', '(']).next().unwrap_or_default()
                );
            }
            let revision = shared.manual_revision.load(Ordering::SeqCst);
            let current_track_failed = availability.current_track_failed(&event, revision);
            if matches!(
                event,
                PlayerEvent::Playing { .. } | PlayerEvent::Paused { .. }
            ) && availability
                .active
                .as_ref()
                .is_some_and(|(_, _, active_revision)| *active_revision != revision)
            {
                continue;
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
                    // mpv can pause itself at the EOF of a replaced PCM
                    // generation while LibreSpot still considers playback
                    // active. Always synchronize its confirmed Playing state,
                    // including Next/Previous and a resumed generation.
                    paused = false;
                    shared.push_event(PcmSourceEvent::Paused(false));
                    if request_id != Some(play_request_id) {
                        request_id = Some(play_request_id);
                        let track = shared.track.lock().ok().and_then(|track| track.clone());
                        if let Some(track) = track {
                            let requested = shared.requested.swap(false, Ordering::SeqCst);
                            shared.push_notice_at(
                                revision,
                                PlaybackNotice::Playing {
                                    track,
                                    position_ms,
                                    requested,
                                },
                            );
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
                PlayerEvent::Unavailable { track_id, .. } if current_track_failed => {
                    shared.push_notice_at(
                        revision,
                        PlaybackNotice::Unavailable {
                            uri: track_id.to_uri().unwrap_or_default(),
                        },
                    );
                }
                _ => {}
            }
        }
    }
}

/// The Connect load request for an Apricot item: its context at the exact
/// occurrence (or the track), a whole album, playlist, artist or show, or a
/// single track.
fn load_request(
    item: &MediaItem,
    uri: String,
    position_ms: u32,
    paused: bool,
    repeat_mode: RepeatMode,
) -> LoadRequest {
    let shuffle = item
        .metadata
        .get("spotify_shuffle")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let text = |key: &str| {
        item.metadata
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    // An album, playlist, artist or show saved as an item plays whole.
    let collection_uri = SpotifyRef::parse(&uri)
        .filter(|reference| {
            !matches!(
                reference.kind,
                SpotifyEntityKind::Track | SpotifyEntityKind::Episode
            )
        })
        .map(|_| uri.clone());
    if let Some(context) = text("spotify_context").or(collection_uri) {
        // The exact occurrence when known, otherwise the track itself; a
        // whole album or playlist lets Spotify choose (shuffle).
        let whole = item
            .metadata
            .get("spotify_collection")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
            || context == uri;
        let playing_track = if whole {
            None
        } else {
            text("spotify_uid")
                .map(PlayingTrack::Uid)
                .or(Some(PlayingTrack::Uri(uri)))
        };
        log::info!("load context {context} (whole {whole}, shuffle {shuffle})");
        LoadRequest::from_context_uri(
            context,
            LoadRequestOptions {
                start_playing: !paused,
                seek_to: position_ms,
                context_options: Some(LoadContextOptions::Options(ConnectOptions {
                    shuffle,
                    repeat: matches!(repeat_mode, RepeatMode::Context),
                    repeat_track: matches!(repeat_mode, RepeatMode::Track),
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
                context_options: Some(LoadContextOptions::Options(ConnectOptions {
                    shuffle,
                    repeat: matches!(repeat_mode, RepeatMode::Context),
                    repeat_track: matches!(repeat_mode, RepeatMode::Track),
                })),
                ..LoadRequestOptions::default()
            },
        )
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
        let current = self.shared.current.lock().ok().and_then(|current| {
            current
                .as_ref()
                .and_then(|(id, generation)| (!generation.is_closed()).then_some(*id))
        });
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
        self.shared.manual_navigation();
        self.shared.requested.store(true, Ordering::SeqCst);
        log::info!("start requested at {position_ms} ms, paused {paused}");
        // Setting shuffle requires a complete Connect options object. Keep
        // repeat too when replacing a live item, rather than reset it as a
        // side effect; an actually closed player begins with defaults.
        let repeat_mode = if current.is_some() {
            self.player_state()
                .as_ref()
                .map_or(RepeatMode::Off, crate::queue::repeat_mode)
        } else {
            RepeatMode::Off
        };
        let request = load_request(item, uri, position_ms, paused, repeat_mode);
        self.shared.close_current();
        if let Ok(mut pending) = self.shared.pending.lock() {
            *pending = None;
        }
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
        self.shared.close_current();
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
    fn manual_navigation_discards_queued_failures_but_automatic_skip_keeps_them() {
        let shared = shared();
        shared.push_notice(PlaybackNotice::Unavailable { uri: "old".into() });
        shared.manual_navigation();
        assert!(shared.take_notice().is_none());
        shared.push_notice(PlaybackNotice::Unavailable {
            uri: "genuine".into(),
        });
        // An automatic skip does not create a new manual intent.
        assert!(
            matches!(shared.take_notice(), Some(PlaybackNotice::Unavailable { uri }) if uri == "genuine")
        );
    }

    #[test]
    fn delayed_failure_after_rapid_manual_next_is_not_the_current_request() {
        let uri =
            librespot_core::SpotifyUri::from_uri("spotify:track:72AZ3V52rs9NfgMNhALxln").unwrap();
        let mut guard = AvailabilityNotices::default();
        assert!(!guard.current_track_failed(
            &PlayerEvent::Loading {
                play_request_id: 1,
                track_id: uri.clone(),
                position_ms: 0
            },
            0
        ));
        // Late play/pause from the old request cannot acquire the new manual revision.
        assert!(!guard.current_track_failed(
            &PlayerEvent::Playing {
                play_request_id: 1,
                track_id: uri.clone(),
                position_ms: 0
            },
            3
        ));
        assert!(!guard.current_track_failed(
            &PlayerEvent::Unavailable {
                play_request_id: 1,
                track_id: uri.clone()
            },
            3
        ));
        assert!(!guard.current_track_failed(
            &PlayerEvent::Loading {
                play_request_id: 4,
                track_id: uri.clone(),
                position_ms: 0
            },
            3
        ));
        assert!(guard.current_track_failed(
            &PlayerEvent::Unavailable {
                play_request_id: 4,
                track_id: uri
            },
            3
        ));
    }

    #[test]
    fn listening_counts_only_pcm_accepted_by_the_bridge() {
        let shared = shared();
        let mut sink = SpotifyPlayback::sink(shared.clone(), Arc::new(Mutex::new(None)));
        let mut converter = Converter::new(None);
        assert_eq!(
            sink.write_counted(AudioPacket::Samples(vec![0.0; 8]), &mut converter)
                .unwrap(),
            0
        );
        shared.open_generation(1, 0, None);
        assert_eq!(
            sink.write_counted(AudioPacket::Samples(vec![0.0; 8]), &mut converter)
                .unwrap(),
            8
        );
        shared.close_all();
        assert_eq!(
            sink.write_counted(AudioPacket::Samples(vec![0.0; 8]), &mut converter)
                .unwrap(),
            0
        );
    }

    #[test]
    fn cancelled_ring_reports_only_the_partial_packet_it_accepted() {
        let generation = Generation::new(7);
        let waiting = generation.clone();
        let writer = std::thread::spawn(move || waiting.push(&vec![0; RING_BYTES + 4]));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while generation.written.load(Ordering::SeqCst) < RING_BYTES as u64 {
            assert!(
                std::time::Instant::now() < deadline,
                "writer did not fill the ring"
            );
            std::thread::yield_now();
        }
        generation.close();
        assert_eq!(writer.join().unwrap(), RING_BYTES);
        assert_eq!(generation.push(&[0; 4]), 0);
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
    fn stopping_playback_releases_the_decoder_when_mpv_stops_reading() {
        let (playback, shared) = SpotifyPlayback::new(320, Arc::new(|| {}));
        shared.open_generation(1, 0, None);
        let generation = shared.current.lock().unwrap().clone().unwrap().1;
        generation.push(&vec![0; RING_BYTES]);
        let (done, received) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            generation.push(&[0; 4]);
            done.send(()).unwrap();
        });
        playback.stop();
        let released = received
            .recv_timeout(std::time::Duration::from_secs(1))
            .is_ok();
        shared.close_all();
        writer.join().unwrap();
        assert!(
            released,
            "stop must release the decoder before another track can load"
        );
    }

    #[test]
    fn resuming_a_stopped_request_opens_a_fresh_pcm_generation() {
        let (playback, shared) = SpotifyPlayback::new(320, Arc::new(|| {}));
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut sink = SpotifyPlayback::sink(shared.clone(), Arc::new(Mutex::new(Some(receiver))));
        let playing = |position_ms| PlayerEvent::Playing {
            play_request_id: 1,
            track_id: librespot_core::SpotifyUri::from_uri("spotify:track:4u7EnebtmKWzUH433cf5Qv")
                .unwrap(),
            position_ms,
        };
        sender.send(playing(0)).unwrap();
        sink.fence();
        assert_eq!(shared.pending.lock().unwrap().take().unwrap().id, 1);
        playback.stop();
        sender.send(playing(500)).unwrap();
        sink.fence();
        let resumed = shared
            .pending
            .lock()
            .unwrap()
            .take()
            .expect("resume must not reuse the closed stream");
        assert_eq!(resumed.id, 2);
        assert_eq!(resumed.base_ms, 500);
        assert!(
            !shared
                .current
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .1
                .is_closed()
        );
    }

    #[tokio::test]
    async fn preload_and_stale_failures_do_not_announce_that_the_playing_track_was_skipped() {
        let shared = shared();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let first =
            librespot_core::SpotifyUri::from_uri("spotify:track:4u7EnebtmKWzUH433cf5Qv").unwrap();
        let next =
            librespot_core::SpotifyUri::from_uri("spotify:track:6rqhFgbbKwnb9MLmUQDhG6").unwrap();
        for event in [
            PlayerEvent::Playing {
                play_request_id: 1,
                track_id: first.clone(),
                position_ms: 0,
            },
            // A failed future preload carries the CURRENT request ID.
            PlayerEvent::Unavailable {
                play_request_id: 1,
                track_id: next.clone(),
            },
            PlayerEvent::Loading {
                play_request_id: 2,
                track_id: next.clone(),
                position_ms: 0,
            },
            PlayerEvent::Unavailable {
                play_request_id: 1,
                track_id: first.clone(),
            },
            // An actual current load failure must still be announced once.
            PlayerEvent::Unavailable {
                play_request_id: 2,
                track_id: next.clone(),
            },
            PlayerEvent::Unavailable {
                play_request_id: 2,
                track_id: next.clone(),
            },
            PlayerEvent::Paused {
                play_request_id: 3,
                track_id: first.clone(),
                position_ms: 0,
            },
            PlayerEvent::Stopped {
                play_request_id: 3,
                track_id: first.clone(),
            },
            PlayerEvent::Unavailable {
                play_request_id: 3,
                track_id: first,
            },
        ] {
            sender.send(event).unwrap();
        }
        drop(sender);
        SpotifyPlayback::listen(shared.clone(), receiver).await;
        let notices = shared.notices.lock().unwrap();
        assert_eq!(notices.len(), 1);
        assert!(
            matches!(&notices[0].1, PlaybackNotice::Unavailable { uri } if *uri == next.to_uri().unwrap())
        );
    }

    #[tokio::test]
    async fn every_playing_event_resynchronizes_mpv_after_its_automatic_eof_pause() {
        let shared = shared();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let track_id =
            librespot_core::SpotifyUri::from_uri("spotify:track:4u7EnebtmKWzUH433cf5Qv").unwrap();
        // First track, Next, then Previous: mpv may have applied its own EOF
        // pause between generations without LibreSpot reporting Paused.
        for play_request_id in [1, 2, 3] {
            sender
                .send(PlayerEvent::Playing {
                    play_request_id,
                    track_id: track_id.clone(),
                    position_ms: 0,
                })
                .unwrap();
        }
        drop(sender);
        SpotifyPlayback::listen(shared.clone(), receiver).await;
        let events = shared.events.lock().unwrap();
        assert_eq!(
            events.len(),
            3,
            "every confirmed Playing must clear mpv's own EOF pause"
        );
        assert!(
            events
                .iter()
                .all(|event| matches!(event, PcmSourceEvent::Paused(false)))
        );
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
            artist_uri: String::new(),
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
            artist_uri: String::new(),
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

    #[test]
    fn a_new_single_track_load_carries_the_session_shuffle_mode() {
        let mut item = SpotifyTrack {
            uri: "spotify:track:4u7EnebtmKWzUH433cf5Qv".into(),
            title: "Title".into(),
            artists: "Artist".into(),
            artist_uri: String::new(),
            album: String::new(),
            duration_ms: 1000,
        }
        .media_item();
        item.metadata.insert("spotify_shuffle".into(), true.into());
        let request = load_request(&item, item.id.0.clone(), 0, false, RepeatMode::Off);
        // LibreSpot exposes the request's Debug representation but keeps
        // its fields private. Check the actual outbound request's mode.
        assert!(format!("{request:?}").contains("shuffle: true"));
    }

    #[test]
    fn replacing_a_single_track_keeps_the_confirmed_repeat_track_mode() {
        let item = SpotifyTrack {
            uri: "spotify:track:4u7EnebtmKWzUH433cf5Qv".into(),
            title: "Title".into(),
            artists: "Artist".into(),
            artist_uri: String::new(),
            album: String::new(),
            duration_ms: 1000,
        }
        .media_item();
        let request = load_request(&item, item.id.0.clone(), 0, false, RepeatMode::Track);
        assert!(format!("{request:?}").contains("repeat_track: true"));
    }
}
