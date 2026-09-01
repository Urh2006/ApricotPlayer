//! Playback engine contract. The first implementation controls an isolated mpv
//! process over bounded JSON IPC.

use apricot_core::MediaItem;
use thiserror::Error;

#[cfg(windows)]
mod mpv_ipc;

#[cfg(windows)]
pub use mpv_ipc::{MpvIpcClient, make_unique_ipc_path};

#[derive(Clone, Debug, PartialEq)]
pub enum PlaybackCommand {
    Load(Box<MediaItem>),
    SetPaused(bool),
    SeekRelative { seconds: f64, exact: bool },
    SeekAbsolute { seconds: f64, exact: bool },
    SetVolume(f64),
    SetSpeed(f64),
    SetPitch(f64),
    Stop,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlaybackEvent {
    Started,
    Paused(bool),
    Position { elapsed: f64, duration: Option<f64> },
    Ended,
    Failed(String),
}

#[derive(Debug, Error)]
pub enum PlaybackError {
    #[error("player is not running")]
    NotRunning,
    #[error("player IPC timed out")]
    Timeout,
    #[error("player returned invalid data: {0}")]
    InvalidData(String),
    #[error("player operation failed: {0}")]
    Operation(String),
}

pub trait PlaybackEngine: Send {
    /// Sends a command to the playback engine.
    ///
    /// # Errors
    ///
    /// Returns [`PlaybackError`] when the engine is unavailable, its IPC times
    /// out, or the engine rejects the command.
    fn execute(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError>;

    /// Returns the next event currently available from the playback engine.
    ///
    /// # Errors
    ///
    /// Returns [`PlaybackError`] when polling the engine fails or its response
    /// cannot be decoded.
    fn poll_event(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError>;
}
