//! Media source interfaces. Implementations must preserve source ordering and
//! stable item IDs while pages and metadata arrive asynchronously.

use apricot_core::{MediaItem, MediaSource};
use thiserror::Error;

pub mod youtube_protocol;
pub mod youtube_runtime;

pub use youtube_protocol::{
    MAX_YOUTUBE_MESSAGE_BYTES, MAX_YOUTUBE_METADATA_ITEMS, RUSTY_YTDL_REVISION,
    YOUTUBE_HELPER_PROTOCOL_VERSION, YoutubeBackend, YoutubeCapability, YoutubeCollectionKind,
    YoutubeCommand, YoutubeErrorCode, YoutubeFormat, YoutubeFormatTracks, YoutubeFormatTransport,
    YoutubeHelperError, YoutubeRequest, YoutubeResponse, YoutubeResponsePayload, YoutubeSearchKind,
    YoutubeSessionConfig, YoutubeStreamPreference, select_youtube_playback_formats,
};
pub use youtube_runtime::{
    YoutubeEngine, YoutubeEngineError, YoutubeRuntime, YoutubeRuntimeError, YoutubeUpdate,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchRequest {
    pub source: MediaSource,
    pub query: String,
    pub page_size: usize,
    pub continuation: Option<String>,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchPage {
    pub items: Vec<MediaItem>,
    pub continuation: Option<String>,
    pub generation: u64,
}

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("operation was cancelled")]
    Cancelled,
    #[error("source returned invalid data: {0}")]
    InvalidData(String),
    #[error("source is unavailable: {0}")]
    Unavailable(String),
}
