//! Python file and folder converter (`apricot/media/media.py`
//! `show_converter_dialog` and the conversion workers in `ui/system.py` and
//! `ui/events.py`): format lists, output paths, ffmpeg arguments and the
//! worker steps. Running ffmpeg is injected, so the steps are testable.

use std::{
    cmp::Ordering,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use apricot_core::TranslationCatalog;

use crate::local_edit::temporary_conversion_path;

/// Python `AUDIO_CONVERT_FORMATS`.
pub const AUDIO_CONVERT_FORMATS: [&str; 12] = [
    "mp3", "m4a", "aac", "wav", "flac", "ogg", "opus", "wma", "aiff", "alac", "ac3", "mp2",
];
/// Python `VIDEO_CONVERT_FORMATS`.
pub const VIDEO_CONVERT_FORMATS: [&str; 15] = [
    "mp4", "mkv", "webm", "mov", "avi", "wmv", "m4v", "mpg", "mpeg", "flv", "3gp", "ogv", "ts",
    "m2ts", "asf",
];
/// Python `AUDIO_INPUT_EXTENSIONS` extras beyond the audio target formats.
const EXTRA_AUDIO_INPUTS: [&str; 4] = ["aif", "aifc", "ape", "mka"];
/// Python `VIDEO_INPUT_EXTENSIONS` extras beyond the video target formats.
const EXTRA_VIDEO_INPUTS: [&str; 6] = ["asf", "divx", "m2ts", "mts", "ts", "vob"];
/// Python `CONVERTER_IMAGE_EXTENSIONS`.
const IMAGE_EXTENSIONS: [&str; 8] = ["jpg", "jpeg", "png", "bmp", "webp", "gif", "tif", "tiff"];
/// Python skips its own hidden work files when it scans a folder.
const WORK_FILE_MARKER: &str = ".apricot-converting";

/// Python `converter_input_kind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputKind {
    Audio,
    Video,
}

/// Python `converter_input_kind`: `None` is Python's empty string.
#[must_use]
pub fn input_kind(path: &Path) -> Option<InputKind> {
    let extension = lower_extension(path)?;
    let extension = extension.as_str();
    if AUDIO_CONVERT_FORMATS.contains(&extension) || EXTRA_AUDIO_INPUTS.contains(&extension) {
        Some(InputKind::Audio)
    } else if VIDEO_CONVERT_FORMATS.contains(&extension) || EXTRA_VIDEO_INPUTS.contains(&extension)
    {
        Some(InputKind::Video)
    } else {
        None
    }
}

/// Python `converter_format_values`: formats of the input's own kind first.
#[must_use]
pub fn format_values(kind: Option<InputKind>) -> Vec<&'static str> {
    if kind == Some(InputKind::Video) {
        VIDEO_CONVERT_FORMATS
            .iter()
            .chain(AUDIO_CONVERT_FORMATS.iter())
            .copied()
            .collect()
    } else {
        AUDIO_CONVERT_FORMATS
            .iter()
            .chain(VIDEO_CONVERT_FORMATS.iter())
            .copied()
            .collect()
    }
}

/// Python `converter_format_labels`.
#[must_use]
pub fn format_label(value: &str) -> String {
    if value == "alac" {
        "ALAC (M4A)".to_owned()
    } else {
        value.to_uppercase()
    }
}

/// Python `converter_output_extension`.
#[must_use]
pub fn output_extension(target: &str) -> &str {
    if target == "alac" { "m4a" } else { target }
}

#[must_use]
pub fn is_video_format(target: &str) -> bool {
    VIDEO_CONVERT_FORMATS.contains(&target)
}

/// Python `converter_default_output_path`.
#[must_use]
pub fn default_output_path(source: &Path, target: &str) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    source.with_file_name(format!("{stem}.{}", output_extension(target)))
}

/// Python `source.with_suffix(f".{extension}")` for a replaced original.
#[must_use]
pub fn replaced_output_path(source: &Path, target: &str) -> PathBuf {
    source.with_extension(output_extension(target))
}

/// Python adds the target extension when the chosen save name has none.
#[must_use]
pub fn with_default_extension(output: PathBuf, target: &str) -> PathBuf {
    if output.extension().is_some() {
        output
    } else {
        output.with_extension(output_extension(target))
    }
}

