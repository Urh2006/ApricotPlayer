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

/// Why a file could not be renamed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RenameError {
    Empty,
    /// The name has a character no file name may contain.
    Invalid,
    /// Another file in the folder already has the name.
    Exists,
    /// The system's message, for example while another program uses it.
    Failed(String),
}

/// Renames `path` to `name` in the same folder. A name without an
/// extension keeps the current one.
///
/// # Errors
///
/// Returns why the new name was refused or the rename failed.
pub fn rename_file(path: &Path, name: &str) -> Result<PathBuf, RenameError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(RenameError::Empty);
    }
    if name.chars().any(|character| {
        matches!(
            character,
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
        )
    }) || name == "."
        || name == ".."
    {
        return Err(RenameError::Invalid);
    }
    let mut name = name.to_owned();
    if Path::new(&name).extension().is_none()
        && let Some(extension) = path.extension()
    {
        name.push('.');
        name.push_str(&extension.to_string_lossy());
    }
    let target = path.with_file_name(&name);
    if target == path {
        return Ok(target);
    }
    // A change of letter case names the same file on Windows and macOS.
    if target.exists() && !same_file(path, &target) {
        return Err(RenameError::Exists);
    }
    fs::rename(path, &target).map_err(|error| RenameError::Failed(error.to_string()))?;
    Ok(target)
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

/// The same file on disk: one path, or two hard links to it.
fn same_file(first: &Path, second: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(first), Ok(second)) = (fs::metadata(first), fs::metadata(second)) {
            return first.dev() == second.dev() && first.ino() == second.ino();
        }
    }
    #[cfg(windows)]
    if let (Some(first), Some(second)) = (file_identity(first), file_identity(second)) {
        return first == second;
    }
    match (fs::canonicalize(first), fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

/// The volume and file index NTFS gives every hard link of one file.
#[cfg(windows)]
fn file_identity(path: &Path) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;

    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle},
    };

    let file = fs::File::open(path).ok()?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: The handle belongs to `file`, which lives for the call.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &raw mut information) }
        .ok()?;
    Some((
        information.dwVolumeSerialNumber,
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    ))
}

/// A hidden name beside `target` for a copy in progress.
fn partial_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let directory = target.parent().unwrap_or_else(|| Path::new(""));
    (0..u32::MAX)
        .map(|number| directory.join(format!(".{name}.{number}.apricot-partial")))
        .find(|candidate| !candidate.exists())
        .unwrap_or_else(|| directory.join(format!(".{name}.apricot-partial")))
}

/// Copies `source` beside `target` first, so `target` (an existing file,
/// or another hard link to `source`) is untouched until the copy is whole.
fn copy_beside(source: &Path, target: &Path) -> Result<PathBuf, String> {
    let partial = partial_path(target);
    if let Err(error) = fs::copy(source, &partial) {
        let _ = fs::remove_file(&partial);
        return Err(error.to_string());
    }
    Ok(partial)
}

/// Copies or moves `source` into `directory`. `collision` decides what
/// happens when the name is taken; with `None` a taken name is skipped.
/// A failure never leaves the destination with less than it had: an
/// existing file is only replaced by a complete copy, and a move across
/// drives removes the original only after its copy has the name.
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
            // Two hard links of one file: moving keeps the destination's
            // link and removes the source's.
            Collision::Replace | Collision::KeepBoth
                if same && mode == TransferMode::Move && source != target =>
            {
                if fs::canonicalize(source).ok() != fs::canonicalize(&target).ok() {
                    fs::remove_file(source).map_err(|error| error.to_string())?;
                    return Ok(TransferOutcome::Done(target));
                }
                return Ok(TransferOutcome::Skipped);
            }
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
            let partial = copy_beside(source, &target)?;
            if let Err(error) = fs::rename(&partial, &target) {
                let _ = fs::remove_file(&partial);
                return Err(error.to_string());
            }
        }
        TransferMode::Move => match fs::rename(source, &target) {
            Ok(()) => {}
            // Another drive: a complete copy takes the name first, and only
            // then the original goes. Any failure leaves the original or a
            // whole copy under the real name, never only a hidden file.
            Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
                move_across_devices(source, &target)?;
            }
            Err(error) => return Err(error.to_string()),
        },
    }
    Ok(TransferOutcome::Done(target))
}

