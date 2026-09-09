//! Nested `YouTube` playlist and channel collection ownership.

use std::collections::HashSet;

use apricot_core::MediaItem;
use apricot_media::YoutubeCollectionKind;
use thiserror::Error;

use crate::DYNAMIC_SEARCH_PAGE_SIZE;

const MAX_FIXED_RESULTS: i64 = 250;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum YoutubeCollectionPhase {
    #[default]
    LoadingInitial,
    Ready,
    LoadingMore,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YoutubeCollectionWorkKind {
    Initial,
    More,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct YoutubeCollectionWork {
    pub generation: u64,
    pub title: String,
    pub url: String,
    pub kind: YoutubeCollectionKind,
    /// Cumulative item count requested from the source.
    pub limit: u32,
    pub work_kind: YoutubeCollectionWorkKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum YoutubeCollectionApplyOutcome {
    Replaced,
    Appended { added: usize },
    IgnoredStale,
    IgnoredUnexpected,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum YoutubeCollectionError {
    #[error("collection URL is empty")]
    EmptyUrl,
}

#[derive(Debug)]
pub struct YoutubeCollectionSession {
    generation: u64,
    title: String,
    url: String,
    kind: YoutubeCollectionKind,
    phase: YoutubeCollectionPhase,
    items: Vec<MediaItem>,
    selected_identity: Option<String>,
    selected_index: usize,
    dynamic: bool,
    requested_limit: u32,
    source_exhausted: bool,
    last_error: Option<String>,
}

impl YoutubeCollectionSession {
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub const fn kind(&self) -> YoutubeCollectionKind {
        self.kind
    }

    pub const fn phase(&self) -> YoutubeCollectionPhase {
        self.phase
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

    pub const fn is_dynamic(&self) -> bool {
        self.dynamic
    }

    pub const fn can_load_more(&self) -> bool {
        self.dynamic
            && !self.source_exhausted
            && matches!(self.phase, YoutubeCollectionPhase::Ready)
            && !self.items.is_empty()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    fn new(
        generation: u64,
        title: String,
        url: String,
        kind: YoutubeCollectionKind,
        configured_limit: i64,
    ) -> Self {
        let dynamic = configured_limit == 0;
        let requested_limit = if dynamic {
            DYNAMIC_SEARCH_PAGE_SIZE
        } else {
            u32::try_from(configured_limit.clamp(1, MAX_FIXED_RESULTS)).unwrap_or(250)
        };
        Self {
            generation,
            title,
            url,
            kind,
            phase: YoutubeCollectionPhase::LoadingInitial,
            items: Vec::new(),
            selected_identity: None,
            selected_index: 0,
            dynamic,
            requested_limit,
            source_exhausted: false,
            last_error: None,
        }
    }

    fn work(&self, work_kind: YoutubeCollectionWorkKind) -> YoutubeCollectionWork {
        YoutubeCollectionWork {
            generation: self.generation,
            title: self.title.clone(),
            url: self.url.clone(),
            kind: self.kind,
            limit: self.requested_limit,
            work_kind,
        }
    }

    fn request_more(&mut self) -> Option<YoutubeCollectionWork> {
        if !self.can_load_more() {
            return None;
        }
        self.requested_limit = self
            .requested_limit
            .saturating_add(DYNAMIC_SEARCH_PAGE_SIZE);
        self.phase = YoutubeCollectionPhase::LoadingMore;
        self.last_error = None;
        Some(self.work(YoutubeCollectionWorkKind::More))
    }

    fn cancel_pending(&mut self) -> bool {
        match self.phase {
            YoutubeCollectionPhase::LoadingMore => {
                self.phase = YoutubeCollectionPhase::Ready;
                self.requested_limit = self
                    .requested_limit
                    .saturating_sub(DYNAMIC_SEARCH_PAGE_SIZE)
                    .max(DYNAMIC_SEARCH_PAGE_SIZE);
            }
            YoutubeCollectionPhase::LoadingInitial
            | YoutubeCollectionPhase::Ready
            | YoutubeCollectionPhase::Failed => return false,
        }
        self.last_error = None;
        true
    }

    fn apply_results(
        &mut self,
        generation: u64,
        fetched: Vec<MediaItem>,
    ) -> YoutubeCollectionApplyOutcome {
        if generation != self.generation {
            return YoutubeCollectionApplyOutcome::IgnoredStale;
        }
        match self.phase {
            YoutubeCollectionPhase::LoadingInitial => {
                let fetched_count = fetched.len();
                self.items = dedupe(fetched);
                self.source_exhausted = fetched_count < self.requested_limit as usize;
                self.phase = YoutubeCollectionPhase::Ready;
                self.last_error = None;
                self.restore_selection(None, 0);
                YoutubeCollectionApplyOutcome::Replaced
            }
            YoutubeCollectionPhase::LoadingMore => {
                let fetched_count = fetched.len();
                let previous_len = self.items.len();
                let old_identity = self.selected_identity.clone();
                let old_index = self.selected_index;
                merge_unique(&mut self.items, fetched);
                let added = self.items.len().saturating_sub(previous_len);
                self.source_exhausted = added == 0 || fetched_count < self.requested_limit as usize;
                self.phase = YoutubeCollectionPhase::Ready;
                self.last_error = None;
                self.restore_selection(old_identity.as_deref(), old_index);
                YoutubeCollectionApplyOutcome::Appended { added }
            }
            YoutubeCollectionPhase::Ready | YoutubeCollectionPhase::Failed => {
                YoutubeCollectionApplyOutcome::IgnoredUnexpected
            }
        }
    }

    fn fail(&mut self, generation: u64, message: impl Into<String>) -> bool {
        if generation != self.generation {
            return false;
        }
        match self.phase {
            YoutubeCollectionPhase::LoadingInitial => self.phase = YoutubeCollectionPhase::Failed,
            YoutubeCollectionPhase::LoadingMore => {
                self.phase = YoutubeCollectionPhase::Ready;
                self.requested_limit = self
                    .requested_limit
                    .saturating_sub(DYNAMIC_SEARCH_PAGE_SIZE)
                    .max(DYNAMIC_SEARCH_PAGE_SIZE);
            }
            YoutubeCollectionPhase::Ready | YoutubeCollectionPhase::Failed => return false,
        }
        self.last_error = Some(message.into());
        true
    }

    fn select(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.selected_index = index;
        self.selected_identity = item_identity(&self.items[index]);
        true
    }

    fn apply_metadata(&mut self, generation: u64, hydrated: &MediaItem) -> bool {
        if generation != self.generation {
            return false;
        }
        let Some(identity) = item_identity(hydrated) else {
            return false;
        };
        self.items
            .iter_mut()
            .find(|item| item_identity(item).as_deref() == Some(identity.as_str()))
            .is_some_and(|item| item.merge_descriptive_metadata(hydrated))
    }

    fn restore_selection(&mut self, identity: Option<&str>, fallback_index: usize) {
        if self.items.is_empty() {
            self.selected_identity = None;
            self.selected_index = 0;
            return;
        }
        self.selected_index = identity
            .and_then(|identity| {
                self.items
                    .iter()
                    .position(|item| item_identity(item).as_deref() == Some(identity))
            })
            .unwrap_or_else(|| fallback_index.min(self.items.len() - 1));
        self.selected_identity = item_identity(&self.items[self.selected_index]);
    }
}

#[derive(Debug, Default)]
pub struct YoutubeCollectionController {
    next_generation: u64,
    sessions: Vec<YoutubeCollectionSession>,
}

impl YoutubeCollectionController {
    pub fn current(&self) -> Option<&YoutubeCollectionSession> {
        self.sessions.last()
    }

    pub const fn depth(&self) -> usize {
        self.sessions.len()
    }

    /// Starts and pushes one nested collection session.
    ///
    /// # Errors
    ///
    /// Returns [`YoutubeCollectionError::EmptyUrl`] when the URL is blank.
    pub fn begin(
        &mut self,
        title: impl Into<String>,
        url: impl Into<String>,
        kind: YoutubeCollectionKind,
        configured_limit: i64,
    ) -> Result<YoutubeCollectionWork, YoutubeCollectionError> {
        let url = url.into();
        if url.trim().is_empty() {
            return Err(YoutubeCollectionError::EmptyUrl);
        }
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let session = YoutubeCollectionSession::new(
            self.next_generation,
            title.into(),
            url,
            kind,
            configured_limit,
        );
        let work = session.work(YoutubeCollectionWorkKind::Initial);
        self.sessions.push(session);
        Ok(work)
    }

    pub fn request_more(&mut self) -> Option<YoutubeCollectionWork> {
        self.sessions.last_mut()?.request_more()
    }

    pub fn cancel_pending(&mut self) -> bool {
        self.sessions
            .last_mut()
            .is_some_and(YoutubeCollectionSession::cancel_pending)
    }

    pub fn apply_results(
        &mut self,
        generation: u64,
        items: Vec<MediaItem>,
    ) -> YoutubeCollectionApplyOutcome {
        self.sessions
            .last_mut()
            .map_or(YoutubeCollectionApplyOutcome::IgnoredStale, |session| {
                session.apply_results(generation, items)
            })
    }

    pub fn apply_metadata(&mut self, generation: u64, hydrated: &MediaItem) -> bool {
        self.sessions
            .last_mut()
            .is_some_and(|session| session.apply_metadata(generation, hydrated))
    }

    pub fn fail(&mut self, generation: u64, message: impl Into<String>) -> bool {
        self.sessions
            .last_mut()
            .is_some_and(|session| session.fail(generation, message))
    }

    pub fn select(&mut self, index: usize) -> bool {
        self.sessions
            .last_mut()
            .is_some_and(|session| session.select(index))
    }

    pub fn pop(&mut self) -> Option<YoutubeCollectionSession> {
        self.sessions.pop()
    }

    pub fn clear(&mut self) {
        self.sessions.clear();
    }
}

fn dedupe(items: Vec<MediaItem>) -> Vec<MediaItem> {
    let mut output = Vec::with_capacity(items.len());
    merge_unique(&mut output, items);
    output
}

fn merge_unique(existing: &mut Vec<MediaItem>, fetched: Vec<MediaItem>) {
    let mut seen: HashSet<String> = existing.iter().filter_map(item_identity).collect();
    for item in fetched {
        let Some(identity) = item_identity(&item) else {
            existing.push(item);
            continue;
        };
        if seen.insert(identity) {
            existing.push(item);
        }
    }
}

fn item_identity(item: &MediaItem) -> Option<String> {
    item.stable_identity()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_media::YoutubeCollectionKind;

    use super::{
        YoutubeCollectionApplyOutcome, YoutubeCollectionController, YoutubeCollectionWorkKind,
    };

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
    fn nested_sessions_restore_the_parent_selection_one_level_at_a_time() {
        let mut controller = YoutubeCollectionController::default();
        let channel = controller
            .begin(
                "Channel playlists",
                "https://www.youtube.com/@creator/playlists",
                YoutubeCollectionKind::ChannelPlaylists,
                0,
            )
            .expect("channel");
        controller.apply_results(
            channel.generation,
            vec![
                item("playlist-one", MediaKind::Playlist),
                item("playlist-two", MediaKind::Playlist),
            ],
        );
        assert!(controller.select(1));

        let playlist = controller
            .begin(
                "Playlist two",
                "https://www.youtube.com/playlist?list=playlist-two",
                YoutubeCollectionKind::PlaylistVideos,
                0,
            )
            .expect("playlist");
        controller.apply_results(
            playlist.generation,
            vec![item("video-one", MediaKind::Video)],
        );
        assert_eq!(controller.depth(), 2);
        assert_eq!(controller.current().expect("child").title(), "Playlist two");

        controller.pop();
        let restored = controller.current().expect("parent");
        assert_eq!(restored.title(), "Channel playlists");
        assert_eq!(restored.selected_index(), 1);
    }

    #[test]
    fn dynamic_collection_append_is_cumulative_deduplicated_and_focus_stable() {
        let mut controller = YoutubeCollectionController::default();
        let initial = controller
            .begin(
                "Playlist",
                "https://www.youtube.com/playlist?list=PL123",
                YoutubeCollectionKind::PlaylistVideos,
                0,
            )
            .expect("collection");
        let first: Vec<_> = (0..20)
            .map(|index| item(&index.to_string(), MediaKind::Video))
            .collect();
        assert_eq!(
            controller.apply_results(initial.generation, first),
            YoutubeCollectionApplyOutcome::Replaced
        );
        assert!(controller.select(12));
        let more = controller.request_more().expect("more");
        assert_eq!(more.limit, 40);
        assert_eq!(more.work_kind, YoutubeCollectionWorkKind::More);
        let cumulative: Vec<_> = (0..40)
            .map(|index| item(&index.to_string(), MediaKind::Video))
            .collect();
        assert_eq!(
            controller.apply_results(more.generation, cumulative),
            YoutubeCollectionApplyOutcome::Appended { added: 20 }
        );
        let current = controller.current().expect("current");
        assert_eq!(current.items().len(), 40);
        assert_eq!(current.selected_index(), 12);
    }

    #[test]
    fn collection_metadata_hydration_preserves_focus_and_rejects_stale_generations() {
        let mut controller = YoutubeCollectionController::default();
        let work = controller
            .begin(
                "Playlist",
                "https://www.youtube.com/playlist?list=PL123",
                YoutubeCollectionKind::PlaylistVideos,
                0,
            )
            .expect("collection");
        let first = item("first", MediaKind::Video);
        controller.apply_results(
            work.generation,
            vec![first.clone(), item("second", MediaKind::Video)],
        );
        assert!(controller.select(1));
        let mut hydrated = first;
        hydrated.channel = "Hydrated channel".to_owned();
        hydrated
            .metadata
            .insert("upload_date".to_owned(), "20260101".into());

        assert!(!controller.apply_metadata(work.generation + 1, &hydrated));
        assert!(controller.apply_metadata(work.generation, &hydrated));
        let current = controller.current().expect("current");
        assert_eq!(current.selected_index(), 1);
        assert_eq!(current.items()[0].channel, "Hydrated channel");
    }
}
