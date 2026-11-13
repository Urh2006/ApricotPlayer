//! Local file operations: the Recycle Bin or Trash, permanent deletion, and
//! copying or moving with an explicit choice for existing names.

#![cfg_attr(windows, allow(unsafe_code))]

use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferMode {
    Copy,
    Move,
}

/// What happens when the destination folder already has the name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Collision {
    Skip,
    KeepBoth,
    Replace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransferOutcome {
    Done(PathBuf),
    Skipped,
}

/// Moves one file to the Windows Recycle Bin or the macOS Trash.
///
/// # Errors
///
/// Returns the system's message when the file cannot be moved there.
#[cfg(windows)]
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use windows::{
        Win32::UI::Shell::{
            FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
            FOF_WANTNUKEWARNING, SHFILEOPSTRUCTW, SHFileOperationW,
        },
        core::PCWSTR,
    };

    let absolute = std::path::absolute(path).map_err(|error| error.to_string())?;
    if !absolute.is_file() {
        return Err(io::Error::from(io::ErrorKind::NotFound).to_string());
    }
    // A file that another program keeps open fails here with the system's
    // own message, which the shell operation below does not give.
    {
        use std::os::windows::fs::OpenOptionsExt;
        const DELETE: u32 = 0x0001_0000;
        const SHARE_ALL: u32 = 0x7;
        fs::OpenOptions::new()
            .access_mode(DELETE)
            .share_mode(SHARE_ALL)
            .open(&absolute)
            .map_err(|error| error.to_string())?;
    }
    // The shell does not take the verbatim paths that folder scans produce.
    let mut from: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    let verbatim_unc: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
    let verbatim: Vec<u16> = r"\\?\".encode_utf16().collect();
    if from.starts_with(&verbatim_unc) {
        from.splice(..verbatim_unc.len(), r"\\".encode_utf16());
    } else if from.starts_with(&verbatim) {
        from.drain(..verbatim.len());
    }
    // pFrom is a list that ends with two zeros.
    from.extend([0, 0]);
    // FOF_WANTNUKEWARNING asks before a file that cannot go to the Recycle
    // Bin, for example on a network drive, is deleted for good.
    let flags = FOF_ALLOWUNDO.0
        | FOF_NOCONFIRMATION.0
        | FOF_NOERRORUI.0
        | FOF_SILENT.0
        | FOF_WANTNUKEWARNING.0;
    let mut operation = SHFILEOPSTRUCTW {
        wFunc: FO_DELETE,
        pFrom: PCWSTR(from.as_ptr()),
        #[allow(clippy::cast_possible_truncation)] // The FOF_ flags fit in 16 bits.
        fFlags: flags as u16,
        ..Default::default()
    };
    // SAFETY: `from` outlives the call and ends with the required two zeros.
    let result = unsafe { SHFileOperationW(&raw mut operation) };
    if operation.fAnyOperationsAborted.as_bool() {
        return Err(io::Error::from(io::ErrorKind::Interrupted).to_string());
    }
    if result != 0 || absolute.exists() {
        // The shell's own codes are not system error codes; access is the
        // usual reason left after the check above.
        const ACCESS_DENIED: i32 = 5;
        return Err(io::Error::from_raw_os_error(ACCESS_DENIED).to_string());
    }
    Ok(())
}

/// Moves one file to the Windows Recycle Bin or the macOS Trash.
///
/// # Errors
///
/// Returns the system's message when the file cannot be moved there.
#[cfg(target_os = "macos")]
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let absolute = std::path::absolute(path).map_err(|error| error.to_string())?;
    if !absolute.is_file() {
        return Err(io::Error::from(io::ErrorKind::NotFound).to_string());
    }
    let url = NSURL::fileURLWithPath(&NSString::from_str(&absolute.to_string_lossy()));
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|error| error.localizedDescription().to_string())
}

/// Moves one file to the Windows Recycle Bin or the macOS Trash.
///
/// # Errors
///
/// Other systems have no supported trash.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn move_to_trash(_path: &Path) -> Result<(), String> {
    Err(io::Error::from(io::ErrorKind::Unsupported).to_string())
}

/// Deletes one file without the Recycle Bin or Trash.
///
/// # Errors
///
/// Returns the system's message when the file cannot be deleted.
pub fn delete_permanently(path: &Path) -> Result<(), String> {
    fs::remove_file(path).map_err(|error| error.to_string())
}

/// The path `source` gets in `directory`.
#[must_use]
pub fn target_in(source: &Path, directory: &Path) -> Option<PathBuf> {
    source.file_name().map(|name| directory.join(name))
}

/// `path` itself when it is free, otherwise "name (2).ext", "name (3).ext"
/// and so on.
#[must_use]
pub fn available_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let directory = path.parent().unwrap_or_else(|| Path::new(""));
    (2..=u32::MAX)
        .map(|number| directory.join(format!("{stem} ({number}){extension}")))
        .find(|candidate| !candidate.exists())
        .unwrap_or_else(|| path.to_path_buf())
}

