//! Per-user single-instance ownership: a named mutex on Windows and an
//! exclusive lock on a per-user file elsewhere.

#![cfg_attr(windows, allow(unsafe_code))]

use crate::{ApplicationIdentity, PlatformError};

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
#[cfg(windows)]
use windows::Win32::System::Threading::CreateMutexW;
#[cfg(windows)]
use windows::core::PCWSTR;

#[derive(Debug)]
pub enum SingleInstanceOutcome {
    Primary(SingleInstanceGuard),
    Secondary,
}

#[derive(Debug)]
pub struct SingleInstanceGuard {
    #[cfg(windows)]
    handle: HANDLE,
    /// The locked file; the lock ends when the file is closed, also when the
    /// process ends abnormally.
    #[cfg(unix)]
    _lock: std::fs::File,
}

/// Acquires the process-lifetime mutex for one `ApricotPlayer` identity.
///
/// Stable Python and local Rust beta identities deliberately use different
/// names, so development never interferes with the installed player.
///
/// # Errors
///
/// Returns an error when Windows cannot create or open the named mutex.
pub fn acquire_single_instance(
    identity: ApplicationIdentity,
) -> Result<SingleInstanceOutcome, PlatformError> {
    let name = match identity {
        ApplicationIdentity::Stable => r"Local\ApricotPlayer",
        ApplicationIdentity::RustBeta => r"Local\ApricotPlayer2Beta",
    };
    acquire_named(name)
}

#[cfg(windows)]
fn acquire_named(name: &str) -> Result<SingleInstanceOutcome, PlatformError> {
    let name: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the UTF-16 name remains alive for the duration of the call. The
    // returned owned handle is closed exactly once by `SingleInstanceGuard`.
    let handle = unsafe { CreateMutexW(None, true, PCWSTR(name.as_ptr())) }
        .map_err(|error| PlatformError::Operation(format!("single-instance mutex: {error}")))?;
    // SAFETY: `GetLastError` must be read immediately after `CreateMutexW` to
    // distinguish a newly created mutex from an existing one.
    let already_exists = unsafe { GetLastError() == ERROR_ALREADY_EXISTS };
    if already_exists {
        // SAFETY: this process does not own the existing instance, but it owns
        // the handle returned by `CreateMutexW` and must close that handle.
        let _ = unsafe { CloseHandle(handle) };
        Ok(SingleInstanceOutcome::Secondary)
    } else {
        Ok(SingleInstanceOutcome::Primary(SingleInstanceGuard {
            handle,
        }))
    }
}

/// The lock file lives in the per-user temporary folder (`$TMPDIR` on macOS),
/// so different users never see each other's instance.
#[cfg(unix)]
fn acquire_named(name: &str) -> Result<SingleInstanceOutcome, PlatformError> {
    let file_name = name.strip_prefix(r"Local\").unwrap_or(name);
    let path = std::env::temp_dir().join(format!("{file_name}.lock"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|error| PlatformError::Operation(format!("single-instance lock: {error}")))?;
    match file.try_lock() {
        Ok(()) => Ok(SingleInstanceOutcome::Primary(SingleInstanceGuard {
            _lock: file,
        })),
        Err(std::fs::TryLockError::WouldBlock) => Ok(SingleInstanceOutcome::Secondary),
        Err(std::fs::TryLockError::Error(error)) => Err(PlatformError::Operation(format!(
            "single-instance lock: {error}"
        ))),
    }
}

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        // SAFETY: the guard uniquely owns this live mutex handle.
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{SingleInstanceOutcome, acquire_named};

    #[test]
    fn a_named_mutex_has_exactly_one_primary_owner() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let name = format!(
            r"Local\ApricotPlayer2BetaTest-{}-{nonce}",
            std::process::id()
        );
        let first = acquire_named(&name).expect("first acquisition");
        assert!(matches!(first, SingleInstanceOutcome::Primary(_)));
        let second = acquire_named(&name).expect("second acquisition");
        assert!(matches!(second, SingleInstanceOutcome::Secondary));
        drop(first);
        let third = acquire_named(&name).expect("reacquisition");
        assert!(matches!(third, SingleInstanceOutcome::Primary(_)));
    }
}
