//! Python-compatible durable playback positions.

use std::{
    io,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value};
use thiserror::Error;

use crate::{JsonFileError, read_json, write_json_atomic};

#[derive(Debug, Error)]
pub enum PlaybackPositionFileError {
    #[error(transparent)]
    Json(#[from] JsonFileError),
    #[error("playback positions data is not a JSON object")]
    InvalidRoot,
}

#[derive(Clone, Debug)]
pub struct PlaybackPositionFile {
    path: PathBuf,
}

impl PlaybackPositionFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the Python identity-to-seconds object without discarding entries.
    /// A missing file represents an empty position map.
    ///
    /// # Errors
    ///
    /// Returns an error when the root is not an object. Individual values are
    /// retained losslessly so one legacy or future entry cannot discard the
    /// rest of the Python-compatible file.
    pub fn load(&self) -> Result<Map<String, Value>, PlaybackPositionFileError> {
        let value = match read_json::<Value>(&self.path) {
            Ok(value) => value,
            Err(JsonFileError::Read { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(Map::new());
            }
            Err(error) => return Err(error.into()),
        };
        let Value::Object(object) = value else {
            return Err(PlaybackPositionFileError::InvalidRoot);
        };
        Ok(object)
    }

    /// Atomically saves the complete Python-compatible position object.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or atomic replacement fails.
    pub fn save(&self, positions: &Map<String, Value>) -> Result<(), PlaybackPositionFileError> {
        write_json_atomic(&self.path, positions)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::PlaybackPositionFile;

    #[test]
    fn python_values_are_retained_without_identity_or_type_changes() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("playback_positions.json");
        fs::write(
            &path,
            br#"{"C:\\Music\\One.mp3":"12.26","https://media.test/two":42,"future":{"seconds":9}}"#,
        )
        .expect("fixture");
        let file = PlaybackPositionFile::new(&path);
        let positions = file.load().expect("positions");
        assert_eq!(positions[r"C:\Music\One.mp3"], "12.26");
        assert_eq!(positions["https://media.test/two"], 42);
        assert_eq!(positions["future"]["seconds"], 9);
        file.save(&positions).expect("save");
        assert_eq!(file.load().expect("reload"), positions);
    }

    #[test]
    fn invalid_individual_position_survives_round_trip() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("playback_positions.json");
        let bytes = br#"{"track":"not a number"}"#;
        fs::write(&path, bytes).expect("fixture");
        let file = PlaybackPositionFile::new(&path);
        let positions = file.load().expect("lossless positions");
        assert_eq!(positions["track"], "not a number");
        file.save(&positions).expect("save");
        assert_eq!(file.load().expect("reload"), positions);
    }
}
