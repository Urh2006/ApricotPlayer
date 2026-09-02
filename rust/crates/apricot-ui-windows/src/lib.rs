//! Native Windows UI adapter.

#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
mod win32;

use apricot_app::MainMenuModel;

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
pub fn run_main_menu(model: MainMenuModel, version: &str) -> windows::core::Result<()> {
    win32::run_main_menu(model, version)
}

#[cfg(not(windows))]
/// Rejects the Windows UI on unsupported targets.
///
/// # Errors
///
/// Always returns an unsupported-platform error.
pub fn run_main_menu(_model: MainMenuModel, _version: &str) -> Result<(), &'static str> {
    Err("ApricotPlayer 2 Beta currently requires Windows")
}