/// Python `converter_is_audio_to_video`.
#[must_use]
pub fn is_audio_to_video(source: &Path, target: &str) -> bool {
    input_kind(source) == Some(InputKind::Audio) && is_video_format(target)
}

/// Python `Path(text.strip().strip('"')).expanduser()`: the dialog's path
/// text as a path. An empty text is Python's `Path("")`, the current folder.
#[must_use]
pub fn path_from_text(text: &str) -> PathBuf {
    let text = text.trim().trim_matches('"');
    if (text == "~" || text.starts_with("~\\") || text.starts_with("~/"))
        && let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
    {
        let rest = text[1..].trim_start_matches(['\\', '/']);
        let home = PathBuf::from(home);
        return if rest.is_empty() {
            home
        } else {
            home.join(rest)
        };
    }
    if text.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(text)
    }
}

/// Python `should_show_audio_video_options`.
#[must_use]
pub fn shows_audio_video_options(folder_mode: bool, path_text: &str, target: &str) -> bool {
    if !is_video_format(target) {
        return false;
    }
    let path = path_from_text(path_text);
    if folder_mode {
        !path.exists() || folder_has_audio_inputs(&path)
    } else {
        is_audio_to_video(&path, target)
    }
}

/// Python `update_formats`: the formats for the dialog's path text.
#[must_use]
pub fn dialog_format_values(folder_mode: bool, path_text: &str) -> Vec<&'static str> {
    let text = path_text.trim().trim_matches('"');
    if folder_mode || text.is_empty() {
        format_values(None)
    } else {
        format_values(input_kind(Path::new(text)))
    }
}

/// Python `folder_has_audio_inputs`.
#[must_use]
pub fn folder_has_audio_inputs(folder: &Path) -> bool {
    let mut files = Vec::new();
    collect_files(folder, &mut files);
    files
        .iter()
        .any(|path| input_kind(path) == Some(InputKind::Audio))
}

/// Python `converter_media_files_in_folder`: every media file below the
/// folder in Python's case-insensitive path order.
#[must_use]
pub fn media_files_in_folder(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files(folder, &mut files);
    files.retain(|path| {
        input_kind(path).is_some()
            && !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains(WORK_FILE_MARKER))
    });
    files.sort_by(|left, right| compare_paths(left, right));
    files
}

fn collect_files(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect_files(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

/// Windows `PurePath` ordering: compares lower-cased components.
fn compare_paths(left: &Path, right: &Path) -> Ordering {
    let parts = |path: &Path| {
        path.components()
            .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
            .collect::<Vec<_>>()
    };
    parts(left).cmp(&parts(right))
}

/// Python `candidate.resolve() == source.resolve()` on Windows paths.
fn same_path(left: &Path, right: &Path) -> bool {
    let resolve = |path: &Path| {
        fs::canonicalize(path)
            .or_else(|_| std::path::absolute(path))
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_lowercase()
    };
    resolve(left) == resolve(right)
}

/// Python `unique_converter_output_path`: "name (2).ext" and onward when the
/// path exists or is the source itself.
#[must_use]
pub fn unique_output_path(path: &Path, source: Option<&Path>) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let suffix = path
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let mut candidate = path.to_path_buf();
    let mut counter = 2;
    while candidate.exists() || source.is_some_and(|source| same_path(&candidate, source)) {
        candidate = path.with_file_name(format!("{stem} ({counter}){suffix}"));
        counter += 1;
    }
    candidate
}

/// Python `unique_folder_path`.
#[must_use]
pub fn unique_folder_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut candidate = path.to_path_buf();
    let mut counter = 2;
    while candidate.exists() {
        candidate = path.with_file_name(format!("{name} ({counter})"));
        counter += 1;
    }
    candidate
}

/// Python `chosen / f"{source.name} converted"` before `unique_folder_path`.
#[must_use]
pub fn converted_folder_path(chosen: &Path, source: &Path) -> PathBuf {
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    unique_folder_path(&chosen.join(format!("{name} converted")))
}

