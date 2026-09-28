//! ffmpeg decoding for the player's BPM announcement, Python
//! `analyze_bpm_worker` and `bpm_ffmpeg_args`.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use apricot_media::tempo::{TEMPO_SAMPLE_RATE, estimate_tempo_from_pcm16_stereo};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Python gives ffmpeg 35 seconds before the analysis is abandoned.
const ANALYSIS_DEADLINE: Duration = Duration::from_secs(35);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug, PartialEq)]
pub struct BpmAnalysisRequest {
    pub ffmpeg: Option<PathBuf>,
    /// What the player plays: a local path or the resolved stream URL.
    pub source: String,
    pub start_seconds: f64,
    pub duration_seconds: f64,
}

/// Python `ffmpeg_executable`: the configured file or folder, the bundled
/// copy, then `PATH`.
pub fn ffmpeg_executable(
    configured: &str,
    application_directory: Option<&Path>,
) -> Option<PathBuf> {
    let file_name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    let configured = configured.trim();
    if !configured.is_empty() {
        let path = PathBuf::from(configured);
        if path.is_dir() {
            let candidate = path.join(file_name);
            if candidate.exists() {
                return Some(candidate);
            }
        } else if path.exists() {
            return Some(path);
        }
    }
    if let Some(bundled) = application_directory
        .map(|directory| directory.join("ffmpeg").join("ffmpeg.exe"))
        .filter(|candidate| candidate.exists())
    {
        return Some(bundled);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(file_name))
            .find(|candidate| candidate.is_file())
    })
}

/// Python `bpm_ffmpeg_args` without the executable. The Rust player keeps no
/// extra HTTP headers for its streams, so none are passed on.
pub fn bpm_ffmpeg_arguments(
    source: &str,
    start_seconds: f64,
    duration_seconds: f64,
) -> Vec<String> {
    let mut arguments: Vec<String> = ["-nostdin", "-hide_banner", "-loglevel", "error"]
        .map(str::to_owned)
        .to_vec();
    if start_seconds > 0.0 {
        arguments.extend(["-ss".to_owned(), format!("{start_seconds:.3}")]);
    }
    arguments.extend([
        "-i".to_owned(),
        source.to_owned(),
        "-t".to_owned(),
        format!("{duration_seconds:.3}"),
        "-filter_complex".to_owned(),
        "[0:a:0]aformat=sample_fmts=fltp:sample_rates=11025:channel_layouts=mono,\
         asplit=2[full][low];[low]highpass=f=35,lowpass=f=250[lowf];\
         [full][lowf]join=inputs=2:channel_layout=stereo[out]"
            .to_owned(),
        "-map".to_owned(),
        "[out]".to_owned(),
        "-ar".to_owned(),
        "11025".to_owned(),
        "-ac".to_owned(),
        "2".to_owned(),
        "-f".to_owned(),
        "s16le".to_owned(),
        "pipe:1".to_owned(),
    ]);
    arguments
}

