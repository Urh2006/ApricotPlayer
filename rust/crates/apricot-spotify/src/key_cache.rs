//! Spotify audio keys kept between sessions (beta 24, optional setting).
//!
//! Spotify allows an account about 30 new audio keys at once and then about
//! one every 30 seconds. A song played before needs no new key when its key
//! is kept here. The file holds at most [`MAX_KEYS`] keys (about 18 MB); the
//! oldest go first. A random file key protected for this user (DPAPI on
//! Windows, the Keychain on macOS) encrypts every record with AES-256-CTR, so
//! the file reveals neither the keys nor which songs were played.

use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use aes::Aes256;
use ctr::cipher::{KeyIvInit, StreamCipher};
use librespot_core::{
    FileId, SpotifyId,
    audio_key::{AudioKey, AudioKeyStore},
};

/// Urh chose room for 250,000 songs (about 18 MB of records).
pub const MAX_KEYS: usize = 250_000;
const FILE_NAME: &str = "audio_keys.bin";
const MAGIC: &[u8; 8] = b"APRKEYS1";
const ENTROPY: &[u8] = b"ApricotPlayer Spotify audio keys v1";
const KEPT: &[u8; 4] = b"KEY1";
const FORGOTTEN: &[u8; 4] = b"DEL1";
const IV_LEN: usize = 16;
/// Track 16 + file 20 + key 16 + record kind 4.
const PLAIN_LEN: usize = 56;
const RECORD_LEN: usize = IV_LEN + PLAIN_LEN;

type Ctr = ctr::Ctr128BE<Aes256>;
type Id = [u8; 36];

fn id(track: SpotifyId, file: FileId) -> Id {
    let mut id = [0; 36];
    id[..16].copy_from_slice(&track.to_raw());
    id[16..].copy_from_slice(&file.0);
    id
}

/// The kept keys of one account.
pub struct KeyCache {
    path: PathBuf,
    secret: [u8; 32],
    inner: Mutex<Inner>,
}

struct Inner {
    keys: HashMap<Id, [u8; 16]>,
    /// Records in the file, kept and forgotten, for compaction.
    records: usize,
    file: Option<File>,
}

impl KeyCache {
    /// The file of the account folder `folder`.
    pub fn path(folder: &Path) -> PathBuf {
        folder.join(FILE_NAME)
    }

    /// Opens or creates the account's key file. A file this user cannot
    /// decrypt (another computer or user) or a damaged header starts over.
    ///
    /// # Errors
    ///
    /// Returns the reason when the file cannot be created or protected.
    pub fn open(folder: &Path) -> Result<Self, String> {
        Self::open_with(folder, &Protection::platform())
    }

