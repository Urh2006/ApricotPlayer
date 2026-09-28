//! Playback engine contract. The first implementation controls an isolated mpv
//! process over bounded JSON IPC.

use apricot_core::MediaItem;
use thiserror::Error;

mod audio_chain;
mod audio_filters;
mod equalizer;
#[cfg(windows)]
mod libmpv;
#[cfg(windows)]
mod mpv_ipc;
#[cfg(windows)]
mod mpv_process;
#[cfg(windows)]
mod runtime;

pub use audio_chain::{
    PITCH_FILTER_LABEL, PitchMode, SPEED_FILTER_LABEL, SpeedAudioMode, audio_filter_chain,
    is_default_rate, mpv_pitch_property, pitch_filter_active, rubberband_pitch_filter,
};
pub use audio_filters::AudioFilterState;
pub use equalizer::{
    EQUALIZER_FILTER_ALT_LABEL, EQUALIZER_FILTER_LABEL, equalizer_filter_graph, equalizer_filters,
    tagged_equalizer_filter,
};
#[cfg(windows)]
pub use libmpv::{LibMpvEngine, probe_audio_output_devices};
#[cfg(windows)]
pub use mpv_ipc::{MpvIpcClient, make_unique_ipc_path};
#[cfg(windows)]
pub use mpv_process::{
    InitialPlaybackState, MpvCacheConfig, MpvLaunchOptions, MpvProcessEngine, MpvVideoMode,
    RepeatMode,
};
#[cfg(windows)]
pub use runtime::{PlaybackPositionReader, PlaybackRuntime, PlaybackRuntimeError, PlaybackUpdate};

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
    SetAudioPitchCorrection(bool),
    SetRepeat(bool),
    SetAudioFilter(Option<String>),
    /// mpv `af add`: appends one tagged filter to the running chain.
    AddAudioFilter(String),
    /// mpv `af remove`: removes one filter by its `@label` reference.
    RemoveAudioFilter(String),
    /// mpv `af-command`: sends a runtime command to one labeled filter.
    AudioFilterCommand {
        label: String,
        command: String,
        argument: String,
    },
    /// Python `apply_equalizer_to_player`: replaces only the equalizer filter
    /// (a `lavfi=[...]` graph, or `None` to clear it) through the alternating
    /// `@apricot_eq` and `@apricot_eq_next` labels, so the speed and pitch
    /// filters keep running. The playback worker expands it into `af` commands.
    SetEqualizerFilter(Option<String>),
    /// Python `apply_rubberband_pitch_filter` (`Some`) and
    /// `clear_rubberband_pitch_filter` (`None`), expanded by the playback worker.
    SetPitchFilter(Option<f64>),
    /// mpv `replaygain`: `no`, `track` or `album`.
    SetReplayGain(String),
    /// mpv `audio-device`, a name from `audio-device-list`.
    SetAudioDevice(String),
    Stop,
}

/// One entry of mpv `audio-device-list`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AudioOutputDevice {
    pub name: String,
    pub description: String,
}

/// Reads mpv `audio-device-list` the way Python `show_output_devices` does:
/// entries without a name are skipped and a missing description falls back
/// to the name.
#[must_use]
pub fn audio_output_devices_from_json(value: &serde_json::Value) -> Vec<AudioOutputDevice> {
    value
        .as_array()
        .map(|devices| {
            devices
                .iter()
                .take(1000)
                .filter_map(|device| {
                    let name = device.get("name")?.as_str()?.trim();
                    if name.is_empty() {
                        return None;
                    }
                    let description = device
                        .get("description")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|description| !description.is_empty())
                        .unwrap_or(name);
                    Some(AudioOutputDevice {
                        name: name.to_owned(),
                        description: description.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaybackMediaInfo {
    pub chapters: Vec<serde_json::Value>,
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
    Position {
        elapsed: f64,
        duration: Option<f64>,
    },
    MediaInfo(PlaybackMediaInfo),
    /// mpv `audio-device-list` changed or was first reported.
    AudioDevices(Vec<AudioOutputDevice>),
    Ended,
    PreviewFinished,
    Failed(String),
    /// A single command failed while the item keeps playing. Python announces
    /// "Timing is not available yet." for this instead of stopping playback.
    CommandFailed(String),
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

#[cfg(test)]
mod tests {
    use super::{AudioOutputDevice, audio_output_devices_from_json};

    #[test]
    fn audio_device_list_skips_unnamed_entries_and_defaults_description() {
        let devices = audio_output_devices_from_json(&serde_json::json!([
            {"name": "auto", "description": "Autoselect device"},
            {"name": "", "description": "Broken"},
            {"description": "No name"},
            {"name": "wasapi/{abc}"},
            "not a device",
        ]));
        assert_eq!(
            devices,
            vec![
                AudioOutputDevice {
                    name: "auto".to_owned(),
                    description: "Autoselect device".to_owned(),
                },
                AudioOutputDevice {
                    name: "wasapi/{abc}".to_owned(),
                    description: "wasapi/{abc}".to_owned(),
                },
            ]
        );
        assert!(audio_output_devices_from_json(&serde_json::Value::Null).is_empty());
    }
}
