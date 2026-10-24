//! Transactional resume positions bound to durable media identities.

use std::path::PathBuf;

use apricot_core::{MediaItem, MediaKind};
use apricot_storage::{PlaybackPositionFile, PlaybackPositionFileError};
use serde_json::{Map, Value};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackPositionUpdate {
    Unchanged,
    Saved,
    Cleared,
}

#[derive(Debug, Error)]
pub enum PlaybackPositionControllerError {
    #[error("playback position changes are blocked because loading {path} failed: {message}")]
    SaveBlocked { path: PathBuf, message: String },
    #[error(transparent)]
    Storage(#[from] PlaybackPositionFileError),
}

#[derive(Debug, Default)]
pub struct PlaybackPositionController {
    positions: Map<String, Value>,
    file: Option<PlaybackPositionFile>,
    load_error: Option<String>,
    save_blocked: bool,
}

impl PlaybackPositionController {
    pub fn load(current: PlaybackPositionFile, legacy: &PlaybackPositionFile) -> Self {
        if current.path().is_file() {
            return match current.load() {
                Ok(positions) => Self::loaded(current, positions),
                Err(error) => Self::blocked(current, error.to_string()),
            };
        }
        if legacy.path().is_file() {
            return match legacy.load() {
                Ok(positions) => Self::loaded(current, positions),
                Err(error) => Self {
                    positions: Map::new(),
                    file: Some(current),
                    load_error: Some(error.to_string()),
                    save_blocked: false,
                },
            };
        }
        Self::loaded(current, Map::new())
    }

    fn loaded(file: PlaybackPositionFile, positions: Map<String, Value>) -> Self {
        Self {
            positions,
            file: Some(file),
            load_error: None,
            save_blocked: false,
        }
    }

    fn blocked(file: PlaybackPositionFile, message: String) -> Self {
        Self {
            positions: Map::new(),
            file: Some(file),
            load_error: Some(message),
            save_blocked: true,
        }
    }

    pub fn positions(&self) -> &Map<String, Value> {
        &self.positions
    }

    pub fn resume_position(&self, item: &MediaItem, enabled: bool) -> Option<f64> {
        if !enabled || item.kind == MediaKind::LiveStream {
            return None;
        }
        let key = playback_key(item)?;
        self.positions
            .get(&key)
            .and_then(flexible_position)
            .filter(|position| position.is_finite() && *position >= 5.0)
    }

    /// Stores or clears one item's resume position using Python's five-second
    /// start and eight-second near-end thresholds.
    ///
    /// # Errors
    ///
    /// Returns an error when a changed position map cannot be persisted.
    pub fn update(
        &mut self,
        item: &MediaItem,
        position: f64,
        duration: Option<f64>,
        enabled: bool,
    ) -> Result<PlaybackPositionUpdate, PlaybackPositionControllerError> {
        if !enabled || !position.is_finite() {
            return Ok(PlaybackPositionUpdate::Unchanged);
        }
        let Some(key) = playback_key(item) else {
            return Ok(PlaybackPositionUpdate::Unchanged);
        };
        let position = position.max(0.0);
        let duration = duration.filter(|value| value.is_finite() && *value >= 0.0);
        let should_clear = item.kind == MediaKind::LiveStream
            || position < 5.0
            || duration.is_some_and(|total| position > 5.0_f64.max(total - 8.0));
        let mut candidate = self.positions.clone();
        let outcome = if should_clear {
            if candidate.remove(&key).is_none() {
                return Ok(PlaybackPositionUpdate::Unchanged);
            }
            PlaybackPositionUpdate::Cleared
        } else {
            let rounded = round_tenth(position);
            if candidate.get(&key).and_then(flexible_position) == Some(rounded) {
                return Ok(PlaybackPositionUpdate::Unchanged);
            }
            candidate.insert(key, Value::from(rounded));
            PlaybackPositionUpdate::Saved
        };
        self.commit(candidate)?;
        Ok(outcome)
    }

