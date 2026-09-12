//! Native Windows media file picker.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::OsString, mem::size_of, os::windows::ffi::OsStringExt, path::PathBuf};

use windows::{
    Win32::UI::Controls::Dialogs::{
        CommDlgExtendedError, GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
        OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    },
    core::{PCWSTR, PWSTR},
};

use windows::Win32::Foundation::HWND;

const FILE_BUFFER_UNITS: usize = 32_768;
const MEDIA_PATTERNS: &str = "*.3g2;*.3ga;*.3gp;*.aac;*.ac3;*.aif;*.aifc;*.aiff;*.alac;*.amr;*.ape;*.asf;*.au;*.avi;*.caf;*.divx;*.dts;*.flac;*.flv;*.m2ts;*.m2v;*.m4a;*.m4v;*.mka;*.mkv;*.mov;*.mp2;*.mp2v;*.mp3;*.mp4;*.mpe;*.mpeg;*.mpg;*.mpv;*.mts;*.mxf;*.oga;*.ogg;*.ogm;*.ogv;*.ogx;*.opus;*.ra;*.rm;*.rmvb;*.snd;*.ts;*.vob;*.wav;*.weba;*.webm;*.wma;*.wmv";

/// Shows the native picker and returns the selected media path.
///
/// # Errors
///
/// Returns a diagnostic string if the Windows common dialog reports a failure.
pub fn choose_media_file(owner: HWND, title: &str) -> Result<Option<PathBuf>, String> {
    // SAFETY: All pointers refer to mutable/local UTF-16 buffers that outlive
    // the synchronous dialog call. The result is copied before they are dropped.
    unsafe { choose_media_file_win32(owner, title) }
}

/// Shows the native OPML import picker.
///
/// # Errors
///
/// Returns a diagnostic string if the Windows common dialog reports a failure.
pub fn choose_opml_file(
    owner: HWND,
    title: &str,
    type_label: &str,
) -> Result<Option<PathBuf>, String> {
    // SAFETY: The synchronous dialog only borrows the owned UTF-16 buffers.
    unsafe {
        choose_file_win32(
            owner,
            title,
            &format!("{type_label} (*.opml)\0*.opml\0All files (*.*)\0*.*\0\0"),
            "",
            false,
        )
    }
}

/// Shows the native OPML export picker with overwrite confirmation.
///
/// # Errors
///
/// Returns a diagnostic string if the Windows common dialog reports a failure.
pub fn save_opml_file(
    owner: HWND,
    title: &str,
    type_label: &str,
) -> Result<Option<PathBuf>, String> {
    // SAFETY: The synchronous dialog only borrows the owned UTF-16 buffers.
    unsafe {
        choose_file_win32(
            owner,
            title,
            &format!("{type_label} (*.opml)\0*.opml\0All files (*.*)\0*.*\0\0"),
            "apricot_feeds.opml",
            true,
        )
    }
}

unsafe fn choose_media_file_win32(owner: HWND, title: &str) -> Result<Option<PathBuf>, String> {
    let mut file = vec![0_u16; FILE_BUFFER_UNITS];
    let title = wide(title);
    let filter = media_filter();
    let mut dialog = OPENFILENAMEW {
        lStructSize: u32::try_from(size_of::<OPENFILENAMEW>())
            .expect("OPENFILENAMEW size fits in u32"),
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: u32::try_from(file.len()).expect("file buffer size fits in u32"),
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if GetOpenFileNameW(&raw mut dialog).as_bool() {
        let length = file
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(file.len());
        return Ok(Some(PathBuf::from(OsString::from_wide(&file[..length]))));
    }
    let error = CommDlgExtendedError();
    if error.0 == 0 {
        Ok(None)
    } else {
        Err(format!("Windows file dialog failed with code {}", error.0))
    }
}

unsafe fn choose_file_win32(
    owner: HWND,
    title: &str,
    filter: &str,
    default_file: &str,
    save: bool,
) -> Result<Option<PathBuf>, String> {
    let mut file = vec![0_u16; FILE_BUFFER_UNITS];
    let default = default_file.encode_utf16().collect::<Vec<_>>();
    file[..default.len()].copy_from_slice(&default);
    let title = wide(title);
    let filter = filter.encode_utf16().collect::<Vec<_>>();
    let extension = wide("opml");
    let flags = OFN_EXPLORER
        | OFN_PATHMUSTEXIST
        | OFN_NOCHANGEDIR
        | if save {
            OFN_OVERWRITEPROMPT
        } else {
            OFN_FILEMUSTEXIST
        };
    let mut dialog = OPENFILENAMEW {
        lStructSize: u32::try_from(size_of::<OPENFILENAMEW>())
            .expect("OPENFILENAMEW size fits in u32"),
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: u32::try_from(file.len()).expect("file buffer size fits in u32"),
        lpstrTitle: PCWSTR(title.as_ptr()),
        lpstrDefExt: PCWSTR(extension.as_ptr()),
        Flags: flags,
        ..Default::default()
    };
    let accepted = if save {
        GetSaveFileNameW(&raw mut dialog)
    } else {
        GetOpenFileNameW(&raw mut dialog)
    };
    if accepted.as_bool() {
        let length = file
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(file.len());
        return Ok(Some(PathBuf::from(OsString::from_wide(&file[..length]))));
    }
    let error = CommDlgExtendedError();
    if error.0 == 0 {
        Ok(None)
    } else {
        Err(format!("Windows file dialog failed with code {}", error.0))
    }
}

fn media_filter() -> Vec<u16> {
    format!("Media files ({MEDIA_PATTERNS})\0{MEDIA_PATTERNS}\0All files (*.*)\0*.*\0\0")
        .encode_utf16()
        .collect()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::media_filter;

    #[test]
    fn media_filter_is_double_null_terminated_and_includes_audio_and_video() {
        let filter = media_filter();
        assert_eq!(filter.last(), Some(&0));
        assert_eq!(filter.get(filter.len() - 2), Some(&0));
        let text = String::from_utf16_lossy(&filter);
        assert!(text.contains("*.mp3"));
        assert!(text.contains("*.mkv"));
        assert!(text.contains("*.*"));
    }
}
