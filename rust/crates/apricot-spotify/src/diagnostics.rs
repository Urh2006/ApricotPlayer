//! `spotify.log` in the Apricot data folder: warnings and errors of
//! `LibreSpot` and short events of this crate, for the diagnostic report
//! (`docs/SPOTIFY_PLAN.md` S54). Tokens and credentials are never logged:
//! the levels below `info` of `LibreSpot`, which can contain request
//! details, are not written.

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

const MAX_BYTES: u64 = 1024 * 1024;

struct FileLogger {
    path: PathBuf,
    file: Mutex<Option<File>>,
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        if metadata.target().starts_with("apricot_spotify")
            || metadata.target().starts_with("apricot_playback")
        {
            metadata.level() <= log::Level::Info
        } else if metadata.target().starts_with("librespot") {
            metadata.level() <= log::Level::Warn
        } else {
            false
        }
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let Ok(mut guard) = self.file.lock() else {
            return;
        };
        if guard.is_none() {
            // Keep one previous log; start a new one past the limit.
            if std::fs::metadata(&self.path).is_ok_and(|meta| meta.len() > MAX_BYTES) {
                let _ = std::fs::rename(&self.path, self.path.with_extension("log.1"));
            }
            *guard = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
        }
        if let Some(file) = guard.as_mut() {
            let seconds = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs());
            let _ = writeln!(
                file,
                "{seconds} {} {}: {}",
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock()
            && let Some(file) = guard.as_mut()
        {
            let _ = file.flush();
        }
    }
}

/// Installs the logger once per process; later calls do nothing.
pub fn install(app_data: &Path) {
    let directory = app_data.join("spotify");
    let _ = std::fs::create_dir_all(&directory);
    let logger = FileLogger {
        path: directory.join("spotify.log"),
        file: Mutex::new(None),
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}
