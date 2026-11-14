//! The system media controls (media keys, the Windows media overlay and
//! macOS Now Playing) and Discord Rich Presence. Discord talks over a pipe
//! that can block, so it runs on its own thread.

use std::{
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    time::Duration,
};

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

/// What Discord shows; every field is already filtered by the user's
/// choices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresenceActivity {
    pub title: String,
    pub artist: String,
    /// An https address of the artwork.
    pub artwork: String,
    /// Unix milliseconds of the start and the end.
    pub started_ms: Option<i64>,
    pub ends_ms: Option<i64>,
}

enum PresenceCommand {
    Show(Option<PresenceActivity>),
    Stop,
}

/// Discord Rich Presence on its own thread. It connects when there is
/// something to show and tries again every 15 seconds while Discord is not
/// running.
pub struct DiscordPresence {
    sender: Sender<PresenceCommand>,
}

impl DiscordPresence {
    #[must_use]
    pub fn start(application_id: &str) -> Self {
        let (sender, receiver) = mpsc::channel();
        let application_id = application_id.to_owned();
        let _ = std::thread::Builder::new()
            .name("apricot-discord".into())
            .spawn(move || presence_worker(&application_id, &receiver));
        Self { sender }
    }

    /// Shows the activity, or nothing with `None`.
    pub fn show(&self, activity: Option<PresenceActivity>) {
        let _ = self.sender.send(PresenceCommand::Show(activity));
    }
}

impl Drop for DiscordPresence {
    fn drop(&mut self) {
        let _ = self.sender.send(PresenceCommand::Stop);
    }
}

fn clipped(text: &str) -> String {
    // Discord takes 2 to 128 characters; a blank braille cell pads a
    // single one.
    let mut text: String = text.trim().chars().take(128).collect();
    while text.chars().count() < 2 {
        text.push('\u{2800}');
    }
    text
}

fn presence_worker(application_id: &str, receiver: &Receiver<PresenceCommand>) {
    use discord_rich_presence::{
        DiscordIpc, DiscordIpcClient,
        activity::{Activity, ActivityType, Assets, Timestamps},
    };
    let mut client = DiscordIpcClient::new(application_id);
    let mut connected = false;
    let mut desired: Option<PresenceActivity> = None;
    let mut shown: Option<Option<PresenceActivity>> = None;
    loop {
        match receiver.recv_timeout(Duration::from_secs(15)) {
            Ok(PresenceCommand::Show(activity)) => desired = activity,
            Ok(PresenceCommand::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if shown.as_ref() == Some(&desired) {
            continue;
        }
        if !connected {
            if desired.is_none() {
                shown = Some(None);
                continue;
            }
            connected = client.connect().is_ok();
            if !connected {
                continue;
            }
        }
        let result = match &desired {
            None => client.clear_activity(),
            Some(activity) => {
                let mut payload = Activity::new()
                    .activity_type(ActivityType::Listening)
                    .details(clipped(&activity.title));
                if !activity.artist.trim().is_empty() {
                    payload = payload.state(clipped(&activity.artist));
                }
                if activity.artwork.starts_with("https://") {
                    payload = payload.assets(Assets::new().large_image(activity.artwork.as_str()));
                }
                if let Some(started) = activity.started_ms {
                    let mut timestamps = Timestamps::new().start(started);
                    if let Some(ends) = activity.ends_ms {
                        timestamps = timestamps.end(ends);
                    }
                    payload = payload.timestamps(timestamps);
                }
                client.set_activity(payload)
            }
        };
        if result.is_ok() {
            shown = Some(desired.clone());
        } else {
            let _ = client.close();
            connected = false;
            shown = None;
        }
    }
    if connected {
        let _ = client.clear_activity();
        let _ = client.close();
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

    #[test]
    fn discord_texts_are_two_to_128_characters() {
        assert_eq!(clipped(&"a".repeat(200)).chars().count(), 128);
        assert_eq!(clipped("x").chars().count(), 2);
    }
}
