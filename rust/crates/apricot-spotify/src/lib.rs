//! Spotify integration for `ApricotPlayer` 2.0 (plan `docs/SPOTIFY_PLAN.md`).
//!
//! The UI never sees tokens, protobuf objects or raw endpoints: it talks to
//! [`service::SpotifyService`] and receives typed events.

pub mod accounts;
pub mod api;
pub mod catalog;
pub mod devices;
pub mod diagnostics;
pub mod library_edit;
pub mod oauth;
pub mod playback;
pub mod queue;
pub mod service;
pub mod settings;
pub mod smart_shuffle;

pub use accounts::{AccountStore, SpotifyAccount, SpotifyAccounts};
pub use catalog::{CatalogItem, CatalogPage, Collection, ItemKind, LibraryFilter, SearchKind};
pub use devices::SpotifyDevice;
pub use library_edit::{EditOutcome, LibraryEdit};
pub use oauth::CallbackPage;
pub use playback::{PlaybackNotice, RepeatMode, SpotifyPlayback, SpotifyTrack};
pub use queue::{QueueEdit, QueueEntry, QueueSection, SpotifyQueue};
pub use service::{CatalogRequest, CatalogResult, SpotifyError, SpotifyEvent, SpotifyService};
