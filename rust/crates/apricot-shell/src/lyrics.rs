//! Lyrics (Windows `show_player_lyrics` with `details_win32::show_async`):
//! a read-only text with Copy and Back that first says "Fetching lyrics",
//! then shows the local sidecar or online lyrics (or that there are none)
//! and highlights the timed line at the playback position.

use std::{
    ops::Range,
    sync::mpsc::{self, Receiver, TryRecvError},
};

use apricot_media::lyrics::LyricsDocument;
use apricot_playback::PlaybackPositionReader;

use crate::{Shell, ShellUi};

/// The lyrics window: its texts and the lyrics arriving from a worker.
pub struct LyricsDialog {
    pub title: String,
    pub copy: String,
    pub back: String,
    /// The text shown while the lyrics load (Windows `text_value`).
    pub loading: String,
    copied: String,
    unavailable_message: String,
    ready: String,
    receiver: Option<Receiver<Option<LyricsDocument>>>,
    position: Option<(PlaybackPositionReader, u64)>,
    /// The loaded text; `None` while loading or when there are no lyrics.
    text: Option<String>,
    timeline: Option<LyricsDocument>,
    active_line: Option<usize>,
}

/// What the window shows once the lyrics arrived.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LyricsLoaded {
    /// The text with `\n` line breaks.
    pub text: String,
    /// Announced, and the text gets focus.
    pub announcement: String,
}

/// A change of the highlighted line: the UTF-16 ranges in the shown text
/// to unmark and to mark (and scroll to).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LyricsHighlight {
    pub previous: Option<Range<usize>>,
    pub current: Option<Range<usize>>,
}

impl LyricsDialog {
    /// Windows `poll_text`: `Some` once, when the worker answered.
    pub fn poll_loaded(&mut self) -> Option<LyricsLoaded> {
        let receiver = self.receiver.as_ref()?;
        let document = match receiver.try_recv() {
            Ok(document) => document,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => None,
        };
        self.receiver = None;
        let loaded = document.filter(|document| !document.text.trim().is_empty());
        let (text, announcement) = match loaded {
            Some(document) => {
                let text = document.text.replace("\r\n", "\n");
                self.text = Some(document.text.clone());
                if self.position.is_some() {
                    self.timeline = Some(document);
                }
                (text, self.ready.clone())
            }
            None => (
                self.unavailable_message.clone(),
                self.unavailable_message.clone(),
            ),
        };
        Some(LyricsLoaded { text, announcement })
    }

    /// Whether the window needs [`LyricsDialog::poll_loaded`] or
    /// [`LyricsDialog::highlight`] calls.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        self.receiver.is_some() || self.timeline.is_some()
    }

    /// Windows `highlight_lyrics`: `Some` when the active line changed.
    pub fn highlight(&mut self) -> Option<LyricsHighlight> {
        let document = self.timeline.as_ref()?;
        let (reader, generation) = self.position.as_ref()?;
        let position = reader.read(*generation)?;
        let active = document.active_line(position);
        if active == self.active_line {
            return None;
        }
        let range = |index: Option<usize>| {
            index
                .and_then(|index| document.timed_lines.get(index))
                .map(|line| line.rich_start..line.rich_end)
        };
        let change = LyricsHighlight {
            previous: range(self.active_line),
            current: range(active),
        };
        self.active_line = active;
        Some(change)
    }

    /// Windows `copy_details`: the text to copy and the message to speak
    /// after copying.
    ///
    /// # Errors
    ///
    /// Returns the message to speak when there are no lyrics to copy.
    pub fn copy(&self) -> Result<(String, String), String> {
        self.text
            .as_ref()
            .map(|text| (text.clone(), self.copied.clone()))
            .ok_or_else(|| self.unavailable_message.clone())
    }
}

