//! In-memory local-folder projection. Discovery is performed by the platform
//! layer; this session owns the folder list and its stable selection.
//!
//! Python's `show_local_media_folder` shows the whole folder at once, so the
//! list holds every discovered file with no incremental batches.

use std::path::{Path, PathBuf};

use apricot_core::MediaItem;

#[derive(Debug, Default)]
pub struct LocalFolderSession {
    generation: u64,
    path: PathBuf,
    items: Vec<MediaItem>,
    selected_index: usize,
}

impl LocalFolderSession {
    pub fn load(&mut self, path: PathBuf, items: Vec<MediaItem>) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.path = path;
        self.items = items;
        self.selected_index = 0;
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn items(&self) -> &[MediaItem] {
        &self.items
    }

    pub const fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn selected_item(&self) -> Option<&MediaItem> {
        self.items.get(self.selected_index)
    }

    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.selected_index = index;
        true
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};

    use super::LocalFolderSession;

    fn item(index: usize) -> MediaItem {
        MediaItem {
            id: MediaId(index.to_string()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: format!("Track {index}"),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: Some(format!(r"C:\Music\{index}.mp3")),
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn the_whole_folder_is_listed_at_once_like_python() {
        let mut session = LocalFolderSession::default();
        session.load("C:/Music".into(), (0..45).map(item).collect());
        assert_eq!(session.items().len(), 45);
        assert!(session.select(44));
        assert_eq!(session.selected_item().expect("selection").id.0, "44");
    }

    #[test]
    fn selection_cannot_escape_the_folder() {
        let mut session = LocalFolderSession::default();
        session.load("C:/Music".into(), (0..30).map(item).collect());
        assert!(session.select(29));
        assert!(!session.select(30));
        assert_eq!(session.selected_index(), 29);
    }
}
