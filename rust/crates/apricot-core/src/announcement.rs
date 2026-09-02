//! Deterministic screen-reader announcement ordering and stale-task rejection.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnnouncementPriority {
    Routine,
    Important,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GenerationToken {
    source: String,
    generation: u64,
}

impl GenerationToken {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnouncementRequest {
    pub text: String,
    pub dedupe_key: String,
    pub priority: AnnouncementPriority,
    pub interrupt: bool,
    pub generation: Option<GenerationToken>,
}

impl AnnouncementRequest {
    pub fn status(text: impl Into<String>, dedupe_key: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            dedupe_key: dedupe_key.into(),
            priority: AnnouncementPriority::Routine,
            interrupt: false,
            generation: None,
        }
    }

    #[must_use]
    pub fn for_generation(mut self, token: GenerationToken) -> Self {
        self.generation = Some(token);
        self
    }
}

#[derive(Debug)]
pub struct AnnouncementBroker {
    repeat_window_ms: u64,
    generations: BTreeMap<String, u64>,
    delivered: BTreeMap<String, (String, u64)>,
}

impl AnnouncementBroker {
    pub fn new(repeat_window_ms: u64) -> Self {
        Self {
            repeat_window_ms,
            generations: BTreeMap::new(),
            delivered: BTreeMap::new(),
        }
    }

    pub fn begin_generation(&mut self, source: impl Into<String>) -> GenerationToken {
        let source = source.into();
        let generation = self
            .generations
            .get(&source)
            .copied()
            .unwrap_or_default()
            .saturating_add(1);
        self.generations.insert(source.clone(), generation);
        GenerationToken { source, generation }
    }

    pub fn accept(
        &mut self,
        request: AnnouncementRequest,
        now_ms: u64,
    ) -> Option<AnnouncementRequest> {
        if let Some(token) = &request.generation
            && self.generations.get(token.source()).copied() != Some(token.generation())
        {
            return None;
        }
        if let Some((previous_text, previous_at)) = self.delivered.get(&request.dedupe_key)
            && previous_text == &request.text
            && now_ms.saturating_sub(*previous_at) < self.repeat_window_ms
        {
            return None;
        }
        self.delivered
            .insert(request.dedupe_key.clone(), (request.text.clone(), now_ms));
        Some(request)
    }
}

impl Default for AnnouncementBroker {
    fn default() -> Self {
        Self::new(750)
    }
}

#[cfg(test)]
mod tests {
    use super::{AnnouncementBroker, AnnouncementRequest};

    #[test]
    fn stale_generations_and_immediate_duplicates_are_suppressed() {
        let mut broker = AnnouncementBroker::new(500);
        let old = broker.begin_generation("search");
        let current = broker.begin_generation("search");
        let stale = AnnouncementRequest::status("Old results", "results").for_generation(old);
        assert_eq!(broker.accept(stale, 100), None);

        let fresh =
            AnnouncementRequest::status("20 results", "results").for_generation(current.clone());
        assert!(broker.accept(fresh.clone(), 100).is_some());
        assert_eq!(broker.accept(fresh.clone(), 200), None);
        assert!(broker.accept(fresh, 700).is_some());
    }

    #[test]
    fn sources_have_independent_generations() {
        let mut broker = AnnouncementBroker::default();
        let search = broker.begin_generation("search");
        let download = broker.begin_generation("download");
        assert!(
            broker
                .accept(
                    AnnouncementRequest::status("Search done", "search").for_generation(search),
                    10,
                )
                .is_some()
        );
        assert!(
            broker
                .accept(
                    AnnouncementRequest::status("Download done", "download")
                        .for_generation(download),
                    10,
                )
                .is_some()
        );
    }
}