/// A move to another drive: the original goes only after its whole copy has
/// the name, and a file the copy replaced is kept aside until then, so a
/// failure at any step leaves both files as they were (review 2.1).
fn move_across_devices(source: &Path, target: &Path) -> Result<(), String> {
    let partial = copy_beside(source, target)?;
    let previous = target.exists().then(|| partial_path(target));
    if let Some(previous) = &previous
        && let Err(error) = fs::rename(target, previous)
    {
        let _ = fs::remove_file(&partial);
        return Err(error.to_string());
    }
    let restore = |partial: &Path| {
        let _ = fs::remove_file(partial);
        if let Some(previous) = &previous {
            let _ = fs::rename(previous, target);
        }
    };
    if let Err(error) = fs::rename(&partial, target) {
        restore(&partial);
        return Err(error.to_string());
    }
    if let Err(error) = fs::remove_file(source) {
        restore(target);
        return Err(error.to_string());
    }
    if let Some(previous) = &previous {
        let _ = fs::remove_file(previous);
    }
    Ok(())
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    #[test]
    fn rename_keeps_the_extension_and_refuses_taken_or_bad_names() {
        let root = tempfile::tempdir().expect("folder");
        let song = root.path().join("song.mp3");
        fs::write(&song, b"x").expect("song");
        fs::write(root.path().join("other.mp3"), b"x").expect("other");
        assert_eq!(rename_file(&song, "  "), Err(RenameError::Empty));
        assert_eq!(rename_file(&song, "a/b"), Err(RenameError::Invalid));
        assert_eq!(rename_file(&song, "other"), Err(RenameError::Exists));
        let renamed = rename_file(&song, "new name").expect("renamed");
        assert_eq!(renamed, root.path().join("new name.mp3"));
        assert!(renamed.is_file() && !song.exists());
    }
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

    /// Review 2.1: two hard links of one file are one file. Replacing one
    /// with the other must not truncate both.
    #[test]
    fn hard_links_are_never_emptied() {
        let root = tempfile::tempdir().expect("folder");
        let from = root.path().join("from");
        let to = root.path().join("to");
        fs::create_dir_all(&from).expect("from");
        fs::create_dir_all(&to).expect("to");
        let source = from.join("song.mp3");
        write(&source, "twenty-four bytes of song");
        fs::hard_link(&source, to.join("song.mp3")).expect("link");
        assert_eq!(
            transfer(&source, &to, TransferMode::Copy, Some(Collision::Replace)),
            Ok(TransferOutcome::Skipped)
        );
        assert_eq!(
            fs::read_to_string(&source).unwrap(),
            "twenty-four bytes of song"
        );
        assert_eq!(
            transfer(&source, &to, TransferMode::Move, Some(Collision::Replace)),
            Ok(TransferOutcome::Done(to.join("song.mp3")))
        );
        assert!(!source.exists());
        assert_eq!(
            fs::read_to_string(to.join("song.mp3")).unwrap(),
            "twenty-four bytes of song"
        );
    }

    /// Review 2.1: a move whose original cannot be removed keeps the file
    /// it was to replace.
    #[cfg(unix)]
    #[test]
    fn a_failed_move_keeps_the_destination() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().expect("folder");
        let from = root.path().join("locked");
        let to = root.path().join("to");
        fs::create_dir_all(&from).expect("from");
        fs::create_dir_all(&to).expect("to");
        let source = from.join("song.mp3");
        write(&source, "new");
        write(&to.join("song.mp3"), "old");
        fs::set_permissions(&from, fs::Permissions::from_mode(0o555)).expect("lock");
        let result = transfer(&source, &to, TransferMode::Move, Some(Collision::Replace));
        fs::set_permissions(&from, fs::Permissions::from_mode(0o755)).expect("unlock");
        assert!(result.is_err(), "{result:?}");
        assert_eq!(fs::read_to_string(&source).unwrap(), "new");
        assert_eq!(fs::read_to_string(to.join("song.mp3")).unwrap(), "old");
        // No partial copy stays behind.
        assert_eq!(fs::read_dir(&to).unwrap().count(), 1);
    }

    /// Review 2.1 on Windows: an original another program holds without
    /// sharing deletion cannot move, and the file it was to replace stays.
    #[cfg(windows)]
    #[test]
    fn a_failed_move_keeps_the_destination_on_windows() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().expect("folder");
        let to = root.path().join("to");
        fs::create_dir_all(&to).expect("to");
        let source = root.path().join("song.mp3");
        write(&source, "new");
        write(&to.join("song.mp3"), "old");
        // Read and write sharing only, so no rename or delete is allowed.
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .open(&source)
            .expect("hold");
        let result = transfer(&source, &to, TransferMode::Move, Some(Collision::Replace));
        drop(held);
        assert!(result.is_err(), "{result:?}");
        assert_eq!(fs::read_to_string(&source).unwrap(), "new");
        assert_eq!(fs::read_to_string(to.join("song.mp3")).unwrap(), "old");
        assert_eq!(fs::read_dir(&to).unwrap().count(), 1);
    }

    /// Review 2.1: a move to another drive whose original cannot be removed
    /// keeps the file it was to replace.
    #[cfg(windows)]
    #[test]
    fn a_failed_move_across_drives_keeps_the_destination() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().expect("folder");
        let to = root.path().join("to");
        fs::create_dir_all(&to).expect("to");
        let source = root.path().join("song.mp3");
        let target = to.join("song.mp3");
        write(&source, "new");
        write(&target, "old");
        // Reading and writing are shared, deleting is not.
        let held = fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .open(&source)
            .expect("hold");
        let result = super::move_across_devices(&source, &target);
        drop(held);
        assert!(result.is_err(), "{result:?}");
        assert_eq!(fs::read_to_string(&source).unwrap(), "new");
        assert_eq!(fs::read_to_string(&target).unwrap(), "old");
        assert_eq!(fs::read_dir(&to).unwrap().count(), 1);
        // Without the hold the same move completes.
        assert_eq!(super::move_across_devices(&source, &target), Ok(()));
        assert!(!source.exists());
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(fs::read_dir(&to).unwrap().count(), 1);
    }

    #[test]
    fn a_replacing_copy_leaves_no_partial_file() {
        let root = tempfile::tempdir().expect("folder");
        let source = root.path().join("song.mp3");
        let to = root.path().join("to");
        fs::create_dir_all(&to).expect("to");
        write(&source, "new");
        write(&to.join("song.mp3"), "old");
        assert_eq!(
            transfer(&source, &to, TransferMode::Copy, Some(Collision::Replace)),
            Ok(TransferOutcome::Done(to.join("song.mp3")))
        );
        assert_eq!(fs::read_to_string(to.join("song.mp3")).unwrap(), "new");
        assert_eq!(fs::read_dir(&to).unwrap().count(), 1);
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
