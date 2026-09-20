//! Playback engine contract. The first implementation controls an isolated mpv
//! process over bounded JSON IPC.

use apricot_core::MediaItem;
use thiserror::Error;

mod equalizer;
#[cfg(windows)]
mod libmpv;
#[cfg(windows)]
mod mpv_ipc;
#[cfg(windows)]
mod mpv_process;
#[cfg(windows)]
mod runtime;

pub use equalizer::{EqualizerFilterConfig, build_equalizer_filter};
#[cfg(windows)]
pub use libmpv::LibMpvEngine;
#[cfg(windows)]
pub use mpv_ipc::{MpvIpcClient, make_unique_ipc_path};
#[cfg(windows)]
pub use mpv_process::{
    InitialPlaybackState, MpvCacheConfig, MpvLaunchOptions, MpvProcessEngine, MpvVideoMode,
    RepeatMode,
};
#[cfg(windows)]
pub use runtime::{PlaybackRuntime, PlaybackRuntimeError, PlaybackUpdate};

#[derive(Clone, Debug, PartialEq)]
pub enum PlaybackCommand {
    Load {
        item: Box<MediaItem>,
        start_position_seconds: Option<f64>,
    },
    SetPaused(bool),
    SeekRelative {
        seconds: f64,
        exact: bool,
    },
    SeekAbsolute {
        seconds: f64,
        exact: bool,
    },
    SetVolume(f64),
    SetVolumeMax(u16),
    SetSpeed(f64),
    SetPitch(f64),
    SetRepeat(bool),
    SetAudioFilter(Option<String>),
    Stop,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaybackMediaInfo {
    pub container: Option<String>,
    pub video_codec: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub audio_codec: Option<String>,
    pub audio_bitrate_bits_per_second: Option<f64>,
    pub sample_rate_hz: Option<u32>,
    pub channel_count: Option<u32>,
    pub channel_layout: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlaybackEvent {
    Started,
    Paused(bool),
    Position { elapsed: f64, duration: Option<f64> },
    MediaInfo(PlaybackMediaInfo),
    Ended,
    PreviewFinished,
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
