//! Importing the Windows data of the player on macOS (plan unit M17): the
//! app data folder of the Python version or of the Rust beta, or a ZIP of
//! it. The durable user files are copied; settings that only make sense on
//! the Windows computer (folders, the audio device, cookie files and
//! profiles) fall back to the Mac defaults, and credentials protected by
//! Windows (Spotify, `AudioVault`) are not taken. The current Mac data is
//! kept in a backup folder first. The application then quits, so the
//! imported files are read on the next start.

use std::fs;
use std::io::Read;
use std::path::Path;

use apricot_storage::{DATA_ENTRIES, DataClass, DataShape};

use crate::{Shell, ShellUi};

/// One imported file must stay below this size.
const FILE_LIMIT: u64 = 64 * 1024 * 1024;

/// Settings tied to the Windows computer.
const MACHINE_SETTINGS: [&str; 7] = [
    "download_folder",
    "cache_folder",
    "audio_output_device",
    "cookies_file",
    "cookies_source_file",
    "cookies_browser_profile",
    "ffmpeg_location",
];

/// The durable user files of the data manifest.
fn importable_files() -> impl Iterator<Item = &'static str> {
    DATA_ENTRIES
        .iter()
        .filter(|entry| {
            entry.class == DataClass::DurableUserData && entry.shape != DataShape::Directory
        })
        .map(|entry| entry.relative_path)
}

/// A Windows absolute path: `C:\...`, `C:/...` or `\\server\...`.
fn is_windows_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    (bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/'))
        || text.starts_with("\\\\")
}

/// The settings without the Windows computer's folders, device and files.
fn portable_settings(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| format!("settings.json: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "settings.json is not a settings object".to_owned())?;
    object.retain(|key, value| {
        !MACHINE_SETTINGS.contains(&key.as_str()) && !value.as_str().is_some_and(is_windows_path)
    });
    serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())
}

/// The files found in a folder or a ZIP: (name, contents).
fn read_source(source: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    if source.is_dir() {
        return read_folder(source);
    }
    read_zip(source)
}

/// The data folder is the source itself or one folder below it, for
/// example `ApricotPlayer` inside a copied `AppData\Roaming`.
fn read_folder(source: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut candidates = vec![source.to_path_buf()];
    for name in [
        "ApricotPlayer2Beta",
        "ApricotPlayer",
        "UrhasaurusYouTubePlayer",
    ] {
        candidates.push(source.join(name));
    }
    let root = candidates
        .into_iter()
        .find(|folder| folder.join("settings.json").is_file())
        .ok_or_else(String::new)?;
    let mut files = Vec::new();
    for name in importable_files() {
        let path = root.join(name);
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > FILE_LIMIT {
            continue;
        }
        let bytes = fs::read(&path).map_err(|error| format!("{name}: {error}"))?;
        files.push((name.to_owned(), bytes));
    }
    Ok(files)
}

fn read_zip(source: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    use apricot_platform::zip_archive::{open_entry, read_entries};

    let mut file = fs::File::open(source).map_err(|error| error.to_string())?;
    let entries = read_entries(&mut file).map_err(|error| error.to_string())?;
    // The folder of the shallowest settings.json, as in `read_folder`.
    let prefix = entries
        .iter()
        .filter(|entry| entry.name == "settings.json" || entry.name.ends_with("/settings.json"))
        .map(|entry| entry.name.trim_end_matches("settings.json").to_owned())
        .filter(|prefix| prefix.matches('/').count() <= 2)
        .min_by_key(|prefix| prefix.matches('/').count())
        .ok_or_else(String::new)?;
    let mut files = Vec::new();
    for name in importable_files() {
        let Some(entry) = entries
            .iter()
            .find(|entry| entry.name == format!("{prefix}{name}"))
        else {
            continue;
        };
        if entry.file_size > FILE_LIMIT {
            continue;
        }
        let mut bytes = Vec::new();
        open_entry(&mut file, entry)
            .and_then(|mut reader| reader.read_to_end(&mut bytes))
            .map_err(|error| format!("{name}: {error}"))?;
        files.push((name.to_owned(), bytes));
    }
    Ok(files)
}

/// Copies the files into `app_data` after moving the current ones into
/// `backup`; the number of imported files. Every JSON file must parse, so
/// nothing is replaced with a damaged copy.
fn import_files(
    files: &[(String, Vec<u8>)],
    app_data: &Path,
    backup: &Path,
) -> Result<usize, String> {
    let mut prepared = Vec::new();
    for (name, bytes) in files {
        let bytes = if name == "settings.json" {
            portable_settings(bytes)?
        } else {
            if Path::new(name)
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                serde_json::from_slice::<serde_json::Value>(bytes)
                    .map_err(|error| format!("{name}: {error}"))?;
            }
            bytes.clone()
        };
        prepared.push((name, bytes));
    }
    fs::create_dir_all(backup).map_err(|error| error.to_string())?;
    for name in importable_files() {
        let current = app_data.join(name);
        if current.is_file() {
            fs::copy(&current, backup.join(name)).map_err(|error| format!("{name}: {error}"))?;
        }
    }
    for (name, bytes) in &prepared {
        let target = app_data.join(name);
        let temporary = app_data.join(format!("{name}.import"));
        fs::write(&temporary, bytes)
            .and_then(|()| fs::rename(&temporary, &target))
            .map_err(|error| format!("{name}: {error}"))?;
    }
    Ok(prepared.len())
}

