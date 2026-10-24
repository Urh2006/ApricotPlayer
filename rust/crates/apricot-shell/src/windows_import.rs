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
const MACHINE_SETTINGS: [&str; 8] = [
    "download_folder",
    "cache_folder",
    "audio_output_device",
    "cookies_file",
    "cookies_source_file",
    "cookies_browser_profile",
    "ffmpeg_location",
    "audiovault_password_protected",
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
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("{name}: {error}")),
        };
        if !metadata.is_file() {
            continue;
        }
        if metadata.len() > FILE_LIMIT {
            return Err(format!("{name}: import file exceeds the size limit"));
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
            return Err(format!("{name}: import file exceeds the size limit"));
        }
        let mut bytes = Vec::new();
        open_entry(&mut file, entry)
            .and_then(|reader| reader.take(FILE_LIMIT + 1).read_to_end(&mut bytes))
            .map_err(|error| format!("{name}: {error}"))?;
        if bytes.len() as u64 > FILE_LIMIT {
            return Err(format!("{name}: import file exceeds the size limit"));
        }
        files.push((name.to_owned(), bytes));
    }
    Ok(files)
}

/// A completely validated and staged import. No live user file is touched
/// until commit, and a failed commit restores every file it replaced.
struct PreparedImport {
    staging: std::path::PathBuf,
    names: Vec<String>,
}

impl Drop for PreparedImport {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.staging);
    }
}

