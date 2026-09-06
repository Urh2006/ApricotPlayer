//! Ordered playback queue with stable-identity deduplication.

use apricot_core::MediaItem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueAddOutcome {
    Added,
    AlreadyPresent,
    Unplayable,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct QueueBatchAddOutcome {
    pub added: usize,
    pub already_present: usize,
    pub unplayable: usize,
}

#[derive(Clone, Debug, Default)]
pub struct PlaybackQueue {
    items: Vec<MediaItem>,
}

impl PlaybackQueue {
    pub fn items(&self) -> &[MediaItem] {
        &self.items
    }

    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub const fn len(&self) -> usize {
        self.items.len()
    }

    pub fn replace(&mut self, items: Vec<MediaItem>) {
        self.items.clear();
        for item in items {
            let _ = self.add(item);
        }
    }

    pub fn add(&mut self, item: MediaItem) -> QueueAddOutcome {
        if !item.is_playable() {
            return QueueAddOutcome::Unplayable;
        }
        let Some(identity) = item.stable_identity() else {
            return QueueAddOutcome::Unplayable;
        };
        if self
            .items
            .iter()
            .any(|queued| queued.stable_identity().as_deref() == Some(&identity))
        {
            return QueueAddOutcome::AlreadyPresent;
        }
        self.items.push(item);
        QueueAddOutcome::Added
    }

    pub fn add_many(&mut self, items: impl IntoIterator<Item = MediaItem>) -> QueueBatchAddOutcome {
        let mut outcome = QueueBatchAddOutcome::default();
        for item in items {
            match self.add(item) {
                QueueAddOutcome::Added => outcome.added += 1,
                QueueAddOutcome::AlreadyPresent => outcome.already_present += 1,
                QueueAddOutcome::Unplayable => outcome.unplayable += 1,
            }
        }
        outcome
    }

    pub fn front(&self) -> Option<&MediaItem> {
        self.items.first()
    }

    pub fn remove(&mut self, index: usize) -> Option<MediaItem> {
        (index < self.items.len()).then(|| self.items.remove(index))
    }

    pub fn remove_item(&mut self, item: &MediaItem) -> Option<MediaItem> {
        let identity = item.stable_identity()?;
        let index = self
            .items
            .iter()
            .position(|queued| queued.stable_identity().as_deref() == Some(&identity))?;
        Some(self.items.remove(index))
    }

    pub fn consume_front_if(&mut self, item: &MediaItem) -> bool {
        let Some(identity) = item.stable_identity() else {
            return false;
        };
        if self
            .items
            .first()
            .and_then(MediaItem::stable_identity)
            .as_deref()
            != Some(&identity)
        {
            return false;
        }
        self.items.remove(0);
        true
    }

    pub fn move_by(&mut self, index: usize, delta: i32) -> Option<usize> {
        let target = i64::try_from(index).ok()? + i64::from(delta);
        let target = usize::try_from(target).ok()?;
        if index >= self.items.len() || target >= self.items.len() {
            return None;
        }
        self.items.swap(index, target);
        Some(target)
    }

    pub fn clear(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        self.items.clear();
        true
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};

    use super::{PlaybackQueue, QueueAddOutcome};

    fn item(id: &str, kind: MediaKind) -> MediaItem {
        MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Youtube,
            kind,
            title: id.to_owned(),
            url: Some(
                format!("https://www.youtube.com/watch?v={id}")
                    .parse()
                    .expect("URL"),
            ),
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn add_rejects_collections_and_duplicate_durable_identities() {
        let mut queue = PlaybackQueue::default();
        assert_eq!(
            queue.add(item("one", MediaKind::Video)),
            QueueAddOutcome::Added
        );
        let mut resolved = item("one", MediaKind::Video);
        resolved.stream_url = Some("https://cdn.example/one".parse().expect("stream URL"));
        assert_eq!(queue.add(resolved), QueueAddOutcome::AlreadyPresent);
        assert_eq!(
            queue.add(item("list", MediaKind::Playlist)),
            QueueAddOutcome::Unplayable
        );
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn failed_candidate_is_not_consumed_until_start_is_confirmed() {
        let mut queue = PlaybackQueue::default();
        let first = item("one", MediaKind::Video);
        let second = item("two", MediaKind::Video);
        queue.add(first.clone());
        queue.add(second.clone());
        assert_eq!(queue.front(), Some(&first));
        assert_eq!(queue.front(), Some(&first));
        assert!(!queue.consume_front_if(&second));
        assert!(queue.consume_front_if(&first));
        assert_eq!(queue.front(), Some(&second));
    }

    #[test]
    fn reorder_remove_and_clear_preserve_explicit_order() {
        let mut queue = PlaybackQueue::default();
        for id in ["one", "two", "three"] {
            queue.add(item(id, MediaKind::Audio));
        }
        assert_eq!(queue.move_by(2, -1), Some(1));
        assert_eq!(
            queue
                .items()
                .iter()
                .map(|item| item.id.0.as_str())
                .collect::<Vec<_>>(),
            ["one", "three", "two"]
        );
        assert_eq!(queue.remove(1).expect("removed").id.0, "three");
        assert!(queue.clear());
        assert!(!queue.clear());
    }

    #[test]
    fn batch_add_preserves_order_and_reports_every_disposition() {
        let mut queue = PlaybackQueue::default();
        queue.add(item("one", MediaKind::Audio));
        let outcome = queue.add_many([
            item("two", MediaKind::Audio),
            item("one", MediaKind::Audio),
            item("collection", MediaKind::Playlist),
            item("three", MediaKind::Audio),
        ]);
        assert_eq!(outcome.added, 2);
        assert_eq!(outcome.already_present, 1);
        assert_eq!(outcome.unplayable, 1);
        assert_eq!(
            queue
                .items()
                .iter()
                .map(|item| item.id.0.as_str())
                .collect::<Vec<_>>(),
            ["one", "two", "three"]
        );
    }
}
