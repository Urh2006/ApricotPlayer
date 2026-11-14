//! The system media controls: media keys, the Windows media overlay and
//! macOS Now Playing.

use std::time::Duration;

/// What a media key or the system overlay asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaKeyEvent {
    Toggle,
    Play,
    Pause,
    Next,
    Previous,
    Stop,
    /// Seconds forward (positive) or back.
    SeekBy(f64),
    /// A position in seconds.
    SetPosition(f64),
}

#[cfg(any(windows, target_os = "macos"))]
fn media_key_event(event: &souvlaki::MediaControlEvent) -> Option<MediaKeyEvent> {
    use souvlaki::{MediaControlEvent as Event, SeekDirection};
    let signed = |direction: SeekDirection, seconds: f64| {
        if direction == SeekDirection::Forward {
            seconds
        } else {
            -seconds
        }
    };
    Some(match event {
        Event::Toggle => MediaKeyEvent::Toggle,
        Event::Play => MediaKeyEvent::Play,
        Event::Pause => MediaKeyEvent::Pause,
        Event::Next => MediaKeyEvent::Next,
        Event::Previous => MediaKeyEvent::Previous,
        Event::Stop => MediaKeyEvent::Stop,
        Event::Seek(direction) => MediaKeyEvent::SeekBy(signed(*direction, 10.0)),
        Event::SeekBy(direction, amount) => {
            MediaKeyEvent::SeekBy(signed(*direction, amount.as_secs_f64()))
        }
        Event::SetPosition(position) => MediaKeyEvent::SetPosition(position.0.as_secs_f64()),
        _ => return None,
    })
}

/// The media keys and the system's now playing display.
#[cfg(any(windows, target_os = "macos"))]
pub struct SystemMediaControls {
    controls: souvlaki::MediaControls,
}

#[cfg(any(windows, target_os = "macos"))]
impl SystemMediaControls {
    /// Connects to the system controls; `window` is the main window handle,
    /// which Windows needs.
    ///
    /// # Errors
    ///
    /// Returns the system's report when the controls are unavailable.
    pub fn new(
        window: Option<*mut std::ffi::c_void>,
        events: impl Fn(MediaKeyEvent) + Send + 'static,
    ) -> Result<Self, String> {
        let mut controls = souvlaki::MediaControls::new(souvlaki::PlatformConfig {
            display_name: "ApricotPlayer",
            dbus_name: "apricotplayer",
            hwnd: window,
        })
        .map_err(|error| format!("{error:?}"))?;
        controls
            .attach(move |event| {
                if let Some(event) = media_key_event(&event) {
                    events(event);
                }
            })
            .map_err(|error| format!("{error:?}"))?;
        Ok(Self { controls })
    }

    /// The title and artist the system shows.
    pub fn now_playing(&mut self, title: &str, artist: &str, duration: Option<f64>) {
        let _ = self.controls.set_metadata(souvlaki::MediaMetadata {
            title: Some(title).filter(|title| !title.is_empty()),
            artist: Some(artist).filter(|artist| !artist.is_empty()),
            duration: duration
                .filter(|duration| duration.is_finite() && *duration > 0.0)
                .map(Duration::from_secs_f64),
            ..souvlaki::MediaMetadata::default()
        });
    }

    pub fn playback(&mut self, playing: bool, position: Option<f64>) {
        let progress = position
            .filter(|position| position.is_finite() && *position >= 0.0)
            .map(|position| souvlaki::MediaPosition(Duration::from_secs_f64(position)));
        let _ = self.controls.set_playback(if playing {
            souvlaki::MediaPlayback::Playing { progress }
        } else {
            souvlaki::MediaPlayback::Paused { progress }
        });
    }

    pub fn stopped(&mut self) {
        let _ = self.controls.set_playback(souvlaki::MediaPlayback::Stopped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn media_events_map_to_seconds() {
        use souvlaki::{MediaControlEvent as Event, MediaPosition, SeekDirection};
        assert_eq!(media_key_event(&Event::Toggle), Some(MediaKeyEvent::Toggle));
        assert_eq!(
            media_key_event(&Event::SeekBy(
                SeekDirection::Backward,
                Duration::from_secs(5)
            )),
            Some(MediaKeyEvent::SeekBy(-5.0))
        );
        assert_eq!(
            media_key_event(&Event::SetPosition(MediaPosition(Duration::from_secs(70)))),
            Some(MediaKeyEvent::SetPosition(70.0))
        );
        assert_eq!(media_key_event(&Event::Raise), None);
    }
}