/// Python `converter_audio_codec_args`.
#[must_use]
pub fn audio_codec_arguments(target: &str) -> Vec<&'static str> {
    match target.to_lowercase().as_str() {
        "mp3" => vec!["-vn", "-c:a", "libmp3lame", "-b:a", "320k"],
        "alac" => vec!["-vn", "-c:a", "alac"],
        "opus" => vec!["-vn", "-c:a", "libopus", "-b:a", "160k"],
        "ogg" => vec!["-vn", "-c:a", "libvorbis", "-q:a", "5"],
        "wma" => vec!["-vn", "-c:a", "wmav2", "-b:a", "192k"],
        "ac3" => vec!["-vn", "-c:a", "ac3", "-b:a", "192k"],
        "mp2" => vec!["-vn", "-c:a", "mp2", "-b:a", "192k"],
        "aiff" => vec!["-vn", "-c:a", "pcm_s16be"],
        "wav" => vec!["-vn", "-c:a", "pcm_s16le"],
        "flac" => vec!["-vn", "-c:a", "flac"],
        _ => vec!["-vn", "-c:a", "aac", "-b:a", "256k"],
    }
}

/// Python `converter_video_codec_args`.
#[must_use]
pub fn video_codec_arguments(target: &str) -> Vec<&'static str> {
    match target.to_lowercase().as_str() {
        "webm" => vec![
            "-c:v",
            "libvpx-vp9",
            "-b:v",
            "0",
            "-crf",
            "32",
            "-c:a",
            "libopus",
            "-b:a",
            "160k",
        ],
        "avi" => vec![
            "-c:v",
            "mpeg4",
            "-q:v",
            "4",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "192k",
        ],
        "wmv" | "asf" => vec![
            "-c:v", "wmv2", "-b:v", "2500k", "-c:a", "wmav2", "-b:a", "192k",
        ],
        "mpg" | "mpeg" => vec![
            "-c:v",
            "mpeg2video",
            "-q:v",
            "4",
            "-c:a",
            "mp2",
            "-b:a",
            "192k",
        ],
        "ts" | "m2ts" => vec![
            "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p", "-c:a",
            "aac", "-b:a", "192k", "-f", "mpegts",
        ],
        "flv" => vec![
            "-c:v",
            "flv",
            "-q:v",
            "4",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "192k",
        ],
        "ogv" => vec![
            "-c:v",
            "libtheora",
            "-q:v",
            "7",
            "-c:a",
            "libvorbis",
            "-q:a",
            "5",
        ],
        other => {
            let mut arguments = vec![
                "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-pix_fmt", "yuv420p",
                "-c:a", "aac", "-b:a", "192k",
            ];
            if matches!(other, "mp4" | "m4v" | "mov") {
                arguments.extend(["-movflags", "+faststart"]);
            }
            arguments
        }
    }
}

/// Python `converter_ffmpeg_args` without the program itself. `None` is
/// Python's "This input format is not supported." error.
#[must_use]
pub fn ffmpeg_arguments(
    source: &Path,
    output: &Path,
    target: &str,
    image: Option<&Path>,
) -> Option<Vec<OsString>> {
    let source_kind = input_kind(source)?;
    let target = target.to_lowercase();
    let mut arguments = strings(&["-y", "-hide_banner", "-loglevel", "error"]);
    if AUDIO_CONVERT_FORMATS.contains(&target.as_str()) {
        arguments.push("-i".into());
        arguments.push(source.into());
        arguments.extend(strings(&audio_codec_arguments(&target)));
        arguments.push(output.into());
        return Some(arguments);
    }
    if !is_video_format(&target) {
        return None;
    }
    if source_kind == InputKind::Audio {
        if let Some(image) = image {
            arguments.extend(strings(&["-loop", "1", "-framerate", "1", "-i"]));
            arguments.push(image.into());
            arguments.push("-i".into());
            arguments.push(source.into());
            arguments.extend(strings(&[
                "-shortest",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-vf",
                "scale=1280:720:force_original_aspect_ratio=decrease,pad=1280:720:(ow-iw)/2:(oh-ih)/2,fps=30",
            ]));
        } else {
            arguments.extend(strings(&[
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=1280x720:r=30",
                "-i",
            ]));
            arguments.push(source.into());
            arguments.extend(strings(&["-shortest", "-map", "0:v:0", "-map", "1:a:0"]));
        }
        arguments.extend(strings(&video_codec_arguments(&target)));
        arguments.push(output.into());
        return Some(arguments);
    }
    arguments.push("-i".into());
    arguments.push(source.into());
    arguments.extend(strings(&video_codec_arguments(&target)));
    arguments.push(output.into());
    Some(arguments)
}

/// One file-dialog filter: its label and its `;`-separated patterns.
pub type FileFilter = (String, String);

