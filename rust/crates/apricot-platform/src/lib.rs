//! Platform services shared by application modules.

use std::path::{Path, PathBuf};

use thiserror::Error;

#[cfg(unix)]
pub mod activation;
pub mod app_update;
pub mod audiovault;
pub mod audiovault_credentials;
pub mod bpm;
pub mod browser_cookies;
pub mod clip_export;
pub mod diagnostics;
pub mod download;
pub mod local_edit;
pub mod local_media;
pub mod lyrics;
pub mod paths;
pub mod podcast_directory;
pub mod rss_client;
pub mod shell;
pub mod single_instance;
pub mod soundcloud_search;
pub mod transcript;
pub mod windows_registration;
pub mod youtube_data_api;
pub mod youtube_helper_process;
pub mod youtube_related;
pub mod youtube_search_service;
pub mod ytdlp_youtube;
pub mod zip_archive;

pub use bpm::{BpmAnalysisRequest, analyze_source_bpm, bpm_ffmpeg_arguments, ffmpeg_executable};
pub use clip_export::{
    ClipExportError, ClipExportMode, ClipExportRequest, build_clip_export_arguments,
    export_marked_clip,
};
pub use diagnostics::{DiagnosticLog, DiagnosticLogError, install_panic_hook};
pub use download::{
    DownloadError, DownloadEvent, DownloadMode, DownloadOptions, DownloadPhase, DownloadRequest,
    DownloadSummary, VideoDownloadFormat, YtDlpDownloader,
};
pub use local_edit::{LocalEditJob, LocalEditRender, run_ffmpeg_conversion, save_local_edit};
pub use local_media::{
    LocalMediaError, MEDIA_EXTENSIONS, scan_local_media_folder, scan_local_media_folder_with_cancel,
};
pub use paths::{
    PathDiscoveryError, discover_app_paths, discover_windows_beta_paths, discover_windows_paths,
};
pub use podcast_directory::{ApplePodcastDirectoryClient, PodcastDirectoryError};
pub use rss_client::{RssClient, RssClientError};
pub use single_instance::{SingleInstanceGuard, SingleInstanceOutcome, acquire_single_instance};
pub use windows_registration::{
    startup_command, startup_value_name, sync_startup_registration, windows_platform_description,
};
pub use youtube_data_api::{YoutubeDataApiClient, YoutubeDataApiError};
pub use youtube_helper_process::{YoutubeHelperProcess, YoutubeProcessError};
pub use youtube_search_service::{
    YoutubeSearchService, YoutubeSearchServiceError, YoutubeSearchServiceUpdate,
};
pub use ytdlp_youtube::{YtDlpYoutubeEngine, spawn_youtube_runtime, stop_playback_resolver};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationIdentity {
    Stable,
    RustBeta,
}

/// Whether this build replaces the Python version (`stable-identity`).
pub const STABLE_BUILD: bool = cfg!(feature = "stable-identity");

/// This build's identity: Python's for the 2.0 that replaces it, otherwise
/// the side-by-side beta.
pub const BUILD_IDENTITY: ApplicationIdentity = if STABLE_BUILD {
    ApplicationIdentity::Stable
} else {
    ApplicationIdentity::RustBeta
};

/// The name in window titles and message boxes.
#[must_use]
pub const fn product_name() -> &'static str {
    if STABLE_BUILD {
        "ApricotPlayer"
    } else {
        "ApricotPlayer 2 Beta"
    }
}

/// The window title while something plays: `<item> - ApricotPlayer`, so a
/// screen reader names both the item and the application.
#[must_use]
pub fn playing_window_title(media: &str) -> String {
    format!("{media} - {}", product_name())
}

/// The computer's name as the user sees it: `COMPUTERNAME` on Windows and the
/// Sharing name (`scutil --get ComputerName`) on macOS; empty when unknown.
#[must_use]
pub fn computer_name() -> String {
    #[cfg(target_os = "macos")]
    return std::process::Command::new("/usr/sbin/scutil")
        .args(["--get", "ComputerName"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    #[cfg(not(target_os = "macos"))]
    std::env::var("COMPUTERNAME").unwrap_or_default()
}

/// The bundled mpv: its executable (for the working directory and the
/// external-process fallback) and the libmpv library the player loads.
/// Windows needs both files in `mpv` next to the application (Windows
/// `playback_launch_options`). macOS uses `mpv/libmpv.2.dylib` next to the
/// executable, in a development build also Homebrew's libmpv.
#[must_use]
pub fn mpv_locations() -> Option<(PathBuf, PathBuf)> {
    let directory = std::env::current_exe().ok()?.parent()?.join("mpv");
    #[cfg(windows)]
    {
        let executable = directory.join("mpv.exe");
        let library = directory.join("libmpv-2.dll");
        (executable.is_file() && library.is_file()).then_some((executable, library))
    }
    #[cfg(not(windows))]
    {
        let bundled = directory.join(if cfg!(target_os = "macos") {
            "libmpv.2.dylib"
        } else {
            "libmpv.so.2"
        });
        let library = std::iter::once(bundled)
            .chain(
                cfg!(debug_assertions)
                    .then(|| {
                        [
                            PathBuf::from("/opt/homebrew/lib/libmpv.2.dylib"),
                            PathBuf::from("/usr/local/lib/libmpv.2.dylib"),
                        ]
                    })
                    .into_iter()
                    .flatten(),
            )
            .find(|path| path.is_file())?;
        let executable = library.with_file_name(executable_name("mpv"));
        Some((executable, library))
    }
}

/// The file name of a bundled or `PATH` program: `name.exe` on Windows and
/// `name` elsewhere.
#[must_use]
pub fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

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
        Self::from_windows_roots_for_identity(
            roaming_app_data,
            user_home,
            runtime,
            ApplicationIdentity::Stable,
        )
    }

    pub fn from_windows_roots_for_identity(
        roaming_app_data: &Path,
        user_home: &Path,
        runtime: &Path,
        identity: ApplicationIdentity,
    ) -> Self {
        let (app_name, legacy_name) = identity.folder_names();
        let app_data = roaming_app_data.join(app_name);
        Self {
            legacy_app_data: roaming_app_data.join(legacy_name),
            cache: app_data.join("cache"),
            logs: app_data.clone(),
            downloads: user_home.join("Downloads").join("ApricotPlayer"),
            runtime: runtime.to_owned(),
            app_data,
        }
    }

    /// The macOS layout: durable data in `Application Support`, cache in
    /// `Caches` and logs in `Logs`, each folder named as on Windows.
    pub fn from_macos_home_for_identity(
        home: &Path,
        runtime: &Path,
        identity: ApplicationIdentity,
    ) -> Self {
        let (app_name, legacy_name) = identity.folder_names();
        let library = home.join("Library");
        let support = library.join("Application Support");
        Self {
            app_data: support.join(app_name),
            legacy_app_data: support.join(legacy_name),
            cache: library.join("Caches").join(app_name),
            logs: library.join("Logs").join(app_name),
            downloads: home.join("Downloads").join("ApricotPlayer"),
            runtime: runtime.to_owned(),
        }
    }
}

impl ApplicationIdentity {
    /// The app-data folder name and the folder this identity migrates from.
    #[must_use]
    pub const fn folder_names(self) -> (&'static str, &'static str) {
        match self {
            Self::Stable => ("ApricotPlayer", "UrhasaurusYouTubePlayer"),
            Self::RustBeta => ("ApricotPlayer2Beta", "ApricotPlayer"),
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
