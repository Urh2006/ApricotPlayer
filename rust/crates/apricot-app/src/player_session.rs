//! Authoritative player-session state, independent of the visible UI route.

use std::collections::{BTreeMap, BTreeSet};

use apricot_core::MediaItem;
use apricot_playback::{PlaybackEvent, PlaybackMediaInfo};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackPhase {
    #[default]
    Closed,
    Starting,
    Playing,
    Paused,
    Ended,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SessionToggle {
    AutoplayNext,
    BassBoost,
    VolumeBoost,
    Repeat,
    Shuffle,
    Fullscreen,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EqualizerSession {
    pub enabled: bool,
    pub gains: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioSession {
    pub volume: f64,
    pub output_device: String,
    pub speed: f64,
    pub pitch: f64,
    pub equalizer: EqualizerSession,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerSessionDefaults {
    pub audio: AudioSession,
    pub enabled_toggles: BTreeSet<SessionToggle>,
    pub starts_paused: bool,
}

#[derive(Debug, Default)]
pub struct PlayerSession {
    generation: u64,
    phase: PlaybackPhase,
    current_item: Option<MediaItem>,
    audio: Option<AudioSession>,
    enabled_toggles: BTreeSet<SessionToggle>,
    position_seconds: f64,
    duration_seconds: Option<f64>,
    media_info: PlaybackMediaInfo,
    last_error: Option<String>,
}

impl PlayerSession {
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn phase(&self) -> PlaybackPhase {
        self.phase
    }

    pub fn is_open(&self) -> bool {
        self.phase != PlaybackPhase::Closed
    }

    pub const fn current_item(&self) -> Option<&MediaItem> {
        self.current_item.as_ref()
    }

    pub const fn audio(&self) -> Option<&AudioSession> {
        self.audio.as_ref()
    }

    pub fn enabled_toggles(&self) -> &BTreeSet<SessionToggle> {
        &self.enabled_toggles
    }

    pub const fn position_seconds(&self) -> f64 {
        self.position_seconds
    }

    pub const fn duration_seconds(&self) -> Option<f64> {
        self.duration_seconds
    }

    pub const fn media_info(&self) -> &PlaybackMediaInfo {
        &self.media_info
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Starts or replaces media. Existing open sessions retain their audio and
    /// toggle state; a genuinely new session receives current defaults.
    pub fn start_item(&mut self, item: MediaItem, defaults: PlayerSessionDefaults) -> u64 {
        if !self.is_open() {
            self.audio = Some(defaults.audio);
            self.enabled_toggles = defaults.enabled_toggles;
        }
        self.advance_generation();
        self.phase = if defaults.starts_paused {
            PlaybackPhase::Paused
        } else {
            PlaybackPhase::Starting
        };
        self.current_item = Some(item);
        self.position_seconds = 0.0;
        self.duration_seconds = None;
        self.media_info = PlaybackMediaInfo::default();
        self.last_error = None;
        self.generation
    }

    pub fn apply_event(&mut self, generation: u64, event: PlaybackEvent) -> bool {
        if generation != self.generation || !self.is_open() {
            return false;
        }
        match event {
            PlaybackEvent::Started => self.phase = PlaybackPhase::Playing,
            PlaybackEvent::Paused(paused) => {
                self.phase = if paused {
                    PlaybackPhase::Paused
                } else {
                    PlaybackPhase::Playing
                };
            }
            PlaybackEvent::Position { elapsed, duration } => {
                self.position_seconds = elapsed.max(0.0);
                self.duration_seconds = duration.filter(|value| value.is_finite() && *value >= 0.0);
            }
            PlaybackEvent::MediaInfo(info) => self.media_info = info,
            PlaybackEvent::Ended => self.phase = PlaybackPhase::Ended,
            PlaybackEvent::Failed(error) => {
                self.phase = PlaybackPhase::Failed;
                self.last_error = Some(error);
            }
        }
        true
    }

    pub fn set_volume(&mut self, volume: f64) {
        if let Some(audio) = &mut self.audio
            && volume.is_finite()
        {
            audio.volume = volume.max(0.0);
        }
    }

    pub fn set_output_device(&mut self, output_device: impl Into<String>) {
        if let Some(audio) = &mut self.audio {
            audio.output_device = output_device.into();
        }
    }

    pub fn set_speed(&mut self, speed: f64) {
        if let Some(audio) = &mut self.audio
            && speed.is_finite()
        {
            audio.speed = speed.clamp(0.01, 100.0);
        }
    }

    pub fn set_pitch(&mut self, pitch: f64) {
        if let Some(audio) = &mut self.audio
            && pitch.is_finite()
        {
            audio.pitch = pitch.clamp(0.01, 100.0);
        }
    }

    pub fn set_toggle(&mut self, toggle: SessionToggle, enabled: bool) {
        if enabled {
            self.enabled_toggles.insert(toggle);
        } else {
            self.enabled_toggles.remove(&toggle);
        }
    }

    pub fn close(&mut self) {
        self.advance_generation();
        self.phase = PlaybackPhase::Closed;
        self.current_item = None;
        self.audio = None;
        self.enabled_toggles.clear();
        self.position_seconds = 0.0;
        self.duration_seconds = None;
        self.media_info = PlaybackMediaInfo::default();
        self.last_error = None;
    }

    fn advance_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use apricot_core::{MediaId, MediaKind, MediaSource};
    use apricot_playback::{PlaybackEvent, PlaybackMediaInfo};

    use super::{
        AudioSession, EqualizerSession, PlaybackPhase, PlayerSession, PlayerSessionDefaults,
        SessionToggle,
    };

    fn item(id: &str) -> apricot_core::MediaItem {
        apricot_core::MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: id.to_owned(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: Some(format!(r"C:\Music\{id}.mp3")),
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    fn defaults() -> PlayerSessionDefaults {
        PlayerSessionDefaults {
            audio: AudioSession {
                volume: 80.0,
                output_device: "speakers".to_owned(),
                speed: 1.0,
                pitch: 1.0,
                equalizer: EqualizerSession {
                    enabled: true,
                    gains: BTreeMap::from([("31".to_owned(), 3.0)]),
                },
            },
            enabled_toggles: BTreeSet::from([SessionToggle::BassBoost]),
            starts_paused: false,
        }
    }

    #[test]
    fn replacement_preserves_session_audio_until_real_close() {
        let mut session = PlayerSession::default();
        session.start_item(item("first"), defaults());
        session.set_volume(110.0);
        session.set_output_device("headphones");
        session.set_toggle(SessionToggle::AutoplayNext, true);

        let mut changed_defaults = defaults();
        changed_defaults.audio.volume = 20.0;
        changed_defaults.audio.output_device = "auto".to_owned();
        session.start_item(item("second"), changed_defaults);

        let audio = session.audio().expect("open audio session");
        assert!((audio.volume - 110.0).abs() < f64::EPSILON);
        assert_eq!(audio.output_device, "headphones");
        assert!(
            session
                .enabled_toggles()
                .contains(&SessionToggle::AutoplayNext)
        );

        session.close();
        assert_eq!(session.phase(), PlaybackPhase::Closed);
        assert!(session.audio().is_none());
        assert!(session.enabled_toggles().is_empty());
    }

    #[test]
    fn late_events_from_replaced_or_closed_players_are_ignored() {
        let mut session = PlayerSession::default();
        let first = session.start_item(item("first"), defaults());
        let second = session.start_item(item("second"), defaults());
        assert!(!session.apply_event(first, PlaybackEvent::Ended));
        assert_eq!(
            session.current_item().map(|media| media.id.0.as_str()),
            Some("second")
        );
        assert!(session.apply_event(second, PlaybackEvent::Started));

        session.close();
        assert!(!session.apply_event(second, PlaybackEvent::Paused(true)));
        assert_eq!(session.phase(), PlaybackPhase::Closed);
    }

    #[test]
    fn event_projection_tracks_transport_and_timing() {
        let mut session = PlayerSession::default();
        let generation = session.start_item(item("track"), defaults());
        assert!(session.apply_event(
            generation,
            PlaybackEvent::Position {
                elapsed: 12.5,
                duration: Some(90.0),
            }
        ));
        assert!((session.position_seconds() - 12.5).abs() < f64::EPSILON);
        assert_eq!(session.duration_seconds(), Some(90.0));
        assert!(session.apply_event(generation, PlaybackEvent::Paused(true)));
        assert_eq!(session.phase(), PlaybackPhase::Paused);
    }

    #[test]
    fn media_information_is_generation_bound_and_cleared_on_replacement() {
        let mut session = PlayerSession::default();
        let first = session.start_item(item("first"), defaults());
        assert!(session.apply_event(
            first,
            PlaybackEvent::MediaInfo(PlaybackMediaInfo {
                audio_codec: Some("flac".to_owned()),
                ..PlaybackMediaInfo::default()
            })
        ));
        assert_eq!(session.media_info().audio_codec.as_deref(), Some("flac"));

        let second = session.start_item(item("second"), defaults());
        assert_eq!(session.media_info(), &PlaybackMediaInfo::default());
        assert!(!session.apply_event(
            first,
            PlaybackEvent::MediaInfo(PlaybackMediaInfo {
                audio_codec: Some("stale".to_owned()),
                ..PlaybackMediaInfo::default()
            })
        ));
        assert!(session.media_info().audio_codec.is_none());
        assert!(session.apply_event(second, PlaybackEvent::Started));
    }
}
