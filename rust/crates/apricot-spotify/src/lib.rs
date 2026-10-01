//! Spotify integration for `ApricotPlayer` 2.0 (plan `docs/SPOTIFY_PLAN.md`).
//!
//! The UI never sees tokens, protobuf objects or raw endpoints: it talks to
//! [`service::SpotifyService`] and receives typed events.

pub mod accounts;
pub mod diagnostics;
pub mod oauth;
pub mod playback;
pub mod queue;
pub mod service;

pub use accounts::{AccountStore, SpotifyAccount, SpotifyAccounts};
pub use oauth::CallbackPage;
pub use playback::{PlaybackNotice, RepeatMode, SpotifyPlayback, SpotifyTrack};
pub use queue::{QueueEdit, QueueEntry, QueueSection, SpotifyQueue};
pub use service::{SpotifyError, SpotifyEvent, SpotifyService};