/// Python `converter_input_wildcard`.
#[must_use]
pub fn input_filters(catalog: &TranslationCatalog) -> Vec<FileFilter> {
    let audio = sorted_extensions(
        AUDIO_CONVERT_FORMATS
            .iter()
            .chain(EXTRA_AUDIO_INPUTS.iter()),
    );
    let video = sorted_extensions(
        VIDEO_CONVERT_FORMATS
            .iter()
            .chain(EXTRA_VIDEO_INPUTS.iter()),
    );
    let all = sorted_extensions(audio.iter().chain(video.iter()));
    vec![
        (catalog.text("media_files").to_owned(), patterns(&all)),
        (catalog.text("audio_files").to_owned(), patterns(&audio)),
        (catalog.text("video_files").to_owned(), patterns(&video)),
        all_files_filter(catalog),
    ]
}

/// Python `converter_image_wildcard`.
#[must_use]
pub fn image_filters(catalog: &TranslationCatalog) -> Vec<FileFilter> {
    vec![
        (
            catalog.text("image_files").to_owned(),
            patterns(&sorted_extensions(IMAGE_EXTENSIONS.iter())),
        ),
        all_files_filter(catalog),
    ]
}

/// Python `converter_wildcard_for_target`.
#[must_use]
pub fn target_filters(catalog: &TranslationCatalog, target: &str) -> Vec<FileFilter> {
    let extension = output_extension(target);
    vec![
        (
            format!("{} (*.{extension})", extension.to_uppercase()),
            format!("*.{extension}"),
        ),
        all_files_filter(catalog),
    ]
}

fn all_files_filter(catalog: &TranslationCatalog) -> FileFilter {
    (
        format!("{} (*.*)", catalog.text("all_files")),
        "*.*".to_owned(),
    )
}

/// Python `sorted(f"*{extension}" ...)` over a set of ".ext" strings.
fn sorted_extensions<'a>(extensions: impl Iterator<Item = &'a &'a str>) -> Vec<&'a str> {
    let mut values = extensions.copied().collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    values
}

