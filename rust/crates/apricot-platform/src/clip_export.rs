//! FFmpeg-backed marked-clip export without shell interpolation.

use std::{
    ffi::OsString,
    fs,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
};

use thiserror::Error;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipExportMode {
    Audio,
    Video,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipExportRequest {
    pub ffmpeg: PathBuf,
    pub primary_input: String,
    pub external_audio_input: Option<String>,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub output_path: PathBuf,
    pub mode: ClipExportMode,
    pub audio_format: String,
    pub audio_quality: String,
}

#[derive(Debug, Error)]
pub enum ClipExportError {
    #[error("invalid clip export configuration: {0}")]
    InvalidConfiguration(String),
    #[error("could not create the clip folder: {0}")]
    CreateFolder(String),
    #[error("could not launch FFmpeg: {0}")]
    Launch(String),
    #[error("FFmpeg clip export failed: {0}")]
    Process(String),
    #[error("could not publish the completed clip: {0}")]
    Publish(String),
}

/// Exports one marked clip synchronously. Callers should run this on a worker
/// thread because `FFmpeg` remains a blocking child process.
///
/// # Errors
/// Returns an error for invalid input, filesystem failures, or a failed child process.
pub fn export_marked_clip(request: &ClipExportRequest) -> Result<PathBuf, ClipExportError> {
    validate_request(request)?;
    if let Some(parent) = request.output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| ClipExportError::CreateFolder(error.to_string()))?;
    }
    // Encode beside the destination, then publish without replacing a file that
    // another export or application may have created in the meantime.
    let parent = request
        .output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let suffix = request
        .output_path
        .extension()
        .map(|value| format!(".{}", value.to_string_lossy()))
        .unwrap_or_default();
    let temporary = tempfile::Builder::new()
        .prefix(".apricot-clip-")
        .suffix(&suffix)
        .tempfile_in(parent)
        .map_err(|error| ClipExportError::Publish(error.to_string()))?
        .into_temp_path();
    let mut staged = request.clone();
    staged.output_path = temporary.to_path_buf();
    let mut arguments = build_clip_export_arguments(&staged)?;
    arguments[0] = OsString::from("-y");
    let mut command = Command::new(&request.ffmpeg);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command
        .spawn()
        .map_err(|error| ClipExportError::Launch(error.to_string()))?;
    let mut diagnostic = Vec::new();
    if let Some(mut stderr) = child.stderr.take() {
        let mut buffer = [0_u8; 4096];
        loop {
            match stderr.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    diagnostic.extend_from_slice(&buffer[..count]);
                    if diagnostic.len() > 8192 {
                        diagnostic.drain(..diagnostic.len() - 8192);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ClipExportError::Process(error.to_string()));
                }
            }
        }
    }
    let status = child
        .wait()
        .map_err(|error| ClipExportError::Process(error.to_string()))?;
    if !status.success() {
        let message = String::from_utf8_lossy(&diagnostic);
        return Err(ClipExportError::Process(tail(&message, 600)));
    }
    if fs::metadata(&temporary)
        .map_err(|error| ClipExportError::Publish(error.to_string()))?
        .len()
        == 0
    {
        return Err(ClipExportError::Process(
            "FFmpeg produced an empty clip".to_owned(),
        ));
    }
    temporary
        .persist_noclobber(&request.output_path)
        .map_err(|error| ClipExportError::Publish(error.to_string()))?;
    Ok(request.output_path.clone())
}

fn validate_request(request: &ClipExportRequest) -> Result<(), ClipExportError> {
    if request.output_path.exists() {
        return Err(ClipExportError::InvalidConfiguration(
            "the output file already exists".to_owned(),
        ));
    }
    if !request.ffmpeg.is_file() {
        return Err(ClipExportError::InvalidConfiguration(
            "FFmpeg was not found".to_owned(),
        ));
    }
    if request.primary_input.trim().is_empty() {
        return Err(ClipExportError::InvalidConfiguration(
            "the current media has no playable input".to_owned(),
        ));
    }
    if !request.start_seconds.is_finite()
        || !request.end_seconds.is_finite()
        || request.start_seconds < 0.0
        || request.end_seconds - request.start_seconds < 0.25
    {
        return Err(ClipExportError::InvalidConfiguration(
            "the end marker must be after the start marker".to_owned(),
        ));
    }
    if request.output_path.file_name().is_none() {
        return Err(ClipExportError::InvalidConfiguration(
            "the output path has no file name".to_owned(),
        ));
    }
    Ok(())
}

