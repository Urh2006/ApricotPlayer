//! Native Windows UI adapter.

#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
mod action_finder_win32;
#[cfg(windows)]
mod activation_win32;
#[cfg(windows)]
mod announcement_win32;
#[cfg(windows)]
mod bookmark_dialog_win32;
#[cfg(windows)]
mod clipboard_win32;
#[cfg(windows)]
mod details_win32;
#[cfg(windows)]
mod file_dialog_win32;
#[cfg(windows)]
mod first_run_language_win32;
#[cfg(windows)]
mod folder_dialog_win32;
#[cfg(windows)]
mod playback_queue_win32;
#[cfg(windows)]
mod player_controls_win32;
#[cfg(windows)]
mod playlist_dialog_win32;
#[cfg(windows)]
mod settings_win32;
#[cfg(windows)]
mod shortcut_win32;
#[cfg(windows)]
mod win32;

use apricot_app::Application;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualificationGate {
    Pending,
    Passed,
    Failed,
}

#[cfg(windows)]
/// Runs the native Windows main-menu message loop.
///
/// # Errors
///
/// Returns a Win32 error when the window class, window, controls, or message
/// loop cannot be created or operated.
pub fn run_application(
    application: Application,
    version: &str,
    start_hidden: bool,
) -> windows::core::Result<()> {
    win32::run_application(application, version, start_hidden)
}

#[cfg(windows)]
pub use activation_win32::{forward_to_existing, show_already_open};

#[cfg(windows)]
pub use first_run_language_win32::show as choose_initial_language;

#[cfg(not(windows))]
/// Rejects the Windows UI on unsupported targets.
///
/// # Errors
///
/// Always returns an unsupported-platform error.
pub fn run_application(
    _application: Application,
    _version: &str,
    _start_hidden: bool,
) -> Result<(), &'static str> {
    Err("ApricotPlayer 2 Beta currently requires Windows")
}
