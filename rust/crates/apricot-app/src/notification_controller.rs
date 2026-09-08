//! Transactional notification-center state.

use std::path::PathBuf;

use apricot_storage::{AppNotification, NotificationFile, NotificationFileError};
use thiserror::Error;

const MAX_NOTIFICATIONS: usize = 200;

#[derive(Debug, Error)]
pub enum NotificationControllerError {
    #[error("notification changes are blocked because loading {path} failed: {message}")]
    SaveBlocked { path: PathBuf, message: String },
    #[error(transparent)]
    Storage(#[from] NotificationFileError),
}

#[derive(Debug, Default)]
pub struct NotificationController {
    notifications: Vec<AppNotification>,
    file: Option<NotificationFile>,
    load_error: Option<String>,
    save_blocked: bool,
}

impl NotificationController {
    pub fn load(current: NotificationFile, legacy: &NotificationFile) -> Self {
        if current.path().is_file() {
            return match current.load() {
                Ok(notifications) => Self::loaded(current, notifications),
                Err(error) => Self::blocked(current, error.to_string()),
            };
        }
        if legacy.path().is_file() {
            return match legacy.load() {
                Ok(notifications) => Self::loaded(current, notifications),
                Err(error) => Self {
                    notifications: Vec::new(),
                    file: Some(current),
                    load_error: Some(error.to_string()),
                    save_blocked: false,
                },
            };
        }
        Self::loaded(current, Vec::new())
    }

    fn loaded(file: NotificationFile, notifications: Vec<AppNotification>) -> Self {
        Self {
            notifications,
            file: Some(file),
            load_error: None,
            save_blocked: false,
        }
    }

    fn blocked(file: NotificationFile, message: String) -> Self {
        Self {
            notifications: Vec::new(),
            file: Some(file),
            load_error: Some(message),
            save_blocked: true,
        }
    }

    pub fn notifications(&self) -> &[AppNotification] {
        &self.notifications
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Inserts one newest-first entry and applies Python's 200-entry bound.
    ///
    /// # Errors
    ///
    /// Returns an error when the complete replacement cannot be persisted.
    pub fn add(
        &mut self,
        notification: AppNotification,
    ) -> Result<(), NotificationControllerError> {
        self.ensure_writable()?;
        let mut candidate = self.notifications.clone();
        candidate.insert(0, notification);
        candidate.truncate(MAX_NOTIFICATIONS);
        self.commit(candidate)
    }

    /// Removes one displayed notification.
    ///
    /// # Errors
    ///
    /// Returns an error when the complete replacement cannot be persisted.
    pub fn remove(
        &mut self,
        index: usize,
    ) -> Result<Option<AppNotification>, NotificationControllerError> {
        self.ensure_writable()?;
        if index >= self.notifications.len() {
            return Ok(None);
        }
        let mut candidate = self.notifications.clone();
        let removed = candidate.remove(index);
        self.commit(candidate)?;
        Ok(Some(removed))
    }

    /// Clears the notification center.
    ///
    /// # Errors
    ///
    /// Returns an error when the empty list cannot be persisted.
    pub fn clear(&mut self) -> Result<bool, NotificationControllerError> {
        self.ensure_writable()?;
        if self.notifications.is_empty() {
            return Ok(false);
        }
        self.commit(Vec::new())?;
        Ok(true)
    }

    fn commit(
        &mut self,
        candidate: Vec<AppNotification>,
    ) -> Result<(), NotificationControllerError> {
        self.ensure_writable()?;
        if let Some(file) = &self.file {
            file.save(&candidate)?;
        }
        self.notifications = candidate;
        Ok(())
    }

    fn ensure_writable(&self) -> Result<(), NotificationControllerError> {
        if self.save_blocked {
            return Err(NotificationControllerError::SaveBlocked {
                path: self
                    .file
                    .as_ref()
                    .map(|file| file.path().to_path_buf())
                    .unwrap_or_default(),
                message: self
                    .load_error
                    .clone()
                    .unwrap_or_else(|| "unknown load error".to_owned()),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use apricot_storage::{AppNotification, NotificationFile};
    use tempfile::tempdir;

    use super::NotificationController;

    fn notification(index: usize) -> AppNotification {
        AppNotification::new(
            "info",
            format!("Title {index}"),
            "Message",
            None,
            f64::from(u32::try_from(index).expect("test index fits")),
        )
    }

    #[test]
    fn stable_data_is_read_only_and_mutations_target_beta() {
        let root = tempdir().expect("temporary directory");
        let current_path = root.path().join("beta/notifications.json");
        let legacy_path = root.path().join("stable/notifications.json");
        let legacy = NotificationFile::new(&legacy_path);
        legacy.save(&[notification(1)]).expect("legacy fixture");
        let original = fs::read(&legacy_path).expect("legacy bytes");
        let mut controller =
            NotificationController::load(NotificationFile::new(&current_path), &legacy);
        controller.add(notification(2)).expect("add");
        assert_eq!(controller.notifications()[0].title, "Title 2");
        assert_eq!(fs::read(legacy_path).expect("legacy preserved"), original);
        assert_eq!(
            NotificationFile::new(current_path)
                .load()
                .expect("beta load")
                .len(),
            2
        );
    }

    #[test]
    fn entries_are_newest_first_and_bounded_like_python() {
        let root = tempdir().expect("temporary directory");
        let mut controller = NotificationController::load(
            NotificationFile::new(root.path().join("notifications.json")),
            &NotificationFile::new(root.path().join("missing.json")),
        );
        for index in 0..205 {
            controller.add(notification(index)).expect("add");
        }
        assert_eq!(controller.notifications().len(), 200);
        assert_eq!(controller.notifications()[0].title, "Title 204");
        assert_eq!(controller.notifications()[199].title, "Title 5");
    }

    #[test]
    fn malformed_current_file_blocks_destructive_changes() {
        let root = tempdir().expect("temporary directory");
        let path = root.path().join("notifications.json");
        fs::write(&path, b"broken").expect("fixture");
        let mut controller = NotificationController::load(
            NotificationFile::new(&path),
            &NotificationFile::new(root.path().join("missing.json")),
        );
        assert!(controller.clear().is_err());
        assert!(controller.remove(0).is_err());
        assert!(controller.add(notification(1)).is_err());
        assert_eq!(fs::read(path).expect("preserved"), b"broken");
    }
}
