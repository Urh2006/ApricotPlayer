//! Native multi-source capture, mixing and transactional `FFmpeg` encoding.
//! Capturers own their OS objects on workers. UI closure never owns the session.

use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

#[cfg(windows)]
#[path = "recording_windows.rs"]
mod native;
#[cfg(target_os = "macos")]
#[path = "recording_macos.rs"]
mod native;

/// Why a recording could not start or finish; the interface words each case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecordingError {
    NoSources,
    TooManySources,
    InvalidFormat,
    InvalidLevel,
    /// The file name is taken, or became taken while recording.
    DestinationExists,
    CreateFolder,
    /// A temporary file of an unfinished recording has the same name.
    UnfinishedExists,
    EncoderStart,
    NoneOpened,
    /// `FFmpeg` failed; its own report follows.
    NotCompleted(String),
    /// The recording stopped early; what was recorded is kept in this file.
    Incomplete(PathBuf),
    /// The recording is whole but could not take its name; it is kept in
    /// this file.
    SaveFailed(PathBuf),
    /// Listing the sources failed, with the system's report.
    Discovery(String),
    Unsupported,
}

pub const CAPTURE_RATE: u32 = 48_000;
const BLOCK_FRAMES: usize = 960;
const MAX_BUFFER_SAMPLES: usize = CAPTURE_RATE as usize * 2 * 2;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CaptureKind {
    Input {
        device: String,
    },
    Output {
        device: String,
    },
    Application {
        process_id: u32,
        identity: String,
        exclude: bool,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CaptureSource {
    pub name: String,
    pub kind: CaptureKind,
    pub gain: f32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum RecordingFormat {
    #[default]
    Flac,
    Wav,
    Mp3,
    Aac,
}

impl RecordingFormat {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Flac => "flac",
            Self::Wav => "wav",
            Self::Mp3 => "mp3",
            Self::Aac => "m4a",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum RecordingFeedback {
    #[default]
    Sounds,
    Speech,
    Both,
    None,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct RecordingSettings {
    pub sources: Vec<CaptureSource>,
    pub format: RecordingFormat,
    pub sample_rate: u32,
    pub channels: u8,
    pub bitrate: u32,
    pub directory: PathBuf,
    pub remember: bool,
    pub feedback: RecordingFeedback,
}

impl Default for RecordingSettings {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            format: RecordingFormat::Flac,
            sample_rate: 48_000,
            channels: 2,
            bitrate: 192,
            directory: PathBuf::new(),
            remember: false,
            feedback: RecordingFeedback::Sounds,
        }
    }
}

impl RecordingSettings {
    /// Checks the settings before a recording starts.
    ///
    /// # Errors
    ///
    /// Returns what is missing or out of range.
    pub fn validate(&self) -> Result<(), RecordingError> {
        if self.sources.is_empty() {
            return Err(RecordingError::NoSources);
        }
        if self.sources.len() > 16 {
            return Err(RecordingError::TooManySources);
        }
        if !matches!(
            self.sample_rate,
            8_000
                | 11_025
                | 16_000
                | 22_050
                | 24_000
                | 32_000
                | 44_100
                | 48_000
                | 88_200
                | 96_000
                | 192_000
        ) || !(1..=2).contains(&self.channels)
            || !(32..=512).contains(&self.bitrate)
        {
            return Err(RecordingError::InvalidFormat);
        }
        if self
            .sources
            .iter()
            .any(|source| !source.gain.is_finite() || !(0.0..=4.0).contains(&source.gain))
        {
            return Err(RecordingError::InvalidLevel);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum RecordingEvent {
    Started {
        output: PathBuf,
        sources: Vec<String>,
    },
    /// `None` when the source never became ready, usually missing
    /// permission; otherwise the system's report.
    SourceFailed {
        name: String,
        error: Option<String>,
    },
    Paused(bool),
    Finished(Result<PathBuf, RecordingError>),
}

/// Stop and Pause of a running recording, usable while its session is out
/// of reach, such as inside an open list window (review 2.1).
#[derive(Clone, Debug)]
pub struct RecordingControls {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    pauses_spoken: Arc<AtomicUsize>,
}

impl RecordingControls {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
    /// Pauses or resumes; `true` when it is now paused. The caller says so,
    /// so the session's own Paused event for it is not spoken again.
    pub fn toggle_pause(&self) -> bool {
        let paused = !self.paused.load(Ordering::Acquire);
        self.paused.store(paused, Ordering::Release);
        self.pauses_spoken.fetch_add(1, Ordering::AcqRel);
        paused
    }
}

pub struct RecordingSession {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    pauses_spoken: Arc<AtomicUsize>,
    events: Receiver<RecordingEvent>,
    worker: Option<JoinHandle<()>>,
}

impl RecordingSession {
    /// Starts recording on a worker; events report the outcome.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid settings or a taken file name.
    pub fn start(
        settings: RecordingSettings,
        ffmpeg: PathBuf,
        output: PathBuf,
    ) -> Result<Self, RecordingError> {
        settings.validate()?;
        if output.exists() {
            return Err(RecordingError::DestinationExists);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let (send, events) = mpsc::channel();
        let worker_stop = Arc::clone(&stop);
        let worker_pause = Arc::clone(&paused);
        let worker = thread::Builder::new()
            .name("apricot-recording".into())
            .spawn(move || {
                let result = record(
                    &settings,
                    &ffmpeg,
                    &output,
                    &worker_stop,
                    &worker_pause,
                    &send,
                );
                worker_stop.store(true, Ordering::Release);
                let _ = send.send(RecordingEvent::Finished(result));
            })
            .map_err(|_| RecordingError::EncoderStart)?;
        Ok(Self {
            stop,
            paused,
            pauses_spoken: Arc::default(),
            events,
            worker: Some(worker),
        })
    }

    #[must_use]
    pub fn controls(&self) -> RecordingControls {
        RecordingControls {
            stop: Arc::clone(&self.stop),
            paused: Arc::clone(&self.paused),
            pauses_spoken: Arc::clone(&self.pauses_spoken),
        }
    }

    /// `true` once for each pause or resume that `RecordingControls`
    /// already said: its Paused event is then left unspoken.
    pub fn take_spoken_pause(&self) -> bool {
        self.pauses_spoken
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_sub(1)
            })
            .is_ok()
    }

    pub fn pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }
    /// Pauses or resumes; `true` when it is now paused.
    pub fn toggle_pause(&self) -> bool {
        let paused = !self.paused.load(Ordering::Acquire);
        self.pause(paused);
        paused
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }
    /// Recording, or ended with its last events not yet polled: the poll
    /// that reads Finished is the one that makes it inactive (review 2.1).
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.worker.is_some()
    }
    /// Stops and waits until the file is saved or the failure is known,
    /// so quitting never leaves a recording unfinished (review 2.1).
    pub fn finish(&mut self) -> Option<Result<PathBuf, RecordingError>> {
        self.stop();
        let worker = self.worker.take()?;
        let _ = worker.join();
        self.events.try_iter().find_map(|event| match event {
            RecordingEvent::Finished(result) => Some(result),
            _ => None,
        })
    }

    pub fn poll(&mut self) -> Vec<RecordingEvent> {
        // A worker that ended has sent its last event, so the events are read
        // after it is joined: Finished is never left behind (review 2.1).
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished)
            && let Some(worker) = self.worker.take()
        {
            let _ = worker.join();
        }
        self.events.try_iter().collect()
    }
}

impl Drop for RecordingSession {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// The microphones, computer audio outputs and applications that can be
/// recorded now.
///
/// # Errors
///
/// Returns the system's report when the list cannot be read.
pub fn sources() -> Result<Vec<CaptureSource>, RecordingError> {
    #[cfg(any(windows, target_os = "macos"))]
    return thread::spawn(native::sources)
        .join()
        .map_err(|_| RecordingError::Discovery(String::new()))?
        .map_err(RecordingError::Discovery);
    #[cfg(not(any(windows, target_os = "macos")))]
    Err(RecordingError::Unsupported)
}

pub(crate) struct CaptureQueue {
    samples: Mutex<VecDeque<f32>>,
    #[cfg(target_os = "macos")]
    error: Mutex<Option<String>>,
}

impl CaptureQueue {
    fn new() -> Self {
        Self {
            samples: Mutex::new(VecDeque::new()),
            #[cfg(target_os = "macos")]
            error: Mutex::new(None),
        }
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn fail(&self, message: &str) {
        if let Ok(mut error) = self.error.lock() {
            *error = Some(message.to_owned());
        }
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn take_error(&self) -> Option<String> {
        self.error.lock().ok()?.take()
    }
    pub(crate) fn push(&self, samples: impl IntoIterator<Item = f32>) {
        if let Ok(mut queue) = self.samples.lock() {
            queue.extend(
                samples
                    .into_iter()
                    .map(|sample| if sample.is_finite() { sample } else { 0.0 }),
            );
            let excess = queue.len().saturating_sub(MAX_BUFFER_SAMPLES);
            if excess > 0 {
                queue.drain(..excess);
            }
        }
    }
    fn mix(&self, output: &mut [f32], gain: f32, discard: bool) {
        if let Ok(mut queue) = self.samples.lock() {
            if discard {
                queue.clear();
                return;
            }
            for target in output {
                if let Some(sample) = queue.pop_front() {
                    *target += sample * gain;
                } else {
                    break;
                }
            }
        }
    }
}

#[allow(clippy::too_many_lines)] // One transaction from encoder start to the final rename.
fn record(
    settings: &RecordingSettings,
    ffmpeg: &Path,
    output: &Path,
    stop: &Arc<AtomicBool>,
    paused: &Arc<AtomicBool>,
    send: &Sender<RecordingEvent>,
) -> Result<PathBuf, RecordingError> {
    let parent = output.parent().ok_or(RecordingError::CreateFolder)?;
    std::fs::create_dir_all(parent).map_err(|_| RecordingError::CreateFolder)?;
    let temporary = output.with_file_name(format!(
        ".{}-recording.{}",
        output.file_stem().unwrap_or_default().to_string_lossy(),
        settings.format.extension()
    ));
    if temporary.exists() {
        return Err(RecordingError::UnfinishedExists);
    }
    let mut command = Command::new(ffmpeg);
    command
        .args(encoder_arguments(settings, &temporary))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut encoder = command.spawn().map_err(|_| RecordingError::EncoderStart)?;
    let stderr = encoder.stderr.take().ok_or(RecordingError::EncoderStart)?;
    let errors = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.take(16_384).read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let Some(mut input) = encoder.stdin.take() else {
        let _ = encoder.kill();
        return Err(RecordingError::EncoderStart);
    };
    let mut capture_workers = Vec::new();
    let mut queues = Vec::new();
    let mut successful = Vec::new();
    for source in &settings.sources {
        // Stop while sources open: no further source is opened (review 2.1).
        if stop.load(Ordering::Acquire) {
            break;
        }
        let source = source.clone();
        let queue = Arc::new(CaptureQueue::new());
        let capture_queue = Arc::clone(&queue);
        let capture_stop = Arc::clone(stop);
        let messages = send.clone();
        let (ready_send, ready) = mpsc::sync_channel(1);
        let name = source.name.clone();
        let source_gain = source.gain;
        let handle = thread::spawn(move || {
            #[cfg(any(windows, target_os = "macos"))]
            let result = native::capture(&source, capture_queue, capture_stop, ready_send);
            #[cfg(not(any(windows, target_os = "macos")))]
            let result: Result<(), String> = {
                let _ = (capture_queue, capture_stop);
                let _ = ready_send.send(Err(String::new()));
                Ok(())
            };
            if let Err(error) = result {
                let _ = messages.send(RecordingEvent::SourceFailed {
                    name: source.name,
                    error: Some(error),
                });
            }
        });
        // Up to 30 seconds for permission or the device, but Stop ends the
        // wait at once.
        let waited = Instant::now();
        let answer = loop {
            match ready.recv_timeout(Duration::from_millis(100)) {
                Err(mpsc::RecvTimeoutError::Timeout)
                    if !stop.load(Ordering::Acquire)
                        && waited.elapsed() < Duration::from_secs(30) => {}
                other => break other,
            }
        };
        match answer {
            Ok(Ok(())) => {
                queues.push((queue, source_gain));
                successful.push(name);
            }
            Ok(Err(error)) => {
                let _ = send.send(RecordingEvent::SourceFailed {
                    name,
                    error: Some(error).filter(|error| !error.is_empty()),
                });
            }
            Err(_) => {
                let _ = send.send(RecordingEvent::SourceFailed { name, error: None });
            }
        }
        capture_workers.push(handle);
    }
    if queues.is_empty() {
        stop.store(true, Ordering::Release);
        drop(input);
        // An encoder that does not end after its input closed never holds
        // Stop or quitting (review 4, A5).
        let _ = end_encoder(&mut encoder, Duration::from_secs(15));
        let _ = std::fs::remove_file(&temporary);
        return Err(RecordingError::NoneOpened);
    }
    // All sources begin together: what a first source captured while a
    // later one was still opening is dropped, so voices and music line up
    // (review 2.1).
    for (queue, _) in &queues {
        queue.mix(&mut [], 1.0, true);
    }
    let _ = send.send(RecordingEvent::Started {
        output: output.to_owned(),
        sources: successful,
    });
    // The encoder is fed by its own thread, so an encoder that stops
    // reading (a full disk, a lost network folder) can never hold Stop or
    // quitting: a block that waits too long ends the recording, and the
    // encoder is ended if its input still does not close (review 2.1).
    let (blocks, encoder_blocks) = mpsc::sync_channel::<Vec<u8>>(100);
    let writer = thread::spawn(move || {
        for bytes in encoder_blocks {
            if input.write_all(&bytes).is_err() {
                return false;
            }
        }
        true
    });
    // Blocks the encoder has not taken yet wait here instead of being
    // dropped, up to ten seconds of sound; a short stall loses nothing, a
    // longer one ends the recording as incomplete (review 4, A3).
    let mut waiting: std::collections::VecDeque<Vec<u8>> = std::collections::VecDeque::new();
    let waiting_limit = (10_000 / 20) as usize;
    let mut deadline = Instant::now();
    let interval = Duration::from_millis(20);
    let mut was_paused = false;
    let mut write_error = false;
    let mut sources_lost = false;
    // One source of several that stopped (an unplugged microphone) makes
    // the recording incomplete too, while the others go on (review 4, A4).
    let mut source_failed = false;
    while !stop.load(Ordering::Acquire) {
        let is_paused = paused.load(Ordering::Acquire);
        if was_paused != is_paused {
            let _ = send.send(RecordingEvent::Paused(is_paused));
            was_paused = is_paused;
        }
        let mut mixed = vec![0.0_f32; BLOCK_FRAMES * 2];
        for (queue, gain) in &queues {
            queue.mix(&mut mixed, *gain, is_paused);
        }
        if !is_paused {
            let bytes: Vec<_> = mixed
                .into_iter()
                .flat_map(|sample| sample.clamp(-1.0, 1.0).to_le_bytes())
                .collect();
            waiting.push_back(bytes);
        }
        if !hand_over(&blocks, &mut waiting) || waiting.len() > waiting_limit {
            write_error = true;
            break;
        }
        deadline += interval;
        if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        } else if deadline.elapsed() > Duration::from_millis(500) {
            deadline = Instant::now();
        }
        // Every source stopped (a device was unplugged): what came before
        // is kept as an incomplete recording, not told as a normal one.
        // A source that ends before Stop was asked for failed; after Stop
        // they all end on purpose.
        if stop.load(Ordering::Acquire) {
            break;
        }
        if capture_workers.iter().any(JoinHandle::is_finished) {
            source_failed = true;
        }
        if capture_workers.iter().all(JoinHandle::is_finished) {
            sources_lost = true;
            break;
        }
    }
    stop.store(true, Ordering::Release);
    // What still waits goes to the encoder before its input closes, for a
    // few seconds at most.
    let flushing_ends = Instant::now() + Duration::from_secs(5);
    while !write_error && !waiting.is_empty() {
        if !hand_over(&blocks, &mut waiting) || Instant::now() > flushing_ends {
            write_error = true;
            break;
        }
        if !waiting.is_empty() {
            thread::sleep(Duration::from_millis(10));
        }
    }
    drop(blocks);
    for worker in capture_workers {
        let _ = worker.join();
    }
    let writing_ends = Instant::now() + Duration::from_secs(15);
    while !writer.is_finished() && Instant::now() < writing_ends {
        thread::sleep(Duration::from_millis(25));
    }
    if !writer.is_finished() {
        // The encoder never read the rest: ending it closes the pipe.
        let _ = encoder.kill();
    }
    if !writer.join().unwrap_or(false) {
        write_error = true;
    }
    let status = end_encoder(&mut encoder, Duration::from_secs(15));
    let errors = errors.join().unwrap_or_default();
    let completed = !write_error
        && !sources_lost
        && !source_failed
        && status.is_some_and(|status| status.success());
    let recorded = std::fs::metadata(&temporary).is_ok_and(|metadata| metadata.len() > 0);
    if !completed {
        if !recorded {
            let _ = std::fs::remove_file(&temporary);
            return Err(RecordingError::NotCompleted(errors.trim().to_owned()));
        }
        // Never throw away what was recorded: it stays, visible, beside
        // the name the recording was to have.
        let kept = crate::file_operations::available_path(&incomplete_name(output));
        return match std::fs::rename(&temporary, &kept) {
            Ok(()) => Err(RecordingError::Incomplete(kept)),
            Err(_) => Err(RecordingError::Incomplete(temporary)),
        };
    }
    // A file that took the name meanwhile keeps it; the recording takes
    // the next free one.
    let saved = crate::file_operations::available_path(output);
    std::fs::rename(&temporary, &saved).map_err(|_| RecordingError::SaveFailed(temporary))?;
    Ok(saved)
}

/// Gives the encoder's writer the waiting blocks in order, as many as it
/// takes now. `false` when the writer is gone.
fn hand_over(
    blocks: &mpsc::SyncSender<Vec<u8>>,
    waiting: &mut std::collections::VecDeque<Vec<u8>>,
) -> bool {
    while let Some(bytes) = waiting.pop_front() {
        match blocks.try_send(bytes) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(bytes)) => {
                waiting.push_front(bytes);
                return true;
            }
            Err(mpsc::TrySendError::Disconnected(_)) => return false,
        }
    }
    true
}

/// Waits for the encoder to end, at most `limit`, then ends it.
fn end_encoder(
    encoder: &mut std::process::Child,
    limit: Duration,
) -> Option<std::process::ExitStatus> {
    let end = Instant::now() + limit;
    loop {
        match encoder.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < end => thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = encoder.kill();
                let _ = encoder.wait();
                return None;
            }
        }
    }
}