    fn open_with(folder: &Path, protection: &Protection) -> Result<Self, String> {
        let path = Self::path(folder);
        if let Some(cache) = Self::read(&path, protection) {
            return Ok(cache);
        }
        std::fs::create_dir_all(folder).map_err(|error| error.to_string())?;
        let mut secret = [0; 32];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut secret);
        let protected = (protection.protect)(&secret)?;
        let mut header = Vec::with_capacity(16 + protected.len());
        header.extend_from_slice(MAGIC);
        let length = u32::try_from(protected.len()).map_err(|error| error.to_string())?;
        header.extend_from_slice(&length.to_le_bytes());
        header.extend_from_slice(protected.as_bytes());
        apricot_storage::json_file::write_bytes_atomic(&path, &header)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            path,
            secret,
            inner: Mutex::new(Inner {
                keys: HashMap::new(),
                records: 0,
                file: None,
            }),
        })
    }

    fn read(path: &Path, protection: &Protection) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let (secret, start) = header(&bytes, protection)?;
        // A record cut short (power loss while writing) would shift every
        // later record: the file ends after the last whole record.
        let whole = start + (bytes.len() - start) / RECORD_LEN * RECORD_LEN;
        if whole != bytes.len() {
            OpenOptions::new()
                .write(true)
                .open(path)
                .and_then(|file| file.set_len(whole as u64))
                .ok()?;
        }
        let bytes = &bytes[..whole];
        let mut keys = HashMap::new();
        let mut records = 0;
        for record in bytes[start..].as_chunks::<RECORD_LEN>().0 {
            records += 1;
            let Some((kind, id, key)) = open_record(&secret, record) else {
                continue;
            };
            if &kind == KEPT {
                keys.insert(id, key);
            } else {
                keys.remove(&id);
            }
        }
        Some(Self {
            path: path.to_path_buf(),
            secret,
            inner: Mutex::new(Inner {
                keys,
                records,
                file: None,
            }),
        })
    }

    /// Removes the account's key file (the setting was turned off). The
    /// protected file key goes with it.
    pub fn delete(folder: &Path) {
        Self::delete_with(folder, &Protection::platform());
    }

    fn delete_with(folder: &Path, protection: &Protection) {
        let path = Self::path(folder);
        if let Ok(bytes) = std::fs::read(&path)
            && let Some(protected) = protected_text(&bytes)
        {
            (protection.forget)(&protected);
        }
        let _ = std::fs::remove_file(path);
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map_or(0, |inner| inner.keys.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn append(&self, inner: &mut Inner, kind: [u8; 4], id: &Id, key: &[u8; 16]) {
        let record = seal_record(&self.secret, kind, id, key);
        if inner.file.is_none() {
            inner.file = OpenOptions::new().append(true).open(&self.path).ok();
        }
        let written = inner
            .file
            .as_mut()
            .is_some_and(|file| file.write_all(&record).is_ok());
        if written {
            inner.records += 1;
        } else {
            inner.file = None;
        }
        if inner.records > MAX_KEYS + MAX_KEYS / 4 {
            self.compact(inner);
        }
    }

    /// Rewrites the file with the newest [`MAX_KEYS`] kept keys.
    fn compact(&self, inner: &mut Inner) {
        inner.file = None;
        let Ok(bytes) = std::fs::read(&self.path) else {
            return;
        };
        let Some(start) = header_len(&bytes) else {
            return;
        };
        // The newest record of each kept key, newest first.
        let mut seen = std::collections::HashSet::new();
        let mut newest = Vec::new();
        for record in bytes[start..].as_chunks::<RECORD_LEN>().0.iter().rev() {
            if let Some((_, id, _)) = open_record(&self.secret, record)
                && seen.insert(id)
                && inner.keys.contains_key(&id)
            {
                newest.push((id, record));
                if newest.len() == MAX_KEYS {
                    break;
                }
            }
        }
        let mut rewritten = bytes[..start].to_vec();
        for (_, record) in newest.iter().rev() {
            rewritten.extend_from_slice(&record[..]);
        }
        if apricot_storage::json_file::write_bytes_atomic(&self.path, &rewritten).is_ok() {
            let kept: std::collections::HashSet<Id> = newest.iter().map(|(id, _)| *id).collect();
            inner.keys.retain(|id, _| kept.contains(id));
            inner.records = newest.len();
        }
    }
}

impl AudioKeyStore for KeyCache {
    fn get(&self, track: SpotifyId, file: FileId) -> Option<AudioKey> {
        let inner = self.inner.lock().ok()?;
        inner.keys.get(&id(track, file)).copied().map(AudioKey)
    }

    fn put(&self, track: SpotifyId, file: FileId, key: AudioKey) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let id = id(track, file);
        if inner.keys.get(&id) == Some(&key.0) {
            return;
        }
        inner.keys.insert(id, key.0);
        self.append(&mut inner, *KEPT, &id, &key.0);
    }

    fn forget(&self, track: SpotifyId, file: FileId) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        let id = id(track, file);
        if inner.keys.remove(&id).is_some() {
            self.append(&mut inner, *FORGOTTEN, &id, &[0; 16]);
        }
    }
}

/// How the file key is protected; tests use a plain stand-in.
struct Protection {
    protect: fn(&[u8]) -> Result<String, String>,
    unprotect: fn(&str) -> Option<Vec<u8>>,
    forget: fn(&str),
}

impl Protection {
    fn platform() -> Self {
        Self {
            protect: |secret| {
                apricot_platform::audiovault_credentials::protect_data(
                    secret,
                    ENTROPY,
                    "ApricotPlayer Spotify audio keys",
                )
            },
            unprotect: |text| {
                apricot_platform::audiovault_credentials::unprotect_data(text, ENTROPY)
            },
            forget: apricot_platform::audiovault_credentials::forget_protected,
        }
    }
}

fn header_len(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 12 || &bytes[..8] != MAGIC {
        return None;
    }
    let length = u32::from_le_bytes(bytes[8..12].try_into().ok()?) as usize;
    let end = 12usize.checked_add(length)?;
    (end <= bytes.len()).then_some(end)
}

