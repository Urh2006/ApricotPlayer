//! Native Win32 accessibility qualification spike.
//!
//! This target is a temporary proof harness, not the production application.

#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
mod windows_spike;

#[cfg(windows)]
fn main() -> windows::core::Result<()> {
    windows_spike::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("The accessibility spike can only run on Windows.");
}
