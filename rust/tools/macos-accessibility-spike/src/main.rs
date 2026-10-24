//! Native AppKit accessibility qualification spike for macOS.
//!
//! This target is a temporary proof harness, not the production application.
//! It checks the AppKit paths the macOS edition depends on: VoiceOver names
//! and roles of native controls, Tab order, letter navigation in a list, the
//! context menu on Shift+F10 and VO+Shift+M, announcements, focus after a
//! modal alert, and libmpv video inside an `NSView`.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(target_os = "macos")]
mod spike;
#[cfg(target_os = "macos")]
mod video;

#[cfg(target_os = "macos")]
fn main() {
    spike::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("The macOS accessibility spike can only run on macOS.");
}
