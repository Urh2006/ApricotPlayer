//! Bounded worker that owns the blocking playback engine off the UI thread.

use std::{
    sync::mpsc::{
        Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError, sync_channel,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use apricot_core::MediaItem;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::{
    LibMpvEngine, MpvLaunchOptions, PlaybackCommand, PlaybackEngine, PlaybackError, PlaybackEvent,
};

const REQUEST_CAPACITY: usize = 32;
const UPDATE_CAPACITY: usize = 128;
const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_EVENTS_PER_TICK: usize = 32;

#[derive(Clone, Default)]
pub struct PlaybackPositionReader(Arc<Mutex<Option<(u64, f64)>>>);

impl PlaybackPositionReader {
    /// Reads cached position without waiting for the worker or consuming events.
    #[must_use]
    pub fn read(&self, generation: u64) -> Option<f64> {
        self.0.try_lock().ok().and_then(|value| {
            value
                .filter(|(current, _)| *current == generation)
                .map(|(_, seconds)| seconds)
        })
    }

    fn publish(&self, position: Option<(u64, f64)>) {
        if let Ok(mut current) = self.0.lock() {
            *current = position;
        }
    }
}

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
    position: PlaybackPositionReader,
    requests: SyncSender<RuntimeRequest>,
    updates: Receiver<PlaybackUpdate>,
    worker: Option<JoinHandle<()>>,
}

impl PlaybackRuntime {
    #[must_use]
    pub fn position_reader(&self) -> PlaybackPositionReader {
        self.position.clone()
    }
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
        let position = PlaybackPositionReader::default();
        let worker_position = position.clone();
        let worker = thread::Builder::new()
            .name("apricot-playback-worker".to_owned())
            .spawn(move || {
                playback_worker(&request_receiver, &update_sender, factory, &worker_position);
            })
            .map_err(|error| PlaybackRuntimeError::Spawn(error.to_string()))?;
        Ok(Self {
            position,
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

// Keep generation-sensitive request transitions in one worker dispatch table.
#[allow(clippy::too_many_lines)]
fn playback_worker(
    requests: &Receiver<RuntimeRequest>,
    updates: &SyncSender<PlaybackUpdate>,
    mut factory: EngineFactory,
    position: &PlaybackPositionReader,
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
                    position.publish(None);
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
                        position.publish(None);
                    }
                }
                RuntimeRequest::Shutdown => break,
            }
        }
        poll_engine_events(&mut active, updates, &mut preview, position);
    }
    position.publish(None);
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
        let reset = item_start_commands(options)
            .into_iter()
            .try_for_each(|command| engine.execute(command));
        if let Err(error) = reset {
            emit_failure(updates, generation, &error);
            return None;
        }
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

/// Python starts a fresh mpv process for every item. A reused libmpv instance
/// therefore receives the per-item launch state before the replacement load.
fn item_start_commands(options: &MpvLaunchOptions) -> Vec<PlaybackCommand> {
    vec![
        PlaybackCommand::SetPaused(
            options.initial_playback_state == crate::InitialPlaybackState::Paused,
        ),
        PlaybackCommand::SetVolumeMax(options.volume_max),
        PlaybackCommand::SetVolume(options.initial_volume),
        PlaybackCommand::SetAudioPitchCorrection(options.audio_pitch_correction),
        PlaybackCommand::SetSpeed(options.initial_speed),
        PlaybackCommand::SetPitch(options.initial_pitch),
        PlaybackCommand::SetRepeat(options.repeat_mode == crate::RepeatMode::One),
        PlaybackCommand::SetAudioFilter(options.initial_audio_filter.clone()),
        PlaybackCommand::SetReplayGain(options.replay_gain.clone()),
    ]
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
        let _ = updates.try_send(PlaybackUpdate {
            generation,
            event: PlaybackEvent::CommandFailed(error.to_string()),
        });
    }
}

