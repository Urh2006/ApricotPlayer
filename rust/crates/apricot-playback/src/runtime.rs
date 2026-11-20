//! Bounded worker that owns the blocking playback engine off the UI thread.

use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::mpsc::{
        Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError, sync_channel,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use apricot_core::MediaItem;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::{
    AudioFilterState, LibMpvEngine, MpvLaunchOptions, PlaybackCommand, PlaybackEngine,
    PlaybackError, PlaybackEvent,
};

const REQUEST_CAPACITY: usize = 32;
const UPDATE_CAPACITY: usize = 128;
const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_EVENTS_PER_TICK: usize = 32;
/// Updates kept while the UI does not read the channel (a modal dialog).
const MAX_BACKLOG: usize = 1024;

struct OutgoingEngine {
    engine: Box<dyn PlaybackEngine>,
    started: Option<Instant>,
    /// While paused the crossfade waits (review 2.1).
    paused_at: Option<Instant>,
    queued: Instant,
    seconds: f64,
    equal_power: bool,
    target_volume: f64,
}

/// The fade at the end of a file.
#[derive(Default)]
struct EndFade {
    seconds: f64,
    target: f64,
    /// The duration of the active file, from its position events.
    duration: Option<f64>,
    /// The volume last set by the fade, `None` while it is not fading.
    applied: Option<f64>,
}

struct VolumeFade {
    started: Option<Instant>,
    seconds: f64,
    from: f64,
    to: f64,
    after: Option<PlaybackCommand>,
    fade_back: bool,
}

/// Runs the command a fade waits for at once and restores the volume, so a
/// different transport command never drops it.
fn flush_pending_fade(
    active: &mut Option<(u64, Box<dyn PlaybackEngine>)>,
    fade: &mut Option<VolumeFade>,
    target_volume: f64,
) {
    let Some(mut task) = fade.take() else {
        return;
    };
    let Some(command) = task.after.take() else {
        // A plain volume fade: the next command starts its own.
        *fade = Some(task);
        return;
    };
    if let Some((_, engine)) = active.as_mut() {
        let _ = engine.execute(command);
        let _ = engine.execute(PlaybackCommand::SetVolume(target_volume));
    }
}

/// A seek that arrives while another waits for its fade: relative steps add
/// up, an absolute target moves by later relative steps, and a new absolute
/// target replaces the waiting one. `None` for anything that is not a seek.
fn combined_seek(pending: &PlaybackCommand, next: &PlaybackCommand) -> Option<PlaybackCommand> {
    Some(match (pending, next) {
        (
            PlaybackCommand::SeekRelative { seconds, exact },
            PlaybackCommand::SeekRelative {
                seconds: more,
                exact: next_exact,
            },
        ) => PlaybackCommand::SeekRelative {
            seconds: seconds + more,
            exact: *exact || *next_exact,
        },
        (
            PlaybackCommand::SeekAbsolute { seconds, exact },
            PlaybackCommand::SeekRelative {
                seconds: more,
                exact: next_exact,
            },
        ) => PlaybackCommand::SeekAbsolute {
            seconds: (seconds + more).max(0.0),
            exact: *exact || *next_exact,
        },
        (
            PlaybackCommand::SeekAbsolute { .. } | PlaybackCommand::SeekRelative { .. },
            PlaybackCommand::SeekAbsolute { .. },
        ) => next.clone(),
        _ => return None,
    })
}

/// Volume over the last `seconds` of a file: full until then, silent at the end.
fn end_fade_volume(target: f64, elapsed: f64, duration: f64, seconds: f64) -> Option<f64> {
    if seconds <= 0.0 || !duration.is_finite() || duration <= seconds * 2.0 {
        return None;
    }
    let remaining = duration - elapsed;
    (remaining < seconds).then(|| target * (remaining / seconds).clamp(0.0, 1.0))
}

#[allow(clippy::too_many_lines)] // Crossfade, end fade and volume fades in one tick.
fn tick_transitions(
    active: &mut Option<(u64, Box<dyn PlaybackEngine>)>,
    outgoing: &mut Option<OutgoingEngine>,
    fade: &mut Option<VolumeFade>,
    end_fade: &mut EndFade,
    position: &PlaybackPositionReader,
    updates: &UpdateSink,
) {
    let Some((generation, engine)) = active.as_mut() else {
        *outgoing = None;
        *fade = None;
        return;
    };
    let now = Instant::now();
    if let Some(old) = outgoing.as_mut()
        && old.paused_at.is_none()
    {
        if old.started.is_none() && position.read(*generation).is_some() {
            old.started = Some(now);
        }
        if let Some(started) = old.started {
            let progress = (now.saturating_duration_since(started).as_secs_f64() / old.seconds)
                .clamp(0.0, 1.0);
            let (outgoing_gain, incoming_gain) = crossfade_gains(progress, old.equal_power);
            let _ = old.engine.execute(PlaybackCommand::SetVolume(
                old.target_volume * crossfade_volume_factor(outgoing_gain),
            ));
            let _ = engine.execute(PlaybackCommand::SetVolume(
                old.target_volume * crossfade_volume_factor(incoming_gain),
            ));
            if progress >= 1.0 {
                *outgoing = None;
            }
        } else if now.saturating_duration_since(old.queued) > Duration::from_secs(15) {
            let _ = engine.execute(PlaybackCommand::SetVolume(old.target_volume));
            *outgoing = None;
        }
    }
    if outgoing.is_none() && fade.is_none() {
        let volume =
            position
                .read(*generation)
                .zip(end_fade.duration)
                .and_then(|(elapsed, duration)| {
                    end_fade_volume(end_fade.target, elapsed, duration, end_fade.seconds)
                });
        match volume {
            Some(volume) => {
                if end_fade
                    .applied
                    .is_none_or(|last| (last - volume).abs() >= 0.5)
                {
                    let _ = engine.execute(PlaybackCommand::SetVolume(volume));
                    end_fade.applied = Some(volume);
                }
            }
            None => {
                // A seek back out of the last seconds restores the volume.
                if end_fade.applied.take().is_some() {
                    let _ = engine.execute(PlaybackCommand::SetVolume(end_fade.target));
                }
            }
        }
    }
    let Some(task) = fade.as_mut() else {
        return;
    };
    if task.started.is_none() {
        if position.read(*generation).is_some() {
            task.started = Some(now);
        } else {
            return;
        }
    }
    let progress = (now
        .saturating_duration_since(task.started.unwrap_or(now))
        .as_secs_f64()
        / task.seconds)
        .clamp(0.0, 1.0);
    let _ = engine.execute(PlaybackCommand::SetVolume(
        task.from + (task.to - task.from) * progress,
    ));
    if progress < 1.0 {
        return;
    }
    let mut completed = fade.take().expect("fade was active");
    if let Some(command) = completed.after.take() {
        if let Err(error) = engine.execute(command.clone()) {
            updates.try_send(PlaybackUpdate {
                generation: *generation,
                event: PlaybackEvent::CommandFailed(error.to_string()),
            });
        }
        if completed.fade_back {
            *fade = Some(VolumeFade {
                started: Some(now),
                seconds: completed.seconds,
                from: 0.0,
                to: completed.from,
                after: None,
                fade_back: false,
            });
        } else if matches!(command, PlaybackCommand::SetPaused(true)) {
            let _ = engine.execute(PlaybackCommand::SetVolume(completed.from));
        }
    }
}

/// The update channel plus a backlog for a full channel. Position updates may
/// be dropped because the latest position is also published through
/// [`PlaybackPositionReader`]; every other update (start, pause, end, failure)
/// is kept in order until the UI reads the channel again.
struct UpdateSink {
    sender: SyncSender<PlaybackUpdate>,
    backlog: RefCell<VecDeque<PlaybackUpdate>>,
}

impl UpdateSink {
    fn new(sender: SyncSender<PlaybackUpdate>) -> Self {
        Self {
            sender,
            backlog: RefCell::new(VecDeque::new()),
        }
    }

    fn try_send(&self, update: PlaybackUpdate) {
        self.flush();
        let mut backlog = self.backlog.borrow_mut();
        let update = if backlog.is_empty() {
            match self.sender.try_send(update) {
                Ok(()) | Err(TrySendError::Disconnected(_)) => return,
                Err(TrySendError::Full(update)) => update,
            }
        } else {
            update
        };
        if !matches!(update.event, PlaybackEvent::Position { .. }) && backlog.len() < MAX_BACKLOG {
            backlog.push_back(update);
        }
    }

    fn flush(&self) {
        let mut backlog = self.backlog.borrow_mut();
        while let Some(update) = backlog.pop_front() {
            match self.sender.try_send(update) {
                Ok(()) => {}
                Err(TrySendError::Full(update)) => {
                    backlog.push_front(update);
                    break;
                }
                Err(TrySendError::Disconnected(_)) => {
                    backlog.clear();
                    break;
                }
            }
        }
    }

    fn has_backlog(&self) -> bool {
        !self.backlog.borrow().is_empty()
    }
}

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

/// A command-only handle for modal previews. It never exposes application state
/// and cannot consume worker events or keep the worker alive after shutdown.
#[derive(Clone)]
pub struct PlaybackCommandSender {
    requests: SyncSender<RuntimeRequest>,
}

impl PlaybackCommandSender {
    /// # Errors
    /// Returns Busy for a full bounded queue, or Stopped after worker shutdown.
    pub fn execute(
        &self,
        generation: u64,
        command: PlaybackCommand,
    ) -> Result<(), PlaybackRuntimeError> {
        self.requests
            .try_send(RuntimeRequest::Execute {
                generation,
                command,
            })
            .map_err(|error| match error {
                TrySendError::Full(_) => PlaybackRuntimeError::Busy,
                TrySendError::Disconnected(_) => PlaybackRuntimeError::Stopped,
            })
    }
}

pub struct PlaybackRuntime {
    position: PlaybackPositionReader,
    requests: SyncSender<RuntimeRequest>,
    updates: Receiver<PlaybackUpdate>,
    worker: Option<JoinHandle<()>>,
}

impl PlaybackRuntime {
    #[must_use]
    pub fn command_sender(&self) -> PlaybackCommandSender {
        PlaybackCommandSender {
            requests: self.requests.clone(),
        }
    }
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
                playback_worker(
                    &request_receiver,
                    &UpdateSink::new(update_sender),
                    factory,
                    &worker_position,
                );
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
    updates: &UpdateSink,
    mut factory: EngineFactory,
    position: &PlaybackPositionReader,
) {
    let mut active: Option<(u64, Box<dyn PlaybackEngine>)> = None;
    let mut filters = AudioFilterState::default();
    let mut preview: Option<(u64, f64, f64, bool)> = None;
    let mut outgoing: Option<OutgoingEngine> = None;
    let mut fade: Option<VolumeFade> = None;
    let mut transition_options = crate::PlaybackTransitionOptions::default();
    let mut target_volume = 100.0;
    let mut end_fade = EndFade::default();
    let mut active_video_mode = None;
    loop {
        updates.flush();
        let request = if active.is_some() || updates.has_backlog() {
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
                                updates.try_send(PlaybackUpdate {
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
                    outgoing = None;
                    fade = None;
                    transition_options = options.transitions;
                    target_volume = options.initial_volume;
                    end_fade = EndFade {
                        seconds: transition_options.end_fade_seconds,
                        target: target_volume,
                        duration: None,
                        applied: None,
                    };
                    let crossfade = active.is_some()
                        && transition_options.crossfade_seconds > 0.0
                        && !crate::pcm_source::is_pcm_item(&item);
                    let mut launch = (*options).clone();
                    if crossfade || transition_options.start_fade_seconds > 0.0 {
                        launch.initial_volume = 0.0;
                    }
                    // Two players never share one render surface: the
                    // incoming audio of a crossfade draws nothing (review 2.1).
                    if crossfade && matches!(launch.video_mode, crate::MpvVideoMode::RenderApi(_)) {
                        launch.video_mode = crate::MpvVideoMode::AudioOnly;
                    }
                    if crossfade {
                        let old = active.take();
                        active = start_or_replace_engine(
                            None,
                            generation,
                            &launch,
                            item,
                            updates,
                            &mut factory,
                        );
                        if active.is_some()
                            && let Some((_, engine)) = old
                        {
                            outgoing = Some(OutgoingEngine {
                                engine,
                                started: None,
                                paused_at: None,
                                queued: Instant::now(),
                                seconds: transition_options.crossfade_seconds.clamp(0.001, 30.0),
                                equal_power: transition_options.equal_power,
                                target_volume,
                            });
                        }
                    } else {
                        if active_video_mode.is_some_and(|mode| mode != launch.video_mode) {
                            active = None;
                        }
                        active = start_or_replace_engine(
                            active,
                            generation,
                            &launch,
                            item,
                            updates,
                            &mut factory,
                        );
                        if transition_options.start_fade_seconds > 0.0 && active.is_some() {
                            fade = Some(VolumeFade {
                                started: None,
                                seconds: transition_options.start_fade_seconds,
                                from: 0.0,
                                to: target_volume,
                                after: None,
                                fade_back: false,
                            });
                        }
                    }
                    active_video_mode = Some(launch.video_mode);
                    filters = AudioFilterState::from_chain(options.initial_audio_filter.as_deref());
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
                    if active
                        .as_ref()
                        .is_some_and(|(current, _)| *current == generation)
                    {
                        match &command {
                            PlaybackCommand::SetTransitions(options) => {
                                transition_options = *options;
                                end_fade.seconds = options.end_fade_seconds;
                                continue;
                            }
                            PlaybackCommand::SetVolume(volume) => {
                                target_volume = *volume;
                                end_fade.target = *volume;
                                if let Some(old) = outgoing.as_mut() {
                                    old.target_volume = *volume;
                                }
                                if let Some(task) = fade.as_mut() {
                                    if task.after.is_none() {
                                        task.to = *volume;
                                    } else {
                                        task.from = *volume;
                                    }
                                }
                            }
                            // Stop ends the outgoing item of a crossfade too.
                            PlaybackCommand::Stop => {
                                outgoing = None;
                                fade = None;
                            }
                            PlaybackCommand::SeekAbsolute { .. }
                            | PlaybackCommand::SeekRelative { .. } => {
                                // A seek ends a crossfade: the new item gets
                                // its full volume back (review 2.1).
                                if outgoing.take().is_some()
                                    && let Some((_, engine)) = active.as_mut()
                                {
                                    let _ =
                                        engine.execute(PlaybackCommand::SetVolume(target_volume));
                                }
                                // Seeks during a seek fade add up: five
                                // presses seek five steps (review 2.1).
                                if let Some(task) = fade.as_mut()
                                    && let Some(pending) = task.after.as_ref()
                                    && let Some(combined) = combined_seek(pending, &command)
                                {
                                    task.after = Some(combined);
                                    continue;
                                }
                                flush_pending_fade(&mut active, &mut fade, target_volume);
                                if transition_options.seek_fade_seconds > 0.0 {
                                    fade = Some(VolumeFade {
                                        started: Some(Instant::now()),
                                        seconds: transition_options.seek_fade_seconds,
                                        from: target_volume,
                                        to: 0.0,
                                        after: Some(command),
                                        fade_back: true,
                                    });
                                    continue;
                                }
                            }
                            PlaybackCommand::SetPaused(paused) => {
                                // A pause never replaces a waiting seek, nor
                                // a seek a waiting pause: the waiting command
                                // runs first (review 2.1).
                                flush_pending_fade(&mut active, &mut fade, target_volume);
                                if let Some(old) = outgoing.as_mut() {
                                    let _ = old.engine.execute(command.clone());
                                    let now = Instant::now();
                                    if *paused {
                                        old.paused_at.get_or_insert(now);
                                    } else if let Some(at) = old.paused_at.take()
                                        && let Some(started) = old.started.as_mut()
                                    {
                                        *started += now.saturating_duration_since(at);
                                    }
                                }
                                if transition_options.pause_fade_seconds > 0.0 && outgoing.is_none()
                                {
                                    if *paused {
                                        fade = Some(VolumeFade {
                                            started: Some(Instant::now()),
                                            seconds: transition_options.pause_fade_seconds,
                                            from: target_volume,
                                            to: 0.0,
                                            after: Some(command),
                                            fade_back: false,
                                        });
                                        continue;
                                    }
                                    if let Some((_, engine)) = active.as_mut() {
                                        let _ = engine.execute(PlaybackCommand::SetVolume(0.0));
                                    }
                                    fade = Some(VolumeFade {
                                        started: Some(Instant::now()),
                                        seconds: transition_options.pause_fade_seconds,
                                        from: 0.0,
                                        to: target_volume,
                                        after: None,
                                        fade_back: false,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    execute_if_current(&mut active, &mut filters, generation, command, updates);
                }
                RuntimeRequest::Close { generation } => {
                    if active
                        .as_ref()
                        .is_some_and(|(active_generation, _)| *active_generation == generation)
                    {
                        active = None;
                        outgoing = None;
                        fade = None;
                        filters = AudioFilterState::default();
                        preview = None;
                        position.publish(None);
                    }
                }
                RuntimeRequest::Shutdown => break,
            }
        }
        poll_engine_events(
            &mut active,
            updates,
            &mut preview,
            position,
            &mut end_fade.duration,
        );
        tick_transitions(
            &mut active,
            &mut outgoing,
            &mut fade,
            &mut end_fade,
            position,
            updates,
        );
    }
    position.publish(None);
}

fn start_or_replace_engine(
    active: Option<(u64, Box<dyn PlaybackEngine>)>,
    generation: u64,
    options: &MpvLaunchOptions,
    item: Box<MediaItem>,
    updates: &UpdateSink,
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
    match factory(options).map(|engine| -> Box<dyn PlaybackEngine> {
        Box::new(crate::headroom_volume::HeadroomVolume::wrap(
            engine, options,
        ))
    }) {
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

/// Amplitude gains of the outgoing and incoming item at `progress` (0 to 1)
/// of a crossfade: equal power keeps the summed power of two songs steady,
/// linear keeps the summed amplitude.
fn crossfade_gains(progress: f64, equal_power: bool) -> (f64, f64) {
    if equal_power {
        let angle = progress * std::f64::consts::FRAC_PI_2;
        (angle.cos(), angle.sin())
    } else {
        (1.0 - progress, progress)
    }
}

/// The factor of the mpv `volume` that plays an amplitude gain. mpv turns its
/// volume into amplitude as (volume / 100)^3, so a gain applied to the volume
/// itself was cubed: halfway through an equal power crossfade both items were
/// at about -9 dB instead of -3 dB and the incoming item stayed almost silent
/// for the first third (2.1.2). The cube root undoes that.
fn crossfade_volume_factor(gain: f64) -> f64 {
    gain.clamp(0.0, 1.0).cbrt()
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
    filters: &mut AudioFilterState,
    generation: u64,
    command: PlaybackCommand,
    updates: &UpdateSink,
) {
    let Some((active_generation, engine)) = active else {
        return;
    };
    if *active_generation != generation {
        return;
    }
    if let Err(error) = filters.execute(engine.as_mut(), command) {
        updates.try_send(PlaybackUpdate {
            generation,
            event: PlaybackEvent::CommandFailed(error.to_string()),
        });
    }
}

fn poll_engine_events(
    active: &mut Option<(u64, Box<dyn PlaybackEngine>)>,
    updates: &UpdateSink,
    preview: &mut Option<(u64, f64, f64, bool)>,
    position: &PlaybackPositionReader,
    duration: &mut Option<f64>,
) {
    let Some((generation, engine)) = active else {
        return;
    };
    for _ in 0..MAX_EVENTS_PER_TICK {
        match engine.poll_event() {
            Ok(Some(mut event)) => {
                if let PlaybackEvent::Position {
                    elapsed,
                    duration: reported,
                } = &event
                    && elapsed.is_finite()
                {
                    position.publish(Some((*generation, elapsed.max(0.0))));
                    if let Some(reported) = reported.filter(|value| value.is_finite()) {
                        *duration = Some(reported);
                    }
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
                updates.try_send(PlaybackUpdate {
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

fn emit_failure(updates: &UpdateSink, generation: u64, error: &PlaybackError) {
    updates.try_send(PlaybackUpdate {
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
        super::poll_engine_events(
            &mut active,
            &super::UpdateSink::new(sender),
            &mut None,
            &reader,
            &mut None,
        );
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
            &mut crate::AudioFilterState::default(),
            7,
            PlaybackCommand::SeekRelative {
                seconds: 5.0,
                exact: false,
            },
            &super::UpdateSink::new(sender),
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
            &super::UpdateSink::new(sender),
            &mut preview,
            &super::PlaybackPositionReader::default(),
            &mut None,
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
            &super::UpdateSink::new(sender),
            &mut preview,
            &super::PlaybackPositionReader::default(),
            &mut None,
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
            &super::UpdateSink::new(sender),
            &mut preview,
            &super::PlaybackPositionReader::default(),
            &mut None,
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
    fn important_updates_survive_a_full_channel_while_positions_are_dropped() {
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        let sink = super::UpdateSink::new(sender);
        for elapsed in 0..10 {
            sink.try_send(PlaybackUpdate {
                generation: 3,
                event: PlaybackEvent::Position {
                    elapsed: f64::from(elapsed),
                    duration: Some(90.0),
                },
            });
        }
        sink.try_send(PlaybackUpdate {
            generation: 3,
            event: PlaybackEvent::Paused(true),
        });
        sink.try_send(PlaybackUpdate {
            generation: 3,
            event: PlaybackEvent::Ended,
        });
        assert!(sink.has_backlog());
        let mut collected = Vec::new();
        for _ in 0..4 {
            while let Ok(update) = receiver.try_recv() {
                collected.push(update.event);
            }
            sink.flush();
        }
        assert!(!sink.has_backlog());
        let important = collected
            .into_iter()
            .filter(|event| !matches!(event, PlaybackEvent::Position { .. }))
            .collect::<Vec<_>>();
        assert_eq!(
            important,
            vec![PlaybackEvent::Paused(true), PlaybackEvent::Ended]
        );
    }

    #[test]
    fn crossfade_gains_reach_the_output_uncubed() {
        // mpv plays (volume / 100)^3, so the volume factor is a cube root.
        let amplitude = |gain: f64| super::crossfade_volume_factor(gain).powi(3);
        for step in 0..=20 {
            let progress = f64::from(step) / 20.0;
            let (outgoing, incoming) = super::crossfade_gains(progress, true);
            let power = amplitude(outgoing).powi(2) + amplitude(incoming).powi(2);
            assert!(
                (power - 1.0).abs() < 1e-9,
                "equal power at {progress}: {power}"
            );
            let (outgoing, incoming) = super::crossfade_gains(progress, false);
            let sum = amplitude(outgoing) + amplitude(incoming);
            assert!((sum - 1.0).abs() < 1e-9, "linear at {progress}: {sum}");
        }
        // Halfway, equal power plays each item at -3 dB: volume 100 * 0.707^(1/3).
        let (outgoing, _) = super::crossfade_gains(0.5, true);
        assert!((100.0 * super::crossfade_volume_factor(outgoing) - 89.09).abs() < 0.01);
        assert!(super::crossfade_volume_factor(0.0).abs() < 1e-12);
        assert!((super::crossfade_volume_factor(1.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn crossfade_keeps_the_old_engine_until_the_volumes_have_crossed() {
        type Commands = Arc<Mutex<Vec<PlaybackCommand>>>;
        let engines: Arc<Mutex<Vec<Commands>>> = Arc::new(Mutex::new(Vec::new()));
        let created = Arc::clone(&engines);
        let runtime = PlaybackRuntime::spawn_with(Box::new(move |_| {
            let commands = Arc::new(Mutex::new(Vec::new()));
            created.lock().expect("engines").push(Arc::clone(&commands));
            Ok(Box::new(FakeEngine {
                events: vec![PlaybackEvent::Position {
                    elapsed: 0.1,
                    duration: Some(60.0),
                }],
                commands,
            }))
        }))
        .expect("runtime");
        let mut first = MpvLaunchOptions::new("mpv.exe");
        first.initial_volume = 80.0;
        runtime.start(1, first, item("first")).expect("first start");
        std::thread::sleep(Duration::from_millis(50));
        let mut second = MpvLaunchOptions::new("mpv.exe");
        second.initial_volume = 80.0;
        second.transitions.crossfade_seconds = 0.2;
        second.transitions.equal_power = false;
        runtime
            .start(2, second, item("second"))
            .expect("second start");
        std::thread::sleep(Duration::from_millis(500));

        let engines = engines.lock().expect("engines");
        assert_eq!(engines.len(), 2, "the next item gets its own engine");
        let volumes = |engine: &Commands| -> Vec<f64> {
            engine
                .lock()
                .expect("commands")
                .iter()
                .filter_map(|command| match command {
                    PlaybackCommand::SetVolume(volume) => Some(*volume),
                    _ => None,
                })
                .collect()
        };
        let outgoing = volumes(&engines[0]);
        let incoming = volumes(&engines[1]);
        assert!(outgoing.len() > 2, "the old item fades out: {outgoing:?}");
        assert!(
            outgoing.windows(2).all(|pair| pair[1] <= pair[0]),
            "{outgoing:?}"
        );
        assert!(
            outgoing.last().is_some_and(|volume| *volume < 1.0),
            "{outgoing:?}"
        );
        assert!(
            incoming.windows(2).all(|pair| pair[1] >= pair[0]),
            "{incoming:?}"
        );
        assert!(
            incoming
                .last()
                .is_some_and(|volume| (*volume - 80.0).abs() < 1.0),
            "{incoming:?}"
        );
    }

    /// An engine that tells when it is dropped, that is, stopped.
    struct DroppedEngine {
        commands: Arc<Mutex<Vec<PlaybackCommand>>>,
        dropped: Arc<AtomicUsize>,
    }

    impl PlaybackEngine for DroppedEngine {
        fn execute(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError> {
            self.commands.lock().expect("commands").push(command);
            Ok(())
        }

        fn poll_event(&mut self) -> Result<Option<PlaybackEvent>, PlaybackError> {
            Ok(Some(PlaybackEvent::Position {
                elapsed: 0.1,
                duration: Some(60.0),
            }))
        }
    }

    impl Drop for DroppedEngine {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    type Launches = Arc<Mutex<Vec<crate::MpvVideoMode>>>;
    type EngineCommands = Arc<Mutex<Vec<Arc<Mutex<Vec<PlaybackCommand>>>>>>;

    fn crossfading_runtime(
        dropped: &Arc<AtomicUsize>,
        launches: &Launches,
        engines: &EngineCommands,
    ) -> PlaybackRuntime {
        let dropped = Arc::clone(dropped);
        let launches = Arc::clone(launches);
        let engines = Arc::clone(engines);
        PlaybackRuntime::spawn_with(Box::new(move |options| {
            launches.lock().expect("launches").push(options.video_mode);
            let commands = Arc::new(Mutex::new(Vec::new()));
            engines.lock().expect("engines").push(Arc::clone(&commands));
            Ok(Box::new(DroppedEngine {
                commands,
                dropped: Arc::clone(&dropped),
            }))
        }))
        .expect("runtime")
    }

    fn start_crossfade(runtime: &PlaybackRuntime, seconds: f64) {
        let mut first = MpvLaunchOptions::new("mpv");
        first.video_mode = crate::MpvVideoMode::RenderApi(7);
        runtime.start(1, first, item("first")).expect("first");
        std::thread::sleep(Duration::from_millis(50));
        let mut second = MpvLaunchOptions::new("mpv");
        second.video_mode = crate::MpvVideoMode::RenderApi(7);
        second.transitions.crossfade_seconds = seconds;
        runtime.start(2, second, item("second")).expect("second");
        std::thread::sleep(Duration::from_millis(80));
    }

    /// Review 2.1: Stop during a crossfade stops the item going out too, and
    /// the incoming item of a crossfade never takes the render surface.
    #[test]
    fn stop_during_a_crossfade_ends_both_items() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let launches = Launches::default();
        let engines = Arc::default();
        let runtime = crossfading_runtime(&dropped, &launches, &engines);
        start_crossfade(&runtime, 20.0);
        assert_eq!(dropped.load(Ordering::SeqCst), 0, "both still play");
        assert_eq!(
            *launches.lock().unwrap(),
            [
                crate::MpvVideoMode::RenderApi(7),
                crate::MpvVideoMode::AudioOnly
            ]
        );
        runtime.execute(2, PlaybackCommand::Stop).expect("stop");
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(dropped.load(Ordering::SeqCst), 1, "the old item stopped");
    }

    /// Review 2.1: a pause freezes the crossfade; it goes on after Play.
    #[test]
    fn a_paused_crossfade_waits() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let launches = Launches::default();
        let engines = EngineCommands::default();
        let runtime = crossfading_runtime(&dropped, &launches, &engines);
        start_crossfade(&runtime, 0.3);
        runtime
            .execute(2, PlaybackCommand::SetPaused(true))
            .expect("pause");
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            0,
            "the old item waits paused"
        );
        runtime
            .execute(2, PlaybackCommand::SetPaused(false))
            .expect("play");
        std::thread::sleep(Duration::from_millis(600));
        assert_eq!(dropped.load(Ordering::SeqCst), 1, "then the crossfade ends");
    }

    /// Review 2.1: a seek that ends a crossfade gives the new item its
    /// volume back, and pause and seek during fades both happen.
    #[test]
    fn a_seek_during_a_crossfade_restores_the_volume() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let launches = Launches::default();
        let engines = EngineCommands::default();
        let runtime = crossfading_runtime(&dropped, &launches, &engines);
        start_crossfade(&runtime, 10.0);
        runtime
            .execute(
                2,
                PlaybackCommand::SeekRelative {
                    seconds: 5.0,
                    exact: false,
                },
            )
            .expect("seek");
        std::thread::sleep(Duration::from_millis(80));
        let incoming = engines.lock().unwrap()[1].lock().unwrap().clone();
        let last_volume = incoming.iter().rev().find_map(|command| match command {
            PlaybackCommand::SetVolume(volume) => Some(*volume),
            _ => None,
        });
        assert_eq!(last_volume, Some(100.0), "{incoming:?}");
    }

    #[test]
    fn a_pause_and_a_seek_during_fades_both_happen() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let launches = Launches::default();
        let engines = EngineCommands::default();
        let runtime = crossfading_runtime(&dropped, &launches, &engines);
        let mut options = MpvLaunchOptions::new("mpv");
        options.transitions.pause_fade_seconds = 2.0;
        options.transitions.seek_fade_seconds = 2.0;
        runtime.start(1, options, item("one")).expect("start");
        std::thread::sleep(Duration::from_millis(50));
        runtime
            .execute(1, PlaybackCommand::SetPaused(true))
            .expect("pause");
        runtime
            .execute(
                1,
                PlaybackCommand::SeekRelative {
                    seconds: 5.0,
                    exact: false,
                },
            )
            .expect("seek");
        std::thread::sleep(Duration::from_millis(80));
        let commands = engines.lock().unwrap()[0].lock().unwrap().clone();
        assert!(
            commands.contains(&PlaybackCommand::SetPaused(true)),
            "the pause is not lost: {commands:?}"
        );
    }

    #[test]
    fn seeks_during_a_seek_fade_add_up() {
        let relative = |seconds| PlaybackCommand::SeekRelative {
            seconds,
            exact: false,
        };
        assert_eq!(
            super::combined_seek(&relative(5.0), &relative(5.0)),
            Some(relative(10.0))
        );
        assert_eq!(
            super::combined_seek(
                &PlaybackCommand::SeekAbsolute {
                    seconds: 30.0,
                    exact: true
                },
                &relative(-5.0)
            ),
            Some(PlaybackCommand::SeekAbsolute {
                seconds: 25.0,
                exact: true
            })
        );
        assert_eq!(
            super::combined_seek(&PlaybackCommand::SetPaused(true), &relative(5.0)),
            None
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

#[cfg(test)]
mod end_fade_tests {
    use super::end_fade_volume;

    #[test]
    fn end_fade_lowers_only_the_last_seconds_of_long_files() {
        assert_eq!(end_fade_volume(80.0, 50.0, 60.0, 2.0), None);
        let volume = end_fade_volume(80.0, 59.0, 60.0, 2.0).expect("fading");
        assert!((volume - 40.0).abs() < 1e-9);
        assert_eq!(end_fade_volume(80.0, 60.5, 60.0, 2.0), Some(0.0));
        assert_eq!(
            end_fade_volume(80.0, 3.0, 3.5, 2.0),
            None,
            "too short to fade"
        );
        assert_eq!(end_fade_volume(80.0, 59.0, 60.0, 0.0), None, "off");
    }
}
