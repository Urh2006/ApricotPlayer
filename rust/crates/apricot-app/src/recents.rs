//! 2.1 recently opened files and folders, newest first, kept in
//! `settings.json`. History keeps what was played; this keeps what was
//! opened, so a folder is one entry.

use std::path::{Path, PathBuf};

use apricot_storage::SettingsDocument;
use serde::{Deserialize, Serialize};

/// Settings key of [`Recents`].
pub const RECENTS_KEY: &str = "recents_v1";

/// Entries kept in each list.
pub const RECENT_LIMIT: usize = 20;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct Recents {
    pub files: Vec<String>,
    pub folders: Vec<String>,
}

fn remember(list: &mut Vec<String>, path: &Path) {
    let path = path.to_string_lossy().into_owned();
    list.retain(|existing| !existing.eq_ignore_ascii_case(&path));
    list.insert(0, path);
    list.truncate(RECENT_LIMIT);
}

impl Recents {
    #[must_use]
    pub fn from_settings(settings: &SettingsDocument) -> Self {
        settings
            .preserved
            .get(RECENTS_KEY)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    }

    pub fn add_file(&mut self, path: &Path) {
        remember(&mut self.files, path);
    }

    pub fn add_folder(&mut self, path: &Path) {
        remember(&mut self.folders, path);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.folders.is_empty()
    }

    /// Every entry, files first, with whether it is a folder.
    #[must_use]
    pub fn entries(&self) -> Vec<(bool, PathBuf)> {
        self.files
            .iter()
            .map(|path| (false, PathBuf::from(path)))
            .chain(self.folders.iter().map(|path| (true, PathBuf::from(path))))
            .collect()
    }
}

impl crate::Application {
    pub fn recents(&self) -> Recents {
        Recents::from_settings(self.settings())
    }

    /// Saves `recents` at once; a failed save keeps the old list.
    pub fn set_recents(&mut self, recents: &Recents) {
        self.set_feature_settings(RECENTS_KEY, serde_json::json!(recents));
        let _ = self.save_settings();
    }

    pub fn record_recent_file(&mut self, path: &Path) {
        let mut recents = self.recents();
        recents.add_file(path);
        self.set_recents(&recents);
    }

    pub fn record_recent_folder(&mut self, path: &Path) {
        let mut recents = self.recents();
        recents.add_folder(path);
        self.set_recents(&recents);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_entry_comes_first_without_repeats() {
        let mut recents = Recents::default();
        recents.add_file(Path::new("C:/a.mp3"));
        recents.add_file(Path::new("C:/b.mp3"));
        recents.add_file(Path::new("C:/A.mp3"));
        assert_eq!(recents.files, vec!["C:/A.mp3", "C:/b.mp3"]);
        for index in 0..30 {
            recents.add_folder(Path::new(&format!("C:/folder{index}")));
        }
        assert_eq!(recents.folders.len(), RECENT_LIMIT);
        assert_eq!(recents.entries()[0], (false, PathBuf::from("C:/A.mp3")));
    }
}
