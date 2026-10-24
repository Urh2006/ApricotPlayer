//! Hands URLs, files and folders to the operating system: Explorer on
//! Windows, as the Windows UI does today, and `open` on macOS.

use std::{
    ffi::OsStr,
    io,
    path::Path,
    process::{Child, Command},
};

/// Opens a URL in the default browser.
///
/// # Errors
///
/// Returns the error of starting the system opener.
pub fn open_url(url: &str) -> io::Result<Child> {
    open(OsStr::new(url))
}

/// Opens a file with its default application, or a folder in the file
/// manager.
///
/// # Errors
///
/// Returns the error of starting the system opener.
pub fn open_path(path: &Path) -> io::Result<Child> {
    open(path.as_os_str())
}

#[cfg(windows)]
fn open(target: &OsStr) -> io::Result<Child> {
    Command::new("explorer.exe").arg(target).spawn()
}

#[cfg(target_os = "macos")]
fn open(target: &OsStr) -> io::Result<Child> {
    Command::new("/usr/bin/open").arg(target).spawn()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open(target: &OsStr) -> io::Result<Child> {
    Command::new("xdg-open").arg(target).spawn()
}
