//! Search ownership independent of platform controls and background workers.

use std::collections::HashSet;

use apricot_core::MediaItem;
use apricot_media::YoutubeSearchKind;
use thiserror::Error;

pub const DYNAMIC_SEARCH_PAGE_SIZE: u32 = 20;
const MAX_FIXED_RESULTS: i64 = 250;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchPhase {
    #[default]
    Idle,
    LoadingInitial,
    Ready,
    LoadingMore,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchWorkKind {
    Initial,
    More,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchWork {
    pub generation: u64,
    pub query: String,
    pub kind: YoutubeSearchKind,
    /// Total result count requested from the source. Sources which expose a
    /// continuation may still return only the next page.
    pub limit: u32,
    pub work_kind: SearchWorkKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchApplyOutcome {
    Replaced,
    Appended { added: usize },
    IgnoredStale,
    IgnoredUnexpected,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SearchSessionError {
    #[error("search query is empty")]
    EmptyQuery,
}

#[derive(Debug)]
pub struct SearchSession {
    generation: u64,
    query: String,
    kind: YoutubeSearchKind,
    phase: SearchPhase,
    items: Vec<MediaItem>,
    continuation: Option<String>,
    selected_identity: Option<String>,
    selected_index: usize,
    dynamic: bool,
    requested_limit: u32,
    source_exhausted: bool,
    last_error: Option<String>,
}

impl Default for SearchSession {
    fn default() -> Self {
        Self {
            generation: 0,
            query: String::new(),
            kind: YoutubeSearchKind::All,
            phase: SearchPhase::Idle,
            items: Vec::new(),
            continuation: None,
            selected_identity: None,
            selected_index: 0,
            dynamic: true,
            requested_limit: DYNAMIC_SEARCH_PAGE_SIZE,
            source_exhausted: false,
            last_error: None,
        }
    }
}

impl SearchSession {
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub const fn phase(&self) -> SearchPhase {
        self.phase
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub const fn kind(&self) -> YoutubeSearchKind {
        self.kind
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

    pub fn continuation(&self) -> Option<&str> {
        self.continuation.as_deref()
    }

    pub const fn is_dynamic(&self) -> bool {
        self.dynamic
    }

    pub const fn can_load_more(&self) -> bool {
        self.dynamic
            && !self.source_exhausted
            && matches!(self.phase, SearchPhase::Ready)
            && !self.items.is_empty()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn restore_snapshot(
        &mut self,
        query: impl Into<String>,
        kind: YoutubeSearchKind,
        items: Vec<MediaItem>,
        selected_index: usize,
    ) -> bool {
        if items.is_empty() {
            return false;
        }
        self.generation = self.generation.wrapping_add(1).max(1);
        self.query = query.into();
        self.kind = kind;
        self.phase = SearchPhase::Ready;
        self.items = dedupe(items);
        if self.items.is_empty() {
            return false;
        }
        self.continuation = None;
        self.selected_index = selected_index.min(self.items.len() - 1);
        self.selected_identity = item_identity(&self.items[self.selected_index]);
        self.dynamic = false;
        self.requested_limit = u32::try_from(self.items.len()).unwrap_or(u32::MAX);
        self.source_exhausted = true;
        self.last_error = None;
        true
    }

    /// Starts a new logical search and invalidates every older response.
    ///
    /// A configured limit of zero enables Python-compatible dynamic loading in
    /// increments of 20; fixed limits are clamped to the Settings range.
    ///
    /// # Errors
    ///
    /// Returns an error without changing the active search when the query is
    /// empty after trimming.
    pub fn begin(
        &mut self,
        query: &str,
        kind: YoutubeSearchKind,
        configured_limit: i64,
    ) -> Result<SearchWork, SearchSessionError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(SearchSessionError::EmptyQuery);
        }
        self.generation = self.generation.wrapping_add(1).max(1);
        query.clone_into(&mut self.query);
        self.kind = kind;
        self.phase = SearchPhase::LoadingInitial;
        self.items.clear();
        self.continuation = None;
        self.selected_identity = None;
        self.selected_index = 0;
        self.dynamic = configured_limit == 0;
        self.requested_limit = if self.dynamic {
            DYNAMIC_SEARCH_PAGE_SIZE
        } else {
            u32::try_from(configured_limit.clamp(1, MAX_FIXED_RESULTS)).unwrap_or(250)
        };
        self.source_exhausted = false;
        self.last_error = None;
        Ok(self.work(SearchWorkKind::Initial))
    }

    /// Requests the next dynamic page while preserving the active generation
    /// and selection. Only one append request may be active at a time.
    pub fn request_more(&mut self) -> Option<SearchWork> {
        if !self.can_load_more() {
            return None;
        }
        self.requested_limit = self
            .requested_limit
            .saturating_add(DYNAMIC_SEARCH_PAGE_SIZE);
        self.phase = SearchPhase::LoadingMore;
        self.last_error = None;
        Some(self.work(SearchWorkKind::More))
    }

    /// Invalidates an outstanding response while retaining completed results.
    pub fn cancel_pending(&mut self) -> bool {
        match self.phase {
            SearchPhase::LoadingInitial => {
                self.phase = SearchPhase::Idle;
                self.items.clear();
                self.selected_identity = None;
                self.selected_index = 0;
            }
            SearchPhase::LoadingMore => {
                self.phase = SearchPhase::Ready;
                self.requested_limit = self
                    .requested_limit
                    .saturating_sub(DYNAMIC_SEARCH_PAGE_SIZE)
                    .max(DYNAMIC_SEARCH_PAGE_SIZE);
            }
            SearchPhase::Idle | SearchPhase::Ready | SearchPhase::Failed => return false,
        }
        self.generation = self.generation.wrapping_add(1).max(1);
        self.last_error = None;
        true
    }

    /// Applies a response only when it belongs to the current logical search
    /// and an initial or append operation is awaiting that response.
    pub fn apply_results(
        &mut self,
        generation: u64,
        fetched: Vec<MediaItem>,
        continuation: Option<String>,
    ) -> SearchApplyOutcome {
        if generation != self.generation {
            return SearchApplyOutcome::IgnoredStale;
        }
        match self.phase {
            SearchPhase::LoadingInitial => {
                let fetched_count = fetched.len();
                self.items = dedupe(fetched);
                self.continuation = continuation;
                self.source_exhausted =
                    self.continuation.is_none() && fetched_count < self.requested_limit as usize;
                self.phase = SearchPhase::Ready;
                self.last_error = None;
                self.restore_selection(None, 0);
                SearchApplyOutcome::Replaced
            }
            SearchPhase::LoadingMore => {
                let fetched_count = fetched.len();
                let previous_len = self.items.len();
                let old_identity = self.selected_identity.clone();
                let old_index = self.selected_index;
                merge_unique(&mut self.items, fetched);
                let added = self.items.len().saturating_sub(previous_len);
                self.continuation = continuation;
                self.source_exhausted = self.continuation.is_none()
                    && (added == 0 || fetched_count < self.requested_limit as usize);
                self.phase = SearchPhase::Ready;
                self.last_error = None;
                self.restore_selection(old_identity.as_deref(), old_index);
                SearchApplyOutcome::Appended { added }
            }
            SearchPhase::Idle | SearchPhase::Ready | SearchPhase::Failed => {
                SearchApplyOutcome::IgnoredUnexpected
            }
        }
    }

    /// Records a current-generation failure. Append failures retain all loaded
    /// items and leave the session retryable.
    pub fn fail(&mut self, generation: u64, message: impl Into<String>) -> bool {
        if generation != self.generation {
            return false;
        }
        match self.phase {
            SearchPhase::LoadingInitial => self.phase = SearchPhase::Failed,
            SearchPhase::LoadingMore => {
                self.phase = SearchPhase::Ready;
                self.requested_limit = self
                    .requested_limit
                    .saturating_sub(DYNAMIC_SEARCH_PAGE_SIZE)
                    .max(DYNAMIC_SEARCH_PAGE_SIZE);
            }
            SearchPhase::Idle | SearchPhase::Ready | SearchPhase::Failed => return false,
        }
        self.last_error = Some(message.into());
        true
    }

    pub fn select(&mut self, index: usize) -> bool {
        if self.items.is_empty() || index >= self.items.len() {
            return false;
        }
        self.selected_index = index;
        self.selected_identity = item_identity(&self.items[index]);
        true
    }

    pub fn apply_metadata(&mut self, generation: u64, hydrated: &MediaItem) -> bool {
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

    fn work(&self, work_kind: SearchWorkKind) -> SearchWork {
        SearchWork {
            generation: self.generation,
            query: self.query.clone(),
            kind: self.kind,
            limit: self.requested_limit,
            work_kind,
        }
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

    use super::{SearchApplyOutcome, SearchPhase, SearchSession, SearchWorkKind};
    use apricot_media::YoutubeSearchKind;

    fn item(id: &str) -> MediaItem {
        MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Youtube,
            kind: MediaKind::Video,
            title: id.to_owned(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: None,
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn stale_results_cannot_replace_a_newer_search() {
        let mut session = SearchSession::default();
        let old = session
            .begin("old", YoutubeSearchKind::Video, 0)
            .expect("old search");
        let new = session
            .begin("new", YoutubeSearchKind::Playlist, 0)
            .expect("new search");
        assert_eq!(
            session.apply_results(old.generation, vec![item("old")], None),
            SearchApplyOutcome::IgnoredStale
        );
        assert_eq!(session.phase(), SearchPhase::LoadingInitial);
        session.apply_results(new.generation, vec![item("new")], None);
        assert_eq!(session.items()[0].id.0, "new");
    }

    #[test]
    fn dynamic_loading_grows_by_twenty_and_preserves_selection_and_order() {
        let mut session = SearchSession::default();
        let initial = session
            .begin("query", YoutubeSearchKind::All, 0)
            .expect("search");
        assert_eq!(initial.limit, 20);
        let first_page: Vec<_> = (0..20).map(|index| item(&index.to_string())).collect();
        session.apply_results(initial.generation, first_page, None);
        assert!(session.select(12));

        let more = session.request_more().expect("more work");
        assert_eq!(more.limit, 40);
        assert_eq!(more.work_kind, SearchWorkKind::More);
        let mut cumulative: Vec<_> = (0..20).map(|index| item(&index.to_string())).collect();
        cumulative.extend((20..40).map(|index| item(&index.to_string())));
        assert_eq!(
            session.apply_results(more.generation, cumulative, None),
            SearchApplyOutcome::Appended { added: 20 }
        );
        assert_eq!(session.items().len(), 40);
        assert_eq!(session.selected_index(), 12);
        assert_eq!(session.selected_item().expect("selected").id.0, "12");
        assert!(
            session
                .items()
                .iter()
                .enumerate()
                .all(|(index, item)| item.id.0 == index.to_string())
        );
    }

    #[test]
    fn restored_snapshot_is_stable_and_cannot_request_an_unknown_next_page() {
        let mut session = SearchSession::default();
        assert!(session.restore_snapshot(
            "remembered query",
            YoutubeSearchKind::Video,
            vec![item("one"), item("two"), item("three")],
            1,
        ));
        assert_eq!(session.phase(), SearchPhase::Ready);
        assert_eq!(session.query(), "remembered query");
        assert_eq!(session.kind(), YoutubeSearchKind::Video);
        assert_eq!(session.selected_item().expect("selected").id.0, "two");
        assert!(!session.is_dynamic());
        assert!(!session.can_load_more());
        assert!(session.request_more().is_none());
    }

    #[test]
    fn duplicate_or_reordered_fetch_cannot_randomize_existing_results() {
        let mut session = SearchSession::default();
        let initial = session
            .begin("query", YoutubeSearchKind::Video, 0)
            .expect("search");
        session.apply_results(
            initial.generation,
            (0..20).map(|index| item(&index.to_string())).collect(),
            None,
        );
        let more = session.request_more().expect("more");
        let mut reordered: Vec<_> = (0..20)
            .rev()
            .map(|index| item(&index.to_string()))
            .collect();
        reordered.extend([item("20"), item("21")]);
        session.apply_results(more.generation, reordered, None);
        assert_eq!(session.items().len(), 22);
        assert!(
            session.items()[..20]
                .iter()
                .enumerate()
                .all(|(index, item)| item.id.0 == index.to_string())
        );
        assert_eq!(session.items()[20].id.0, "20");
        assert_eq!(session.items()[21].id.0, "21");
    }

    #[test]
    fn append_failure_keeps_results_and_can_be_retried() {
        let mut session = SearchSession::default();
        let initial = session
            .begin("query", YoutubeSearchKind::Video, 0)
            .expect("search");
        session.apply_results(
            initial.generation,
            (0..20).map(|index| item(&index.to_string())).collect(),
            None,
        );
        let more = session.request_more().expect("more");
        assert!(session.fail(more.generation, "temporary failure"));
        assert_eq!(session.phase(), SearchPhase::Ready);
        assert_eq!(session.items().len(), 20);
        assert_eq!(session.last_error(), Some("temporary failure"));
        assert_eq!(session.request_more().expect("retry").limit, 40);
    }

    #[test]
    fn fixed_limit_does_not_offer_dynamic_loading() {
        let mut session = SearchSession::default();
        let work = session
            .begin("query", YoutubeSearchKind::Video, 50)
            .expect("search");
        assert_eq!(work.limit, 50);
        session.apply_results(
            work.generation,
            (0..50).map(|index| item(&index.to_string())).collect(),
            None,
        );
        assert!(!session.is_dynamic());
        assert!(!session.can_load_more());
        assert!(session.request_more().is_none());
    }

    #[test]
    fn empty_query_does_not_invalidate_the_active_search() {
        let mut session = SearchSession::default();
        let active = session
            .begin("query", YoutubeSearchKind::Video, 0)
            .expect("search");
        assert!(session.begin("  ", YoutubeSearchKind::All, 0).is_err());
        assert_eq!(session.generation(), active.generation);
        assert_eq!(session.query(), "query");
    }

    #[test]
    fn cancelling_invalidates_late_results_and_retains_completed_pages() {
        let mut session = SearchSession::default();
        let initial = session
            .begin("query", YoutubeSearchKind::Video, 0)
            .expect("search");
        session.apply_results(
            initial.generation,
            (0..20).map(|index| item(&index.to_string())).collect(),
            None,
        );
        let more = session.request_more().expect("more");
        assert!(session.cancel_pending());
        assert_eq!(session.phase(), SearchPhase::Ready);
        assert_eq!(session.items().len(), 20);
        assert_eq!(
            session.apply_results(more.generation, vec![item("late")], None),
            SearchApplyOutcome::IgnoredStale
        );
        assert_eq!(
            session.request_more().expect("retry after return").limit,
            40
        );
    }

    #[test]
    fn metadata_hydration_is_generation_scoped_and_preserves_selection_and_urls() {
        let mut session = SearchSession::default();
        let work = session
            .begin("query", YoutubeSearchKind::Video, 0)
            .expect("search");
        let mut first = item("first");
        first.url = Some(
            "https://www.youtube.com/watch?v=first"
                .parse()
                .expect("URL"),
        );
        first.stream_url = Some("https://media.example/first".parse().expect("stream URL"));
        let second = item("second");
        session.apply_results(work.generation, vec![first.clone(), second], None);
        assert!(session.select(1));

        let mut hydrated = first.clone();
        hydrated.title = "Hydrated first".to_owned();
        hydrated
            .metadata
            .insert("view_count".to_owned(), 42_u64.into());
        hydrated.stream_url = Some("https://media.example/replacement".parse().expect("stream"));
        assert!(!session.apply_metadata(work.generation + 1, &hydrated));
        assert!(session.apply_metadata(work.generation, &hydrated));

        assert_eq!(session.selected_index(), 1);
        assert_eq!(session.items()[0].title, "Hydrated first");
        assert_eq!(session.items()[0].url, first.url);
        assert_eq!(session.items()[0].stream_url, first.stream_url);
    }
}
