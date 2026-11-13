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

    /// Drops the files that left the folder. The selection keeps its row, so
    /// it moves to the file after the last removed one.
    pub fn remove_paths(&mut self, removed: &[PathBuf]) {
        if removed.is_empty() {
            return;
        }
        self.items.retain(|item| {
            !item
                .local_path
                .as_deref()
                .is_some_and(|path| removed.iter().any(|gone| gone == Path::new(path)))
        });
        self.selected_index = self.selected_index.min(self.items.len().saturating_sub(1));
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

    #[test]
    fn removed_files_leave_the_list_and_the_selection_keeps_its_row() {
        let mut session = LocalFolderSession::default();
        let items = (0..4)
            .map(|index| MediaItem {
                local_path: Some(format!("C:/Music/{index}.mp3")),
                ..item(index)
            })
            .collect();
        session.load("C:/Music".into(), items);
        assert!(session.select(1));
        session.remove_paths(&["C:/Music/1.mp3".into(), "C:/Music/3.mp3".into()]);
        let ids: Vec<_> = session
            .items()
            .iter()
            .map(|item| item.id.0.as_str())
            .collect();
        assert_eq!(ids, ["0", "2"]);
        assert_eq!(session.selected_item().expect("selection").id.0, "2");
        session.remove_paths(&["C:/Music/0.mp3".into(), "C:/Music/2.mp3".into()]);
        assert!(session.is_empty());
    }
}
