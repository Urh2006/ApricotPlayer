//! In-memory last-session state with ordered background persistence.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
};

use apricot_storage::{LastPlayerSession, LastPlayerSessionFile};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LastPlayerSessionControllerError {
    #[error("last player session changes are blocked because loading {path} failed: {message}")]
    SaveBlocked { path: PathBuf, message: String },
    #[error("last player session writer stopped unexpectedly")]
    WriterStopped,
}

#[derive(Debug)]
struct SessionWriter {
    sender: Option<mpsc::Sender<LastPlayerSession>>,
    worker: Option<JoinHandle<()>>,
    last_error: Arc<Mutex<Option<String>>>,
}

impl SessionWriter {
    fn start(file: LastPlayerSessionFile) -> Self {
        let (sender, receiver) = mpsc::channel::<LastPlayerSession>();
        let last_error = Arc::new(Mutex::new(None));
        let worker_error = Arc::clone(&last_error);
        let worker = thread::Builder::new()
            .name("apricot-last-session-writer".to_owned())
            .spawn(move || {
                while let Ok(mut snapshot) = receiver.recv() {
                    while let Ok(newer) = receiver.try_recv() {
                        snapshot = newer;
                    }
                    let result = file.save(&snapshot).map_err(|error| error.to_string());
                    if let Ok(mut current) = worker_error.lock() {
                        *current = result.err();
                    }
                }
            })
            .expect("last player session writer thread must start");
        Self {
            sender: Some(sender),
            worker: Some(worker),
            last_error,
        }
    }

    fn save(&self, snapshot: LastPlayerSession) -> Result<(), LastPlayerSessionControllerError> {
        self.sender
            .as_ref()
            .ok_or(LastPlayerSessionControllerError::WriterStopped)?
            .send(snapshot)
            .map_err(|_| LastPlayerSessionControllerError::WriterStopped)
    }

    fn last_error(&self) -> Option<String> {
        self.last_error.lock().ok().and_then(|error| error.clone())
    }
}

impl Drop for SessionWriter {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[derive(Debug, Default)]
pub struct LastPlayerSessionController {
    session: Option<LastPlayerSession>,
    current_path: Option<PathBuf>,
    load_error: Option<String>,
    save_blocked: bool,
    writer: Option<SessionWriter>,
}

impl LastPlayerSessionController {
    pub fn load(current: LastPlayerSessionFile, legacy: &LastPlayerSessionFile) -> Self {
        let current_path = current.path().to_path_buf();
        if current.path().is_file() {
            return match current.load() {
                Ok(session) => Self::loaded(current, session),
                Err(error) => Self {
                    session: None,
                    current_path: Some(current_path),
                    load_error: Some(error.to_string()),
                    save_blocked: true,
                    writer: None,
                },
            };
        }
        if legacy.path().is_file() {
            return match legacy.load() {
                Ok(session) => Self::loaded(current, session),
                Err(error) => Self {
                    session: None,
                    current_path: Some(current_path),
                    load_error: Some(error.to_string()),
                    save_blocked: false,
                    writer: Some(SessionWriter::start(current)),
                },
            };
        }
        Self::loaded(current, None)
    }

    fn loaded(file: LastPlayerSessionFile, session: Option<LastPlayerSession>) -> Self {
        Self {
            session,
            current_path: Some(file.path().to_path_buf()),
            load_error: None,
            save_blocked: false,
            writer: Some(SessionWriter::start(file)),
        }
    }

    pub fn session(&self) -> Option<&LastPlayerSession> {
        self.session
            .as_ref()
            .filter(|session| session.is_available())
    }

    pub fn is_available(&self) -> bool {
        self.session().is_some()
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub fn write_error(&self) -> Option<String> {
        self.writer.as_ref().and_then(SessionWriter::last_error)
    }

    /// Replaces the current snapshot immediately in memory and queues its
    /// durable write without blocking the UI thread.
    ///
    /// # Errors
    ///
    /// Returns an error when corrupt current data blocks writes or the ordered
    /// background writer has stopped.
    pub fn replace(
        &mut self,
        session: LastPlayerSession,
    ) -> Result<(), LastPlayerSessionControllerError> {
        if self.save_blocked {
            return Err(LastPlayerSessionControllerError::SaveBlocked {
                path: self.current_path.clone().unwrap_or_default(),
                message: self
                    .load_error
                    .clone()
                    .unwrap_or_else(|| "unknown load error".to_owned()),
            });
        }
        let writer = self
            .writer
            .as_ref()
            .ok_or(LastPlayerSessionControllerError::WriterStopped)?;
        writer.save(session.clone())?;
        self.session = Some(session);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_storage::{LastPlayerSession, LastPlayerSessionFile};
    use serde_json::Map;
    use tempfile::tempdir;

    use super::LastPlayerSessionController;

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
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn stable_snapshot_is_read_without_modifying_stable_data() {
        let root = tempdir().expect("temporary directory");
        let current_path = root.path().join("beta/last_player_session.json");
        let legacy_path = root.path().join("stable/last_player_session.json");
        let legacy_file = LastPlayerSessionFile::new(&legacy_path);
        legacy_file
            .save(&LastPlayerSession::new(
                1.0,
                item("legacy"),
                "local_file",
                Map::new(),
                Vec::new(),
            ))
            .expect("legacy fixture");
        let original = fs::read(&legacy_path).expect("legacy bytes");
        let mut controller = LastPlayerSessionController::load(
            LastPlayerSessionFile::new(&current_path),
            &legacy_file,
        );
        assert_eq!(
            controller.session().expect("legacy session").item.title,
            "legacy"
        );
        controller
            .replace(LastPlayerSession::new(
                2.0,
                item("current"),
                "local_file",
                Map::new(),
                Vec::new(),
            ))
            .expect("queue current snapshot");
        drop(controller);
        assert_eq!(fs::read(legacy_path).expect("legacy preserved"), original);
        assert_eq!(
            LastPlayerSessionFile::new(current_path)
                .load()
                .expect("current load")
                .expect("current session")
                .item
                .title,
            "current"
        );
    }

    #[test]
    fn rapid_replacements_are_persisted_in_order_with_the_latest_winning() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("last_player_session.json");
        let mut controller = LastPlayerSessionController::load(
            LastPlayerSessionFile::new(&path),
            &LastPlayerSessionFile::new(root.path().join("missing.json")),
        );
        for index in 0..20 {
            controller
                .replace(LastPlayerSession::new(
                    f64::from(index),
                    item(&index.to_string()),
                    "local_file",
                    Map::new(),
                    Vec::new(),
                ))
                .expect("queue snapshot");
        }
        drop(controller);
        assert_eq!(
            LastPlayerSessionFile::new(path)
                .load()
                .expect("load")
                .expect("session")
                .item
                .title,
            "19"
        );
    }

    #[test]
    fn corrupt_current_snapshot_blocks_replacement_and_preserves_bytes() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("last_player_session.json");
        fs::write(&path, b"broken").expect("fixture");
        let mut controller = LastPlayerSessionController::load(
            LastPlayerSessionFile::new(&path),
            &LastPlayerSessionFile::new(root.path().join("missing.json")),
        );
        assert!(
            controller
                .replace(LastPlayerSession::new(
                    1.0,
                    item("new"),
                    "local_file",
                    Map::new(),
                    Vec::new(),
                ))
                .is_err()
        );
        assert_eq!(fs::read(path).expect("preserved"), b"broken");
    }
}
