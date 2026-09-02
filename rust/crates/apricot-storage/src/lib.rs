//! Compatible and atomic user-data storage.

pub mod data_manifest;
pub mod json_file;
pub mod locales;
pub mod settings;

pub use data_manifest::{DATA_ENTRIES, DataClass, DataEntry, DataShape, RecoveryPolicy};
pub use json_file::{JsonFileError, read_json, write_json_atomic};
pub use locales::{LocaleLoadError, load_translation_catalog};
pub use settings::SettingsDocument;
