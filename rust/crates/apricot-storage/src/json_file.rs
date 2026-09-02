use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JsonFileError {
    #[error("could not read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("could not parse {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("could not write {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("could not serialize data: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Flushes bytes to a sibling temporary file and replaces the destination.
///
/// # Errors
///
/// Returns [`JsonFileError`] when the directory or file cannot be written,
/// flushed, synchronized, or replaced.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), JsonFileError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| JsonFileError::Write {
        path: parent.to_owned(),
        source,
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("data");
    let temporary = parent.join(format!(".{file_name}.tmp"));
    let file = File::create(&temporary).map_err(|source| JsonFileError::Write {
        path: temporary.clone(),
        source,
    })?;
    let mut writer = BufWriter::new(file);
    writer
        .write_all(bytes)
        .map_err(|source| JsonFileError::Write {
            path: temporary.clone(),
            source,
        })?;
    writer.flush().map_err(|source| JsonFileError::Write {
        path: temporary.clone(),
        source,
    })?;
    writer
        .get_ref()
        .sync_all()
        .map_err(|source| JsonFileError::Write {
            path: temporary.clone(),
            source,
        })?;
    fs::rename(&temporary, path).map_err(|source| JsonFileError::Write {
        path: path.to_owned(),
        source,
    })
}

/// Reads and deserializes one JSON document from `path`.
///
/// # Errors
///
/// Returns [`JsonFileError`] when the file cannot be read or does not contain
/// valid JSON for `T`.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, JsonFileError> {
    let bytes = fs::read(path).map_err(|source| JsonFileError::Read {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| JsonFileError::Parse {
        path: path.to_owned(),
        source,
    })
}

/// Serializes `value`, flushes it to a sibling temporary file, and replaces
/// `path` only after the complete document reaches storage.
///
/// # Errors
///
/// Returns [`JsonFileError`] when serialization, directory creation, writing,
/// syncing, or replacement fails.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), JsonFileError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_bytes_atomic(path, &bytes)
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    use super::{read_json, write_bytes_atomic, write_json_atomic};

    #[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
    struct Fixture {
        title: String,
        count: usize,
    }

    #[test]
    fn atomic_round_trip_replaces_complete_json() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("settings.json");
        let expected = Fixture {
            title: "Apricot".to_owned(),
            count: 116,
        };
        let obsolete = Fixture {
            title: "Python baseline".to_owned(),
            count: 1,
        };
        write_json_atomic(&path, &obsolete).expect("initial write");
        write_json_atomic(&path, &expected).expect("write");
        assert_eq!(read_json::<Fixture>(&path).expect("read"), expected);
        assert!(!directory.path().join(".settings.json.tmp").exists());
    }

    #[test]
    fn byte_backups_use_a_distinct_temporary_name() {
        let directory = tempdir().expect("temporary directory");
        let primary = directory.path().join("settings.json");
        let backup = directory.path().join("settings.json.bak");
        write_bytes_atomic(&primary, b"primary").expect("primary");
        write_bytes_atomic(&backup, b"backup").expect("backup");
        assert_eq!(std::fs::read(primary).expect("read primary"), b"primary");
        assert_eq!(std::fs::read(backup).expect("read backup"), b"backup");
    }
}
