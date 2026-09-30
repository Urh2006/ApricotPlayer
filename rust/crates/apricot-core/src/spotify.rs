//! Durable Spotify identities, capabilities and request epochs.
//!
//! Only stable references live here: never tokens, CDN URLs or playback keys.

use serde::{Deserialize, Serialize};
use url::Url;

/// Entity kinds a durable Spotify reference can point to.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpotifyEntityKind {
    Track,
    Episode,
    Album,
    Artist,
    Playlist,
    Show,
    Audiobook,
    Chapter,
    User,
    /// Liked Songs of a user, `spotify:user:<name>:collection`.
    LikedSongs,
}

impl SpotifyEntityKind {
    const fn uri_segment(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Episode => "episode",
            Self::Album => "album",
            Self::Artist => "artist",
            Self::Playlist => "playlist",
            Self::Show => "show",
            Self::Audiobook => "audiobook",
            Self::Chapter => "chapter",
            Self::User | Self::LikedSongs => "user",
        }
    }

    fn from_segment(segment: &str) -> Option<Self> {
        Some(match segment {
            "track" => Self::Track,
            "episode" => Self::Episode,
            "album" => Self::Album,
            "artist" => Self::Artist,
            "playlist" => Self::Playlist,
            "show" => Self::Show,
            "audiobook" => Self::Audiobook,
            "chapter" => Self::Chapter,
            "user" => Self::User,
            _ => return None,
        })
    }
}

/// A durable Spotify reference. `id` is the base62 ID, or the user name for
/// [`SpotifyEntityKind::User`] and [`SpotifyEntityKind::LikedSongs`].
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct SpotifyRef {
    pub kind: SpotifyEntityKind,
    pub id: String,
}

fn is_base62_id(id: &str) -> bool {
    id.len() == 22 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn is_user_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'%'))
}

impl SpotifyRef {
    pub fn new(kind: SpotifyEntityKind, id: impl Into<String>) -> Option<Self> {
        let id = id.into();
        let valid = match kind {
            SpotifyEntityKind::User | SpotifyEntityKind::LikedSongs => is_user_name(&id),
            _ => is_base62_id(&id),
        };
        valid.then_some(Self { kind, id })
    }

    /// Parses a `spotify:` URI or an `open.spotify.com` link (with or without
    /// an `intl-xx` segment and query). Short `spotify.link` links need a
    /// network redirect and are not handled here.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if let Some(rest) = text.strip_prefix("spotify:") {
            return Self::from_segments(&rest.split(':').collect::<Vec<_>>());
        }
        let url = Url::parse(text).ok()?;
        if !matches!(url.scheme(), "https" | "http") || url.host_str()? != "open.spotify.com" {
            return None;
        }
        let segments: Vec<&str> = url
            .path_segments()?
            .filter(|segment| !segment.is_empty() && !segment.starts_with("intl-"))
            .collect();
        Self::from_segments(&segments)
    }

    fn from_segments(segments: &[&str]) -> Option<Self> {
        match segments {
            ["user", name, "collection"] => Self::new(SpotifyEntityKind::LikedSongs, *name),
            // Legacy `spotify:user:<name>:playlist:<id>`.
            ["user", _, "playlist", id] => Self::new(SpotifyEntityKind::Playlist, *id),
            [kind, id] => Self::new(SpotifyEntityKind::from_segment(kind)?, *id),
            _ => None,
        }
    }

    pub fn to_uri(&self) -> String {
        match self.kind {
            SpotifyEntityKind::LikedSongs => format!("spotify:user:{}:collection", self.id),
            kind => format!("spotify:{}:{}", kind.uri_segment(), self.id),
        }
    }

    /// Permanent share link; Liked Songs is private and has none.
    pub fn to_url(&self) -> Option<String> {
        (self.kind != SpotifyEntityKind::LikedSongs).then(|| {
            format!(
                "https://open.spotify.com/{}/{}",
                self.kind.uri_segment(),
                self.id
            )
        })
    }
}

/// What an action may do with a Spotify target for the active account.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpotifyCapability {
    Read,
    Play,
    Save,
    Unsave,
    EditPlaylist,
    ManageQueue,
    Dislike,
    Lyrics,
    Transcript,
    Chapters,
    LocalDsp,
    RemoteControl,
}