fn poll_engine_events(
    active: &mut Option<(u64, Box<dyn PlaybackEngine>)>,
    updates: &SyncSender<PlaybackUpdate>,
    preview: &mut Option<(u64, f64, f64, bool)>,
    position: &PlaybackPositionReader,
) {
    let Some((generation, engine)) = active else {
        return;
    };
    for _ in 0..MAX_EVENTS_PER_TICK {
        match engine.poll_event() {
            Ok(Some(mut event)) => {
                if let PlaybackEvent::Position { elapsed, .. } = &event
                    && elapsed.is_finite()
                {
                    position.publish(Some((*generation, elapsed.max(0.0))));
                }
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
                    event = PlaybackEvent::PreviewFinished;
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

    #[test]
    fn position_reader_is_generation_bound_and_does_not_consume_updates() {
        let reader = super::PlaybackPositionReader::default();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let mut active: Option<(u64, Box<dyn PlaybackEngine>)> = Some((
            7,
            Box::new(FakeEngine {
                events: vec![PlaybackEvent::Position {
                    elapsed: 12.5,
                    duration: Some(100.0),
                }],
                commands: Arc::new(Mutex::new(Vec::new())),
            }),
        ));
        super::poll_engine_events(&mut active, &sender, &mut None, &reader);
        assert_eq!(reader.read(7), Some(12.5));
        assert_eq!(reader.read(8), None);
        assert_eq!(receiver.try_recv().unwrap().generation, 7);
        let guard = reader.0.lock().unwrap();
        assert_eq!(reader.read(7), None);
        drop(guard);
        reader.publish(None);
        assert_eq!(reader.read(7), None);
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

    struct RejectingEngine;

    impl PlaybackEngine for RejectingEngine {
        fn execute(&mut self, _command: PlaybackCommand) -> Result<(), PlaybackError> {
            Err(PlaybackError::Operation("rejected".to_owned()))
        }

        fn poll_event(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError> {
            Ok(None)
        }
    }

    #[test]
    fn rejected_command_is_reported_without_ending_the_item() {
        let mut active: Option<(u64, Box<dyn PlaybackEngine>)> =
            Some((7, Box::new(RejectingEngine)));
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        super::execute_if_current(
            &mut active,
            7,
            PlaybackCommand::SeekRelative {
                seconds: 5.0,
                exact: false,
            },
            &sender,
        );
        let update = receiver.try_recv().expect("command failure update");
        assert_eq!(update.generation, 7);
        assert!(matches!(update.event, PlaybackEvent::CommandFailed(_)));
        assert!(active.is_some(), "the item keeps playing");
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
    fn preview_ignores_pre_seek_and_nonfinite_positions() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let engine = FakeEngine {
            events: vec![
                PlaybackEvent::Position {
                    elapsed: f64::NAN,
                    duration: None,
                },
                PlaybackEvent::Position {
                    elapsed: 80.0,
                    duration: Some(90.0),
                },
            ],
            commands: Arc::clone(&commands),
        };
        let mut active = Some((7, Box::new(engine) as Box<dyn PlaybackEngine>));
        let (sender, _receiver) = std::sync::mpsc::sync_channel(128);
        let mut preview = Some((7, 10.0, 15.0, false));
        super::poll_engine_events(
            &mut active,
            &sender,
            &mut preview,
            &super::PlaybackPositionReader::default(),
        );
        assert_eq!(preview, Some((7, 10.0, 15.0, false)));
        assert!(commands.lock().expect("commands").is_empty());
    }

    #[test]
    fn preview_end_of_file_is_not_projected_as_autoplay_end() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let engine = FakeEngine {
            events: vec![PlaybackEvent::Ended],
            commands: Arc::clone(&commands),
        };
        let mut active = Some((7, Box::new(engine) as Box<dyn PlaybackEngine>));
        let (sender, receiver) = std::sync::mpsc::sync_channel(128);
        let mut preview = Some((7, 10.0, 15.0, true));
        super::poll_engine_events(
            &mut active,
            &sender,
            &mut preview,
            &super::PlaybackPositionReader::default(),
        );
        assert!(preview.is_none());
        assert_eq!(
            receiver.try_recv().expect("completion").event,
            PlaybackEvent::PreviewFinished
        );
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            *commands.lock().expect("commands"),
            vec![PlaybackCommand::SetPaused(true)]
        );
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
        super::poll_engine_events(
            &mut active,
            &sender,
            &mut preview,
            &super::PlaybackPositionReader::default(),
        );
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
        let mut replacement_options = MpvLaunchOptions::new("mpv.exe");
        replacement_options.initial_speed = 1.25;
        replacement_options.audio_pitch_correction = false;
        replacement_options.initial_audio_filter = Some("@apricot_speed:scaletempo".to_owned());
        runtime
            .start(2, replacement_options, item("second"))
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
        assert_eq!(commands.len(), 12);
        assert!(matches!(
            &commands[0],
            PlaybackCommand::Load {
                item,
                start_position_seconds: Some(position),
            } if item.id.0 == "first" && (*position - 12.3).abs() < f64::EPSILON
        ));
        // A reused engine receives Python's fresh-process start state first.
        assert_eq!(
            &commands[1..10],
            &[
                PlaybackCommand::SetPaused(false),
                PlaybackCommand::SetVolumeMax(100),
                PlaybackCommand::SetVolume(100.0),
                PlaybackCommand::SetAudioPitchCorrection(false),
                PlaybackCommand::SetSpeed(1.25),
                PlaybackCommand::SetPitch(1.0),
                PlaybackCommand::SetRepeat(false),
                PlaybackCommand::SetAudioFilter(Some("@apricot_speed:scaletempo".to_owned())),
                PlaybackCommand::SetReplayGain("no".to_owned()),
            ]
        );
        assert!(matches!(
            &commands[10],
            PlaybackCommand::Load {
                item,
                start_position_seconds: None,
            } if item.id.0 == "second"
        ));
        assert!(matches!(
            &commands[11],
            PlaybackCommand::Load { item, .. } if item.id.0 == "third"
        ));
    }
}
