//! Bounded worker that owns the blocking playback engine off the UI thread.

use std::{
    sync::mpsc::{
        Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError, sync_channel,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use apricot_core::MediaItem;
use thiserror::Error;

use crate::{
    LibMpvEngine, MpvLaunchOptions, PlaybackCommand, PlaybackEngine, PlaybackError, PlaybackEvent,
};

const REQUEST_CAPACITY: usize = 32;
const UPDATE_CAPACITY: usize = 128;
const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_EVENTS_PER_TICK: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub struct PlaybackUpdate {
    pub generation: u64,
    pub event: PlaybackEvent,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum PlaybackRuntimeError {
    #[error("playback worker is busy")]
    Busy,
    #[error("playback worker has stopped")]
    Stopped,
    #[error("could not create playback worker: {0}")]
    Spawn(String),
}

enum RuntimeRequest {
    CancelPreview {
        generation: u64,
    },
    Preview {
        generation: u64,
        start: f64,
        end: f64,
    },
    Start {
        generation: u64,
        options: Box<MpvLaunchOptions>,
        item: Box<MediaItem>,
    },
    Execute {
        generation: u64,
        command: PlaybackCommand,
    },
    Close {
        generation: u64,
    },
    Shutdown,
}

type EngineFactory = Box<
    dyn FnMut(&MpvLaunchOptions) -> Result<Box<dyn PlaybackEngine>, PlaybackError> + Send + 'static,
>;

pub struct PlaybackRuntime {
    requests: SyncSender<RuntimeRequest>,
    updates: Receiver<PlaybackUpdate>,
    worker: Option<JoinHandle<()>>,
}

impl PlaybackRuntime {
    /// Seeks, unpauses, and arms a generation-bound preview in one worker turn.
    ///
    /// # Errors
    /// Returns an error if the worker queue is full or disconnected.
    pub fn preview(
        &self,
        generation: u64,
        start: f64,
        end: f64,
    ) -> Result<(), PlaybackRuntimeError> {
        self.send(RuntimeRequest::Preview {
            generation,
            start,
            end,
        })
    }
    /// Cancels the matching preview without changing playback position or pause.
    ///
    /// # Errors
    /// Returns an error if the worker queue is full or disconnected.
    pub fn cancel_preview(&self, generation: u64) -> Result<(), PlaybackRuntimeError> {
        self.send(RuntimeRequest::CancelPreview { generation })
    }
    /// Creates the worker thread without loading libmpv. The first media request
    /// lazily creates one in-process player, keeping application launch fast.
    ///
    /// # Errors
    ///
    /// Returns an error only when the operating system cannot create the worker.
    pub fn spawn() -> Result<Self, PlaybackRuntimeError> {
        Self::spawn_with(Box::new(|options| {
            LibMpvEngine::load(options).map(|engine| Box::new(engine) as Box<dyn PlaybackEngine>)
        }))
    }

    /// Enqueues a new player generation without blocking the caller.
    ///
    /// # Errors
    ///
    /// Returns [`PlaybackRuntimeError::Busy`] when the bounded request queue is
    /// full, or [`PlaybackRuntimeError::Stopped`] after worker shutdown.
    pub fn start(
        &self,
        generation: u64,
        options: MpvLaunchOptions,
        item: MediaItem,
    ) -> Result<(), PlaybackRuntimeError> {
        self.send(RuntimeRequest::Start {
            generation,
            options: Box::new(options),
            item: Box::new(item),
        })
    }

    /// Enqueues one command for the specified player generation.
    ///
    /// # Errors
    ///
    /// Returns [`PlaybackRuntimeError::Busy`] when the bounded request queue is
    /// full, or [`PlaybackRuntimeError::Stopped`] after worker shutdown.
    pub fn execute(
        &self,
        generation: u64,
        command: PlaybackCommand,
    ) -> Result<(), PlaybackRuntimeError> {
        self.send(RuntimeRequest::Execute {
            generation,
            command,
        })
    }

    /// Closes only the currently matching player generation.
    ///
    /// # Errors
    ///
    /// Returns [`PlaybackRuntimeError::Busy`] when the bounded request queue is
    /// full, or [`PlaybackRuntimeError::Stopped`] after worker shutdown.
    pub fn close(&self, generation: u64) -> Result<(), PlaybackRuntimeError> {
        self.send(RuntimeRequest::Close { generation })
    }

    /// Returns the next currently buffered playback update.
    ///
    /// # Errors
    ///
    /// Returns [`PlaybackRuntimeError::Stopped`] if the worker has exited and
    /// its update channel has disconnected.
    pub fn poll_update(&self) -> Result<Option<PlaybackUpdate>, PlaybackRuntimeError> {
        match self.updates.try_recv() {
            Ok(update) => Ok(Some(update)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(PlaybackRuntimeError::Stopped),
        }
    }

    fn send(&self, request: RuntimeRequest) -> Result<(), PlaybackRuntimeError> {
        self.requests
            .try_send(request)
            .map_err(|error| match error {
                TrySendError::Full(_) => PlaybackRuntimeError::Busy,
                TrySendError::Disconnected(_) => PlaybackRuntimeError::Stopped,
            })
    }

    fn spawn_with(factory: EngineFactory) -> Result<Self, PlaybackRuntimeError> {
        let (requests, request_receiver) = sync_channel(REQUEST_CAPACITY);
        let (update_sender, updates) = sync_channel(UPDATE_CAPACITY);
        let worker = thread::Builder::new()
            .name("apricot-playback-worker".to_owned())
            .spawn(move || playback_worker(&request_receiver, &update_sender, factory))
            .map_err(|error| PlaybackRuntimeError::Spawn(error.to_string()))?;
        Ok(Self {
            requests,
            updates,
            worker: Some(worker),
        })
    }
}

impl Drop for PlaybackRuntime {
    fn drop(&mut self) {
        let _ = self.requests.send(RuntimeRequest::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn playback_worker(
    requests: &Receiver<RuntimeRequest>,
    updates: &SyncSender<PlaybackUpdate>,
    mut factory: EngineFactory,
) {
    let mut active: Option<(u64, Box<dyn PlaybackEngine>)> = None;
    let mut preview: Option<(u64, f64, f64, bool)> = None;
    loop {
        let request = if active.is_some() {
            match requests.recv_timeout(ACTIVE_POLL_INTERVAL) {
                Ok(request) => Some(request),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match requests.recv() {
                Ok(request) => Some(request),
                Err(_) => break,
            }
        };
        if let Some(request) = request {
            match request {
                RuntimeRequest::CancelPreview { generation } => {
                    if preview.is_some_and(|(current, ..)| current == generation) {
                        preview = None;
                    }
                }
                RuntimeRequest::Preview {
                    generation,
                    start,
                    end,
                } => {
                    if start.is_finite()
                        && end.is_finite()
                        && start >= 0.0
                        && end - start >= 0.25
                        && let Some((current, engine)) = active.as_mut()
                        && *current == generation
                    {
                        preview = None;
                        let result = engine
                            .execute(PlaybackCommand::SeekAbsolute {
                                seconds: start,
                                exact: true,
                            })
                            .and_then(|()| engine.execute(PlaybackCommand::SetPaused(false)));
                        match result {
                            Ok(()) => preview = Some((generation, start, end, false)),
                            Err(error) => {
                                let _ = updates.try_send(PlaybackUpdate {
                                    generation,
                                    event: PlaybackEvent::Failed(error.to_string()),
                                });
                            }
                        }
                    }
                }
                RuntimeRequest::Start {
                    generation,
                    options,
                    item,
                } => {
                    preview = None;
                    active = start_or_replace_engine(
                        active,
                        generation,
                        &options,
                        item,
                        updates,
                        &mut factory,
                    );
                }
                RuntimeRequest::Execute {
                    generation,
                    command,
                } => {
                    if active
                        .as_ref()
                        .is_some_and(|(current, _)| *current == generation)
                        && matches!(
                            command,
                            PlaybackCommand::SeekAbsolute { .. }
                                | PlaybackCommand::SeekRelative { .. }
                                | PlaybackCommand::SetPaused(_)
                        )
                    {
                        preview = None;
                    }
                    execute_if_current(&mut active, generation, command, updates);
                }
                RuntimeRequest::Close { generation } => {
                    if active
                        .as_ref()
                        .is_some_and(|(active_generation, _)| *active_generation == generation)
                    {
                        active = None;
                        preview = None;
                    }
                }
                RuntimeRequest::Shutdown => break,
            }
        }
        poll_engine_events(&mut active, updates, &mut preview);
    }
}

fn start_or_replace_engine(
    active: Option<(u64, Box<dyn PlaybackEngine>)>,
    generation: u64,
    options: &MpvLaunchOptions,
    item: Box<MediaItem>,
    updates: &SyncSender<PlaybackUpdate>,
    factory: &mut EngineFactory,
) -> Option<(u64, Box<dyn PlaybackEngine>)> {
    if let Some((_, mut engine)) = active {
        return match engine.execute(PlaybackCommand::Load {
            item,
            start_position_seconds: options.initial_position_seconds,
        }) {
            Ok(()) => Some((generation, engine)),
            Err(error) => {
                emit_failure(updates, generation, &error);
                None
            }
        };
    }
    match factory(options) {
        Ok(mut engine) => match engine.execute(PlaybackCommand::Load {
            item,
            start_position_seconds: options.initial_position_seconds,
        }) {
            Ok(()) => Some((generation, engine)),
            Err(error) => {
                emit_failure(updates, generation, &error);
                None
            }
        },
        Err(error) => {
            emit_failure(updates, generation, &error);
            None
        }
    }
}

fn execute_if_current(
    active: &mut Option<(u64, Box<dyn PlaybackEngine>)>,
    generation: u64,
    command: PlaybackCommand,
    updates: &SyncSender<PlaybackUpdate>,
) {
    let Some((active_generation, engine)) = active else {
        return;
    };
    if *active_generation != generation {
        return;
    }
    if let Err(error) = engine.execute(command) {
        emit_failure(updates, generation, &error);
    }
}

fn poll_engine_events(
    active: &mut Option<(u64, Box<dyn PlaybackEngine>)>,
    updates: &SyncSender<PlaybackUpdate>,
    preview: &mut Option<(u64, f64, f64, bool)>,
) {
    let Some((generation, engine)) = active else {
        return;
    };
    for _ in 0..MAX_EVENTS_PER_TICK {
        match engine.poll_event() {
            Ok(Some(mut event)) => {
                let finish = preview
                    .as_mut()
                    .is_some_and(|(token, start, end, arrived)| {
                        if token != generation {
                            return false;
                        }
                        match &event {
                            PlaybackEvent::Position { elapsed, .. } if elapsed.is_finite() => {
                                if *elapsed >= *start - 0.1 && *elapsed < *end - 0.03 {
                                    *arrived = true;
                                }
                                *arrived && *elapsed >= *end - 0.03
                            }
                            PlaybackEvent::Ended => true,
                            _ => false,
                        }
                    });
                if finish {
                    *preview = None;
                    if let Err(error) = engine.execute(PlaybackCommand::SetPaused(true)) {
                        emit_failure(updates, *generation, &error);
                    }
                    if matches!(event, PlaybackEvent::Ended) {
                        event = PlaybackEvent::Paused(true);
                    }
                }
                let _ = updates.try_send(PlaybackUpdate {
                    generation: *generation,
                    event,
                });
            }
            Ok(None) => break,
            Err(error) => {
                emit_failure(updates, *generation, &error);
                break;
            }
        }
    }
}

fn emit_failure(updates: &SyncSender<PlaybackUpdate>, generation: u64, error: &PlaybackError) {
    let _ = updates.try_send(PlaybackUpdate {
        generation,
        event: PlaybackEvent::Failed(error.to_string()),
    });
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    use apricot_core::{MediaId, MediaKind, MediaSource};

    use super::{PlaybackRuntime, PlaybackUpdate};
    use crate::{MpvLaunchOptions, PlaybackCommand, PlaybackEngine, PlaybackError, PlaybackEvent};

    struct FakeEngine {
        events: Vec<PlaybackEvent>,
        commands: Arc<Mutex<Vec<PlaybackCommand>>>,
    }

    impl PlaybackEngine for FakeEngine {
        fn execute(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError> {
            self.commands.lock().expect("commands").push(command);
            Ok(())
        }

        fn poll_event(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError> {
            Ok(self.events.pop())
        }
    }

    fn item(id: &str) -> apricot_core::MediaItem {
        apricot_core::MediaItem {
            id: MediaId(id.to_owned()),
            source: MediaSource::Local,
            kind: MediaKind::Audio,
            title: id.to_owned(),
            url: None,
            stream_url: None,
            external_audio_url: None,
            local_path: Some(format!(r"C:\Music\{id}.mp3")),
            channel: String::new(),
            duration_seconds: None,
            metadata: BTreeMap::new(),
        }
    }

    fn wait_for_update(runtime: &PlaybackRuntime) -> PlaybackUpdate {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(update) = runtime.poll_update().expect("runtime update") {
                return update;
            }
            assert!(Instant::now() < deadline, "runtime update timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn runtime_tags_events_and_ignores_stale_commands() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let fake_commands = Arc::clone(&commands);
        let runtime = PlaybackRuntime::spawn_with(Box::new(move |_| {
            Ok(Box::new(FakeEngine {
                events: vec![PlaybackEvent::Started],
                commands: Arc::clone(&fake_commands),
            }))
        }))
        .expect("runtime");
        runtime
            .start(7, MpvLaunchOptions::new("mpv.exe"), item("track"))
            .expect("start");
        assert_eq!(
            wait_for_update(&runtime),
            PlaybackUpdate {
                generation: 7,
                event: PlaybackEvent::Started,
            }
        );
        runtime
            .execute(6, PlaybackCommand::SetVolume(20.0))
            .expect("enqueue stale command");
        runtime
            .execute(7, PlaybackCommand::SetPaused(true))
            .expect("enqueue current command");
        std::thread::sleep(Duration::from_millis(30));

        let commands = commands.lock().expect("commands");
        assert_eq!(commands.len(), 2);
        assert!(matches!(commands[0], PlaybackCommand::Load { .. }));
        assert_eq!(commands[1], PlaybackCommand::SetPaused(true));
    }

    #[test]
    fn preview_seeks_and_unpauses_only_the_current_generation() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let fake_commands = Arc::clone(&commands);
        let runtime = PlaybackRuntime::spawn_with(Box::new(move |_| {
            Ok(Box::new(FakeEngine {
                events: Vec::new(),
                commands: Arc::clone(&fake_commands),
            }))
        }))
        .expect("runtime");
        runtime
            .start(7, MpvLaunchOptions::new("mpv.exe"), item("track"))
            .expect("start");
        runtime.preview(6, 10.0, 15.0).expect("stale preview");
        runtime.preview(7, 10.0, 10.1).expect("invalid range");
        runtime.preview(7, 10.0, 15.0).expect("preview");
        runtime.cancel_preview(7).expect("cancel");
        // Shutdown joins the worker after all previously queued requests.
        drop(runtime);
        let commands = commands.lock().expect("commands");
        assert_eq!(commands.len(), 3);
        assert_eq!(
            commands[1],
            PlaybackCommand::SeekAbsolute {
                seconds: 10.0,
                exact: true
            }
        );
        assert_eq!(commands[2], PlaybackCommand::SetPaused(false));
    }

    #[test]
    fn preview_pauses_in_worker_without_ui_consuming_updates() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let engine = FakeEngine {
            events: vec![
                PlaybackEvent::Position {
                    elapsed: 15.0,
                    duration: Some(90.0),
                },
                PlaybackEvent::Position {
                    elapsed: 10.0,
                    duration: Some(90.0),
                },
            ],
            commands: Arc::clone(&commands),
        };
        let mut active = Some((7, Box::new(engine) as Box<dyn PlaybackEngine>));
        let (sender, _unread_receiver) = std::sync::mpsc::sync_channel(128);
        let mut preview = Some((7, 10.0, 15.0, false));
        super::poll_engine_events(&mut active, &sender, &mut preview);
        assert!(preview.is_none());
        assert!(
            commands
                .lock()
                .expect("commands")
                .iter()
                .any(|command| matches!(command, PlaybackCommand::SetPaused(true)))
        );
    }

    #[test]
    fn media_replacement_reuses_one_engine_until_close() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let fake_commands = Arc::clone(&commands);
        let calls = Arc::clone(&factory_calls);
        let runtime = PlaybackRuntime::spawn_with(Box::new(move |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(FakeEngine {
                events: Vec::new(),
                commands: Arc::clone(&fake_commands),
            }))
        }))
        .expect("runtime");

        let mut bookmark_options = MpvLaunchOptions::new("mpv.exe");
        bookmark_options.initial_position_seconds = Some(12.3);
        runtime
            .start(1, bookmark_options, item("first"))
            .expect("first start");
        runtime
            .start(2, MpvLaunchOptions::new("mpv.exe"), item("second"))
            .expect("replacement start");
        std::thread::sleep(Duration::from_millis(30));

        assert_eq!(factory_calls.load(Ordering::Relaxed), 1);
        runtime.close(2).expect("close session");
        runtime
            .start(3, MpvLaunchOptions::new("mpv.exe"), item("third"))
            .expect("new session start");
        std::thread::sleep(Duration::from_millis(30));

        assert_eq!(factory_calls.load(Ordering::Relaxed), 2);
        let commands = commands.lock().expect("commands");
        assert_eq!(commands.len(), 3);
        assert!(matches!(
            &commands[0],
            PlaybackCommand::Load {
                item,
                start_position_seconds: Some(position),
            } if item.id.0 == "first" && (*position - 12.3).abs() < f64::EPSILON
        ));
        assert!(matches!(
            &commands[1],
            PlaybackCommand::Load {
                item,
                start_position_seconds: None,
            } if item.id.0 == "second"
        ));
        assert!(matches!(
            &commands[2],
            PlaybackCommand::Load { item, .. } if item.id.0 == "third"
        ));
    }
}