impl PreparedImport {
    fn new(files: &[(String, Vec<u8>)], app_data: &Path) -> Result<Self, String> {
        let mut prepared = Vec::new();
        for (name, bytes) in files {
            let entry = DATA_ENTRIES
                .iter()
                .find(|entry| {
                    entry.relative_path == name
                        && entry.class == DataClass::DurableUserData
                        && entry.shape != DataShape::Directory
                })
                .ok_or_else(|| format!("{name}: not an importable user file"))?;
            if prepared.iter().any(|(existing, _)| existing == name) {
                return Err(format!("{name}: duplicate import file"));
            }
            let bytes = if name == "settings.json" {
                portable_settings(bytes)?
            } else {
                if matches!(entry.shape, DataShape::JsonArray | DataShape::JsonObject) {
                    let value: serde_json::Value = serde_json::from_slice(bytes)
                        .map_err(|error| format!("{name}: {error}"))?;
                    let valid = match entry.shape {
                        DataShape::JsonArray => value.is_array(),
                        DataShape::JsonObject => value.is_object(),
                        _ => unreachable!(),
                    };
                    if !valid {
                        return Err(format!("{name}: wrong JSON document shape"));
                    }
                }
                bytes.clone()
            };
            let target = app_data.join(name);
            if target.exists() && !target.is_file() {
                return Err(format!("{name}: target is not a regular file"));
            }
            prepared.push((name.clone(), bytes));
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let staging = app_data.join(format!(".windows-import-{}-{stamp}", std::process::id()));
        fs::create_dir(&staging).map_err(|error| error.to_string())?;
        let mut import = Self {
            staging,
            names: Vec::new(),
        };
        for (name, bytes) in prepared {
            apricot_storage::write_bytes_atomic(&import.staging.join(&name), &bytes)
                .map_err(|error| format!("{name}: {error}"))?;
            validate_staged_data(&name, &import.staging.join(&name))
                .map_err(|error| format!("{name}: {error}"))?;
            import.names.push(name);
        }
        Ok(import)
    }

    fn commit(self, app_data: &Path, backup: &Path) -> Result<usize, String> {
        self.commit_with(app_data, backup, |source, target| {
            fs::rename(source, target)
        })
    }

    fn commit_with(
        self,
        app_data: &Path,
        backup: &Path,
        mut replace: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    ) -> Result<usize, String> {
        // Never reuse a previous backup: it is the recovery source if even
        // rollback encounters an I/O failure.
        fs::create_dir(backup).map_err(|error| error.to_string())?;
        for name in importable_files() {
            let current = app_data.join(name);
            if current.is_file() {
                fs::copy(&current, backup.join(name))
                    .map_err(|error| format!("{name}: {error}"))?;
            }
        }
        let mut committed: Vec<&str> = Vec::new();
        for name in &self.names {
            if let Err(error) = replace(&self.staging.join(name), &app_data.join(name)) {
                let mut failures = Vec::new();
                for changed in committed.iter().rev() {
                    let target = app_data.join(changed);
                    let saved = backup.join(changed);
                    let restored = if saved.is_file() {
                        fs::read(&saved)
                            .map_err(|error| error.to_string())
                            .and_then(|bytes| {
                                apricot_storage::write_bytes_atomic(&target, &bytes)
                                    .map_err(|error| error.to_string())
                            })
                    } else {
                        fs::remove_file(&target).map_err(|error| error.to_string())
                    };
                    if let Err(error) = restored {
                        failures.push(format!("{changed}: {error}"));
                    }
                }
                let recovery = if failures.is_empty() {
                    String::new()
                } else {
                    format!(
                        "; rollback failed: {}; backup: {}",
                        failures.join("; "),
                        backup.display()
                    )
                };
                return Err(format!("{name}: {error}{recovery}"));
            }
            committed.push(name);
        }
        Ok(self.names.len())
    }
}

/// Use the real data readers too: a JSON array containing incompatible
/// entries must not replace a valid collection and appear empty at restart.
fn validate_staged_data(name: &str, path: &Path) -> Result<(), String> {
    use apricot_storage::{
        BookmarkFile, LastPlayerSessionFile, MediaListFile, NotificationFile, PlaybackPositionFile,
        PlaybackQueueFile, RssFeedFile, SubscriptionFile, UserPlaylistFile,
    };
    match name {
        "favorites.json" | "history.json" => MediaListFile::new(path)
            .load()
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "bookmarks.json" => BookmarkFile::new(path)
            .load()
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "subscriptions.json" => SubscriptionFile::new(path)
            .load()
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "rss_feeds.json" => RssFeedFile::new(path)
            .load()
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "playlists.json" => UserPlaylistFile::new(path)
            .load()
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "notifications.json" => NotificationFile::new(path)
            .load()
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "playback_positions.json" => PlaybackPositionFile::new(path)
            .load()
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "playback_queue.json" => PlaybackQueueFile::new(path)
            .load()
            .map(|_| ())
            .map_err(|error| error.to_string()),
        "last_player_session.json" => LastPlayerSessionFile::new(path)
            .load()
            .map(|_| ())
            .map_err(|error| error.to_string()),
        _ => Ok(()),
    }
}

#[cfg(test)]
fn import_files(
    files: &[(String, Vec<u8>)],
    app_data: &Path,
    backup: &Path,
) -> Result<usize, String> {
    PreparedImport::new(files, app_data)?.commit(app_data, backup)
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
        let prepared = match PreparedImport::new(&files, &app_data) {
            Ok(prepared) => prepared,
            Err(error) => return failed(self, ui, &error),
        };
        // Persist the old player's final position before its file is replaced.
        self.shutdown(ui);
        match prepared.commit(&app_data, &backup) {
            Ok(count) => {
                self.finish_windows_import();
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
    fn wrong_shaped_data_replaces_nothing() {
        let folder = test_folder();
        let backup = folder.path().join("backup");
        let files = vec![("favorites.json".to_owned(), b"{}".to_vec())];
        assert!(import_files(&files, folder.path(), &backup).is_err());
        assert!(!folder.path().join("favorites.json").exists());
        assert!(!backup.exists());
    }

    #[test]
    fn invalid_media_entries_replaces_nothing() {
        let folder = test_folder();
        let files = vec![("favorites.json".to_owned(), b"[1]".to_vec())];
        assert!(import_files(&files, folder.path(), &folder.path().join("backup")).is_err());
        assert!(!folder.path().join("favorites.json").exists());
    }

    #[test]
    fn a_failed_later_write_rolls_back_earlier_new_files() {
        let folder = test_folder();
        fs::create_dir(folder.path().join("history.json")).unwrap();
        let files = vec![
            ("favorites.json".to_owned(), b"[]".to_vec()),
            ("history.json".to_owned(), b"[]".to_vec()),
        ];
        assert!(import_files(&files, folder.path(), &folder.path().join("backup")).is_err());
        assert!(
            !folder.path().join("favorites.json").exists(),
            "a failed import must replace nothing"
        );
        assert!(folder.path().join("history.json").is_dir());
    }

    #[test]
    fn injected_commit_failure_restores_existing_and_removes_new_files() {
        let folder = test_folder();
        fs::write(
            folder.path().join("settings.json"),
            b"{\"language\":\"en\"}",
        )
        .unwrap();
        let files = vec![
            (
                "settings.json".to_owned(),
                b"{\"language\":\"sl\"}".to_vec(),
            ),
            ("favorites.json".to_owned(), b"[]".to_vec()),
            ("history.json".to_owned(), b"[]".to_vec()),
        ];
        let prepared = PreparedImport::new(&files, folder.path()).unwrap();
        let mut writes = 0;
        let backup = folder.path().join("backup");
        let result = prepared.commit_with(folder.path(), &backup, |source, target| {
            writes += 1;
            if writes == 3 {
                return Err(std::io::Error::other("injected commit failure"));
            }
            fs::rename(source, target)
        });
        assert!(result.unwrap_err().contains("injected commit failure"));
        assert_eq!(
            fs::read(folder.path().join("settings.json")).unwrap(),
            b"{\"language\":\"en\"}"
        );
        assert!(!folder.path().join("favorites.json").exists());
        assert!(!folder.path().join("history.json").exists());
        assert_eq!(
            fs::read(backup.join("settings.json")).unwrap(),
            b"{\"language\":\"en\"}"
        );
        assert_eq!(
            fs::read_dir(folder.path()).unwrap().count(),
            2,
            "staging is cleaned up"
        );
    }

    #[test]
    fn a_windows_protected_password_is_never_imported() {
        let bytes = portable_settings(b"{\"audiovault_password_protected\":\"DPAPI-blob\",\"audiovault_email\":\"a@example.org\"}").unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(value.get("audiovault_password_protected").is_none());
        assert_eq!(value["audiovault_email"], "a@example.org");
    }

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
