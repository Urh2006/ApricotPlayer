//! Compatible and atomic user-data storage.

pub mod json_file;
pub mod settings;

pub use json_file::{JsonFileError, read_json, write_json_atomic};
pub use settings::SettingsDocument;