fn patterns(extensions: &[&str]) -> String {
    extensions
        .iter()
        .map(|extension| format!("*.{extension}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// Runs one ffmpeg command: Python `run_ffmpeg_conversion`.
pub type ConversionRunner<'a> = dyn FnMut(&Path, &[OsString]) -> Result<(), String> + 'a;

/// What Python `start_file_conversion` hands to `file_conversion_worker`.
#[derive(Clone, Debug, PartialEq)]
pub struct FileConversionJob {
    pub ffmpeg: Option<PathBuf>,
    pub source: PathBuf,
    pub output: PathBuf,
    pub target: String,
    pub image: Option<PathBuf>,
    pub replace_original: bool,
    /// Localized "This input format is not supported."
    pub unsupported_message: String,
}

impl FileConversionJob {
    /// Python `start_file_conversion`: a new file never overwrites an
    /// existing one or the source.
    #[must_use]
    pub fn new(
        ffmpeg: Option<PathBuf>,
        source: PathBuf,
        output: PathBuf,
        target: &str,
        image: Option<PathBuf>,
        replace_original: bool,
        unsupported_message: String,
    ) -> Self {
        let output = if replace_original {
            output
        } else {
            unique_output_path(&output, Some(&source))
        };
        Self {
            ffmpeg,
            source,
            output,
            target: target.to_owned(),
            image,
            replace_original,
            unsupported_message,
        }
    }
}

/// Python `file_conversion_worker`. Returns the converted file.
///
/// # Errors
/// Returns the error text Python shows in "Conversion failed: ...".
pub fn run_file_conversion(
    job: &FileConversionJob,
    run: &mut ConversionRunner<'_>,
) -> Result<PathBuf, String> {
    let ffmpeg = job
        .ffmpeg
        .as_ref()
        .ok_or_else(|| "FFmpeg was not found".to_owned())?;
    if let Some(parent) = job
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let work_output = if job.replace_original {
        temporary_conversion_path(&job.output)
    } else {
        job.output.clone()
    };
    let arguments = ffmpeg_arguments(&job.source, &work_output, &job.target, job.image.as_deref())
        .ok_or_else(|| job.unsupported_message.clone())?;
    run(ffmpeg, &arguments)?;
    if job.replace_original {
        replace_converted_original(&job.source, &work_output, &job.output)?;
    }
    Ok(job.output.clone())
}

/// Python `replace_converted_original`.
///
/// # Errors
/// Returns the file-system error text.
pub fn replace_converted_original(
    source: &Path,
    work_output: &Path,
    final_output: &Path,
) -> Result<(), String> {
    if !work_output.exists() {
        return Err("Converted file was not created".to_owned());
    }
    if !same_path(work_output, final_output) {
        fs::rename(work_output, final_output).map_err(|error| error.to_string())?;
    }
    if !same_path(source, final_output) && source.exists() {
        fs::remove_file(source).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// What Python `start_folder_conversion` hands to `folder_conversion_worker`.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderConversionJob {
    pub ffmpeg: Option<PathBuf>,
    pub source_folder: PathBuf,
    pub output_folder: PathBuf,
    pub target: String,
    pub image: Option<PathBuf>,
    pub replace_originals: bool,
}

/// Python `ui_queue` and `wx.CallAfter` messages from the folder worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FolderConversionEvent {
    /// Python `show_conversion_progress_dialog(len(files))`.
    Started { total: usize },
    /// Python status "Conversion started. 2/5: name".
    FileStarted {
        index: usize,
        total: usize,
        name: String,
    },
    /// Python `conversion_progress`.
    Progress {
        file: String,
        converted: usize,
        total: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FolderConversionOutcome {
    /// Python `conversion_no_media_files`.
    NoMediaFiles,
    Done {
        converted: usize,
        failed: usize,
    },
}

/// Python `folder_conversion_worker`.
///
/// # Errors
/// Returns the error text Python shows in "Conversion failed: ...".
pub fn run_folder_conversion(
    job: &FolderConversionJob,
    run: &mut ConversionRunner<'_>,
    events: &mut dyn FnMut(FolderConversionEvent),
) -> Result<FolderConversionOutcome, String> {
    let ffmpeg = job
        .ffmpeg
        .as_ref()
        .ok_or_else(|| "FFmpeg was not found".to_owned())?;
    let files = media_files_in_folder(&job.source_folder);
    if files.is_empty() {
        return Ok(FolderConversionOutcome::NoMediaFiles);
    }
    if !job.replace_originals {
        fs::create_dir_all(&job.output_folder).map_err(|error| error.to_string())?;
    }
    let total = files.len();
    events(FolderConversionEvent::Started { total });
    let extension = output_extension(&job.target);
    let mut converted = 0;
    let mut failed = 0;
    for (index, source) in files.iter().enumerate() {
        let (target, work_target) = if job.replace_originals {
            let target = source.with_extension(extension);
            let work = temporary_conversion_path(&target);
            (target, work)
        } else {
            let relative = source.strip_prefix(&job.source_folder).map_or_else(
                |_| PathBuf::from(source.file_name().unwrap_or_default()),
                Path::to_path_buf,
            );
            let target = unique_output_path(
                &job.output_folder.join(relative.with_extension(extension)),
                Some(source),
            );
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            (target.clone(), target)
        };
        let name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        events(FolderConversionEvent::FileStarted {
            index: index + 1,
            total,
            name: name.clone(),
        });
        events(FolderConversionEvent::Progress {
            file: name.clone(),
            converted,
            total,
        });
        let result = ffmpeg_arguments(source, &work_target, &job.target, job.image.as_deref())
            .ok_or_else(String::new)
            .and_then(|arguments| run(ffmpeg, &arguments))
            .and_then(|()| {
                if job.replace_originals {
                    replace_converted_original(source, &work_target, &target)
                } else {
                    Ok(())
                }
            });
        if result.is_err() {
            if job.replace_originals {
                let _ = fs::remove_file(&work_target);
            }
            failed += 1;
            continue;
        }
        converted += 1;
        events(FolderConversionEvent::Progress {
            file: name,
            converted,
            total,
        });
    }
    Ok(FolderConversionOutcome::Done { converted, failed })
}

/// Python `conversion_progress_message`.
#[must_use]
pub fn progress_message(
    catalog: &TranslationCatalog,
    file: &str,
    converted: usize,
    total: usize,
) -> String {
    let total = total.max(1);
    let converted = converted.min(total);
    catalog
        .text("conversion_progress_message")
        .replace("{file}", file)
        .replace("{converted}", &converted.to_string())
        .replace("{total}", &total.to_string())
        .replace("{remaining}", &(total - converted).to_string())
}

/// Python `conversion_folder_done` or `conversion_folder_done_with_errors`.
#[must_use]
pub fn folder_done_message(
    catalog: &TranslationCatalog,
    converted: usize,
    failed: usize,
) -> String {
    if failed > 0 {
        catalog
            .text("conversion_folder_done_with_errors")
            .replace("{count}", &converted.to_string())
            .replace("{failed}", &failed.to_string())
    } else {
        catalog
            .text("conversion_folder_done")
            .replace("{count}", &converted.to_string())
    }
}

fn strings(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn lower_extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
    };

    use super::*;
    use crate::embedded_catalog;

    fn text(arguments: &[OsString]) -> Vec<String> {
        arguments
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn input_kinds_and_format_order_match_python() {
        assert_eq!(input_kind(Path::new("a.MKA")), Some(InputKind::Audio));
        assert_eq!(input_kind(Path::new("a.vob")), Some(InputKind::Video));
        assert_eq!(input_kind(Path::new("a.asf")), Some(InputKind::Video));
        assert_eq!(input_kind(Path::new("a.txt")), None);
        assert_eq!(input_kind(Path::new("mp3")), None);
        let video = format_values(Some(InputKind::Video));
        assert_eq!(video.first(), Some(&"mp4"));
        assert_eq!(video.get(15), Some(&"mp3"));
        assert_eq!(format_values(None).first(), Some(&"mp3"));
        assert_eq!(format_values(Some(InputKind::Audio)).len(), 27);
        assert_eq!(format_label("alac"), "ALAC (M4A)");
        assert_eq!(format_label("m2ts"), "M2TS");
        assert_eq!(dialog_format_values(false, " \"C:\\a.mkv\" ")[0], "mp4");
        assert_eq!(dialog_format_values(true, "C:\\a.mkv")[0], "mp3");
    }

    #[test]
    fn output_paths_match_python() {
        let source = Path::new(r"C:\Music\a.b.flac");
        assert_eq!(
            default_output_path(source, "alac"),
            PathBuf::from(r"C:\Music\a.b.m4a")
        );
        assert_eq!(
            replaced_output_path(source, "mp3"),
            PathBuf::from(r"C:\Music\a.b.mp3")
        );
        assert_eq!(
            with_default_extension(PathBuf::from(r"C:\x\song"), "opus"),
            PathBuf::from(r"C:\x\song.opus")
        );
        assert_eq!(
            with_default_extension(PathBuf::from(r"C:\x\song.ogg"), "opus"),
            PathBuf::from(r"C:\x\song.ogg")
        );
    }

    #[test]
    fn unique_paths_skip_existing_files_and_the_source() {
        let folder = tempfile::tempdir().expect("folder");
        let source = folder.path().join("song.mp3");
        fs::write(&source, b"x").expect("source");
        assert_eq!(
            unique_output_path(&source, Some(&source)),
            folder.path().join("song (2).mp3")
        );
        fs::write(folder.path().join("song (2).mp3"), b"x").expect("second");
        assert_eq!(
            unique_output_path(&source, None),
            folder.path().join("song (3).mp3")
        );
        let other = folder.path().join("SONG.wav");
        assert_eq!(unique_output_path(&other, Some(&source)), other);
        let converted = folder.path().join("Music converted");
        fs::create_dir(&converted).expect("folder");
        assert_eq!(
            converted_folder_path(folder.path(), Path::new(r"C:\Music")),
            folder.path().join("Music converted (2)")
        );
    }

    #[test]
    fn ffmpeg_arguments_match_python() {
        let audio = text(
            &ffmpeg_arguments(Path::new("in.flac"), Path::new("out.mp3"), "mp3", None)
                .expect("audio"),
        );
        assert_eq!(
            audio,
            [
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                "in.flac",
                "-vn",
                "-c:a",
                "libmp3lame",
                "-b:a",
                "320k",
                "out.mp3"
            ]
        );
        let black = text(
            &ffmpeg_arguments(Path::new("in.mp3"), Path::new("out.mp4"), "mp4", None)
                .expect("black"),
        );
        assert_eq!(
            black[4..11],
            [
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=1280x720:r=30",
                "-i",
                "in.mp3",
                "-shortest"
            ]
        );
        assert_eq!(
            black[black.len() - 3..],
            ["-movflags", "+faststart", "out.mp4"]
        );
        let image = text(
            &ffmpeg_arguments(
                Path::new("in.mp3"),
                Path::new("out.webm"),
                "webm",
                Some(Path::new("cover.png")),
            )
            .expect("image"),
        );
        assert_eq!(
            image[4..12],
            [
                "-loop",
                "1",
                "-framerate",
                "1",
                "-i",
                "cover.png",
                "-i",
                "in.mp3"
            ]
        );
        assert!(
            image
                .iter()
                .any(|value| value.starts_with("scale=1280:720"))
        );
        assert!(image.contains(&"libvpx-vp9".to_owned()));
        let video = text(
            &ffmpeg_arguments(Path::new("in.mkv"), Path::new("out.ts"), "ts", None).expect("video"),
        );
        assert_eq!(video[4..6], ["-i", "in.mkv"]);
        assert_eq!(video[video.len() - 3..], ["-f", "mpegts", "out.ts"]);
        assert_eq!(
            audio_codec_arguments("m4a"),
            ["-vn", "-c:a", "aac", "-b:a", "256k"]
        );
        assert!(ffmpeg_arguments(Path::new("in.txt"), Path::new("o.mp3"), "mp3", None).is_none());
    }

    #[test]
    fn filters_match_python_wildcards() {
        let english = embedded_catalog("en");
        let input = input_filters(&english);
        assert_eq!(input[0].0, "Audio and video files");
        assert!(input[0].1.starts_with("*.3gp;*.aac;*.ac3;*.aif;"));
        assert_eq!(input[3], ("All files (*.*)".to_owned(), "*.*".to_owned()));
        assert_eq!(
            image_filters(&english)[0].1,
            "*.bmp;*.gif;*.jpeg;*.jpg;*.png;*.tif;*.tiff;*.webp"
        );
        assert_eq!(
            target_filters(&english, "alac")[0],
            ("M4A (*.m4a)".to_owned(), "*.m4a".to_owned())
        );
    }

    #[test]
    fn folder_scan_is_recursive_sorted_and_skips_work_files() {
        let folder = tempfile::tempdir().expect("folder");
        fs::create_dir(folder.path().join("Sub")).expect("sub");
        for name in [
            "b.MP3",
            "A.wav",
            "Sub/c.mkv",
            "notes.txt",
            ".x.apricot-converting-00.mp3",
        ] {
            fs::write(folder.path().join(name), b"x").expect("file");
        }
        let files = media_files_in_folder(folder.path());
        let names = files
            .iter()
            .map(|path| {
                path.strip_prefix(folder.path())
                    .expect("inside")
                    .to_path_buf()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                PathBuf::from("A.wav"),
                PathBuf::from("b.MP3"),
                PathBuf::from("Sub").join("c.mkv")
            ]
        );
        assert!(folder_has_audio_inputs(folder.path()));
        assert!(shows_audio_video_options(
            true,
            &folder.path().to_string_lossy(),
            "mp4"
        ));
        assert!(!shows_audio_video_options(
            true,
            &folder.path().to_string_lossy(),
            "mp3"
        ));
        assert!(shows_audio_video_options(true, r"Z:\missing folder", "mkv"));
        assert!(shows_audio_video_options(false, "a.mp3", "mkv"));
        assert!(!shows_audio_video_options(false, "a.mkv", "mp4"));
    }

    #[test]
    fn file_conversion_replaces_the_original_only_after_success() {
        let folder = tempfile::tempdir().expect("folder");
        let source = folder.path().join("song.flac");
        fs::write(&source, b"flac").expect("source");
        let output = replaced_output_path(&source, "mp3");
        let job = FileConversionJob::new(
            Some(PathBuf::from("ffmpeg.exe")),
            source.clone(),
            output.clone(),
            "mp3",
            None,
            true,
            "unsupported".to_owned(),
        );
        let mut calls = Vec::new();
        let result = run_file_conversion(&job, &mut |program, arguments| {
            calls.push(program.to_path_buf());
            let work = PathBuf::from(arguments.last().expect("output"));
            assert!(
                work.file_name()
                    .expect("name")
                    .to_string_lossy()
                    .starts_with(".song.apricot-converting-")
            );
            fs::write(work, b"mp3").map_err(|error| error.to_string())
        });
        assert_eq!(result, Ok(output.clone()));
        assert_eq!(calls, [PathBuf::from("ffmpeg.exe")]);
        assert_eq!(fs::read(&output).expect("output"), b"mp3");
        assert!(!source.exists());
    }

    #[test]
    fn file_conversion_to_a_new_file_never_overwrites() {
        let folder = tempfile::tempdir().expect("folder");
        let source = folder.path().join("song.mp3");
        fs::write(&source, b"mp3").expect("source");
        let job = FileConversionJob::new(
            None,
            source.clone(),
            source.clone(),
            "mp3",
            None,
            false,
            String::new(),
        );
        assert_eq!(job.output, folder.path().join("song (2).mp3"));
        assert_eq!(
            run_file_conversion(&job, &mut |_, _| Ok(())),
            Err("FFmpeg was not found".to_owned())
        );
    }

    #[test]
    fn folder_conversion_counts_failures_and_reports_progress() {
        let folder = tempfile::tempdir().expect("folder");
        let source = folder.path().join("Music");
        fs::create_dir_all(source.join("Disc")).expect("source");
        fs::write(source.join("a.flac"), b"a").expect("a");
        fs::write(source.join("Disc").join("b.wav"), b"b").expect("b");
        let output = folder.path().join("Music converted");
        let job = FolderConversionJob {
            ffmpeg: Some(PathBuf::from("ffmpeg.exe")),
            source_folder: source.clone(),
            output_folder: output.clone(),
            target: "mp3".to_owned(),
            image: None,
            replace_originals: false,
        };
        let mut events = Vec::new();
        let outcome = run_folder_conversion(
            &job,
            &mut |_, arguments| {
                let target = PathBuf::from(arguments.last().expect("output"));
                if target.ends_with("b.mp3") {
                    return Err("broken".to_owned());
                }
                fs::write(target, b"mp3").map_err(|error| error.to_string())
            },
            &mut |event| events.push(event),
        );
        assert_eq!(
            outcome,
            Ok(FolderConversionOutcome::Done {
                converted: 1,
                failed: 1
            })
        );
        assert!(output.join("a.mp3").exists());
        assert!(output.join("Disc").is_dir());
        assert_eq!(events[0], FolderConversionEvent::Started { total: 2 });
        assert_eq!(
            events[1],
            FolderConversionEvent::FileStarted {
                index: 1,
                total: 2,
                name: "a.flac".to_owned()
            }
        );
        assert_eq!(
            events.last(),
            Some(&FolderConversionEvent::Progress {
                file: "b.wav".to_owned(),
                converted: 1,
                total: 2
            })
        );
        let empty = FolderConversionJob {
            source_folder: output.join("Disc"),
            ..job
        };
        assert_eq!(
            run_folder_conversion(&empty, &mut |_, _| Ok(()), &mut |_| {}),
            Ok(FolderConversionOutcome::NoMediaFiles)
        );
    }

    #[test]
    fn folder_replace_mode_removes_failed_work_files() {
        let folder = tempfile::tempdir().expect("folder");
        fs::write(folder.path().join("a.wav"), b"a").expect("a");
        let job = FolderConversionJob {
            ffmpeg: Some(PathBuf::from("ffmpeg.exe")),
            source_folder: folder.path().to_path_buf(),
            output_folder: folder.path().to_path_buf(),
            target: "flac".to_owned(),
            image: None,
            replace_originals: true,
        };
        let outcome = run_folder_conversion(
            &job,
            &mut |_, arguments| {
                fs::write(PathBuf::from(arguments.last().expect("output")), b"partial")
                    .map_err(|error| error.to_string())?;
                Err("failed".to_owned())
            },
            &mut |_| {},
        );
        assert_eq!(
            outcome,
            Ok(FolderConversionOutcome::Done {
                converted: 0,
                failed: 1
            })
        );
        let names = fs::read_dir(folder.path())
            .expect("list")
            .map(|entry| entry.expect("entry").file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, [OsString::from("a.wav")]);
    }

    #[test]
    fn progress_and_done_messages_use_python_texts() {
        let english = embedded_catalog("en");
        assert_eq!(
            progress_message(&english, "a.mp3", 1, 3),
            "a.mp3\nConverted: 1 of 3\nRemaining: 2"
        );
        assert_eq!(
            folder_done_message(&english, 2, 0),
            "Folder conversion complete: 2 files."
        );
        assert_eq!(
            folder_done_message(&english, 2, 1),
            "Folder conversion complete: 2 files converted, 1 failed."
        );
    }
}
