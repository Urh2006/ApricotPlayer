//! `YouTube` search command sequencing above the bounded component runtime.

use std::path::Path;

use apricot_core::MediaItem;
use apricot_media::{
    YoutubeBackend, YoutubeCommand, YoutubeResponsePayload, YoutubeRuntime, YoutubeRuntimeError,
    YoutubeSearchKind, YoutubeSessionConfig,
};
use thiserror::Error;

use crate::spawn_youtube_runtime;

#[derive(Clone, Debug, PartialEq)]
pub enum YoutubeSearchServiceUpdate {
    Results {
        generation: u64,
        items: Vec<MediaItem>,
        continuation: Option<String>,
    },
    Failed {
        generation: u64,
        message: String,
    },
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum YoutubeSearchServiceError {
    #[error(transparent)]
    Runtime(#[from] YoutubeRuntimeError),
    #[error("a YouTube search is already active")]
    Busy,
}

#[derive(Clone, Debug)]
struct PendingSearch {
    generation: u64,
    query: String,
    kind: YoutubeSearchKind,
    limit: u32,
    stage: SearchStage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SearchStage {
    Configuring,
    Searching,
}

#[derive(Default)]
pub struct YoutubeSearchService {
    runtime: Option<YoutubeRuntime>,
    backend: Option<YoutubeBackend>,
    pending: Option<PendingSearch>,
}

impl YoutubeSearchService {
    /// Starts one search without blocking the caller or launching a component
    /// before it is needed.
    ///
    /// # Errors
    ///
    /// Returns an error when another request is active, the runtime cannot be
    /// created, or its bounded request queue cannot accept configuration.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        backend: YoutubeBackend,
        components_directory: &Path,
        config: YoutubeSessionConfig,
        generation: u64,
        query: String,
        kind: YoutubeSearchKind,
        limit: u32,
    ) -> Result<(), YoutubeSearchServiceError> {
        if self.pending.is_some() {
            return Err(YoutubeSearchServiceError::Busy);
        }
        if self.backend != Some(backend) {
            self.runtime = None;
            self.backend = None;
        }
        if self.runtime.is_none() {
            self.runtime = Some(spawn_youtube_runtime(backend, components_directory)?);
            self.backend = Some(backend);
        }
        let runtime = self.runtime.as_ref().ok_or(YoutubeRuntimeError::Stopped)?;
        runtime.execute(generation, YoutubeCommand::Configure { config })?;
        self.pending = Some(PendingSearch {
            generation,
            query,
            kind,
            limit,
            stage: SearchStage::Configuring,
        });
        Ok(())
    }

    /// Polls currently buffered component events. Configuration acknowledgments
    /// are consumed internally; callers receive only terminal search outcomes.
    ///
    /// # Errors
    ///
    /// Returns an error when the runtime worker has stopped.
    pub fn poll(
        &mut self,
    ) -> Result<Option<YoutubeSearchServiceUpdate>, YoutubeSearchServiceError> {
        loop {
            let Some(runtime) = self.runtime.as_ref() else {
                return Ok(None);
            };
            let Some(update) = runtime.poll_update()? else {
                return Ok(None);
            };
            let Some(pending) = self.pending.as_mut() else {
                continue;
            };
            if update.generation != pending.generation {
                continue;
            }
            match update.result {
                Ok(YoutubeResponsePayload::Configured)
                    if pending.stage == SearchStage::Configuring =>
                {
                    let command = YoutubeCommand::Search {
                        query: pending.query.clone(),
                        kind: pending.kind,
                        limit: pending.limit,
                        safe_search: false,
                    };
                    if let Err(error) = runtime.execute(pending.generation, command) {
                        return Ok(Some(self.fail_pending(error.to_string())));
                    }
                    pending.stage = SearchStage::Searching;
                }
                Ok(YoutubeResponsePayload::SearchResults {
                    items,
                    continuation,
                }) if pending.stage == SearchStage::Searching => {
                    let generation = pending.generation;
                    self.pending = None;
                    return Ok(Some(YoutubeSearchServiceUpdate::Results {
                        generation,
                        items,
                        continuation,
                    }));
                }
                Ok(_) => {
                    return Ok(Some(self.fail_pending(
                        "YouTube component returned an unexpected response".to_owned(),
                    )));
                }
                Err(error) => return Ok(Some(self.fail_pending(error.to_string()))),
            }
        }
    }

    pub fn cancel(&mut self) -> bool {
        self.pending.take().is_some()
    }

    pub const fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    fn fail_pending(&mut self, message: String) -> YoutubeSearchServiceUpdate {
        let generation = self.pending.take().map_or(0, |pending| pending.generation);
        YoutubeSearchServiceUpdate::Failed {
            generation,
            message,
        }
    }

    #[cfg(test)]
    fn with_runtime(backend: YoutubeBackend, runtime: YoutubeRuntime) -> Self {
        Self {
            runtime: Some(runtime),
            backend: Some(backend),
            pending: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
    use apricot_media::{
        YoutubeBackend, YoutubeCommand, YoutubeEngine, YoutubeEngineError, YoutubeResponsePayload,
        YoutubeRuntime, YoutubeSessionConfig,
    };

    use super::{YoutubeSearchService, YoutubeSearchServiceUpdate};

    struct FakeEngine {
        commands: Arc<Mutex<Vec<YoutubeCommand>>>,
    }

    impl YoutubeEngine for FakeEngine {
        fn execute(
            &mut self,
            command: YoutubeCommand,
        ) -> Result<YoutubeResponsePayload, YoutubeEngineError> {
            self.commands
                .lock()
                .expect("commands")
                .push(command.clone());
            match command {
                YoutubeCommand::Configure { .. } => Ok(YoutubeResponsePayload::Configured),
                YoutubeCommand::Search { query, .. } => Ok(YoutubeResponsePayload::SearchResults {
                    items: vec![MediaItem {
                        id: MediaId("one".to_owned()),
                        source: MediaSource::Youtube,
                        kind: MediaKind::Video,
                        title: query,
                        url: None,
                        local_path: None,
                        channel: String::new(),
                        duration_seconds: None,
                        metadata: std::collections::BTreeMap::default(),
                    }],
                    continuation: None,
                }),
                _ => Err(YoutubeEngineError::new("unexpected command", false)),
            }
        }
    }

    #[test]
    fn configuration_completes_before_search_and_only_results_escape() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&commands);
        let runtime = YoutubeRuntime::spawn(Box::new(move || {
            Ok(Box::new(FakeEngine {
                commands: Arc::clone(&captured),
            }))
        }))
        .expect("runtime");
        let mut service = YoutubeSearchService::with_runtime(YoutubeBackend::YtDlp, runtime);
        service
            .start(
                YoutubeBackend::YtDlp,
                Path::new("unused"),
                YoutubeSessionConfig::default(),
                7,
                "query".to_owned(),
                apricot_media::YoutubeSearchKind::Video,
                20,
            )
            .expect("start");
        let deadline = Instant::now() + Duration::from_secs(1);
        let update = loop {
            if let Some(update) = service.poll().expect("poll") {
                break update;
            }
            assert!(Instant::now() < deadline, "service timed out");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(matches!(
            update,
            YoutubeSearchServiceUpdate::Results { generation: 7, items, .. }
                if items[0].title == "query"
        ));
        let commands = commands.lock().expect("commands");
        assert!(matches!(commands[0], YoutubeCommand::Configure { .. }));
        assert!(matches!(commands[1], YoutubeCommand::Search { .. }));
    }

    #[test]
    fn cancel_discards_late_worker_updates() {
        let runtime = YoutubeRuntime::spawn(Box::new(|| {
            Ok(Box::new(FakeEngine {
                commands: Arc::new(Mutex::new(Vec::new())),
            }))
        }))
        .expect("runtime");
        let mut service = YoutubeSearchService::with_runtime(YoutubeBackend::YtDlp, runtime);
        service
            .start(
                YoutubeBackend::YtDlp,
                Path::new("unused"),
                YoutubeSessionConfig::default(),
                9,
                "cancelled".to_owned(),
                apricot_media::YoutubeSearchKind::All,
                20,
            )
            .expect("start");
        assert!(service.cancel());
        let deadline = Instant::now() + Duration::from_millis(100);
        while Instant::now() < deadline {
            assert_eq!(service.poll().expect("poll"), None);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!service.is_pending());
    }
}
