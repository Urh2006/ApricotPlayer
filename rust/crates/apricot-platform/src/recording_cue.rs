//! Distinct start/stop cues kept alive for asynchronous native playback.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.
#![cfg_attr(windows, allow(unsafe_code))]
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};

pub fn play(started: bool) {
    #[cfg(windows)]
    {
        use std::sync::OnceLock;
        use windows::{
            Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT},
            core::PCWSTR,
        };
        static CUES: OnceLock<[Vec<u8>; 2]> = OnceLock::new();
        let cues = CUES.get_or_init(|| [wave(false), wave(true)]);
        // SAFETY: SND_MEMORY treats this as WAV bytes, kept in static storage.
        unsafe {
            let _ = PlaySoundW(
                PCWSTR(cues[usize::from(started)].as_ptr().cast()),
                None,
                SND_ASYNC | SND_MEMORY | SND_NODEFAULT,
            );
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("/usr/bin/afplay")
            .arg(if started {
                "/System/Library/Sounds/Glass.aiff"
            } else {
                "/System/Library/Sounds/Pop.aiff"
            })
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = started;
}

#[cfg(windows)]
fn wave(started: bool) -> Vec<u8> {
    let frames = 6615u32;
    let rate = 44100u32;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + frames * 2).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend((rate * 2).to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend((frames * 2).to_le_bytes());
    let mut phase = 0.0f64;
    for frame in 0..frames {
        let fraction = f64::from(frame) / f64::from(frames);
        let frequency = if started {
            620.0 + fraction * 500.0
        } else {
            1120.0 - fraction * 500.0
        };
        phase += 2.0 * std::f64::consts::PI * frequency / f64::from(rate);
        let amplitude = (fraction * 30.0).min(1.0) * ((1.0 - fraction) * 30.0).min(1.0);
        bytes.extend(((phase.sin() * amplitude * 6000.0) as i16).to_le_bytes());
    }
    bytes
}
