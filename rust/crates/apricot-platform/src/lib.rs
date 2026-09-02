//! Platform services shared by application modules.

use std::path::{Path, PathBuf};

use thiserror::Error;

pub mod diagnostics;
pub mod paths;

pub use diagnostics::{DiagnosticLog, DiagnosticLogError, install_panic_hook};
pub use paths::{PathDiscoveryError, discover_windows_paths};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformPaths {
    pub app_data: PathBuf,
    pub legacy_app_data: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
    pub downloads: PathBuf,
    pub runtime: PathBuf,
}

impl PlatformPaths {
    pub fn from_windows_roots(roaming_app_data: &Path, user_home: &Path, runtime: &Path) -> Self {
        let app_data = roaming_app_data.join("ApricotPlayer");
        Self {
            legacy_app_data: roaming_app_data.join("UrhasaurusYouTubePlayer"),
            cache: app_data.join("cache"),
            logs: app_data.clone(),
            downloads: user_home.join("Downloads").join("ApricotPlayer"),
            runtime: runtime.to_owned(),
            app_data,
        }
    }
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
