//! The Spotify queue of this Connect device (`docs/SPOTIFY_PLAN.md` 4.2, 6.1).
//!
//! Connect (the patched Spirc) owns the queue. This module only projects its
//! confirmed state for the UI and turns one edit into the complete
//! `next_tracks` list for `Spirc::set_queue`, which applies it only while the
//! queue still has the revision the view was built from.

use librespot_protocol::player::{PlayerState, ProvidedTrack};

use crate::playback::RepeatMode;

const PROVIDER_QUEUE: &str = "queue";
const PROVIDER_AUTOPLAY: &str = "autoplay";
const PROVIDER_UNAVAILABLE: &str = "unavailable";
const DELIMITER: &str = "delimiter";

/// Where an upcoming track comes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueSection {
    /// Added by the user ("Added manually"); Clear removes these only.
    Manual,
    /// The next tracks of the playing album or playlist.
    Context,
    /// Spotify continues with similar music after the context.
    Autoplay,
    /// Recommended by smart shuffle.
    SmartShuffle,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueEntry {
    /// The occurrence: duplicates of one track have different UIDs.
    pub uid: String,
    pub uri: String,
    pub section: QueueSection,
    pub title: String,
    pub artists: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyQueue {
    /// Apricot is the device that plays; only then can it change the queue.
    pub active: bool,
    pub revision: String,
    pub context_uri: String,
    pub current: Option<QueueEntry>,
    pub entries: Vec<QueueEntry>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

/// One change of the queue view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueueEdit {
    Remove(String),
    /// Moves a manually added track one place within the manual tracks.
    Move {
        uid: String,
        up: bool,
    },
    /// Removes the manually added tracks; the context stays.
    ClearManual,
    /// Puts a manual track or smart recommendation first, to play by `next`.
    ToFront(String),
}

fn section(track: &ProvidedTrack) -> QueueSection {
    match track.provider.as_str() {
        PROVIDER_QUEUE => QueueSection::Manual,
        PROVIDER_AUTOPLAY => QueueSection::Autoplay,
        crate::smart_shuffle::PROVIDER => QueueSection::SmartShuffle,
        _ => QueueSection::Context,
    }
}

/// Tracks and episodes; not the page delimiters of a repeated context.
fn is_listed(track: &ProvidedTrack) -> bool {
    !track.uid.starts_with(DELIMITER)
        && track.provider != PROVIDER_UNAVAILABLE
        && (track.uri.starts_with("spotify:track:") || track.uri.starts_with("spotify:episode:"))
}

fn entry(track: &ProvidedTrack) -> QueueEntry {
    let text = |key: &str| track.metadata.get(key).cloned().unwrap_or_default();
    QueueEntry {
        uid: track.uid.clone(),
        uri: track.uri.clone(),
        section: section(track),
        title: text("title"),
        artists: text("artist_name"),
    }
}

pub fn repeat_mode(state: &PlayerState) -> RepeatMode {
    match state.options.as_ref() {
        Some(options) if options.repeating_track => RepeatMode::Track,
        Some(options) if options.repeating_context => RepeatMode::Context,
        _ => RepeatMode::Off,
    }
}

pub fn shuffle(state: &PlayerState) -> bool {
    state
        .options
        .as_ref()
        .is_some_and(|options| options.shuffling_context)
}

/// The confirmed queue; titles the state does not carry are filled later.
pub fn snapshot(state: &PlayerState) -> SpotifyQueue {
    let current = state
        .track
        .as_ref()
        .filter(|track| is_listed(track))
        .map(entry);
    SpotifyQueue {
        active: current.is_some() && (state.is_playing || state.is_paused),
        revision: state.queue_revision.clone(),
        context_uri: state.context_uri.clone(),
        current,
        entries: state
            .next_tracks
            .iter()
            .filter(|track| is_listed(track))
            .map(entry)
            .collect(),
        shuffle: shuffle(state),
        repeat: repeat_mode(state),
    }
}

/// The next tracks after `edit`, or `None` when it changes nothing (an
/// unknown UID, a move past the manual tracks, nothing to clear).
pub fn apply(next: &[ProvidedTrack], edit: &QueueEdit) -> Option<Vec<ProvidedTrack>> {
    let position = |uid: &str| next.iter().position(|track| track.uid == uid);
    let mut tracks = next.to_vec();
    match edit {
        QueueEdit::Remove(uid) => {
            tracks.remove(position(uid)?);
        }
        QueueEdit::Move { uid, up } => {
            let index = position(uid)?;
            let other = if *up {
                index.checked_sub(1)?
            } else {
                index + 1
            };
            let manual = |at: usize| {
                tracks
                    .get(at)
                    .is_some_and(|track| section(track) == QueueSection::Manual)
            };
            if !manual(index) || !manual(other) {
                return None;
            }
            tracks.swap(index, other);
        }
        QueueEdit::ClearManual => {
            tracks.retain(|track| section(track) != QueueSection::Manual);
            if tracks.len() == next.len() {
                return None;
            }
        }
        QueueEdit::ToFront(uid) => {
            let index = position(uid)?;
            if !matches!(
                section(&tracks[index]),
                QueueSection::Manual | QueueSection::SmartShuffle
            ) {
                return None;
            }
            let track = tracks.remove(index);
            tracks.insert(0, track);
        }
    }
    Some(tracks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use librespot_protocol::player::ContextPlayerOptions;

    fn track(uid: &str, provider: &str) -> ProvidedTrack {
        let mut track = ProvidedTrack::new();
        track.uid = uid.to_owned();
        track.uri = format!("spotify:track:{uid}");
        track.provider = provider.to_owned();
        track
    }

    fn uids(tracks: &[ProvidedTrack]) -> Vec<&str> {
        tracks.iter().map(|track| track.uid.as_str()).collect()
    }

    fn next() -> Vec<ProvidedTrack> {
        vec![
            track("q0", "queue"),
            track("q1", "queue"),
            track("c1", "context"),
            track("c2", "context"),
        ]
    }

    #[test]
    fn snapshot_separates_manual_and_context_and_skips_delimiters() {
        let mut state = PlayerState::new();
        state.track = Some(track("c0", "context")).into();
        state.is_playing = true;
        state.queue_revision = "7".to_owned();
        let mut delimiter = track("delimiter0", "context");
        delimiter.uri = "spotify:delimiter".to_owned();
        let mut tracks = next();
        tracks.push(delimiter);
        tracks.push(track("a1", "autoplay"));
        state.next_tracks = tracks;
        let mut options = ContextPlayerOptions::new();
        options.repeating_context = true;
        options.shuffling_context = true;
        state.options = Some(options).into();

        let queue = snapshot(&state);
        assert!(queue.active);
        assert_eq!(queue.revision, "7");
        assert_eq!(queue.current.map(|entry| entry.uid), Some("c0".to_owned()));
        let sections: Vec<_> = queue.entries.iter().map(|e| e.section).collect();
        assert_eq!(
            sections,
            [
                QueueSection::Manual,
                QueueSection::Manual,
                QueueSection::Context,
                QueueSection::Context,
                QueueSection::Autoplay,
            ]
        );
        assert!(queue.shuffle);
        assert_eq!(queue.repeat, RepeatMode::Context);
    }

    #[test]
    fn idle_device_has_no_active_queue() {
        assert!(!snapshot(&PlayerState::new()).active);
    }

    #[test]
    fn remove_takes_the_exact_occurrence() {
        let mut tracks = next();
        tracks.push(track("c1b", "context"));
        tracks[4].uri = tracks[2].uri.clone();
        let edited = apply(&tracks, &QueueEdit::Remove("c1b".to_owned())).unwrap();
        assert_eq!(uids(&edited), ["q0", "q1", "c1", "c2"]);
        assert_eq!(apply(&tracks, &QueueEdit::Remove("x".to_owned())), None);
    }

    #[test]
    fn move_stays_within_manual_tracks() {
        let tracks = next();
        let down = apply(
            &tracks,
            &QueueEdit::Move {
                uid: "q0".to_owned(),
                up: false,
            },
        )
        .unwrap();
        assert_eq!(uids(&down), ["q1", "q0", "c1", "c2"]);
        let past_manual = QueueEdit::Move {
            uid: "q1".to_owned(),
            up: false,
        };
        assert_eq!(apply(&tracks, &past_manual), None);
        let first_up = QueueEdit::Move {
            uid: "q0".to_owned(),
            up: true,
        };
        assert_eq!(apply(&tracks, &first_up), None);
        let context = QueueEdit::Move {
            uid: "c2".to_owned(),
            up: true,
        };
        assert_eq!(apply(&tracks, &context), None);
    }

    #[test]
    fn clear_keeps_the_context() {
        let edited = apply(&next(), &QueueEdit::ClearManual).unwrap();
        assert_eq!(uids(&edited), ["c1", "c2"]);
        assert_eq!(apply(&edited, &QueueEdit::ClearManual), None);
    }

    #[test]
    fn to_front_rejects_normal_context_tracks() {
        let edited = apply(&next(), &QueueEdit::ToFront("q1".to_owned())).unwrap();
        assert_eq!(uids(&edited), ["q1", "q0", "c1", "c2"]);
        assert_eq!(apply(&next(), &QueueEdit::ToFront("c1".to_owned())), None);
    }

    #[test]
    fn play_now_fronts_a_smart_recommendation_without_losing_manual_or_context_tracks() {
        let mut tracks = next();
        tracks.insert(3, track("smart1", crate::smart_shuffle::PROVIDER));
        let edited = apply(&tracks, &QueueEdit::ToFront("smart1".to_owned()))
            .expect("a recommendation plays from the queue, not the original context");
        assert_eq!(uids(&edited), ["smart1", "q0", "q1", "c1", "c2"]);
        assert_eq!(
            edited[0], tracks[3],
            "preserve the exact occurrence and provider"
        );
        tracks.push(track("auto1", "autoplay"));
        assert!(apply(&tracks, &QueueEdit::ToFront("auto1".to_owned())).is_none());
    }
}
