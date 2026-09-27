//! Local media discovery with the same recursive and natural-order contract as
//! the Python player.

use std::{
    cmp::Ordering,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering as AtomicOrdering},
};

use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
use thiserror::Error;

pub(crate) const MEDIA_EXTENSIONS: &[&str] = &[
    "3g2", "3ga", "3gp", "aac", "ac3", "aif", "aifc", "aiff", "alac", "amr", "ape", "asf", "au",
    "avi", "caf", "divx", "dts", "flac", "flv", "m2ts", "m2v", "m4a", "m4v", "mka", "mkv", "mov",
    "mp2", "mp2v", "mp3", "mp4", "mpe", "mpeg", "mpg", "mpv", "mts", "mxf", "oga", "ogg", "ogm",
    "ogv", "ogx", "opus", "ra", "rm", "rmvb", "snd", "ts", "vob", "wav", "weba", "webm", "wma",
    "wmv",
];

const VIDEO_EXTENSIONS: &[&str] = &[
    "3g2", "3gp", "asf", "avi", "divx", "flv", "m2ts", "m2v", "m4v", "mkv", "mov", "mp2v", "mp4",
    "mpe", "mpeg", "mpg", "mpv", "mts", "mxf", "ogm", "ogv", "ogx", "rm", "rmvb", "ts", "vob",
    "webm", "wmv",
];

#[derive(Debug, Error)]
pub enum LocalMediaError {
    #[error("the selected folder does not exist")]
    Missing,
    #[error("the selected path is not a folder")]
    NotDirectory,
    #[error("the selected folder could not be read: {0}")]
    Unreadable(#[source] std::io::Error),
    #[error("the local folder scan was cancelled")]
    Cancelled,
}

/// Recursively discovers supported media without following directory symlinks.
/// Unreadable descendants are skipped, matching the stable Python behavior.
///
/// # Errors
///
/// Returns an error if the selected root is missing, not a directory, or cannot
/// be opened at all.
pub fn scan_local_media_folder(folder: &Path) -> Result<Vec<MediaItem>, LocalMediaError> {
    scan_local_media_folder_with_cancel(folder, &AtomicBool::new(false))
}

/// Recursively discovers supported media and stops promptly when cancellation
/// is requested by the owning UI route.
///
/// # Errors
///
/// Returns the same root errors as [`scan_local_media_folder`] or
/// [`LocalMediaError::Cancelled`] when the cancellation flag is set.
pub fn scan_local_media_folder_with_cancel(
    folder: &Path,
    cancelled: &AtomicBool,
) -> Result<Vec<MediaItem>, LocalMediaError> {
    check_cancelled(cancelled)?;
    if !folder.exists() {
        return Err(LocalMediaError::Missing);
    }
    if !folder.is_dir() {
        return Err(LocalMediaError::NotDirectory);
    }
    let canonical_root = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());
    let root_entries = fs::read_dir(&canonical_root).map_err(LocalMediaError::Unreadable)?;
    let mut directories = Vec::new();
    let mut files = Vec::new();
    collect_entries(root_entries, &mut directories, &mut files, cancelled)?;
    while let Some(directory) = directories.pop() {
        check_cancelled(cancelled)?;
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        collect_entries(entries, &mut directories, &mut files, cancelled)?;
    }
    check_cancelled(cancelled)?;
    files.sort_by(|left, right| {
        let left = relative_sort_text(&canonical_root, left);
        let right = relative_sort_text(&canonical_root, right);
        natural_cmp(&left, &right)
    });
    Ok(files
        .into_iter()
        .map(|path| local_media_item(&path, &canonical_root))
        .collect())
}

fn collect_entries(
    entries: fs::ReadDir,
    directories: &mut Vec<PathBuf>,
    files: &mut Vec<PathBuf>,
    cancelled: &AtomicBool,
) -> Result<(), LocalMediaError> {
    for entry in entries.flatten() {
        check_cancelled(cancelled)?;
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            directories.push(entry.path());
        } else if file_type.is_file() && is_supported_media(&entry.path()) {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), LocalMediaError> {
    if cancelled.load(AtomicOrdering::Relaxed) {
        Err(LocalMediaError::Cancelled)
    } else {
        Ok(())
    }
}

fn is_supported_media(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            MEDIA_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}

fn local_media_item(path: &Path, root: &Path) -> MediaItem {
    let title = path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map_or_else(|| path.display().to_string(), ToOwned::to_owned);
    let kind = if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            VIDEO_EXTENSIONS
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        }) {
        MediaKind::Video
    } else {
        MediaKind::Audio
    };
    let path_text = path.to_string_lossy().into_owned();
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    let folder = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_owned();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    MediaItem {
        id: MediaId(path_text.clone()),
        source: MediaSource::Local,
        kind,
        title,
        url: None,
        stream_url: None,
        external_audio_url: None,
        local_path: Some(path_text),
        channel: folder.clone(),
        duration_seconds: None,
        metadata: [
            ("relative_path".to_owned(), relative.into()),
            ("folder".to_owned(), folder.into()),
            ("extension".to_owned(), extension.into()),
        ]
        .into_iter()
        .collect(),
    }
}

