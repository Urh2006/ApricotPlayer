//! Native AppKit UI of `ApricotPlayer` for macOS.
//!
//! The UI renders what [`apricot_shell::Shell`] decides and sends user input
//! back to it. Only native AppKit controls are used, so VoiceOver gets real
//! roles, names, values and states. Keyboard rules follow the Windows edition:
//! an explicit Tab order on every control, Enter on the focused control,
//! letter navigation in lists as in a Windows list box, and shortcuts stored
//! with Windows names but pressed and shown with Command for Ctrl and Option
//! for Alt.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(target_os = "macos")]
mod action_finder;
#[cfg(target_os = "macos")]
mod announce;
#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod audiovault_login;
#[cfg(target_os = "macos")]
mod bookmarks;
#[cfg(target_os = "macos")]
mod comments;
#[cfg(target_os = "macos")]
mod context_menu;
#[cfg(target_os = "macos")]
mod controls;
#[cfg(target_os = "macos")]
mod converter;
#[cfg(target_os = "macos")]
mod download_progress;
#[cfg(target_os = "macos")]
mod equalizer;
#[cfg(target_os = "macos")]
mod form_controls;
pub mod keys;
#[cfg(target_os = "macos")]
mod language;
#[cfg(target_os = "macos")]
mod lyrics;
#[cfg(target_os = "macos")]
mod player_page;
#[cfg(target_os = "macos")]
mod queue;
#[cfg(target_os = "macos")]
mod screen_buttons;
#[cfg(target_os = "macos")]
mod screen_fields;
#[cfg(all(target_os = "macos", debug_assertions))]
mod self_test;
#[cfg(target_os = "macos")]
mod settings;
#[cfg(target_os = "macos")]
mod transcript;
#[cfg(target_os = "macos")]
mod video;
#[cfg(target_os = "macos")]
mod window;

use apricot_app::Application;

/// Runs the macOS application until it quits. With `prompt_language` the
/// one-time language choice comes first (Windows `choose_initial_language`
/// before `run_application`).
///
/// # Errors
///
/// Returns a message when the application cannot start on this thread.
#[cfg(target_os = "macos")]
pub fn run_application(
    application: Application,
    version: &str,
    start_hidden: bool,
    prompt_language: bool,
) -> Result<(), String> {
    app::run(application, version, start_hidden, prompt_language)
}

/// A second launch without a file while the first instance keeps its
/// window (Windows `show_already_open`): a message in this process.
#[cfg(target_os = "macos")]
pub fn show_already_open(message: &str) {
    app::show_already_open(message);
}

/// Rejects the macOS UI on other systems.
///
/// # Errors
///
/// Always returns an unsupported-platform message.
#[cfg(not(target_os = "macos"))]
pub fn run_application(
    _application: Application,
    _version: &str,
    _start_hidden: bool,
    _prompt_language: bool,
) -> Result<(), String> {
    Err("the macOS user interface requires macOS".to_owned())
}
