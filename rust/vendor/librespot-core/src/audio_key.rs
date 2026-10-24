use std::{
    collections::{HashMap, VecDeque},
    io::Write,
    sync::Arc,
    time::Duration,
};

use byteorder::{BigEndian, ByteOrder, WriteBytesExt};
use bytes::Bytes;
use thiserror::Error;
use tokio::sync::oneshot;

use crate::{Error, FileId, SpotifyId, packet::PacketType, util::SeqGenerator};

#[derive(Debug, Hash, PartialEq, Eq, Copy, Clone)]
pub struct AudioKey(pub [u8; 16]);

/// ApricotPlayer: keys kept between sessions, so a song played before needs
/// no new key from Spotify's per-account limit (beta 24).
pub trait AudioKeyStore: Send + Sync {
    fn get(&self, track: SpotifyId, file: FileId) -> Option<AudioKey>;
    fn put(&self, track: SpotifyId, file: FileId, key: AudioKey);
    fn forget(&self, track: SpotifyId, file: FileId);
}

/// Spotify's refusal code when this account requested too many audio keys.
/// Measured on 2026-10-19: about 30 keys at once, then about one every 30 s,
/// for the whole account; refused requests did not delay the next key.
pub const AUDIO_KEY_RATE_LIMITED: u16 = 0x0002;

#[derive(Debug, Error)]
pub enum AudioKeyError {
    #[error("audio key error {0:#06x}")]
    AesKey(u16),
    #[error("other end of channel disconnected")]
    Channel,
    #[error("unexpected packet type {0}")]
    Packet(u8),
    #[error("sequence {0} not pending")]
    Sequence(u32),
    #[error("audio key response timeout")]
    Timeout,
}

impl From<AudioKeyError> for Error {
    fn from(err: AudioKeyError) -> Self {
        match err {
            AudioKeyError::AesKey(_) => Error::unavailable(err),
            AudioKeyError::Channel => Error::aborted(err),
            AudioKeyError::Sequence(_) => Error::aborted(err),
            AudioKeyError::Packet(_) => Error::unimplemented(err),
            AudioKeyError::Timeout => Error::aborted(err),
        }
    }
}

component! {
    AudioKeyManager : AudioKeyManagerInner {
        sequence: SeqGenerator<u32> = SeqGenerator::new(0),
        pending: HashMap<u32, oneshot::Sender<Result<AudioKey, Error>>> = HashMap::new(),
        // Keys belong to an exact track/file pair and this authenticated
        // session only. Replaying a song must not request its key repeatedly.
        keys: VecDeque<((SpotifyId, FileId), AudioKey)> = VecDeque::new(),
        store: Option<Arc<dyn AudioKeyStore>> = None,
    }
}

struct PendingKeyRequest<'a> {
    manager: &'a AudioKeyManager,
    sequence: u32,
}

impl Drop for PendingKeyRequest<'_> {
    fn drop(&mut self) {
        self.manager
            .lock(|inner| inner.pending.remove(&self.sequence));
    }
}

impl AudioKeyError {
    /// Spotify refused the key only because this account asked for too many
    /// keys recently; the same request succeeds once the limit allows it.
    pub fn is_rate_limited(error: &Error) -> bool {
        matches!(
            error.error.downcast_ref::<Self>(),
            Some(Self::AesKey(AUDIO_KEY_RATE_LIMITED))
        )
    }
}

impl AudioKeyManager {
    pub(crate) fn dispatch(&self, cmd: PacketType, mut data: Bytes) -> Result<(), Error> {
        let seq = BigEndian::read_u32(data.split_to(4).as_ref());

        let sender = self
            .lock(|inner| inner.pending.remove(&seq))
            .ok_or(AudioKeyError::Sequence(seq))?;

        match cmd {
            PacketType::AesKey => {
                let mut key = [0u8; 16];
                key.copy_from_slice(data.as_ref());
                sender
                    .send(Ok(AudioKey(key)))
                    .map_err(|_| AudioKeyError::Channel)?
            }
            PacketType::AesKeyError => {
                let code = match data.as_ref() {
                    [high, low, ..] => u16::from_be_bytes([*high, *low]),
                    _ => 0,
                };
                error!("error audio key {:x} {:x}", code >> 8, code & 0xff);
                sender
                    .send(Err(AudioKeyError::AesKey(code).into()))
                    .map_err(|_| AudioKeyError::Channel)?
            }
            _ => {
                trace!("Did not expect {cmd:?} AES key packet with data {data:#?}");
                return Err(AudioKeyError::Packet(cmd as u8).into());
            }
        }

        Ok(())
    }

    /// Keys kept between sessions, or `None` for this session's keys only.
    pub fn set_store(&self, store: Option<Arc<dyn AudioKeyStore>>) {
        self.lock(|inner| inner.store = store);
    }

    pub fn has_store(&self) -> bool {
        self.lock(|inner| inner.store.is_some())
    }

    /// A key that did not decrypt its file is dropped everywhere, so the
    /// next request asks Spotify again.
    pub fn forget(&self, track: SpotifyId, file: FileId) {
        let store = self.lock(|inner| {
            inner.keys.retain(|(pair, _)| *pair != (track, file));
            inner.store.clone()
        });
        if let Some(store) = store {
            store.forget(track, file);
        }
    }

    pub async fn request(&self, track: SpotifyId, file: FileId) -> Result<AudioKey, Error> {
        let (known, store) = self.lock(|inner| {
            let known = inner
                .keys
                .iter()
                .rev()
                .find_map(|(pair, key)| (*pair == (track, file)).then_some(*key));
            (known, inner.store.clone())
        });
        if let Some(key) = known {
            return Ok(key);
        }
        if let Some(key) = store.as_ref().and_then(|store| store.get(track, file)) {
            self.remember(track, file, key);
            return Ok(key);
        }
        let (tx, rx) = oneshot::channel();

        let seq = self.lock(move |inner| {
            let seq = inner.sequence.get();
            inner.pending.insert(seq, tx);
            seq
        });

        // Also remove canceled requests when Next/Previous drops its loader,
        // and on send errors/timeouts; otherwise pending senders accumulate.
        let _pending = PendingKeyRequest {
            manager: self,
            sequence: seq,
        };

        self.send_key_request(seq, track, file)?;
        const KEY_RESPONSE_TIMEOUT: Duration = Duration::from_millis(1500);
        let key = match tokio::time::timeout(KEY_RESPONSE_TIMEOUT, rx).await {
            Err(_) => {
                error!("Audio key response timeout");
                return Err(AudioKeyError::Timeout.into());
            }
            Ok(k) => k??,
        };
        self.remember(track, file, key);
        if let Some(store) = self.lock(|inner| inner.store.clone()) {
            store.put(track, file, key);
        }
        Ok(key)
    }

    fn remember(&self, track: SpotifyId, file: FileId, key: AudioKey) {
        self.lock(|inner| {
            const MAX_SESSION_KEYS: usize = 256;
            if inner.keys.len() == MAX_SESSION_KEYS {
                inner.keys.pop_front();
            }
            inner.keys.push_back(((track, file), key));
        });
    }

    fn send_key_request(&self, seq: u32, track: SpotifyId, file: FileId) -> Result<(), Error> {
        let mut data: Vec<u8> = Vec::new();
        data.write_all(&file.0)?;
        data.write_all(&track.to_raw())?;
        data.write_u32::<BigEndian>(seq)?;
        data.write_u16::<BigEndian>(0x0000)?;

        self.session().send_packet(PacketType::RequestKey, data)
    }
}
