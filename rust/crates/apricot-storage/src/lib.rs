//! Compatible and atomic user-data storage.

pub mod bookmark_file;
pub mod compat_snapshot;
pub mod data_manifest;
pub mod json_file;
pub mod last_player_session_file;
pub mod legacy_media;
pub mod locales;
pub mod media_list_file;
pub mod playback_position_file;
pub mod playback_queue_file;
pub mod settings;
pub mod settings_file;
pub mod user_playlist_file;

pub use bookmark_file::{Bookmark, BookmarkFile, BookmarkFileError, bookmark_media_key};
pub use compat_snapshot::{
    ArtifactSnapshot, ArtifactSource, CompatibilitySnapshot, CompatibilitySnapshotError,
    DEFAULT_MAX_ARTIFACT_BYTES, SnapshotValue,
};
pub use data_manifest::{DATA_ENTRIES, DataClass, DataEntry, DataShape, RecoveryPolicy};
pub use json_file::{JsonFileError, read_json, write_bytes_atomic, write_json_atomic};
pub use last_player_session_file::{
    LastPlayerSession, LastPlayerSessionFile, LastPlayerSessionFileError,
};
pub use legacy_media::{media_item_from_python_value, media_item_to_python_value};
pub use locales::{LocaleLoadError, load_translation_catalog};
pub use media_list_file::{MediaListFile, MediaListFileError};
pub use playback_position_file::{PlaybackPositionFile, PlaybackPositionFileError};
pub use playback_queue_file::{PlaybackQueueFile, PlaybackQueueFileError};
pub use settings::{SettingsDocument, SettingsLoadError};
pub use settings_file::{
    SettingsLoadOutcome, SettingsPaths, SettingsSaveError, SettingsSource, load_settings,
    save_loaded_settings,
};
pub use user_playlist_file::{UserPlaylist, UserPlaylistFile, UserPlaylistFileError};