fn same_file(first: &Path, second: &Path) -> bool {
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

/// Copies or moves `source` into `directory`. `collision` decides what
/// happens when the name is taken; with `None` a taken name is skipped.
///
/// # Errors
///
/// Returns the system's message when the file cannot be copied or moved.
pub fn transfer(
    source: &Path,
    directory: &Path,
    mode: TransferMode,
    collision: Option<Collision>,
) -> Result<TransferOutcome, String> {
    let Some(mut target) = target_in(source, directory) else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput).to_string());
    };
    if target.exists() {
        let same = same_file(source, &target);
        match collision.unwrap_or(Collision::Skip) {
            Collision::Skip => return Ok(TransferOutcome::Skipped),
            // A file never replaces itself, and moving it to its own folder
            // changes nothing.
            Collision::Replace if same => return Ok(TransferOutcome::Skipped),
            Collision::KeepBoth if same && mode == TransferMode::Move => {
                return Ok(TransferOutcome::Skipped);
            }
            Collision::Replace => {}
            Collision::KeepBoth => target = available_path(&target),
        }
    }
    match mode {
        TransferMode::Copy => {
            fs::copy(source, &target).map_err(|error| error.to_string())?;
        }
        TransferMode::Move => {
            if fs::rename(source, &target).is_err() {
                // Another drive: copy, then remove the original only when
                // the copy is complete.
                fs::copy(source, &target).map_err(|error| error.to_string())?;
                if let Err(error) = fs::remove_file(source) {
                    let _ = fs::remove_file(&target);
                    return Err(error.to_string());
                }
            }
        }
    }
    Ok(TransferOutcome::Done(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::write(path, text).expect("write");
    }

    #[test]
    fn a_taken_name_is_skipped_kept_or_replaced() {
        let root = tempfile::tempdir().expect("folder");
        let from = root.path().join("from");
        let to = root.path().join("to");
        fs::create_dir_all(&from).expect("from");
        fs::create_dir_all(&to).expect("to");
        let source = from.join("song.mp3");
        write(&source, "new");
        write(&to.join("song.mp3"), "old");

        assert_eq!(
            transfer(&source, &to, TransferMode::Copy, None),
            Ok(TransferOutcome::Skipped)
        );
        assert_eq!(
            transfer(&source, &to, TransferMode::Copy, Some(Collision::KeepBoth)),
            Ok(TransferOutcome::Done(to.join("song (2).mp3")))
        );
        assert_eq!(fs::read_to_string(to.join("song.mp3")).expect("old"), "old");
        assert_eq!(
            transfer(&source, &to, TransferMode::Move, Some(Collision::Replace)),
            Ok(TransferOutcome::Done(to.join("song.mp3")))
        );
        assert!(!source.exists());
        assert_eq!(fs::read_to_string(to.join("song.mp3")).expect("new"), "new");
    }

    #[test]
    fn a_file_never_replaces_itself() {
        let root = tempfile::tempdir().expect("folder");
        let source = root.path().join("song.mp3");
        write(&source, "x");

        assert_eq!(
            transfer(
                &source,
                root.path(),
                TransferMode::Copy,
                Some(Collision::Replace)
            ),
            Ok(TransferOutcome::Skipped)
        );
        assert_eq!(
            transfer(
                &source,
                root.path(),
                TransferMode::Move,
                Some(Collision::KeepBoth)
            ),
            Ok(TransferOutcome::Skipped)
        );
        assert_eq!(
            transfer(
                &source,
                root.path(),
                TransferMode::Copy,
                Some(Collision::KeepBoth)
            ),
            Ok(TransferOutcome::Done(root.path().join("song (2).mp3")))
        );
        assert!(source.exists());
    }

    #[test]
    fn available_names_count_up_and_keep_the_extension() {
        let root = tempfile::tempdir().expect("folder");
        let path = root.path().join("a.b.flac");
        assert_eq!(available_path(&path), path);
        write(&path, "x");
        write(&root.path().join("a.b (2).flac"), "x");
        assert_eq!(available_path(&path), root.path().join("a.b (3).flac"));
    }

    /// Leaves a small file in the Recycle Bin, so it runs only on request:
    /// `cargo test -p apricot-platform recycle_bin -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore = "writes to the user's Recycle Bin"]
    fn the_recycle_bin_takes_a_file() {
        let root = tempfile::tempdir().expect("folder");
        // Folder scans give verbatim paths.
        let path = fs::canonicalize(root.path())
            .expect("canonical")
            .join("apricot-trash-test.txt");
        write(&path, "x");
        let open = fs::File::open(&path).expect("open");
        assert!(move_to_trash(&path).is_ok(), "a reader shares deletion");
        drop(open);
        write(&path, "x");
        assert_eq!(move_to_trash(&path), Ok(()));
        assert!(!path.exists());
        assert!(move_to_trash(&path).is_err());
    }
}
