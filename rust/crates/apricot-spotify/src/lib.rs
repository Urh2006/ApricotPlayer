//! Spotify integration for `ApricotPlayer` 2.0 (plan `docs/SPOTIFY_PLAN.md`).
//!
//! The UI never sees tokens, protobuf objects or raw endpoints: it talks to
//! [`service::SpotifyService`] and receives typed events.

pub mod accounts;
pub mod diagnostics;
pub mod oauth;
pub mod playback;
pub mod service;

pub use accounts::{AccountStore, SpotifyAccount, SpotifyAccounts};
pub use oauth::CallbackPage;
pub use playback::{PlaybackNotice, SpotifyTrack};
pub use service::{SpotifyError, SpotifyEvent, SpotifyService};
