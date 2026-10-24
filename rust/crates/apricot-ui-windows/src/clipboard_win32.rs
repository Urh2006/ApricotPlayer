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