impl Shell {
    /// Windows `show_player_lyrics`.
    pub(crate) fn show_player_lyrics(&mut self, ui: &mut dyn ShellUi) {
        self.stop_key_repeat(ui);
        if !self.ensure_player_for_auxiliary_view(ui) {
            return;
        }
        let Some(item) = self.application.player_session().current_item().cloned() else {
            return;
        };
        let catalog = self.catalog();
        let local_source = catalog.text("lyrics_source_local").to_owned();
        let online_source = catalog.text("lyrics_source_online").to_owned();
        let settings = self.application.settings();
        let online = settings.enable_online_lyrics;
        let proxy = settings.proxy.clone();
        // Spotify's own lyrics and episode transcripts arrive with Spotify
        // (unit M13).
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            use apricot_platform::lyrics::{LyricsQuery, LyricsSource, fetch_lyrics};
            let query = LyricsQuery::from_item(&item);
            let document = fetch_lyrics(
                item.local_path.as_deref().map(std::path::Path::new),
                &query,
                online,
                Some(&proxy),
            )
            .ok()
            .flatten()
            .map(|lyrics| {
                let source = match lyrics.source {
                    LyricsSource::Local => &local_source,
                    LyricsSource::Online => &online_source,
                };
                LyricsDocument::parse(&lyrics.text, source)
            });
            let _ = sender.send(document);
        });
        let title = catalog.text("lyrics").to_owned();
        ui.open_lyrics(LyricsDialog {
            copy: catalog.text("copy_lyrics").to_owned(),
            back: catalog.text("back").to_owned(),
            loading: catalog.text("lyrics_fetching").to_owned(),
            copied: catalog.text("lyrics_copied").to_owned(),
            unavailable_message: catalog.text("no_lyrics_available").to_owned(),
            ready: title.clone(),
            title,
            receiver: Some(receiver),
            position: self.player.playback.as_ref().map(|runtime| {
                (
                    runtime.position_reader(),
                    self.application.player_session().generation(),
                )
            }),
            text: None,
            timeline: None,
            active_line: None,
        });
    }

    /// The lyrics window closed: focus goes to the player
    /// (Windows `SetFocus(active_primary_control)`).
    pub fn lyrics_closed(&mut self, ui: &mut dyn ShellUi) {
        ui.focus_primary_control();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(document: Option<LyricsDocument>) -> LyricsDialog {
        let (sender, receiver) = mpsc::channel();
        sender.send(document).expect("send");
        LyricsDialog {
            title: "Lyrics".to_owned(),
            copy: "Copy lyrics".to_owned(),
            back: "Back".to_owned(),
            loading: "Fetching lyrics...".to_owned(),
            copied: "Lyrics copied.".to_owned(),
            unavailable_message: "No lyrics available.".to_owned(),
            ready: "Lyrics".to_owned(),
            receiver: Some(receiver),
            position: None,
            text: None,
            timeline: None,
            active_line: None,
        }
    }

    #[test]
    fn loaded_lyrics_are_shown_announced_and_copied() {
        let mut dialog = dialog(Some(LyricsDocument::parse(
            "[00:01.00]First\n[00:03.00]Second",
            "Source: local file",
        )));
        let loaded = dialog.poll_loaded().expect("loaded");
        assert!(!loaded.text.contains('\r'));
        assert!(loaded.text.contains("First\nSecond"), "{:?}", loaded.text);
        assert_eq!(loaded.announcement, "Lyrics");
        assert!(dialog.poll_loaded().is_none(), "announced once");
        let (text, message) = dialog.copy().expect("copy");
        assert!(text.contains("First"));
        assert_eq!(message, "Lyrics copied.");
    }

    #[test]
    fn missing_lyrics_say_so_and_copy_nothing() {
        let mut dialog = dialog(None);
        let loaded = dialog.poll_loaded().expect("loaded");
        assert_eq!(loaded.text, "No lyrics available.");
        assert_eq!(loaded.announcement, "No lyrics available.");
        assert_eq!(dialog.copy(), Err("No lyrics available.".to_owned()));
        assert!(!dialog.is_live());
    }

    #[test]
    fn timed_ranges_select_the_line_in_the_shown_text() {
        let document = LyricsDocument::parse("[00:01.00]First\n[00:03.00]Second", "Source");
        let shown: Vec<u16> = document.text.replace("\r\n", "\n").encode_utf16().collect();
        let line = &document.timed_lines[1];
        assert_eq!(
            String::from_utf16(&shown[line.rich_start..line.rich_end]).expect("text"),
            "Second"
        );
    }
}
