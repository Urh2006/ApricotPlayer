//! `YouTube` search command sequencing above the bounded component runtime.

use std::path::Path;

use apricot_core::MediaItem;
use apricot_media::{
    YoutubeBackend, YoutubeCommand, YoutubeFormat, YoutubeResponsePayload, YoutubeRuntime,
    YoutubeRuntimeError, YoutubeSearchKind, YoutubeSessionConfig, YoutubeStreamPreference,
};
use thiserror::Error;

use crate::spawn_youtube_runtime;

#[derive(Clone, Debug, PartialEq)]
pub enum YoutubeSearchServiceUpdate {
    Results {
        token: u64,
        items: Vec<MediaItem>,
        continuation: Option<String>,
    },
    Resolved {
        token: u64,
        item: Box<MediaItem>,
        formats: Vec<YoutubeFormat>,
    },
    Failed {
        token: u64,
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
struct PendingOperation {
    runtime_generation: u64,
    token: u64,
    command: YoutubeCommand,
    stage: OperationStage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OperationStage {
    Configuring,
    Executing,
}

#[derive(Default)]
pub struct YoutubeSearchService {
    runtime: Option<YoutubeRuntime>,
    backend: Option<YoutubeBackend>,
    next_runtime_generation: u64,
    pending: Option<PendingOperation>,
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
        token: u64,
        query: String,
        kind: YoutubeSearchKind,
        limit: u32,
    ) -> Result<(), YoutubeSearchServiceError> {
        self.start_operation(
            backend,
            components_directory,
            config,
            token,
            YoutubeCommand::Search {
                query,
                kind,
                limit,
                safe_search: false,
            },
        )
    }

    /// Resolves one media page through the selected component without blocking
    /// the caller or exposing component configuration acknowledgments.
    ///
    /// # Errors
    ///
    /// Returns an error under the same bounded-runtime conditions as [`Self::start`].
    pub fn start_resolve(
        &mut self,
        backend: YoutubeBackend,
        components_directory: &Path,
        config: YoutubeSessionConfig,
        token: u64,
        url: String,
        preference: YoutubeStreamPreference,
    ) -> Result<(), YoutubeSearchServiceError> {
        self.start_operation(
            backend,
            components_directory,
            config,
            token,
            YoutubeCommand::Resolve { url, preference },
        )
    }

    fn start_operation(
        &mut self,
        backend: YoutubeBackend,
        components_directory: &Path,
        config: YoutubeSessionConfig,
        token: u64,
        command: YoutubeCommand,
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
        self.next_runtime_generation = self.next_runtime_generation.wrapping_add(1).max(1);
        let runtime_generation = self.next_runtime_generation;
        runtime.execute(runtime_generation, YoutubeCommand::Configure { config })?;
        self.pending = Some(PendingOperation {
            runtime_generation,
            token,
            command,
            stage: OperationStage::Configuring,
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
            if update.generation != pending.runtime_generation {
                continue;
            }
            match update.result {
                Ok(YoutubeResponsePayload::Configured)
                    if pending.stage == OperationStage::Configuring =>
                {
                    if let Err(error) =
                        runtime.execute(pending.runtime_generation, pending.command.clone())
                    {
                        return Ok(Some(self.fail_pending(error.to_string())));
                    }
                    pending.stage = OperationStage::Executing;
                }
                Ok(YoutubeResponsePayload::SearchResults {
                    items,
                    continuation,
                }) if pending.stage == OperationStage::Executing => {
                    let token = pending.token;
                    self.pending = None;
                    return Ok(Some(YoutubeSearchServiceUpdate::Results {
                        token,
                        items,
                        continuation,
                    }));
                }
                Ok(YoutubeResponsePayload::Resolved { item, formats })
                    if pending.stage == OperationStage::Executing =>
                {
                    let token = pending.token;
                    self.pending = None;
                    return Ok(Some(YoutubeSearchServiceUpdate::Resolved {
                        token,
                        item,
                        formats,
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
        let token = self.pending.take().map_or(0, |pending| pending.token);
        YoutubeSearchServiceUpdate::Failed { token, message }
    }

    #[cfg(test)]
    fn with_runtime(backend: YoutubeBackend, runtime: YoutubeRuntime) -> Self {
        Self {
            runtime: Some(runtime),
            backend: Some(backend),
            next_runtime_generation: 0,
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
                        stream_url: None,
                        external_audio_url: None,
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
            YoutubeSearchServiceUpdate::Results { token: 7, items, .. }
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

    #[test]
    fn resolve_uses_a_distinct_internal_generation_and_returns_the_client_token() {
        struct ResolveEngine;

        impl YoutubeEngine for ResolveEngine {
            fn execute(
                &mut self,
                command: YoutubeCommand,
            ) -> Result<YoutubeResponsePayload, YoutubeEngineError> {
                match command {
                    YoutubeCommand::Configure { .. } => Ok(YoutubeResponsePayload::Configured),
                    YoutubeCommand::Resolve { url, .. } => {
                        let mut item = super::tests::MediaItem {
                            id: MediaId("resolved".to_owned()),
                            source: MediaSource::Youtube,
                            kind: MediaKind::Video,
                            title: url,
                            url: None,
                            stream_url: None,
                            external_audio_url: None,
                            local_path: None,
                            channel: String::new(),
                            duration_seconds: None,
                            metadata: std::collections::BTreeMap::default(),
                        };
                        item.metadata.insert("resolved".to_owned(), true.into());
                        Ok(YoutubeResponsePayload::Resolved {
                            item: Box::new(item),
                            formats: Vec::new(),
                        })
                    }
                    _ => Err(YoutubeEngineError::new("unexpected command", false)),
                }
            }
        }

        let runtime =
            YoutubeRuntime::spawn(Box::new(|| Ok(Box::new(ResolveEngine)))).expect("runtime");
        let mut service = YoutubeSearchService::with_runtime(YoutubeBackend::YtDlp, runtime);
        service
            .start_resolve(
                YoutubeBackend::YtDlp,
                Path::new("unused"),
                YoutubeSessionConfig::default(),
                41,
                "https://www.youtube.com/watch?v=test".to_owned(),
                apricot_media::YoutubeStreamPreference::Automatic,
            )
            .expect("resolve");
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(update) = service.poll().expect("poll") {
                assert!(matches!(
                    update,
                    YoutubeSearchServiceUpdate::Resolved { token: 41, item, .. }
                        if item.id.0 == "resolved"
                ));
                break;
            }
            assert!(Instant::now() < deadline, "service timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
