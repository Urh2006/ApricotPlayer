//! Native multi-source capture, mixing and transactional FFmpeg encoding.
//! Capturers own their OS objects on workers. UI closure never owns the session.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.

use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
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
    pub fn validate(&self) -> Result<(), String> {
        if self.sources.is_empty() {
            return Err("Choose at least one recording source.".into());
        }
        if self.sources.len() > 16 {
            return Err("Use no more than 16 recording sources.".into());
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
            return Err("Invalid recording sample rate, channels or bitrate.".into());
        }
        if self
            .sources
            .iter()
            .any(|source| !source.gain.is_finite() || !(0.0..=4.0).contains(&source.gain))
        {
            return Err("Invalid recording source level.".into());
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
    SourceFailed {
        name: String,
        error: String,
    },
    Paused(bool),
    Finished(Result<PathBuf, String>),
}

pub struct RecordingSession {
    stop: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    events: Receiver<RecordingEvent>,
    worker: Option<JoinHandle<()>>,
}

impl RecordingSession {
    pub fn start(
        settings: RecordingSettings,
        ffmpeg: PathBuf,
        output: PathBuf,
    ) -> Result<Self, String> {
        settings.validate()?;
        if output.exists() {
            return Err("The recording destination already exists. Choose a new file name.".into());
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
            .map_err(|_| "Unable to start the recording worker.".to_owned())?;
        Ok(Self {
            stop,
            paused,
            events,
            worker: Some(worker),
        })
    }

    pub fn pause(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release)
    }
    pub fn toggle_pause(&self) -> bool {
        let paused = !self.paused.load(Ordering::Acquire);
        self.pause(paused);
        paused
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release)
    }
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
    }
    pub fn poll(&mut self) -> Vec<RecordingEvent> {
        let events = self.events.try_iter().collect();
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
        events
    }
}

impl Drop for RecordingSession {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn sources() -> Result<Vec<CaptureSource>, String> {
    #[cfg(any(windows, target_os = "macos"))]
    return thread::spawn(native::sources)
        .join()
        .map_err(|_| "Recording source discovery stopped unexpectedly.".to_owned())?;
    #[cfg(not(any(windows, target_os = "macos")))]
    Err("Native recording is available on Windows and macOS.".into())
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
            *error = Some(message.to_owned())
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
                    *target += sample * gain
                } else {
                    break;
                }
            }
        }
    }
}

fn record(
    settings: &RecordingSettings,
    ffmpeg: &Path,
    output: &Path,
    stop: &Arc<AtomicBool>,
    paused: &Arc<AtomicBool>,
    send: &Sender<RecordingEvent>,
) -> Result<PathBuf, String> {
    let parent = output
        .parent()
        .ok_or("Choose a recording destination folder.")?;
    std::fs::create_dir_all(parent)
        .map_err(|_| "Unable to create the recording destination.".to_owned())?;
    let temporary = output.with_file_name(format!(
        ".{}-recording.{}",
        output.file_stem().unwrap_or_default().to_string_lossy(),
        settings.format.extension()
    ));
    if temporary.exists() {
        return Err("A previous unfinished recording has the same name. Choose a new recording destination.".into());
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
    let mut encoder = command
        .spawn()
        .map_err(|_| "Unable to start FFmpeg for recording.".to_owned())?;
    let stderr = encoder
        .stderr
        .take()
        .ok_or("Unable to read the recording encoder status.")?;
    let errors = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.take(16_384).read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let Some(mut input) = encoder.stdin.take() else {
        let _ = encoder.kill();
        return Err("Unable to open the recording encoder input.".into());
    };
    let mut capture_workers = Vec::new();
    let mut queues = Vec::new();
    let mut successful = Vec::new();
    for source in &settings.sources {
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
                let _ = ready_send.send(Err("Native recording is unavailable.".into()));
                Ok(())
            };
            if let Err(error) = result {
                let _ = messages.send(RecordingEvent::SourceFailed {
                    name: source.name,
                    error,
                });
            }
        });
        match ready.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(())) => {
                queues.push((queue, source_gain));
                successful.push(name);
            }
            Ok(Err(error)) => {
                let _ = send.send(RecordingEvent::SourceFailed { name, error });
            }
            Err(_) => {
                let _ = send.send(RecordingEvent::SourceFailed {
                    name,
                    error: "The source did not become ready. Check recording permissions.".into(),
                });
            }
        }
        capture_workers.push(handle);
    }
    if queues.is_empty() {
        stop.store(true, Ordering::Release);
        drop(input);
        let _ = encoder.wait();
        let _ = std::fs::remove_file(&temporary);
        return Err("None of the selected recording sources could be opened.".into());
    }
    let _ = send.send(RecordingEvent::Started {
        output: output.to_owned(),
        sources: successful,
    });
    let mut deadline = Instant::now();
    let interval = Duration::from_millis(20);
    let mut was_paused = false;
    let mut write_error = false;
    while !stop.load(Ordering::Acquire) {
        let is_paused = paused.load(Ordering::Acquire);
        if was_paused != is_paused {
            let _ = send.send(RecordingEvent::Paused(is_paused));
            was_paused = is_paused;
        }
        let mut mixed = vec![0.0_f32; BLOCK_FRAMES * 2];
        for (queue, gain) in &queues {
            queue.mix(&mut mixed, *gain, is_paused)
        }
        if !is_paused {
            let bytes: Vec<_> = mixed
                .into_iter()
                .flat_map(|sample| sample.clamp(-1.0, 1.0).to_le_bytes())
                .collect();
            if input.write_all(&bytes).is_err() {
                write_error = true;
                break;
            }
        }
        deadline += interval;
        if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
            thread::sleep(wait)
        } else if deadline.elapsed() > Duration::from_millis(500) {
            deadline = Instant::now();
        }
        if capture_workers.iter().all(JoinHandle::is_finished) {
            write_error = true;
            break;
        }
    }
    stop.store(true, Ordering::Release);
    drop(input);
    for worker in capture_workers {
        let _ = worker.join();
    }
    let end = Instant::now() + Duration::from_secs(15);
    let status = loop {
        match encoder.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < end => thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = encoder.kill();
                let _ = encoder.wait();
                break None;
            }
        }
    };
    let errors = errors.join().unwrap_or_default();
    if write_error || !status.is_some_and(|status| status.success()) {
        let _ = std::fs::remove_file(&temporary);
        return Err(format!(
            "Recording could not be completed. {}",
            errors.trim()
        ));
    }
    if output.exists() {
        let _ = std::fs::remove_file(&temporary);
        return Err(
            "The recording destination was created by another operation. It was not overwritten."
                .into(),
        );
    }
    std::fs::rename(&temporary, output)
        .map_err(|_| "Unable to save the completed recording.".to_owned())?;
    Ok(output.to_owned())
}

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
    use super::*;

    #[test]
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
