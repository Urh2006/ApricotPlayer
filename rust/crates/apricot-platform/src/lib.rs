//! Platform services shared by application modules.

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformPaths {
    pub app_data: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub downloads: PathBuf,
    pub runtime: PathBuf,
}

#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("platform operation failed: {0}")]
    Operation(String),
}

pub trait PlatformServices: Send + Sync {
    fn paths(&self) -> &PlatformPaths;

    /// Opens a local file or directory through the operating system.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is invalid or the shell rejects it.
    fn open_path(&self, path: &Path) -> Result<(), PlatformError>;

    /// Opens a validated URL in the user's browser.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL cannot be handed to the operating system.
    fn open_url(&self, url: &str) -> Result<(), PlatformError>;

    /// Copies text to the system clipboard.
    ///
    /// # Errors
    ///
    /// Returns an error when the clipboard cannot be opened or updated.
    fn copy_text(&self, text: &str) -> Result<(), PlatformError>;

    /// Sends one deduplicated accessibility announcement.
    ///
    /// # Errors
    ///
    /// Returns an error when no configured announcement adapter accepts it.
    fn announce(&self, text: &str) -> Result<(), PlatformError>;
}
