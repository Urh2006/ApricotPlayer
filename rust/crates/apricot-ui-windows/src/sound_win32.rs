//! Short feedback sounds matching Python `play_default_sound`.

#![allow(unsafe_code)]

use std::path::PathBuf;

use windows::{
    Win32::{
        Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME},
        System::Diagnostics::Debug::MessageBeep,
        UI::WindowsAndMessaging::MB_OK,
    },
    core::PCWSTR,
};

/// Python `DEFAULT_REACHED_SOUND`, bundled under `assets` next to the executable.
const DEFAULT_REACHED_SOUND: &str = "default_reached.wav";

fn default_reached_sound_path() -> Option<PathBuf> {
    let directory = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let path = directory.join("assets").join(DEFAULT_REACHED_SOUND);
    path.is_file().then_some(path)
}

/// Plays the "default speed/pitch reached" cue asynchronously, or the
/// standard OK beep when the bundled sound is missing.
pub fn play_default_reached_sound() {
    if let Some(path) = default_reached_sound_path() {
        let wide: Vec<u16> = path
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the
        // call; SND_ASYNC copies the file name before PlaySoundW returns.
        let played =
            unsafe { PlaySoundW(PCWSTR(wide.as_ptr()), None, SND_FILENAME | SND_ASYNC).as_bool() };
        if played {
            return;
        }
    }
    // SAFETY: MessageBeep has no pointer arguments.
    unsafe {
        let _ = MessageBeep(MB_OK);
    }
}