    fn commit(
        &mut self,
        candidate: Map<String, Value>,
    ) -> Result<(), PlaybackPositionControllerError> {
        if self.save_blocked {
            let file = self.file.as_ref().expect("blocked positions have a file");
            return Err(PlaybackPositionControllerError::SaveBlocked {
                path: file.path().to_path_buf(),
                message: self
                    .load_error
                    .clone()
                    .unwrap_or_else(|| "unknown load error".to_owned()),
            });
        }
        if let Some(file) = &self.file {
            file.save(&candidate)?;
        }
        self.positions = candidate;
        Ok(())
    }
}

fn playback_key(item: &MediaItem) -> Option<String> {
    item.copy_location()
}

fn flexible_position(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
        .filter(|position| position.is_finite() && *position >= 0.0)
}

fn round_tenth(value: f64) -> f64 {
    (value.max(0.0) * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_storage::PlaybackPositionFile;
    use tempfile::tempdir;

    use super::{PlaybackPositionController, PlaybackPositionUpdate};

    fn item(id: &str) -> MediaItem {
        MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: id.to_owned(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: Some(format!(r"C:\Music\{id}.mp3")),
            channel: String::new(),
            duration_seconds: Some(100.0),
            metadata: BTreeMap::new(),
        }
    }

    fn controller(path: &std::path::Path) -> PlaybackPositionController {
        PlaybackPositionController::load(
            PlaybackPositionFile::new(path),
            &PlaybackPositionFile::new(path.with_extension("missing")),
        )
    }

    #[test]
    fn positions_are_independent_and_follow_start_and_end_thresholds() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("playback_positions.json");
        let mut positions = controller(&path);
        let first = item("first");
        let second = item("second");
        assert_eq!(
            positions
                .update(&first, 12.26, Some(100.0), true)
                .expect("save"),
            PlaybackPositionUpdate::Saved
        );
        assert_eq!(
            positions
                .update(&second, 32.0, Some(100.0), true)
                .expect("save"),
            PlaybackPositionUpdate::Saved
        );
        assert_eq!(positions.resume_position(&first, true), Some(12.3));
        assert_eq!(positions.resume_position(&second, true), Some(32.0));
        assert_eq!(
            positions
                .update(&first, 94.0, Some(100.0), true)
                .expect("clear"),
            PlaybackPositionUpdate::Cleared
        );
        assert_eq!(positions.resume_position(&first, true), None);
        assert_eq!(positions.resume_position(&second, true), Some(32.0));
    }

    #[test]
    fn disabled_resume_does_not_change_existing_positions() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("playback_positions.json");
        let mut positions = controller(&path);
        let track = item("track");
        positions.update(&track, 20.0, None, true).expect("seed");
        assert_eq!(
            positions
                .update(&track, 40.0, None, false)
                .expect("disabled"),
            PlaybackPositionUpdate::Unchanged
        );
        assert_eq!(positions.resume_position(&track, false), None);
        assert_eq!(positions.resume_position(&track, true), Some(20.0));
    }

    #[test]
    fn corrupt_current_file_blocks_mutation_and_preserves_bytes() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("playback_positions.json");
        fs::write(&path, b"broken").expect("fixture");
        let mut positions = controller(&path);
        assert!(positions.update(&item("track"), 20.0, None, true).is_err());
        assert_eq!(fs::read(path).expect("preserved"), b"broken");
    }

    #[test]
    fn updating_one_item_preserves_unrelated_unknown_values() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("playback_positions.json");
        fs::write(
            &path,
            br#"{"future":{"seconds":9},"legacy":"not a number"}"#,
        )
        .expect("fixture");
        let mut positions = controller(&path);
        positions
            .update(&item("track"), 20.0, None, true)
            .expect("save target");

        let saved = PlaybackPositionFile::new(&path).load().expect("saved file");
        assert_eq!(saved["future"]["seconds"], 9);
        assert_eq!(saved["legacy"], "not a number");
        assert_eq!(saved[r"C:\Music\track.mp3"], 20.0);
    }
}