fn protected_text(bytes: &[u8]) -> Option<String> {
    let end = header_len(bytes)?;
    String::from_utf8(bytes[12..end].to_vec()).ok()
}

fn header(bytes: &[u8], protection: &Protection) -> Option<([u8; 32], usize)> {
    let end = header_len(bytes)?;
    let protected = protected_text(bytes)?;
    let secret: [u8; 32] = (protection.unprotect)(&protected)?.try_into().ok()?;
    Some((secret, end))
}

fn seal_record(secret: &[u8; 32], kind: [u8; 4], id: &Id, key: &[u8; 16]) -> [u8; RECORD_LEN] {
    let mut record = [0; RECORD_LEN];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut record[..IV_LEN]);
    record[IV_LEN..IV_LEN + 36].copy_from_slice(id);
    record[IV_LEN + 36..IV_LEN + 52].copy_from_slice(key);
    record[IV_LEN + 52..].copy_from_slice(&kind);
    let (iv, plain) = record.split_at_mut(IV_LEN);
    Ctr::new_from_slices(secret, iv)
        .expect("AES-256 key and CTR block")
        .apply_keystream(plain);
    record
}

fn open_record(secret: &[u8; 32], record: &[u8]) -> Option<([u8; 4], Id, [u8; 16])> {
    let (iv, sealed) = record.split_at(IV_LEN);
    let mut plain = [0; PLAIN_LEN];
    plain.copy_from_slice(sealed);
    Ctr::new_from_slices(secret, iv)
        .expect("AES-256 key and CTR block")
        .apply_keystream(&mut plain);
    let kind: [u8; 4] = plain[52..].try_into().ok()?;
    if &kind != KEPT && &kind != FORGOTTEN {
        return None;
    }
    Some((
        kind,
        plain[..36].try_into().ok()?,
        plain[36..52].try_into().ok()?,
    ))
}

