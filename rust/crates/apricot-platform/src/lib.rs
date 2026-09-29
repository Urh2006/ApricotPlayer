//! Platform services shared by application modules.

use std::path::{Path, PathBuf};

use thiserror::Error;

pub mod app_update;
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
pub mod single_instance;
pub mod soundcloud_search;
pub mod transcript;
pub mod windows_registration;
pub mod youtube_data_api;
pub mod youtube_helper_process;
pub mod youtube_related;
pub mod youtube_search_service;
pub mod ytdlp_youtube;

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
    LocalMediaError, scan_local_media_folder, scan_local_media_folder_with_cancel,
};
pub use paths::{PathDiscoveryError, discover_windows_beta_paths, discover_windows_paths};
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
pub use ytdlp_youtube::{YtDlpYoutubeEngine, spawn_youtube_runtime};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationIdentity {
    Stable,
    RustBeta,
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
        let (app_name, legacy_name) = match identity {
            ApplicationIdentity::Stable => ("ApricotPlayer", "UrhasaurusYouTubePlayer"),
            ApplicationIdentity::RustBeta => ("ApricotPlayer2Beta", "ApricotPlayer"),
        };
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