/// "Recording 2026-11-15.flac" becomes "Recording 2026-11-15 (incomplete).flac".
fn incomplete_name(output: &Path) -> PathBuf {
    let stem = output.file_stem().unwrap_or_default().to_string_lossy();
    let extension = output
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    output.with_file_name(format!("{stem} (incomplete){extension}"))
}

/// `FFmpeg` reads the mixed 48 kHz stereo floats from its input and never
/// overwrites a file.
#[must_use]
pub fn encoder_arguments(settings: &RecordingSettings, output: &Path) -> Vec<String> {
    let mut values: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-n",
        "-f",
        "f32le",
        "-ar",
        "48000",
        "-ac",
        "2",
        "-i",
        "pipe:0",
        "-vn",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    values.extend([
        "-ar".into(),
        settings.sample_rate.to_string(),
        "-ac".into(),
        settings.channels.to_string(),
    ]);
    let format: Vec<String> = match settings.format {
        RecordingFormat::Flac => vec!["-c:a".into(), "flac".into()],
        RecordingFormat::Wav => vec!["-c:a".into(), "pcm_s24le".into()],
        RecordingFormat::Mp3 => vec![
            "-c:a".into(),
            "libmp3lame".into(),
            "-b:a".into(),
            format!("{}k", settings.bitrate.min(320)),
        ],
        RecordingFormat::Aac => vec![
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            format!("{}k", settings.bitrate),
            "-movflags".into(),
            "+faststart".into(),
        ],
    };
    values.extend(format);
    values.push(output.to_string_lossy().into_owned());
    values
}