/// Decodes the analysis window and estimates the source tempo. Returns `None`
/// for every failure, as Python announces `bpm_not_available` for all of them.
/// `cancelled` is checked while ffmpeg runs, like Python's generation and
/// speed or pitch checks.
pub fn analyze_source_bpm(request: &BpmAnalysisRequest, cancelled: &AtomicBool) -> Option<f64> {
    let ffmpeg = request.ffmpeg.as_ref()?;
    if request.source.trim().is_empty() {
        return None;
    }
    let mut command = Command::new(ffmpeg);
    command
        .args(bpm_ffmpeg_arguments(
            &request.source,
            request.start_seconds,
            request.duration_seconds,
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let mut stderr = child.stderr.take()?;
    let reader = std::thread::spawn(move || {
        let mut pcm = Vec::new();
        let _ = stdout.read_to_end(&mut pcm);
        pcm
    });
    let diagnostics = std::thread::spawn(move || {
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
    });
    let deadline = Instant::now() + ANALYSIS_DEADLINE;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let pcm = reader.join().unwrap_or_default();
    let _ = diagnostics.join();
    if !status?.success() || pcm.len() < TEMPO_SAMPLE_RATE * 2 * 2 * 6 {
        return None;
    }
    estimate_tempo_from_pcm16_stereo(&pcm, TEMPO_SAMPLE_RATE).map(|estimate| estimate.bpm)
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::atomic::AtomicBool};

    use super::{BpmAnalysisRequest, analyze_source_bpm, bpm_ffmpeg_arguments, ffmpeg_executable};

    #[test]
    fn arguments_match_python() {
        let arguments = bpm_ffmpeg_arguments("song.mp3", 12.5, 72.0);
        assert_eq!(
            arguments[..8],
            [
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-ss",
                "12.500",
                "-i",
                "song.mp3"
            ]
        );
        assert_eq!(arguments[8..10], ["-t", "72.000"]);
        assert_eq!(
            arguments[11],
            "[0:a:0]aformat=sample_fmts=fltp:sample_rates=11025:channel_layouts=mono,asplit=2[full][low];[low]highpass=f=35,lowpass=f=250[lowf];[full][lowf]join=inputs=2:channel_layout=stereo[out]"
        );
        assert_eq!(
            arguments[12..],
            [
                "-map", "[out]", "-ar", "11025", "-ac", "2", "-f", "s16le", "pipe:1"
            ]
        );
        assert!(!bpm_ffmpeg_arguments("song.mp3", 0.0, 30.0).contains(&"-ss".to_owned()));
    }

    #[test]
    fn configured_ffmpeg_folder_and_file_are_found() {
        let folder = tempfile::tempdir().expect("folder");
        let file_name = if cfg!(windows) {
            "ffmpeg.exe"
        } else {
            "ffmpeg"
        };
        let executable = folder.path().join(file_name);
        std::fs::write(&executable, b"").expect("fake ffmpeg");
        assert_eq!(
            ffmpeg_executable(&folder.path().to_string_lossy(), None),
            Some(executable.clone())
        );
        assert_eq!(
            ffmpeg_executable(&executable.to_string_lossy(), None),
            Some(executable)
        );
    }

    #[test]
    fn missing_ffmpeg_or_source_has_no_tempo() {
        let cancelled = AtomicBool::new(false);
        let request = BpmAnalysisRequest {
            ffmpeg: None,
            source: "song.mp3".to_owned(),
            start_seconds: 0.0,
            duration_seconds: 72.0,
        };
        assert_eq!(analyze_source_bpm(&request, &cancelled), None);
        let request = BpmAnalysisRequest {
            ffmpeg: Some(PathBuf::from("ffmpeg.exe")),
            source: " ".to_owned(),
            ..request
        };
        assert_eq!(analyze_source_bpm(&request, &cancelled), None);
    }

    /// Set `APRICOT_TEST_FFMPEG` to run this against the real decoder.
    #[test]
    #[ignore = "needs APRICOT_TEST_FFMPEG"]
    fn real_ffmpeg_finds_the_tempo_of_a_click_track() {
        let ffmpeg = PathBuf::from(std::env::var_os("APRICOT_TEST_FFMPEG").expect("FFmpeg path"));
        let folder = tempfile::tempdir().expect("folder");
        let source = folder.path().join("clicks.wav");
        let status = std::process::Command::new(&ffmpeg)
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=1000:beep_factor=4:duration=40:sample_rate=44100",
                "-af",
                "volume=0.8",
            ])
            .arg(&source)
            .status()
            .expect("ffmpeg");
        assert!(status.success());
        let request = BpmAnalysisRequest {
            ffmpeg: Some(ffmpeg),
            source: source.to_string_lossy().into_owned(),
            start_seconds: 0.0,
            duration_seconds: 40.0,
        };
        let bpm = analyze_source_bpm(&request, &AtomicBool::new(false)).expect("tempo");
        // `beep_factor` beeps once a second.
        assert!((bpm - 60.0).abs() < 1.0, "got {bpm}");
    }
}
