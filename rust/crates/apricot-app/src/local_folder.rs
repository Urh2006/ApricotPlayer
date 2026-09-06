//! In-memory local-folder projection. Discovery is performed by the platform
//! layer; this session owns stable selection and bounded list presentation.

use std::path::{Path, PathBuf};

use apricot_core::MediaItem;

pub const DEFAULT_FOLDER_BATCH_SIZE: usize = 20;

#[derive(Debug, Default)]
pub struct LocalFolderSession {
    generation: u64,
    path: PathBuf,
    items: Vec<MediaItem>,
    selected_index: usize,
    visible_count: usize,
    batch_size: usize,
}

impl LocalFolderSession {
    pub fn load(&mut self, path: PathBuf, items: Vec<MediaItem>, configured_batch_size: usize) {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.path = path;
        self.items = items;
        self.selected_index = 0;
        self.batch_size = if configured_batch_size == 0 {
            DEFAULT_FOLDER_BATCH_SIZE
        } else {
            configured_batch_size
        };
        self.visible_count = self.batch_size.min(self.items.len());
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

    pub fn visible_items(&self) -> &[MediaItem] {
        &self.items[..self.visible_count]
    }

    pub const fn selected_index(&self) -> usize {
        self.selected_index
    }

    pub fn selected_item(&self) -> Option<&MediaItem> {
        self.items.get(self.selected_index)
    }

    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.visible_count {
            return false;
        }
        self.selected_index = index;
        true
    }

    pub fn reveal_and_select(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.visible_count = self.visible_count.max(index + 1);
        self.selected_index = index;
        true
    }

    pub fn append_visible_batch(&mut self) -> usize {
        let before = self.visible_count;
        self.visible_count = self
            .visible_count
            .saturating_add(self.batch_size)
            .min(self.items.len());
        self.visible_count - before
    }

    pub fn has_more(&self) -> bool {
        self.visible_count < self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};

    use super::{DEFAULT_FOLDER_BATCH_SIZE, LocalFolderSession};

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
    fn unlimited_setting_uses_dynamic_batches_without_truncating_the_folder() {
        let mut session = LocalFolderSession::default();
        session.load("C:/Music".into(), (0..45).map(item).collect(), 0);
        assert_eq!(session.visible_items().len(), DEFAULT_FOLDER_BATCH_SIZE);
        assert_eq!(session.items().len(), 45);
        assert_eq!(session.append_visible_batch(), DEFAULT_FOLDER_BATCH_SIZE);
        assert_eq!(session.append_visible_batch(), 5);
        assert_eq!(session.append_visible_batch(), 0);
        assert!(!session.has_more());
    }

    #[test]
    fn selection_cannot_escape_the_visible_projection() {
        let mut session = LocalFolderSession::default();
        session.load("C:/Music".into(), (0..30).map(item).collect(), 10);
        assert!(session.select(9));
        assert!(!session.select(10));
        assert_eq!(session.selected_item().expect("selection").id.0, "9");
        session.append_visible_batch();
        assert!(session.select(10));
        assert!(session.reveal_and_select(29));
        assert_eq!(session.visible_items().len(), 30);
        assert_eq!(session.selected_index(), 29);
    }
}
