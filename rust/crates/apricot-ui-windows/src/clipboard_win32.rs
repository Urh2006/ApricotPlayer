//! Small ownership-audited Win32 Unicode clipboard adapter.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ptr, thread, time::Duration};

use windows::{
    Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND},
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GHND, GlobalAlloc, GlobalLock, GlobalUnlock},
        },
    },
    core::{Error, Result},
};

const CF_UNICODETEXT: u32 = 13;
const OPEN_ATTEMPTS: usize = 6;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(10);

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: This guard is created only after OpenClipboard succeeds on
        // the same UI thread, and ownership is closed exactly once here.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Copies UTF-16 text to the Windows clipboard and transfers allocation
/// ownership only after `SetClipboardData` succeeds.
pub fn copy_text(owner: HWND, text: &str) -> Result<()> {
    let encoded = clipboard_utf16(text);
    // SAFETY: The clipboard lifetime, movable global allocation, locking, and
    // transfer semantics are contained in `copy_text_inner`.
    unsafe { copy_text_inner(owner, &encoded) }
}

unsafe fn copy_text_inner(owner: HWND, encoded: &[u16]) -> Result<()> {
    open_with_retry(owner)?;
    let _clipboard = ClipboardGuard;
    EmptyClipboard()?;

    let byte_len = encoded
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(Error::from_thread)?;
    let memory = GlobalAlloc(GHND, byte_len)?;
    let destination = GlobalLock(memory).cast::<u16>();
    if destination.is_null() {
        free_global(memory);
        return Err(Error::from_thread());
    }
    ptr::copy_nonoverlapping(encoded.as_ptr(), destination, encoded.len());
    let _ = GlobalUnlock(memory);

    match SetClipboardData(CF_UNICODETEXT, Some(HANDLE(memory.0))) {
        Ok(_) => Ok(()),
        Err(error) => {
            free_global(memory);
            Err(error)
        }
    }
}

const CF_HDROP: u32 = 15;

/// Puts files on the clipboard as Explorer copies them (`CF_HDROP`).
pub fn copy_files(owner: HWND, paths: &[std::path::PathBuf]) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    // DROPFILES: pFiles, pt (x, y), fNC and fWide, then the list of paths,
    // each ending with a zero and the list with another zero.
    const HEADER: usize = 20;
    let mut list: Vec<u16> = Vec::new();
    for path in paths {
        list.extend(path.as_os_str().encode_wide());
        list.push(0);
    }
    list.push(0);
    let bytes = HEADER + list.len() * size_of::<u16>();
    // SAFETY: the allocation is filled within its size and handed to the
    // clipboard only after SetClipboardData succeeds.
    unsafe {
        open_with_retry(owner)?;
        let _clipboard = ClipboardGuard;
        EmptyClipboard()?;
        let memory = GlobalAlloc(GHND, bytes)?;
        let destination = GlobalLock(memory).cast::<u8>();
        if destination.is_null() {
            free_global(memory);
            return Err(Error::from_thread());
        }
        let header: [u32; 5] = [u32::try_from(HEADER).unwrap_or(20), 0, 0, 0, 1];
        ptr::copy_nonoverlapping(header.as_ptr().cast::<u8>(), destination, HEADER);
        ptr::copy_nonoverlapping(
            list.as_ptr().cast::<u8>(),
            destination.add(HEADER),
            list.len() * size_of::<u16>(),
        );
        let _ = GlobalUnlock(memory);
        if let Err(error) = SetClipboardData(CF_HDROP, Some(HANDLE(memory.0))) {
            free_global(memory);
            return Err(error);
        }
    }
    Ok(())
}

