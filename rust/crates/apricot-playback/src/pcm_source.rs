//! External PCM sources played through libmpv (Spotify, `docs/SPOTIFY_PLAN.md` 8).
//!
//! The source owns decoding, seeking and the transport of its items; mpv only
//! plays the PCM through Apricot's own filters (EQ, speed, pitch, boost) and
//! output device. Every seek or new track opens a new stream generation, so
//! mpv never plays PCM of an old position.

use std::sync::{Arc, RwLock};

use apricot_core::MediaItem;

/// Fixed PCM layout of every generation: signed 16-bit little-endian stereo.
pub const PCM_RATE: u32 = 44_100;
pub const PCM_CHANNELS: u32 = 2;
/// mpv protocol of PCM generations, `apricot-pcm://<generation>`.
pub const PCM_PROTOCOL: &str = "apricot-pcm";

/// One generation of PCM: a blocking reader that ends (returns 0) when the
/// generation is closed. mpv reads it on its own thread and may cancel it
/// from another one, so the reader is shared.
pub trait PcmStream: Send + Sync {
    /// Blocks until data is available; `0` ends the stream.
    fn read(&self, buffer: &mut [u8]) -> usize;
    /// Unblocks a pending read; later reads return `0`.
    fn cancel(&self);
}

/// Something the engine reports back from the source.
#[derive(Clone, Debug, PartialEq)]
pub enum PcmSourceEvent {
    /// The source paused or resumed by itself (a remote device, Connect).
    Paused(bool),
    /// The item cannot be played; a short localized reason key or text.
    Failed(String),
}

/// A PCM generation mpv should load now, starting at `base_ms` of the item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PcmGeneration {
    pub id: u64,
    pub base_ms: u32,
}

pub trait PcmSource: Send + Sync {
    /// Starts `item` at `position_ms`. When `attach` is true the source is
    /// already playing it (for example after a Connect transfer) and only the
    /// current generation is reused.
    ///
    /// # Errors
    ///
    /// Returns a short reason when the item cannot be started.
    fn start(
        &self,
        item: &MediaItem,
        position_ms: u32,
        paused: bool,
        attach: bool,
    ) -> Result<(), String>;
    fn set_paused(&self, paused: bool);
    fn seek(&self, position_ms: u32);
    fn stop(&self);
    /// The next generation to load, once per generation.
    fn take_generation(&self) -> Option<PcmGeneration>;
    /// The reader of generation `id`; `None` when it is already replaced.
    fn open(&self, id: u64) -> Option<Arc<dyn PcmStream>>;
    fn poll_event(&self) -> Option<PcmSourceEvent>;
    /// Codec and bitrate for the format status (`F`).
    fn format(&self) -> (String, Option<f64>);
}

static SOURCE: RwLock<Option<Arc<dyn PcmSource>>> = RwLock::new(None);

/// Installs (or with `None` removes) the source behind `apricot-pcm://`.
pub fn set_pcm_source(source: Option<Arc<dyn PcmSource>>) {
    if let Ok(mut slot) = SOURCE.write() {
        *slot = source;
    }
}

#[must_use]
pub fn pcm_source() -> Option<Arc<dyn PcmSource>> {
    SOURCE.read().ok().and_then(|slot| slot.clone())
}

/// Items with this source play as PCM.
#[must_use]
pub fn is_pcm_item(item: &MediaItem) -> bool {
    item.source == apricot_core::MediaSource::Spotify
}

/// Per-file mpv options of a PCM generation.
#[must_use]
pub fn pcm_file_options() -> String {
    format!(
        "demuxer=rawaudio,demuxer-rawaudio-format=s16le,demuxer-rawaudio-rate={PCM_RATE},demuxer-rawaudio-channels={PCM_CHANNELS},cache=no,demuxer-readahead-secs=0.1"
    )
}