impl Shell {
    /// Settings "Import data from Windows": choose the folder or ZIP,
    /// confirm, import and quit.
    pub fn import_windows_data(&mut self, ui: &mut dyn ShellUi) {
        let Some(source) = ui.choose_import_source(&self.text("import_windows_data_title")) else {
            return;
        };
        let failed = |shell: &Self, ui: &mut dyn ShellUi, error: &str| {
            ui.show_error(
                &shell
                    .text("import_windows_data_failed")
                    .replace("{error}", error),
            );
        };
        // An empty error means no data folder was found there.
        let files = match read_source(&source) {
            Ok(files) if !files.is_empty() => files,
            Err(error) if !error.is_empty() => return failed(self, ui, &error),
            _ => {
                ui.show_error(&self.text("import_windows_data_missing"));
                return;
            }
        };
        let Some(app_data) = apricot_platform::discover_app_paths()
            .ok()
            .map(|paths| paths.app_data)
        else {
            return failed(self, ui, "Application data path is unavailable");
        };
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let backup = app_data.join(format!("backup-before-windows-import-{stamp}"));
        let question = self
            .text("import_windows_data_confirm")
            .replace("{path}", &source.to_string_lossy())
            .replace("{backup}", &backup.to_string_lossy());
        if !ui.ask_yes_no(&question, &self.text("yes"), &self.text("no")) {
            return;
        }
        match import_files(&files, &app_data, &backup) {
            Ok(count) => {
                ui.show_error(
                    &self
                        .text("import_windows_data_done")
                        .replace("{count}", &count.to_string()),
                );
                // The controllers hold the old data; the next start reads
                // the imported files.
                ui.exit();
            }
            Err(error) => failed(self, ui, &error),
        }
    }
}

/// The folder an import may write into, for tests.
#[cfg(test)]
fn test_folder() -> tempfile::TempDir {
    tempfile::tempdir().expect("folder")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_and_devices_fall_back_to_mac_defaults() {
        let settings = serde_json::json!({
            "language": "sl",
            "download_folder": "C:\\Users\\Urh\\Music",
            "audio_output_device": "wasapi/{1234}",
            "last_folder": "D:/Media",
            "share": "\\\\nas\\music",
            "volume": 80,
        });
        let portable: serde_json::Value = serde_json::from_slice(
            &portable_settings(&serde_json::to_vec(&settings).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            portable,
            serde_json::json!({"language": "sl", "volume": 80})
        );
    }

    #[test]
    fn a_folder_import_keeps_a_backup_and_skips_credentials() {
        let folder = test_folder();
        let windows = folder.path().join("Roaming/ApricotPlayer");
        fs::create_dir_all(&windows).unwrap();
        fs::write(windows.join("settings.json"), b"{\"language\": \"sl\"}").unwrap();
        fs::write(windows.join("favorites.json"), b"[]").unwrap();
        fs::write(windows.join("spotify_accounts.json"), b"{}").unwrap();
        let mac = folder.path().join("mac");
        fs::create_dir_all(&mac).unwrap();
        fs::write(mac.join("favorites.json"), b"[1]").unwrap();
        let files = read_source(&folder.path().join("Roaming")).unwrap();
        let names: Vec<_> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["settings.json", "favorites.json"]);
        let backup = mac.join("backup");
        assert_eq!(import_files(&files, &mac, &backup), Ok(2));
        assert_eq!(fs::read(mac.join("favorites.json")).unwrap(), b"[]");
        assert_eq!(fs::read(backup.join("favorites.json")).unwrap(), b"[1]");
        assert!(!mac.join("spotify_accounts.json").exists());
    }

    #[test]
    fn a_damaged_file_replaces_nothing() {
        let folder = test_folder();
        let mac = folder.path().join("mac");
        fs::create_dir_all(&mac).unwrap();
        fs::write(mac.join("history.json"), b"[1]").unwrap();
        let files = vec![
            ("settings.json".to_owned(), b"{}".to_vec()),
            ("history.json".to_owned(), b"[broken".to_vec()),
        ];
        assert!(import_files(&files, &mac, &mac.join("backup")).is_err());
        assert_eq!(fs::read(mac.join("history.json")).unwrap(), b"[1]");
        assert!(!mac.join("settings.json").exists());
    }

    #[test]
    fn a_zip_import_finds_the_data_folder() {
        let folder = test_folder();
        let source = folder.path().join("data");
        fs::create_dir_all(source.join("ApricotPlayer2Beta")).unwrap();
        fs::write(source.join("ApricotPlayer2Beta/settings.json"), b"{}").unwrap();
        fs::write(source.join("ApricotPlayer2Beta/playlists.json"), b"[]").unwrap();
        let zip = folder.path().join("data.zip");
        let status = std::process::Command::new("zip")
            .current_dir(&source)
            .args(["-qr", zip.to_str().unwrap(), "ApricotPlayer2Beta"])
            .status();
        if !status.is_ok_and(|status| status.success()) {
            return; // No zip program on this computer.
        }
        let files = read_source(&zip).unwrap();
        let names: Vec<_> = files.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["settings.json", "playlists.json"]);
    }
}
