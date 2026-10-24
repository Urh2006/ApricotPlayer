use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    panic,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use thiserror::Error;

const DEFAULT_MAX_BYTES: u64 = 2 * 1024 * 1024;
const DEFAULT_RETAIN_BYTES: usize = 512 * 1024;

#[derive(Debug, Error)]
pub enum DiagnosticLogError {
    #[error("diagnostic log lock is poisoned")]
    Poisoned,
    #[error("diagnostic log operation failed for {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
}

#[derive(Debug)]
pub struct DiagnosticLog {
    path: PathBuf,
    max_bytes: u64,
    retain_bytes: usize,
    lock: Mutex<()>,
}

impl DiagnosticLog {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            max_bytes: DEFAULT_MAX_BYTES,
            retain_bytes: DEFAULT_RETAIN_BYTES,
            lock: Mutex::new(()),
        }
    }

    #[cfg(test)]
    fn with_limits(path: impl Into<PathBuf>, max_bytes: u64, retain_bytes: usize) -> Self {
        Self {
            path: path.into(),
            max_bytes,
            retain_bytes,
            lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one timestamped line after bounding retained log history.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticLogError`] when locking, directory creation,
    /// rotation, or appending fails.
    pub fn append(&self, message: &str) -> Result<(), DiagnosticLogError> {
        let _guard = self.lock.lock().map_err(|_| DiagnosticLogError::Poisoned)?;
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|source| Self::io_error(parent, source))?;
        self.rotate_if_needed()?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|source| Self::io_error(&self.path, source))?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        writeln!(file, "{timestamp} {}", single_line(message))
            .map_err(|source| Self::io_error(&self.path, source))
    }

    fn rotate_if_needed(&self) -> Result<(), DiagnosticLogError> {
        let Ok(metadata) = fs::metadata(&self.path) else {
            return Ok(());
        };
        if metadata.len() <= self.max_bytes {
            return Ok(());
        }
        let bytes = fs::read(&self.path).map_err(|source| Self::io_error(&self.path, source))?;
        let start = bytes.len().saturating_sub(self.retain_bytes);
        fs::write(&self.path, &bytes[start..]).map_err(|source| Self::io_error(&self.path, source))
    }

    fn io_error(path: &Path, source: io::Error) -> DiagnosticLogError {
        DiagnosticLogError::Io {
            path: path.to_owned(),
            source,
        }
    }
}

pub fn install_panic_hook(log: Arc<DiagnosticLog>) {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");
        let location = info.location().map_or_else(
            || "unknown location".to_owned(),
            |location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            },
        );
        let _ = log.append(&format!("panic at {location}: {payload}"));
        previous(info);
    }));
}

fn single_line(message: &str) -> String {
    message
        .chars()
        .map(|character| {
            if matches!(character, '\r' | '\n') {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::DiagnosticLog;

    #[test]
    fn diagnostic_log_is_single_line_and_bounded() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("rust.log");
        let log = DiagnosticLog::with_limits(&path, 32, 12);
        log.append("first\nline").expect("first");
        log.append("0123456789012345678901234567890123456789")
            .expect("large");
        log.append("last").expect("rotation");
        let output = std::fs::read_to_string(path).expect("log");
        assert!(!output.contains("first\nline"));
        assert!(output.contains("first line") || output.contains("last"));
        assert!(output.contains("last"));
        assert!(output.len() < 100);
    }
}
