//! Runs Python `save_edited_local_file_worker`: mpv renders the edited audio,
//! ffmpeg writes the output, and a replaced original is swapped in only after
//! the new file is complete.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// mpv renders the audio chain to a WAV file that ffmpeg then encodes.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalEditRender {
    pub mpv: PathBuf,
    pub mpv_arguments: Vec<OsString>,
    pub pcm: PathBuf,
    pub ffmpeg: PathBuf,
    pub mux_arguments: Vec<OsString>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalEditJob {
    /// The file that is played.
    pub source: PathBuf,
    /// The file the user gets: a new copy, or the source when it is replaced.
    pub output: PathBuf,
    /// Where ffmpeg writes. A hidden sibling of the source when it is replaced.
    pub temporary_output: PathBuf,
    /// Used when both mpv and ffmpeg are available.
    pub render: Option<LocalEditRender>,
    /// ffmpeg alone (Python `local_edit_ffmpeg_args`) when mpv is missing.
    pub fallback: Option<(PathBuf, Vec<OsString>)>,
}

impl LocalEditJob {
    fn replaces_original(&self) -> bool {
        self.temporary_output != self.output
    }
}

/// Saves the edited file and returns the path the user gets. Callers should
/// run this on a worker thread.
///
/// # Errors
/// Returns Python's error text: the tail of the failing program's output, or
/// a description of the missing encoder or failed replacement.
pub fn save_local_edit(job: &LocalEditJob) -> Result<PathBuf, String> {
    let result = run(job);
    if result.is_err() {
        // A failed copy leaves no partial "name - edited" file (approved
        // deviation P-4; Python keeps it). The output name was unused before.
        let _ = fs::remove_file(&job.temporary_output);
    }
    result.map(|()| job.output.clone())
}

fn run(job: &LocalEditJob) -> Result<(), String> {
    if let Some(render) = &job.render {
        let result = render_with_mpv(render, &job.temporary_output);
        let _ = fs::remove_file(&render.pcm);
        result?;
    } else {
        let (ffmpeg, arguments) = job
            .fallback
            .as_ref()
            .ok_or_else(|| "Media encoder was not found".to_owned())?;
        let output = run_program(ffmpeg, arguments)?;
        if !output.success {
            return Err(program_error(&output, "FFmpeg"));
        }
    }
    if job.replaces_original() {
        // Python `os.replace`.
        fs::rename(&job.temporary_output, &job.source).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn render_with_mpv(render: &LocalEditRender, output: &Path) -> Result<(), String> {
    let mpv = run_program(&render.mpv, &render.mpv_arguments)?;
    if !mpv.success || !non_empty_file(&render.pcm) {
        return Err(program_error(&mpv, "mpv"));
    }
    let ffmpeg = run_program(&render.ffmpeg, &render.mux_arguments)?;
    if !ffmpeg.success || !non_empty_file(output) {
        return Err(program_error(&ffmpeg, "FFmpeg"));
    }
    Ok(())
}

struct ProgramOutput {
    success: bool,
    code: Option<i32>,
    text: String,
}

fn run_program(program: &Path, arguments: &[OsString]) -> Result<ProgramOutput, String> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().map_err(|error| error.to_string())?;
    // Python `(stderr or stdout).strip()`.
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let text = if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        stderr
    };
    Ok(ProgramOutput {
        success: output.status.success(),
        code: output.status.code(),
        text,
    })
}

/// Python `error[-600:]`, or "<program> exited with code N".
fn program_error(output: &ProgramOutput, program: &str) -> String {
    if output.text.is_empty() {
        let code = output
            .code
            .map_or_else(|| "None".to_owned(), |code| code.to_string());
        return format!("{program} exited with code {code}");
    }
    let characters = output.text.chars().count();
    output
        .text
        .chars()
        .skip(characters.saturating_sub(600))
        .collect()
}

fn non_empty_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

#[cfg(test)]
mod tests {
    use super::{LocalEditJob, ProgramOutput, program_error, save_local_edit};

    #[test]
    fn missing_encoder_fails_and_removes_the_replacement_copy() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let source = folder.path().join("song.mp3");
        std::fs::write(&source, b"original").expect("source");
        let temporary = folder
            .path()
            .join(".song.apricot-converting-000000000000.mp3");
        std::fs::write(&temporary, b"partial").expect("temporary");
        let job = LocalEditJob {
            source: source.clone(),
            output: source.clone(),
            temporary_output: temporary.clone(),
            render: None,
            fallback: None,
        };
        assert_eq!(
            save_local_edit(&job),
            Err("Media encoder was not found".to_owned())
        );
        assert!(!temporary.exists());
        assert_eq!(std::fs::read(&source).expect("source"), b"original");
    }

    #[test]
    fn errors_use_the_last_600_characters_or_the_exit_code() {
        let long = ProgramOutput {
            success: false,
            code: Some(1),
            text: format!("{}{}", "a".repeat(10), "b".repeat(600)),
        };
        assert_eq!(program_error(&long, "FFmpeg"), "b".repeat(600));
        let silent = ProgramOutput {
            success: false,
            code: Some(2),
            text: String::new(),
        };
        assert_eq!(program_error(&silent, "mpv"), "mpv exited with code 2");
    }

    #[test]
    fn a_missing_program_reports_the_launch_error() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let source = folder.path().join("song.mp3");
        let output = folder.path().join("song - edited.mp3");
        let job = LocalEditJob {
            source,
            output: output.clone(),
            temporary_output: output,
            render: None,
            fallback: Some((folder.path().join("missing-ffmpeg.exe"), Vec::new())),
        };
        assert!(save_local_edit(&job).is_err());
    }

    #[test]
    fn a_failed_copy_removes_the_partial_edited_file() {
        let folder = tempfile::tempdir().expect("temporary folder");
        let source = folder.path().join("song.mp3");
        std::fs::write(&source, b"original").expect("source");
        let output = folder.path().join("song - edited.mp3");
        std::fs::write(&output, b"partial").expect("partial copy");
        let job = LocalEditJob {
            source: source.clone(),
            output: output.clone(),
            temporary_output: output.clone(),
            render: None,
            fallback: Some((folder.path().join("missing-ffmpeg.exe"), Vec::new())),
        };
        assert!(save_local_edit(&job).is_err());
        assert!(!output.exists());
        assert_eq!(std::fs::read(&source).expect("source"), b"original");
    }
}