/// The key file's bytes (tests).
#[cfg(test)]
fn file_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("key file")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Protection {
        Protection {
            protect: |secret| {
                use std::fmt::Write as _;
                Ok(secret.iter().fold(String::new(), |mut text, byte| {
                    let _ = write!(text, "{byte:02x}");
                    text
                }))
            },
            unprotect: |text| {
                (0..text.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
                    .collect()
            },
            forget: |_| {},
        }
    }

    fn track(n: u128) -> SpotifyId {
        SpotifyId { id: n }
    }

    #[test]
    fn kept_keys_survive_a_restart_and_forgotten_keys_do_not() {
        let folder = tempfile::tempdir().unwrap();
        let cache = KeyCache::open_with(folder.path(), &plain()).unwrap();
        cache.put(track(1), FileId([1; 20]), AudioKey([7; 16]));
        cache.put(track(2), FileId([2; 20]), AudioKey([8; 16]));
        cache.put(track(2), FileId([2; 20]), AudioKey([8; 16]));
        cache.forget(track(1), FileId([1; 20]));
        drop(cache);
        let reopened = KeyCache::open_with(folder.path(), &plain()).unwrap();
        assert_eq!(reopened.get(track(1), FileId([1; 20])), None);
        assert_eq!(
            reopened.get(track(2), FileId([2; 20])),
            Some(AudioKey([8; 16]))
        );
        assert_eq!(reopened.len(), 1);
    }

    #[test]
    fn the_file_shows_neither_keys_nor_songs() {
        let folder = tempfile::tempdir().unwrap();
        let cache = KeyCache::open_with(folder.path(), &plain()).unwrap();
        cache.put(track(0x1234_5678), FileId([0xab; 20]), AudioKey([0xcd; 16]));
        let bytes = file_bytes(&KeyCache::path(folder.path()));
        let records = &bytes[header_len(&bytes).unwrap()..];
        assert_eq!(records.len(), RECORD_LEN);
        assert!(!records.windows(4).any(|w| w == [0xab; 4] || w == [0xcd; 4]));
        assert!(!records.windows(4).any(|w| w == KEPT));
    }

    #[test]
    fn a_file_this_user_cannot_decrypt_starts_over() {
        let folder = tempfile::tempdir().unwrap();
        let cache = KeyCache::open_with(folder.path(), &plain()).unwrap();
        cache.put(track(3), FileId([3; 20]), AudioKey([3; 16]));
        drop(cache);
        let foreign = Protection {
            unprotect: |_| None,
            ..plain()
        };
        let other = KeyCache::open_with(folder.path(), &foreign).unwrap();
        assert!(other.is_empty());
        other.put(track(4), FileId([4; 20]), AudioKey([4; 16]));
        drop(other);
        assert_eq!(
            KeyCache::open_with(folder.path(), &foreign)
                .map(|cache| cache.len())
                .unwrap(),
            0,
            "a new file key replaced the old one"
        );
    }

    #[test]
    fn compaction_keeps_the_newest_keys_within_the_limit() {
        let folder = tempfile::tempdir().unwrap();
        let cache = KeyCache::open_with(folder.path(), &plain()).unwrap();
        {
            let mut inner = cache.inner.lock().unwrap();
            for n in 0..10_u8 {
                let id = id(track(u128::from(n)), FileId([n; 20]));
                inner.keys.insert(id, [n; 16]);
                let record = seal_record(&cache.secret, *KEPT, &id, &[n; 16]);
                OpenOptions::new()
                    .append(true)
                    .open(&cache.path)
                    .unwrap()
                    .write_all(&record)
                    .unwrap();
                inner.records += 1;
            }
            // Pretend only the newest four fit.
            inner.keys.retain(|id, _| id[15] >= 6);
            cache.compact(&mut inner);
            assert_eq!(inner.records, 4);
        }
        drop(cache);
        let reopened = KeyCache::open_with(folder.path(), &plain()).unwrap();
        assert_eq!(reopened.len(), 4);
        assert_eq!(
            reopened.get(track(9), FileId([9; 20])),
            Some(AudioKey([9; 16]))
        );
        assert_eq!(reopened.get(track(5), FileId([5; 20])), None);
    }

    #[test]
    fn a_record_cut_short_is_dropped_and_later_records_stay_aligned() {
        let folder = tempfile::tempdir().unwrap();
        let cache = KeyCache::open_with(folder.path(), &plain()).unwrap();
        cache.put(track(6), FileId([6; 20]), AudioKey([6; 16]));
        drop(cache);
        let path = KeyCache::path(folder.path());
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&[1, 2, 3])
            .unwrap();
        let reopened = KeyCache::open_with(folder.path(), &plain()).unwrap();
        reopened.put(track(7), FileId([7; 20]), AudioKey([7; 16]));
        drop(reopened);
        let again = KeyCache::open_with(folder.path(), &plain()).unwrap();
        assert_eq!(again.len(), 2);
        assert_eq!(
            again.get(track(7), FileId([7; 20])),
            Some(AudioKey([7; 16]))
        );
    }

    /// A full cache: size on disk, compaction and reopening stay quick.
    #[test]
    #[ignore = "writes about 23 MB; run with --release"]
    fn a_full_cache_stays_within_its_size_and_reopens_quickly() {
        let folder = tempfile::tempdir().unwrap();
        let cache = KeyCache::open_with(folder.path(), &plain()).unwrap();
        let started = std::time::Instant::now();
        let total = MAX_KEYS + MAX_KEYS / 4 + 10;
        for n in 0..total {
            let n = u32::try_from(n).unwrap();
            let mut file = [0; 20];
            file[..4].copy_from_slice(&n.to_le_bytes());
            cache.put(track(u128::from(n)), FileId(file), AudioKey([1; 16]));
        }
        let filled = started.elapsed();
        drop(cache);
        let size = std::fs::metadata(KeyCache::path(folder.path())).unwrap().len();
        let started = std::time::Instant::now();
        let reopened = KeyCache::open_with(folder.path(), &plain()).unwrap();
        let opened = started.elapsed();
        println!(
            "{total} puts in {filled:?}, file {size} bytes, {} keys, reopened in {opened:?}",
            reopened.len()
        );
        assert!(reopened.len() <= MAX_KEYS + MAX_KEYS / 4);
        assert!(size <= 25_000_000);
        let mut newest = [0; 20];
        let last = u32::try_from(total - 1).unwrap();
        newest[..4].copy_from_slice(&last.to_le_bytes());
        assert!(reopened.get(track(u128::from(last)), FileId(newest)).is_some());
    }

    #[test]
    fn deleting_removes_the_file() {
        let folder = tempfile::tempdir().unwrap();
        let cache = KeyCache::open_with(folder.path(), &plain()).unwrap();
        cache.put(track(5), FileId([5; 20]), AudioKey([5; 16]));
        drop(cache);
        KeyCache::delete_with(folder.path(), &plain());
        assert!(!KeyCache::path(folder.path()).exists());
    }
}