/// Files copied in Explorer, or paths copied as text, one per line.
pub fn read_paths(owner: HWND) -> Vec<std::path::PathBuf> {
    use windows::Win32::{
        System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable},
        UI::Shell::{DragQueryFileW, HDROP},
    };
    // SAFETY: the clipboard stays open while its data is read and the
    // handles are only borrowed, never freed here.
    unsafe {
        if open_with_retry(owner).is_err() {
            return Vec::new();
        }
        let _clipboard = ClipboardGuard;
        if IsClipboardFormatAvailable(CF_HDROP).is_ok()
            && let Ok(handle) = GetClipboardData(CF_HDROP)
        {
            let drop = HDROP(handle.0);
            let count = DragQueryFileW(drop, u32::MAX, None);
            return (0..count)
                .filter_map(|index| {
                    let length = DragQueryFileW(drop, index, None) as usize;
                    let mut buffer = vec![0u16; length + 1];
                    let copied = DragQueryFileW(drop, index, Some(&mut buffer)) as usize;
                    (copied > 0).then(|| {
                        std::path::PathBuf::from(String::from_utf16_lossy(&buffer[..copied]))
                    })
                })
                .collect();
        }
        if IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok()
            && let Ok(handle) = GetClipboardData(CF_UNICODETEXT)
        {
            let text = GlobalLock(HGLOBAL(handle.0)).cast::<u16>();
            if text.is_null() {
                return Vec::new();
            }
            let mut length = 0;
            while *text.add(length) != 0 && length < 32_768 {
                length += 1;
            }
            let value = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            let _ = GlobalUnlock(HGLOBAL(handle.0));
            return value
                .lines()
                .map(|line| line.trim().trim_matches('"'))
                .filter(|line| !line.is_empty())
                .map(std::path::PathBuf::from)
                .collect();
        }
    }
    Vec::new()
}

/// Opens the Windows properties of a file.
pub fn show_properties(owner: HWND, path: &std::path::Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::{
        Shell::{SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_SHOW,
    };
    use windows::{
        Win32::UI::Shell::{SHOP_FILEPATH, SHObjectProperties},
        core::PCWSTR,
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let verb: Vec<u16> = "properties".encode_utf16().chain([0]).collect();
    // SAFETY: `wide` and `verb` end with a zero and outlive the calls.
    unsafe {
        if SHObjectProperties(
            Some(owner),
            SHOP_FILEPATH,
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
        )
        .as_bool()
        {
            return true;
        }
        let mut info = SHELLEXECUTEINFOW {
            cbSize: u32::try_from(size_of::<SHELLEXECUTEINFOW>()).unwrap_or(0),
            fMask: SEE_MASK_INVOKEIDLIST,
            hwnd: owner,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(wide.as_ptr()),
            nShow: SW_SHOW.0,
            ..Default::default()
        };
        ShellExecuteExW(&raw mut info).is_ok()
    }
}

unsafe fn open_with_retry(owner: HWND) -> Result<()> {
    let mut last_error = None;
    for attempt in 0..OPEN_ATTEMPTS {
        match OpenClipboard(Some(owner)) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        if attempt + 1 < OPEN_ATTEMPTS {
            thread::sleep(OPEN_RETRY_DELAY);
        }
    }
    Err(last_error.unwrap_or_else(Error::from_thread))
}

unsafe fn free_global(memory: HGLOBAL) {
    // GlobalFree returns null on success; the generated Result wrapper models
    // handle-returning APIs and therefore cannot be used as a success signal.
    let _ = GlobalFree(Some(memory));
}

fn clipboard_utf16(text: &str) -> Vec<u16> {
    text.encode_utf16()
        .map(|unit| if unit == 0 { 0xfffd } else { unit })
        .chain([0])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::clipboard_utf16;

    #[test]
    fn clipboard_encoding_is_unicode_null_terminated_and_not_truncated() {
        assert_eq!(
            clipboard_utf16("Apricot ž\0Player"),
            [
                65, 112, 114, 105, 99, 111, 116, 32, 382, 65_533, 80, 108, 97, 121, 101, 114, 0
            ]
        );
    }
}

#[cfg(test)]
mod file_tests {
    #[test]
    #[ignore = "uses the real clipboard"]
    fn copied_files_read_back() {
        let owner = windows::Win32::Foundation::HWND::default();
        let path = std::env::current_exe().unwrap();
        super::copy_files(owner, std::slice::from_ref(&path)).unwrap();
        assert_eq!(super::read_paths(owner), vec![path]);
    }
}
