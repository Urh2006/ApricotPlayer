use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;
use thiserror::Error;

use crate::{
    JsonFileError, SettingsDocument, SettingsLoadError, write_bytes_atomic, write_json_atomic,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsPaths {
    pub primary: PathBuf,
    pub backup: PathBuf,
    pub legacy: PathBuf,
}

impl SettingsPaths {
    pub fn for_app_data(app_data: &Path, legacy_app_data: &Path) -> Self {
        Self {
            primary: app_data.join("settings.json"),
            backup: app_data.join("settings.json.bak"),
            legacy: legacy_app_data.join("settings.json"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsSource {
    Primary,
    Backup,
    Legacy,
    Defaults,
}

#[derive(Debug)]
pub struct SettingsLoadOutcome {
    pub settings: SettingsDocument,
    pub source: SettingsSource,
    pub migrated: bool,
    pub save_blocked: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Error)]
pub enum SettingsSaveError {
    #[error("settings save is blocked because existing preferences could not be loaded safely")]
    Blocked,
    #[error(transparent)]
    Json(#[from] JsonFileError),
}

pub fn load_settings(paths: &SettingsPaths, defaults: SettingsDocument) -> SettingsLoadOutcome {
    let sources = [
        (&paths.primary, SettingsSource::Primary),
        (&paths.backup, SettingsSource::Backup),
        (&paths.legacy, SettingsSource::Legacy),
    ];
    let mut errors = Vec::new();
    for (path, source) in sources {
        if !path.exists() {
            continue;
        }
        match load_one(path, defaults.clone()) {
            Ok(settings) => {
                let migrated = source != SettingsSource::Primary;
                return SettingsLoadOutcome {
                    settings,
                    source,
                    migrated,
                    save_blocked: false,
                    errors,
                };
            }
            Err(error) => errors.push(format!("{}: {error}", path.display())),
        }
    }
    SettingsLoadOutcome {
        settings: defaults,
        source: SettingsSource::Defaults,
        migrated: false,
        save_blocked: paths.primary.exists() || paths.backup.exists(),
        errors,
    }
}

/// Saves settings loaded through [`load_settings`] and advances the outcome to
/// a valid primary source.
///
/// A backup is created only when the current primary file was the successfully
/// loaded source. This prevents a recovery save from replacing a valid backup
/// with the corrupt primary file that caused recovery.
///
/// # Errors
///
/// Returns [`SettingsSaveError`] when saves are blocked or either atomic write fails.
pub fn save_loaded_settings(
    paths: &SettingsPaths,
    outcome: &mut SettingsLoadOutcome,
    settings: &SettingsDocument,
) -> Result<(), SettingsSaveError> {
    if outcome.save_blocked {
        return Err(SettingsSaveError::Blocked);
    }
    if outcome.source == SettingsSource::Primary && paths.primary.exists() {
        let bytes = fs::read(&paths.primary).map_err(|source| JsonFileError::Read {
            path: paths.primary.clone(),
            source,
        })?;
        write_bytes_atomic(&paths.backup, &bytes)?;
    }
    write_json_atomic(&paths.primary, settings)?;
    outcome.settings = settings.clone();
    outcome.source = SettingsSource::Primary;
    outcome.migrated = false;
    outcome.errors.clear();
    Ok(())
}

fn load_one(
    path: &Path,
    defaults: SettingsDocument,
) -> Result<SettingsDocument, SettingsFileError> {
    let bytes = fs::read(path)?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(SettingsFileError::Empty);
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    Ok(SettingsDocument::from_value_with_defaults(
        &value, defaults,
    )?)
}

#[derive(Debug, Error)]
enum SettingsFileError {
    #[error("could not read file: {0}")]
    Read(#[from] std::io::Error),
    #[error("settings file is empty")]
    Empty,
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Settings(#[from] SettingsLoadError),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::{
        SettingsPaths, SettingsSaveError, SettingsSource, load_settings, save_loaded_settings,
    };
    use crate::SettingsDocument;

    fn fixture_paths(root: &Path) -> SettingsPaths {
        SettingsPaths::for_app_data(&root.join("ApricotPlayer"), &root.join("legacy"))
    }

    use std::path::Path;

    #[test]
    fn backup_is_loaded_when_primary_is_corrupt() {
        let root = tempdir().expect("temporary directory");
        let paths = fixture_paths(root.path());
        fs::create_dir_all(paths.primary.parent().expect("parent")).expect("directory");
        fs::write(&paths.primary, b"{broken").expect("primary");
        fs::write(&paths.backup, br#"{"language":"sl"}"#).expect("backup");
        let outcome = load_settings(&paths, SettingsDocument::default());
        assert_eq!(outcome.source, SettingsSource::Backup);
        assert_eq!(outcome.settings.language, "sl");
        assert!(outcome.migrated);
        assert!(!outcome.save_blocked);
        assert_eq!(outcome.errors.len(), 1);
    }

    #[test]
    fn corrupt_primary_and_backup_block_default_overwrite() {
        let root = tempdir().expect("temporary directory");
        let paths = fixture_paths(root.path());
        fs::create_dir_all(paths.primary.parent().expect("parent")).expect("directory");
        fs::write(&paths.primary, b"{broken").expect("primary");
        fs::write(&paths.backup, b"").expect("backup");
        let mut outcome = load_settings(&paths, SettingsDocument::default());
        assert_eq!(outcome.source, SettingsSource::Defaults);
        assert!(outcome.save_blocked);
        assert!(matches!(
            save_loaded_settings(&paths, &mut outcome, &SettingsDocument::default()),
            Err(SettingsSaveError::Blocked)
        ));
        assert_eq!(fs::read(&paths.primary).expect("unchanged"), b"{broken");
    }

    #[test]
    fn successful_save_preserves_previous_primary_as_backup() {
        let root = tempdir().expect("temporary directory");
        let paths = fixture_paths(root.path());
        fs::create_dir_all(paths.primary.parent().expect("parent")).expect("directory");
        fs::write(&paths.primary, br#"{"language":"en"}"#).expect("primary");
        let mut settings = SettingsDocument {
            language: "sl".to_owned(),
            ..SettingsDocument::default()
        };
        settings
            .preserved
            .insert("future".to_owned(), json!({"kept": true}));
        let mut outcome = load_settings(&paths, SettingsDocument::default());
        save_loaded_settings(&paths, &mut outcome, &settings).expect("save");
        assert_eq!(
            fs::read_to_string(&paths.backup).expect("backup"),
            r#"{"language":"en"}"#
        );
        let saved: Value =
            serde_json::from_slice(&fs::read(&paths.primary).expect("primary")).expect("JSON");
        assert_eq!(saved["language"], "sl");
        assert_eq!(saved["future"], json!({"kept": true}));
        assert_eq!(outcome.source, SettingsSource::Primary);
        assert!(!outcome.migrated);
        assert!(outcome.errors.is_empty());
    }

    #[test]
    fn recovery_save_preserves_the_valid_backup() {
        let root = tempdir().expect("temporary directory");
        let paths = fixture_paths(root.path());
        fs::create_dir_all(paths.primary.parent().expect("parent")).expect("directory");
        fs::write(&paths.primary, b"{broken").expect("primary");
        let backup = br#"{"language":"sl"}"#;
        fs::write(&paths.backup, backup).expect("backup");
        let mut outcome = load_settings(&paths, SettingsDocument::default());
        assert_eq!(outcome.source, SettingsSource::Backup);

        let mut changed = outcome.settings.clone();
        changed.enable_history = false;
        save_loaded_settings(&paths, &mut outcome, &changed).expect("recovery save");

        assert_eq!(
            fs::read(&paths.backup).expect("backup remains valid"),
            backup
        );
        let reloaded = load_settings(&paths, SettingsDocument::default());
        assert_eq!(reloaded.source, SettingsSource::Primary);
        assert_eq!(reloaded.settings.language, "sl");
        assert!(!reloaded.settings.enable_history);
    }
}