fn relative_sort_text(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

#[derive(Debug, Eq, PartialEq)]
enum NaturalPart {
    Text(String),
    Number(String),
}

impl Ord for NaturalPart {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Text(left), Self::Text(right)) => left.cmp(right),
            (Self::Number(left), Self::Number(right)) => natural_number_cmp(left, right),
            (Self::Text(_), Self::Number(..)) => Ordering::Less,
            (Self::Number(..), Self::Text(_)) => Ordering::Greater,
        }
    }
}

impl PartialOrd for NaturalPart {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn natural_cmp(left: &str, right: &str) -> Ordering {
    natural_parts(left).cmp(&natural_parts(right))
}

fn natural_number_cmp(left: &str, right: &str) -> Ordering {
    let left = left.trim_start_matches('0');
    let right = right.trim_start_matches('0');
    let left = if left.is_empty() { "0" } else { left };
    let right = if right.is_empty() { "0" } else { right };
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn natural_parts(value: &str) -> Vec<NaturalPart> {
    let folded = value.to_lowercase();
    let normalized = folded.rfind('.').map_or(folded.clone(), |index| {
        format!("{}\0{}", &folded[..index], &folded[index + 1..])
    });
    let mut parts = Vec::new();
    let mut start = 0;
    let mut digits = None;
    for (index, character) in normalized.char_indices() {
        let is_digit = character.is_ascii_digit();
        match digits {
            None => digits = Some(is_digit),
            Some(previous) if previous != is_digit => {
                push_natural_part(&mut parts, &normalized[start..index], previous);
                start = index;
                digits = Some(is_digit);
            }
            Some(_) => {}
        }
    }
    if let Some(is_digit) = digits {
        push_natural_part(&mut parts, &normalized[start..], is_digit);
    }
    parts
}

fn push_natural_part(parts: &mut Vec<NaturalPart>, value: &str, digits: bool) {
    if digits {
        parts.push(NaturalPart::Number(value.to_owned()));
    } else {
        parts.push(NaturalPart::Text(value.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::atomic::AtomicBool};

    use tempfile::tempdir;

    use super::{
        LocalMediaError, MediaKind, natural_cmp, scan_local_media_folder,
        scan_local_media_folder_with_cancel,
    };

    #[test]
    fn natural_order_keeps_plain_then_numeric_suffixes() {
        let mut names = [
            "Telemach(15).mp3",
            "Telemach(2).mp3",
            "Telemach.mp3",
            "Telemach(10).mp3",
            "Telemach(1).mp3",
        ];
        names.sort_by(|left, right| natural_cmp(left, right));
        assert_eq!(
            names,
            [
                "Telemach.mp3",
                "Telemach(1).mp3",
                "Telemach(2).mp3",
                "Telemach(10).mp3",
                "Telemach(15).mp3"
            ]
        );
    }

    #[test]
    fn natural_order_handles_numbers_larger_than_machine_integers() {
        let mut names = [
            "track100000000000000000000000000000000000000.mp3",
            "track9.mp3",
            "track0009.mp3",
            "track99999999999999999999999999999999999999.mp3",
        ];
        names.sort_by(|left, right| natural_cmp(left, right));
        assert_eq!(names[0], "track9.mp3");
        assert_eq!(names[1], "track0009.mp3");
        assert_eq!(names[2], "track99999999999999999999999999999999999999.mp3");
        assert_eq!(names[3], "track100000000000000000000000000000000000000.mp3");
    }

    #[test]
    fn scanner_is_recursive_filters_extensions_and_does_not_follow_symlinks() {
        let temporary = tempdir().expect("temporary folder");
        fs::write(temporary.path().join("track10.MP3"), b"").expect("track 10");
        fs::write(temporary.path().join("notes.txt"), b"").expect("notes");
        fs::create_dir(temporary.path().join("disc2")).expect("disc folder");
        fs::write(temporary.path().join("disc2").join("track2.flac"), b"").expect("track 2");
        fs::write(temporary.path().join("movie.webm"), b"").expect("movie");

        let items = scan_local_media_folder(temporary.path()).expect("scan");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].title, "track2");
        assert_eq!(items[1].title, "movie");
        assert_eq!(items[1].kind, MediaKind::Video);
        assert_eq!(items[2].title, "track10");
        assert!(items.iter().all(|item| item.local_path.is_some()));
        assert!(items.iter().all(|item| item.url.is_none()));
    }

    #[test]
    fn scanner_rejects_a_file_as_the_root() {
        let temporary = tempdir().expect("temporary folder");
        let file = temporary.path().join("track.mp3");
        fs::write(&file, b"").expect("track");
        assert!(scan_local_media_folder(Path::new(&file)).is_err());
    }

    #[test]
    fn scanner_honors_cancellation_before_touching_the_root() {
        let cancelled = AtomicBool::new(true);
        assert!(matches!(
            scan_local_media_folder_with_cancel(Path::new("missing"), &cancelled),
            Err(LocalMediaError::Cancelled)
        ));
    }
}
