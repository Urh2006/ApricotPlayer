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
    MpvLaunchOptions, MpvProcessEngine, PlaybackCommand, PlaybackEngine, PlaybackError,
    PlaybackEvent,
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
    Start {
        generation: u64,
        options: Box<MpvLaunchOptions>,
        item: Box<MediaItem>,
    },
    Execute {
        generation: u64,
        command: PlaybackCommand,
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
    /// Creates the worker thread without starting mpv. The first media request
    /// lazily creates the process, keeping normal application launch fast.
    ///
    /// # Errors
    ///
    /// Returns an error only when the operating system cannot create the worker.
    pub fn spawn() -> Result<Self, PlaybackRuntimeError> {
        Self::spawn_with(Box::new(|options| {
            MpvProcessEngine::spawn(options)
                .map(|engine| Box::new(engine) as Box<dyn PlaybackEngine>)
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
                RuntimeRequest::Start {
                    generation,
                    options,
                    item,
                } => {
                    active = start_engine(generation, &options, item, updates, &mut factory);
                }
                RuntimeRequest::Execute {
                    generation,
                    command,
                } => execute_if_current(&mut active, generation, command, updates),
                RuntimeRequest::Shutdown => break,
            }
        }
        poll_engine_events(&mut active, updates);
    }
}

fn start_engine(
    generation: u64,
    options: &MpvLaunchOptions,
    item: Box<MediaItem>,
    updates: &SyncSender<PlaybackUpdate>,
    factory: &mut EngineFactory,
) -> Option<(u64, Box<dyn PlaybackEngine>)> {
    match factory(options) {
        Ok(mut engine) => match engine.execute(PlaybackCommand::Load(item)) {
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
) {
    let Some((generation, engine)) = active else {
        return;
    };
    for _ in 0..MAX_EVENTS_PER_TICK {
        match engine.poll_event() {
            Ok(Some(event)) => {
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
        sync::{Arc, Mutex},
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
        assert!(matches!(commands[0], PlaybackCommand::Load(_)));
        assert_eq!(commands[1], PlaybackCommand::SetPaused(true));
    }
}
