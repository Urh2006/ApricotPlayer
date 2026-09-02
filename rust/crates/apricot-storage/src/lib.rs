//! Compatible and atomic user-data storage.

pub mod data_manifest;
pub mod json_file;
pub mod locales;
pub mod settings;
pub mod settings_file;

pub use data_manifest::{DATA_ENTRIES, DataClass, DataEntry, DataShape, RecoveryPolicy};
pub use json_file::{JsonFileError, read_json, write_bytes_atomic, write_json_atomic};
pub use locales::{LocaleLoadError, load_translation_catalog};
pub use settings::{SettingsDocument, SettingsLoadError};
pub use settings_file::{
    SettingsLoadOutcome, SettingsPaths, SettingsSaveError, SettingsSource, load_settings,
    save_settings,
};
