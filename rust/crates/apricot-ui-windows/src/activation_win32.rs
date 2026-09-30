//! Bounded second-launch activation transport for the native Windows shell.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{
    ffi::c_void, mem::size_of, os::windows::ffi::OsStrExt, path::PathBuf, thread, time::Duration,
};

use apricot_app::ActivationRequest;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::DataExchange::COPYDATASTRUCT,
        UI::WindowsAndMessaging::{
            FindWindowW, MB_ICONINFORMATION, MB_OK, MessageBoxW, SMTO_ABORTIFHUNG, SW_RESTORE,
            SendMessageTimeoutW, SetForegroundWindow, ShowWindow, WM_COPYDATA,
        },
    },
    core::{PCWSTR, Result, w},
};

pub(crate) const MAIN_WINDOW_CLASS: PCWSTR = w!("ApricotPlayer2BetaMainWindow");
const ACTIVATION_SHOW: usize = 1;
const ACTIVATION_OPEN_FILE: usize = 2;
const ACTIVATION_SETTINGS: usize = 3;
const MAX_ACTIVATION_BYTES: usize = 64 * 1024;
const FIND_ATTEMPTS: usize = 40;
const FIND_RETRY: Duration = Duration::from_millis(50);

/// Delivers one bounded activation request to the existing beta process.
///
/// # Errors
///
/// Returns a Win32 error when the existing window cannot be found or does not
/// acknowledge the request before the timeout.
pub fn forward_to_existing(request: &ActivationRequest) -> Result<()> {
    // SAFETY: the target is found by the exact private beta window class. The
    // payload remains alive for the complete synchronous timeout-bounded send.
    unsafe { forward_to_existing_win32(request) }
}

unsafe fn forward_to_existing_win32(request: &ActivationRequest) -> Result<()> {
    let target = find_existing_window()?;
    let mut path = match request {
        ActivationRequest::OpenFile(path) => path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>(),
        ActivationRequest::Show | ActivationRequest::OpenSettings => Vec::new(),
    };
    let byte_count = path.len().saturating_mul(size_of::<u16>());
    if byte_count > MAX_ACTIVATION_BYTES {
        return Err(windows::core::Error::new(
            windows::core::HRESULT(0x8007_0057_u32.cast_signed()),
            "activation path is too large",
        ));
    }
    let mut data = COPYDATASTRUCT {
        dwData: match request {
            ActivationRequest::Show => ACTIVATION_SHOW,
            ActivationRequest::OpenFile(_) => ACTIVATION_OPEN_FILE,
            ActivationRequest::OpenSettings => ACTIVATION_SETTINGS,
        },
        cbData: u32::try_from(byte_count).expect("bounded activation size fits u32"),
        lpData: path.as_mut_ptr().cast::<c_void>(),
    };
    let mut result = 0_usize;
    let delivered = SendMessageTimeoutW(
        target,
        WM_COPYDATA,
        WPARAM(0),
        LPARAM((&raw mut data).cast::<c_void>() as isize),
        SMTO_ABORTIFHUNG,
        2_000,
        Some(&raw mut result),
    );
    if delivered.0 == 0 || result == 0 {
        return Err(windows::core::Error::from_thread());
    }
    Ok(())
}

unsafe fn find_existing_window() -> Result<HWND> {
    for _ in 0..FIND_ATTEMPTS {
        if let Ok(window) = FindWindowW(MAIN_WINDOW_CLASS, PCWSTR::null()) {
            return Ok(window);
        }
        thread::sleep(FIND_RETRY);
    }
    FindWindowW(MAIN_WINDOW_CLASS, PCWSTR::null())
}

pub(crate) unsafe fn decode_request(lparam: LPARAM) -> Option<ActivationRequest> {
    let data = (lparam.0 as *const COPYDATASTRUCT).as_ref()?;
    match data.dwData {
        ACTIVATION_SHOW if data.cbData == 0 => Some(ActivationRequest::Show),
        ACTIVATION_SETTINGS if data.cbData == 0 => Some(ActivationRequest::OpenSettings),
        ACTIVATION_OPEN_FILE => decode_file(data).map(ActivationRequest::OpenFile),
        _ => None,
    }
}

unsafe fn decode_file(data: &COPYDATASTRUCT) -> Option<PathBuf> {
    let byte_count = usize::try_from(data.cbData).ok()?;
    if byte_count < size_of::<u16>()
        || byte_count > MAX_ACTIVATION_BYTES
        || !byte_count.is_multiple_of(size_of::<u16>())
        || data.lpData.is_null()
    {
        return None;
    }
    let units = std::slice::from_raw_parts(data.lpData.cast::<u16>(), byte_count / 2);
    let (&0, text) = units.split_last()? else {
        return None;
    };
    if text.contains(&0) {
        return None;
    }
    let path = String::from_utf16(text).ok()?;
    (!path.trim().is_empty()).then(|| PathBuf::from(path))
}

pub fn show_already_open(message: &str) {
    let message: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: both UTF-16 buffers remain alive for the modal call.
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            crate::PRODUCT_CAPTION,
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

pub(crate) unsafe fn restore_window(window: HWND) {
    let _ = ShowWindow(window, SW_RESTORE);
    let _ = SetForegroundWindow(window);
}

#[cfg(test)]
mod tests {
    use std::{ffi::c_void, mem::size_of, path::Path};

    use windows::{Win32::Foundation::LPARAM, Win32::System::DataExchange::COPYDATASTRUCT};

    use super::{ACTIVATION_OPEN_FILE, MAX_ACTIVATION_BYTES, decode_request};

    #[test]
    fn file_activation_is_bounded_and_null_terminated() {
        let mut value: Vec<u16> = r"C:\Music\Track.mp3"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let data = COPYDATASTRUCT {
            dwData: ACTIVATION_OPEN_FILE,
            cbData: u32::try_from(value.len() * size_of::<u16>()).expect("size"),
            lpData: value.as_mut_ptr().cast::<c_void>(),
        };
        let decoded =
            unsafe { decode_request(LPARAM((&raw const data).cast::<c_void>() as isize)) };
        assert!(matches!(
            decoded,
            Some(apricot_app::ActivationRequest::OpenFile(path))
                if path == Path::new(r"C:\Music\Track.mp3")
        ));
    }

    #[test]
    fn oversized_or_unterminated_payload_is_rejected() {
        let mut value = vec![u16::from(b'a'); MAX_ACTIVATION_BYTES / 2 + 1];
        let oversized = COPYDATASTRUCT {
            dwData: ACTIVATION_OPEN_FILE,
            cbData: u32::try_from(value.len() * 2).expect("size"),
            lpData: value.as_mut_ptr().cast::<c_void>(),
        };
        assert!(
            unsafe { decode_request(LPARAM((&raw const oversized).cast::<c_void>() as isize,)) }
                .is_none()
        );

        value.truncate(3);
        let unterminated = COPYDATASTRUCT {
            dwData: ACTIVATION_OPEN_FILE,
            cbData: u32::try_from(value.len() * 2).expect("size"),
            lpData: value.as_mut_ptr().cast::<c_void>(),
        };
        assert!(
            unsafe { decode_request(LPARAM((&raw const unterminated).cast::<c_void>() as isize,)) }
                .is_none()
        );
    }
}