/// Builds the argument vector for one marked clip.
///
/// # Errors
/// Returns an error when the marker range is invalid.
pub fn build_clip_export_arguments(
    request: &ClipExportRequest,
) -> Result<Vec<OsString>, ClipExportError> {
    if !request.start_seconds.is_finite()
        || !request.end_seconds.is_finite()
        || request.start_seconds < 0.0
        || request.end_seconds - request.start_seconds < 0.25
    {
        return Err(ClipExportError::InvalidConfiguration(
            "the end marker must be after the start marker".to_owned(),
        ));
    }
    let duration = request.end_seconds - request.start_seconds;
    let external_audio = request
        .external_audio_input
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let selected_audio = external_audio.unwrap_or(request.primary_input.trim());
    let mut arguments = vec![
        OsString::from("-n"),
        OsString::from("-nostdin"),
        OsString::from("-hide_banner"),
        OsString::from("-loglevel"),
        OsString::from("error"),
    ];
    match request.mode {
        ClipExportMode::Audio => {
            add_input(&mut arguments, request.start_seconds, selected_audio);
            arguments.extend([
                OsString::from("-t"),
                OsString::from(format!("{duration:.3}")),
            ]);
            arguments.extend(audio_codec_arguments(
                &request.audio_format,
                &request.audio_quality,
            ));
        }
        ClipExportMode::Video => {
            add_input(
                &mut arguments,
                request.start_seconds,
                request.primary_input.trim(),
            );
            if let Some(audio) = external_audio {
                add_input(&mut arguments, request.start_seconds, audio);
                arguments.extend([
                    OsString::from("-map"),
                    OsString::from("0:v:0?"),
                    OsString::from("-map"),
                    OsString::from("1:a:0?"),
                    OsString::from("-shortest"),
                ]);
            } else {
                arguments.extend([OsString::from("-map"), OsString::from("0")]);
            }
            arguments.extend([
                OsString::from("-t"),
                OsString::from(format!("{duration:.3}")),
                OsString::from("-c"),
                OsString::from("copy"),
                OsString::from("-avoid_negative_ts"),
                OsString::from("make_zero"),
            ]);
        }
    }
    arguments.push(request.output_path.as_os_str().to_owned());
    Ok(arguments)
}

fn add_input(arguments: &mut Vec<OsString>, start_seconds: f64, input: &str) {
    arguments.extend([
        OsString::from("-ss"),
        OsString::from(format!("{start_seconds:.3}")),
        OsString::from("-i"),
        OsString::from(input),
    ]);
}

fn audio_codec_arguments(format: &str, quality: &str) -> Vec<OsString> {
    let format = match format.trim().to_ascii_lowercase().as_str() {
        "m4a" | "opus" | "wav" | "flac" => format.trim().to_ascii_lowercase(),
        _ => "mp3".to_owned(),
    };
    let quality = normalized_audio_quality(quality);
    let values: Vec<&str> = match format.as_str() {
        "m4a" => vec![
            "-vn",
            "-c:a",
            "aac",
            "-b:a",
            if quality == "0" { "256k" } else { "" },
        ],
        "opus" => vec![
            "-vn",
            "-c:a",
            "libopus",
            "-b:a",
            if quality == "0" { "160k" } else { "" },
        ],
        "wav" => vec!["-vn", "-c:a", "pcm_s16le"],
        "flac" => vec!["-vn", "-c:a", "flac"],
        _ => vec![
            "-vn",
            "-c:a",
            "libmp3lame",
            "-b:a",
            if quality == "0" { "320k" } else { "" },
        ],
    };
    let mut result = values.into_iter().map(OsString::from).collect::<Vec<_>>();
    if matches!(format.as_str(), "mp3" | "m4a" | "opus")
        && quality != "0"
        && let Some(last) = result.last_mut()
    {
        *last = OsString::from(format!("{quality}k"));
    }
    result
}

fn normalized_audio_quality(value: &str) -> String {
    const OPTIONS: [&str; 18] = [
        "0", "320", "256", "192", "160", "128", "96", "64", "1", "2", "3", "4", "5", "6", "7", "8",
        "9", "10",
    ];
    let normalized = value
        .trim()
        .to_ascii_lowercase()
        .replace("kbps", "")
        .replace('k', "")
        .trim()
        .to_owned();
    if OPTIONS.contains(&normalized.as_str()) {
        normalized
    } else {
        "0".to_owned()
    }
}