/// Capability state. `Unknown` never allows an action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapabilityState {
    Available,
    /// Not available, with the text key that explains why.
    Unavailable(&'static str),
    Unknown,
}

impl CapabilityState {
    pub const fn allows(&self) -> bool {
        matches!(self, Self::Available)
    }
}

/// Stamp carried by every Spotify request and event: the account session it
/// belongs to and the request generation inside that session.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct SpotifyStamp {
    pub account_epoch: u64,
    pub generation: u64,
}

/// Issues [`SpotifyStamp`]s and decides whether a late completion still
/// belongs to the current account and the newest request of its kind.
#[derive(Debug, Default)]
pub struct SpotifyEpochs {
    account_epoch: u64,
    generation: u64,
}

impl SpotifyEpochs {
    /// A new account session (login, switch, logout): every older stamp
    /// becomes stale.
    pub fn next_account(&mut self) -> u64 {
        self.account_epoch += 1;
        self.generation = 0;
        self.account_epoch
    }

    pub const fn account_epoch(&self) -> u64 {
        self.account_epoch
    }

    /// A new request that supersedes the previous one of the same kind.
    pub fn begin(&mut self) -> SpotifyStamp {
        self.generation += 1;
        SpotifyStamp {
            account_epoch: self.account_epoch,
            generation: self.generation,
        }
    }

    pub const fn is_current_account(&self, stamp: SpotifyStamp) -> bool {
        stamp.account_epoch == self.account_epoch
    }

    pub const fn is_latest(&self, stamp: SpotifyStamp) -> bool {
        stamp.account_epoch == self.account_epoch && stamp.generation == self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACK: &str = "4u7EnebtmKWzUH433cf5Qv";

    #[test]
    fn parses_uris_and_links_to_the_same_reference() {
        let expected = SpotifyRef::new(SpotifyEntityKind::Track, TRACK).unwrap();
        for text in [
            format!("spotify:track:{TRACK}"),
            format!("https://open.spotify.com/track/{TRACK}"),
            format!("https://open.spotify.com/intl-sl/track/{TRACK}?si=abc123"),
            format!("  http://open.spotify.com/track/{TRACK}/ "),
        ] {
            assert_eq!(SpotifyRef::parse(&text), Some(expected.clone()), "{text}");
        }
        assert_eq!(expected.to_uri(), format!("spotify:track:{TRACK}"));
        assert_eq!(
            expected.to_url().as_deref(),
            Some(format!("https://open.spotify.com/track/{TRACK}").as_str())
        );
    }

    #[test]
    fn parses_liked_songs_and_legacy_playlist_uris() {
        let liked = SpotifyRef::parse("spotify:user:someone.1:collection").unwrap();
        assert_eq!(liked.kind, SpotifyEntityKind::LikedSongs);
        assert_eq!(liked.to_uri(), "spotify:user:someone.1:collection");
        assert_eq!(liked.to_url(), None);
        let playlist = SpotifyRef::parse("spotify:user:x:playlist:37i9dQZF1E354PdVhnf53R").unwrap();
        assert_eq!(playlist.kind, SpotifyEntityKind::Playlist);
    }

    #[test]
    fn rejects_foreign_hosts_bad_ids_and_unknown_kinds() {
        for text in [
            "",
            "spotify:track:short",
            "spotify:video:4u7EnebtmKWzUH433cf5Qv",
            "https://example.com/track/4u7EnebtmKWzUH433cf5Qv",
            "https://open.spotify.com/track/4u7Enebtm-WzUH433cf5Qv",
            "https://spotify.link/abc",
        ] {
            assert_eq!(SpotifyRef::parse(text), None, "{text}");
        }
    }

    #[test]
    fn unknown_capability_never_allows() {
        assert!(CapabilityState::Available.allows());
        assert!(!CapabilityState::Unknown.allows());
        assert!(!CapabilityState::Unavailable("spotify_premium_required").allows());
    }

    #[test]
    fn late_completions_of_an_old_account_or_request_are_stale() {
        let mut epochs = SpotifyEpochs::default();
        epochs.next_account();
        let first = epochs.begin();
        let second = epochs.begin();
        assert!(!epochs.is_latest(first));
        assert!(epochs.is_latest(second));
        epochs.next_account();
        assert!(!epochs.is_current_account(second));
        assert!(!epochs.is_latest(second));
    }
}
