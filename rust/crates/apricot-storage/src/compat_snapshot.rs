//! Lossless compatibility snapshots for serialized Python app data.

use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
};

use serde_json::Value;
use thiserror::Error;

use crate::{DATA_ENTRIES, DataShape, JsonFileError, write_bytes_atomic, write_json_atomic};

pub const DEFAULT_MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactSource {
    Current,
    Legacy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SnapshotValue {
    Json(Value),
    Text(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactSnapshot {
    pub id: &'static str,
    pub relative_path: &'static str,
    pub source: ArtifactSource,
    pub value: SnapshotValue,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CompatibilitySnapshot {
    pub artifacts: Vec<ArtifactSnapshot>,
}

#[derive(Debug, Error)]
pub enum CompatibilitySnapshotError {
    #[error("could not inspect {path}: {source}")]
    Inspect { path: PathBuf, source: io::Error },
    #[error("could not read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("{id} exceeds the {max_bytes}-byte compatibility limit")]
    TooLarge { id: &'static str, max_bytes: usize },
    #[error("could not parse {id} as JSON: {source}")]
    Parse {
        id: &'static str,
        source: serde_json::Error,
    },
    #[error("{id} must contain a JSON {expected}")]
    Shape {
        id: &'static str,
        expected: &'static str,
    },
    #[error("could not write {id}: {source}")]
    Write {
        id: &'static str,
        source: JsonFileError,
    },
}

impl CompatibilitySnapshot {
    /// Loads every existing serialized compatibility artifact.
    ///
    /// Missing files are valid. A current file takes precedence over its legacy
    /// counterpart, and invalid current data is never hidden by a legacy copy.
    ///
    /// # Errors
    ///
    /// Returns [`CompatibilitySnapshotError`] for inaccessible, oversized,
    /// malformed, or shape-incompatible artifacts.
    pub fn load(
        app_data_directory: &Path,
        max_artifact_bytes: usize,
    ) -> Result<Self, CompatibilitySnapshotError> {
        let app_data_parent = app_data_directory
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let mut artifacts = Vec::new();

        for entry in DATA_ENTRIES
            .iter()
            .filter(|entry| entry.is_compatibility_snapshot_candidate())
        {
            let current = entry.path_in(app_data_directory);
            let selected = if path_exists(&current)? {
                Some((current, ArtifactSource::Current))
            } else if let Some(legacy) = entry.legacy_path_in(app_data_parent) {
                path_exists(&legacy)?.then_some((legacy, ArtifactSource::Legacy))
            } else {
                None
            };
            let Some((path, source)) = selected else {
                continue;
            };
            let bytes = read_bounded(&path, entry.id, max_artifact_bytes)?;
            let value = decode_value(entry.id, entry.shape, &bytes)?;
            artifacts.push(ArtifactSnapshot {
                id: entry.id,
                relative_path: entry.relative_path,
                source,
                value,
            });
        }

        Ok(Self { artifacts })
    }

    /// Writes a snapshot into a separate app-data directory.
    ///
    /// JSON formatting may change, but its complete value is preserved. Text
    /// artifacts, including cookies, are preserved byte for byte.
    ///
    /// # Errors
    ///
    /// Returns [`CompatibilitySnapshotError`] if any atomic write fails.
    pub fn write_to(&self, app_data_directory: &Path) -> Result<(), CompatibilitySnapshotError> {
        for artifact in &self.artifacts {
            let path = app_data_directory.join(artifact.relative_path);
            let result = match &artifact.value {
                SnapshotValue::Json(value) => write_json_atomic(&path, value),
                SnapshotValue::Text(bytes) => write_bytes_atomic(&path, bytes),
            };
            result.map_err(|source| CompatibilitySnapshotError::Write {
                id: artifact.id,
                source,
            })?;
        }
        Ok(())
    }
}

fn path_exists(path: &Path) -> Result<bool, CompatibilitySnapshotError> {
    path.try_exists()
        .map_err(|source| CompatibilitySnapshotError::Inspect {
            path: path.to_owned(),
            source,
        })
}

fn read_bounded(
    path: &Path,
    id: &'static str,
    max_bytes: usize,
) -> Result<Vec<u8>, CompatibilitySnapshotError> {
    let file = File::open(path).map_err(|source| CompatibilitySnapshotError::Read {
        path: path.to_owned(),
        source,
    })?;
    let read_limit = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX - 1)
        .saturating_add(1);
    let mut bytes = Vec::new();
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|source| CompatibilitySnapshotError::Read {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() > max_bytes {
        return Err(CompatibilitySnapshotError::TooLarge { id, max_bytes });
    }
    Ok(bytes)
}

fn decode_value(
    id: &'static str,
    shape: DataShape,
    bytes: &[u8],
) -> Result<SnapshotValue, CompatibilitySnapshotError> {
    match shape {
        DataShape::JsonObject | DataShape::JsonArray => {
            let json_bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
            let value: Value = serde_json::from_slice(json_bytes)
                .map_err(|source| CompatibilitySnapshotError::Parse { id, source })?;
            let valid = match shape {
                DataShape::JsonObject => value.is_object(),
                DataShape::JsonArray => value.is_array(),
                DataShape::Text | DataShape::Directory => unreachable!(),
            };
            if !valid {
                return Err(CompatibilitySnapshotError::Shape {
                    id,
                    expected: match shape {
                        DataShape::JsonObject => "object",
                        DataShape::JsonArray => "array",
                        DataShape::Text | DataShape::Directory => unreachable!(),
                    },
                });
            }
            Ok(SnapshotValue::Json(value))
        }
        DataShape::Text => Ok(SnapshotValue::Text(bytes.to_vec())),
        DataShape::Directory => unreachable!("directories are not snapshot candidates"),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use tempfile::tempdir;

    use super::{ArtifactSource, CompatibilitySnapshot, CompatibilitySnapshotError, SnapshotValue};

    #[test]
    fn current_and_legacy_artifacts_are_loaded_without_content_loss() {
        let root = tempdir().expect("root");
        let current = root.path().join("ApricotPlayer");
        let legacy = root.path().join("UrhasaurusYouTubePlayer");
        fs::create_dir_all(&current).expect("current directory");
        fs::create_dir_all(&legacy).expect("legacy directory");
        fs::write(current.join("history.json"), br#"[{"title":"current"}]"#)
            .expect("current history");
        fs::write(legacy.join("favorites.json"), br#"[{"title":"legacy"}]"#)
            .expect("legacy favorites");
        fs::write(current.join("cookies.txt"), [0, 255, 10, 13]).expect("cookies");

        let snapshot = CompatibilitySnapshot::load(&current, 1024).expect("snapshot");
        let sources: BTreeMap<_, _> = snapshot
            .artifacts
            .iter()
            .map(|artifact| (artifact.id, artifact.source))
            .collect();
        assert_eq!(sources["history"], ArtifactSource::Current);
        assert_eq!(sources["favorites"], ArtifactSource::Legacy);
        let cookies = snapshot
            .artifacts
            .iter()
            .find(|artifact| artifact.id == "cached_cookies")
            .expect("cookies");
        assert_eq!(cookies.value, SnapshotValue::Text(vec![0, 255, 10, 13]));
    }

    #[test]
    fn round_trip_preserves_json_values_and_exact_text_bytes() {
        let root = tempdir().expect("root");
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir_all(&source).expect("source directory");
        fs::write(
            source.join("playback_positions.json"),
            br#"{"video":12.5,"nested":{"future":true}}"#,
        )
        .expect("positions");
        fs::write(source.join("download-archive.txt"), b"one\r\ntwo\n").expect("archive");

        let first = CompatibilitySnapshot::load(&source, 1024).expect("first");
        first.write_to(&target).expect("write");
        let second = CompatibilitySnapshot::load(&target, 1024).expect("second");
        let first_values: BTreeMap<_, _> = first
            .artifacts
            .iter()
            .map(|artifact| (artifact.id, &artifact.value))
            .collect();
        let second_values: BTreeMap<_, _> = second
            .artifacts
            .iter()
            .map(|artifact| (artifact.id, &artifact.value))
            .collect();
        assert_eq!(first_values, second_values);
    }

    #[test]
    fn malformed_shape_is_rejected_instead_of_silently_reset() {
        let root = tempdir().expect("root");
        fs::write(root.path().join("history.json"), br#"{"wrong":"shape"}"#).expect("history");
        assert!(matches!(
            CompatibilitySnapshot::load(root.path(), 1024),
            Err(CompatibilitySnapshotError::Shape { id: "history", .. })
        ));
    }

    #[test]
    fn oversized_artifact_is_bounded_before_parsing() {
        let root = tempdir().expect("root");
        fs::write(root.path().join("history.json"), b"[123456789]").expect("history");
        assert!(matches!(
            CompatibilitySnapshot::load(root.path(), 4),
            Err(CompatibilitySnapshotError::TooLarge { id: "history", .. })
        ));
    }

    #[test]
    fn utf8_bom_json_from_existing_windows_files_is_accepted() {
        let root = tempdir().expect("root");
        fs::write(
            root.path().join("playback_queue.json"),
            b"\xEF\xBB\xBF[]\r\n",
        )
        .expect("queue");
        let snapshot = CompatibilitySnapshot::load(root.path(), 1024).expect("snapshot");
        let queue = snapshot
            .artifacts
            .iter()
            .find(|artifact| artifact.id == "playback_queue")
            .expect("queue");
        assert_eq!(queue.value, SnapshotValue::Json(serde_json::json!([])));
    }
}