fn tail(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    let count = trimmed.chars().count();
    if count <= max_chars {
        return trimmed.to_owned();
    }
    trimmed.chars().skip(count - max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ClipExportMode, ClipExportRequest, build_clip_export_arguments, normalized_audio_quality,
    };
    use std::{ffi::OsString, path::PathBuf};

    fn request(mode: ClipExportMode) -> ClipExportRequest {
        ClipExportRequest {
            ffmpeg: PathBuf::from(r"C:\ffmpeg\ffmpeg.exe"),
            primary_input: "https://media.test/video".to_owned(),
            external_audio_input: None,
            start_seconds: 12.5,
            end_seconds: 20.0,
            output_path: PathBuf::from(r"C:\clips\clip.mp4"),
            mode,
            audio_format: "mp3".to_owned(),
            audio_quality: "320kbps".to_owned(),
        }
    }

    fn arguments(request: &ClipExportRequest) -> Vec<String> {
        build_clip_export_arguments(request)
            .expect("arguments")
            .into_iter()
            .map(|value: OsString| value.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn video_export_maps_a_separate_youtube_audio_stream() {
        let mut request = request(ClipExportMode::Video);
        request.external_audio_input = Some("https://media.test/audio".to_owned());
        let args = arguments(&request);
        assert!(args.windows(2).any(|pair| pair == ["-map", "0:v:0?"]));
        assert!(args.windows(2).any(|pair| pair == ["-map", "1:a:0?"]));
        assert_eq!(args.iter().filter(|value| *value == "-ss").count(), 2);
        assert!(args.windows(2).any(|pair| pair == ["-t", "7.500"]));
    }

    #[test]
    fn audio_export_prefers_external_audio_and_normalizes_quality() {
        let mut request = request(ClipExportMode::Audio);
        request.external_audio_input = Some("https://media.test/audio".to_owned());
        let args = arguments(&request);
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-i", "https://media.test/audio"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-b:a", "320k"]));
        assert_eq!(normalized_audio_quality("nonsense"), "0");
    }

    #[test]
    fn reversed_or_too_short_ranges_are_rejected() {
        let mut request = request(ClipExportMode::Video);
        request.end_seconds = 12.6;
        assert!(build_clip_export_arguments(&request).is_err());
        request.start_seconds = -1.0;
        request.end_seconds = 5.0;
        assert!(build_clip_export_arguments(&request).is_err());
    }

    #[test]
    #[ignore = "requires APRICOT_TEST_FFMPEG pointing to the packaged executable"]
    fn real_ffmpeg_exports_audio_and_preserves_existing_output() {
        let ffmpeg = PathBuf::from(std::env::var_os("APRICOT_TEST_FFMPEG").expect("FFmpeg path"));
        let folder = tempfile::tempdir().expect("temporary folder");
        let source = folder.path().join("source.wav");
        let mut command = std::process::Command::new(&ffmpeg);
        command.args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
        ]);
        command.arg(&source);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(super::CREATE_NO_WINDOW);
        }
        assert!(command.status().expect("fixture generation").success());
        let mut request = request(ClipExportMode::Audio);
        request.ffmpeg = ffmpeg;
        request.primary_input = source.to_string_lossy().into_owned();
        request.start_seconds = 0.5;
        request.end_seconds = 1.5;
        request.output_path = folder.path().join("clip.wav");
        request.audio_format = "wav".to_owned();
        let output = super::export_marked_clip(&request).expect("actual export");
        let bytes = std::fs::read(&output).expect("exported audio");
        assert!(bytes.starts_with(b"RIFF"));
        assert!((80_000..100_000).contains(&bytes.len()));
        assert!(super::export_marked_clip(&request).is_err());
        assert_eq!(std::fs::read(output).expect("unchanged export"), bytes);
        request.output_path = folder.path().join("failed.wav");
        request.primary_input = folder
            .path()
            .join("missing.wav")
            .to_string_lossy()
            .into_owned();
        assert!(super::export_marked_clip(&request).is_err());
        assert!(!request.output_path.exists());
        assert_eq!(
            std::fs::read_dir(folder.path())
                .expect("folder entries")
                .count(),
            2
        );
    }
}