#[cfg(test)]
mod tests {
    /// Review 4, A3: blocks the encoder cannot take yet wait in order
    /// instead of being dropped.
    #[test]
    fn blocks_wait_while_the_encoder_is_busy() {
        let (send, receive) = std::sync::mpsc::sync_channel::<Vec<u8>>(1);
        let mut waiting: std::collections::VecDeque<Vec<u8>> =
            [vec![1], vec![2], vec![3]].into_iter().collect();
        assert!(super::hand_over(&send, &mut waiting));
        assert_eq!(waiting.len(), 2);
        assert_eq!(receive.recv().unwrap(), vec![1]);
        assert!(super::hand_over(&send, &mut waiting));
        assert_eq!(receive.recv().unwrap(), vec![2]);
        assert!(super::hand_over(&send, &mut waiting));
        assert_eq!(receive.recv().unwrap(), vec![3]);
        assert!(waiting.is_empty());
        drop(receive);
        waiting.push_back(vec![4]);
        assert!(!super::hand_over(&send, &mut waiting));
    }

    /// Review 4, A5: an encoder that does not end is ended after the limit.
    #[cfg(windows)]
    #[test]
    fn an_encoder_that_does_not_end_is_ended() {
        let mut child = std::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn");
        let started = std::time::Instant::now();
        let status = super::end_encoder(&mut child, std::time::Duration::from_millis(300));
        assert!(status.is_none());
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    /// Review 2.1: a worker that sends Finished and ends between two polls
    /// is still told, and the session stays active until it is.
    #[test]
    fn the_last_event_of_a_finished_worker_is_never_lost() {
        use std::sync::{Arc, atomic::AtomicBool, mpsc};
        let (send, events) = mpsc::channel();
        let (go, wait) = mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let _ = wait.recv();
            let _ = send.send(super::RecordingEvent::Finished(Ok("done.flac".into())));
        });
        let mut session = super::RecordingSession {
            stop: Arc::new(AtomicBool::new(false)),
            paused: Arc::new(AtomicBool::new(false)),
            pauses_spoken: Arc::default(),
            events,
            worker: Some(worker),
        };
        assert!(session.poll().is_empty());
        assert!(session.is_active());
        go.send(()).expect("go");
        let mut finished = false;
        for _ in 0..200 {
            finished = session
                .poll()
                .iter()
                .any(|event| matches!(event, super::RecordingEvent::Finished(Ok(_))));
            if finished {
                break;
            }
            assert!(session.is_active(), "inactive before Finished was read");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(finished);
        assert!(!session.is_active());
    }

