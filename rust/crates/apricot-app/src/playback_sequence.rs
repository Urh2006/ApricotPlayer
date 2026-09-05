//! Stable source sequence for deterministic previous and next playback.

use apricot_core::MediaItem;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackSequenceSource {
    Search { generation: u64 },
    Collection,
}

#[derive(Debug, Default)]
pub struct PlaybackSequence {
    source: Option<PlaybackSequenceSource>,
    items: Vec<MediaItem>,
    current_identity: Option<String>,
}

impl PlaybackSequence {
    pub const fn source(&self) -> Option<PlaybackSequenceSource> {
        self.source
    }

    pub fn items(&self) -> &[MediaItem] {
        &self.items
    }

    pub fn set(
        &mut self,
        source: PlaybackSequenceSource,
        items: &[MediaItem],
        current: &MediaItem,
    ) -> bool {
        let Some(current_identity) = current.stable_identity() else {
            self.clear();
            return false;
        };
        let playable: Vec<_> = items
            .iter()
            .filter(|item| item.is_playable())
            .cloned()
            .collect();
        if !playable
            .iter()
            .any(|item| item.stable_identity().as_deref() == Some(&current_identity))
        {
            self.clear();
            return false;
        }
        self.source = Some(source);
        self.items = playable;
        self.current_identity = Some(current_identity);
        true
    }

    pub fn sync(&mut self, source: PlaybackSequenceSource, items: &[MediaItem]) -> bool {
        if self.source != Some(source) {
            return false;
        }
        let Some(current_identity) = self.current_identity.as_deref() else {
            return false;
        };
        let playable: Vec<_> = items
            .iter()
            .filter(|item| item.is_playable())
            .cloned()
            .collect();
        if !playable
            .iter()
            .any(|item| item.stable_identity().as_deref() == Some(current_identity))
        {
            self.clear();
            return false;
        }
        self.items = playable;
        true
    }

    pub fn activate(&mut self, item: &MediaItem) -> bool {
        let Some(identity) = item.stable_identity() else {
            self.clear();
            return false;
        };
        if self
            .items
            .iter()
            .any(|candidate| candidate.stable_identity().as_deref() == Some(&identity))
        {
            self.current_identity = Some(identity);
            true
        } else {
            self.clear();
            false
        }
    }

    pub fn relative(&self, delta: i32) -> Option<MediaItem> {
        let current = self.current_index()?;
        let candidate = i64::try_from(current).ok()? + i64::from(delta);
        let candidate = usize::try_from(candidate).ok()?;
        self.items.get(candidate).cloned()
    }

    pub fn clear(&mut self) {
        self.source = None;
        self.items.clear();
        self.current_identity = None;
    }

    fn current_index(&self) -> Option<usize> {
        let identity = self.current_identity.as_deref()?;
        self.items
            .iter()
            .position(|item| item.stable_identity().as_deref() == Some(identity))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};

    use super::{PlaybackSequence, PlaybackSequenceSource};

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
    fn relative_navigation_filters_collections_without_changing_order() {
        let items = vec![
            item("one", MediaKind::Video),
            item("channel", MediaKind::Channel),
            item("two", MediaKind::Video),
            item("playlist", MediaKind::Playlist),
            item("three", MediaKind::LiveStream),
        ];
        let mut sequence = PlaybackSequence::default();
        assert!(sequence.set(
            PlaybackSequenceSource::Search { generation: 4 },
            &items,
            &items[2],
        ));
        assert_eq!(sequence.relative(-1).expect("previous").id.0, "one");
        assert_eq!(sequence.relative(1).expect("next").id.0, "three");
        assert_eq!(sequence.items().len(), 3);
    }

    #[test]
    fn activating_resolved_copy_uses_durable_identity() {
        let current = item("two", MediaKind::Video);
        let items = vec![item("one", MediaKind::Video), current.clone()];
        let mut sequence = PlaybackSequence::default();
        assert!(sequence.set(PlaybackSequenceSource::Collection, &items, &current));
        let mut resolved = current;
        resolved.stream_url = Some("https://cdn.example/expiring".parse().expect("stream URL"));
        assert!(sequence.activate(&resolved));
        assert_eq!(sequence.relative(-1).expect("previous").id.0, "one");
    }

    #[test]
    fn only_matching_search_generation_can_extend_sequence() {
        let current = item("one", MediaKind::Video);
        let mut sequence = PlaybackSequence::default();
        assert!(sequence.set(
            PlaybackSequenceSource::Search { generation: 8 },
            std::slice::from_ref(&current),
            &current,
        ));
        assert!(!sequence.sync(
            PlaybackSequenceSource::Search { generation: 9 },
            &[current.clone(), item("wrong", MediaKind::Video)],
        ));
        assert!(sequence.relative(1).is_none());
        assert!(sequence.sync(
            PlaybackSequenceSource::Search { generation: 8 },
            &[current, item("next", MediaKind::Video)],
        ));
        assert_eq!(sequence.relative(1).expect("next").id.0, "next");
    }
}
