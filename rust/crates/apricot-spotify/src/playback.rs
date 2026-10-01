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
use librespot_connect::{LoadRequest, LoadRequestOptions, Spirc};
use librespot_metadata::audio::{AudioItem, UniqueFields};
use librespot_playback::{
    audio_backend::{Sink, SinkResult},
    convert::Converter,
    decoder::AudioPacket,
    mixer::{Mixer, MixerConfig, NoOpVolume, VolumeGetter},
    player::{PlayerEvent, PlayerEventChannel},
};

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
}

struct Generation {
    request_id: u64,
    data: Mutex<(VecDeque<u8>, bool)>,
    cond: Condvar,
}

impl Generation {
    fn new(request_id: u64) -> Arc<Self> {
        Arc::new(Self {
            request_id,
            data: Mutex::new((VecDeque::with_capacity(RING_BYTES), false)),
            cond: Condvar::new(),
        })
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
    /// URI Apricot asked for; the next new track with it is "requested".
    requested: Mutex<Option<String>>,
    track: Mutex<Option<SpotifyTrack>>,
    bitrate_kbps: u32,
    notify: Notify,
}

impl Shared {
    fn open_generation(&self, request_id: u64, base_ms: u32) {
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
            *pending = Some(PcmGeneration { id, base_ms });
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
                PlayerEvent::Seeked {
                    play_request_id,
                    position_ms,
                    ..
                } => {
                    self.request_id = Some(play_request_id);
                    self.shared.open_generation(play_request_id, position_ms);
                }
                PlayerEvent::Playing {
                    play_request_id,
                    position_ms,
                    ..
                } if self.request_id != Some(play_request_id) => {
                    self.request_id = Some(play_request_id);
                    self.shared.open_generation(play_request_id, position_ms);
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
            requested: Mutex::default(),
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
                            let requested = shared
                                .requested
                                .lock()
                                .ok()
                                .and_then(|mut requested| requested.take())
                                .is_some_and(|uri| uri == track.uri);
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
                        && generation.request_id == play_request_id
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
                    *pending = Some(PcmGeneration { id, base_ms });
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
        if let Ok(mut requested) = self.shared.requested.lock() {
            *requested = Some(uri.clone());
        }
        log::info!("start requested at {position_ms} ms, paused {paused}");
        let options = LoadRequestOptions {
            start_playing: !paused,
            seek_to: position_ms,
            ..LoadRequestOptions::default()
        };
        let mut result = Err("spotify_not_connected".to_owned());
        self.with_spirc(|spirc| {
            result = spirc
                .activate()
                .and_then(|()| spirc.load(LoadRequest::from_tracks(vec![uri], options)))
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
        shared.open_generation(1, 0);
        let first = shared.current.lock().unwrap().clone().unwrap().1;
        first.push(&[1, 2, 3, 4]);
        shared.open_generation(1, 60_000);
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