    use super::*;

    /// Review 2.1: a recording that stops early keeps its sound under a
    /// visible name beside the one it was to have.
    #[test]
    fn an_early_stop_is_kept_as_an_incomplete_file() {
        assert_eq!(
            incomplete_name(Path::new("/Rec/Recording 2026-11-15 10-00-00.flac")),
            Path::new("/Rec/Recording 2026-11-15 10-00-00 (incomplete).flac")
        );
        assert_eq!(
            incomplete_name(Path::new("/Rec/take")),
            Path::new("/Rec/take (incomplete)")
        );
    }

    #[test]
    #[allow(clippy::float_cmp)] // Exact binary fractions.
    fn mixer_combines_levels_and_does_not_replay_paused_audio() {
        let queue = CaptureQueue::new();
        queue.push([0.5, 0.25, -0.5, -0.25]);
        let mut output = [0.0; 4];
        queue.mix(&mut output, 2.0, false);
        assert_eq!(output, [1.0, 0.5, -1.0, -0.5]);
        queue.push([0.9, 0.9]);
        queue.mix(&mut output, 1.0, true);
        let mut resumed = [0.0; 2];
        queue.mix(&mut resumed, 1.0, false);
        assert_eq!(resumed, [0.0; 2]);
    }

    #[test]
    fn defaults_are_lossless_and_encoder_arguments_do_not_overwrite() {
        let settings = RecordingSettings::default();
        assert_eq!(settings.format, RecordingFormat::Flac);
        assert!(!settings.remember);
        let args = encoder_arguments(&settings, Path::new("recording.flac"));
        assert!(args.contains(&"-n".into()));
        assert!(args.contains(&"flac".into()));
        assert_eq!(args.last().unwrap(), "recording.flac");
    }
}
