//! Related videos (Windows `play_related_video` and `poll_related_videos`):
//! the next related `YouTube` video plays, by key or after the end with
//! "autoplay related".

use std::sync::mpsc::{self, Receiver, TryRecvError};

use apricot_core::MediaItem;

use crate::{Shell, ShellUi};

/// Windows `PendingRelatedVideos`.
pub(crate) struct PendingRelated {
    generation: u64,
    manual: bool,
    receiver: Receiver<Result<Vec<MediaItem>, String>>,
}

impl Shell {
    /// Windows `play_related_video`.
    pub(crate) fn play_related_video(&mut self, ui: &mut dyn ShellUi, manual: bool) {
        let session = self.application.player_session();
        let current = session.is_open().then(|| session.current_item()).flatten();
        let Some(item) = current else {
            ui.set_status(&self.text("no_player"), true);
            return;
        };
        let Some(video_id) = item.youtube_video_id() else {
            ui.set_status(&self.text("no_related_video"), true);
            return;
        };
        let generation = session.generation();
        if manual {
            ui.set_status(&self.text("loading_related_video"), false);
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let url = format!("https://www.youtube.com/watch?v={video_id}");
            let result = apricot_platform::youtube_related::fetch_related_videos(&url)
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.related = Some(PendingRelated {
            generation,
            manual,
            receiver,
        });
        ui.set_background_polling(true);
    }

    /// Windows `poll_related_videos`; `true` while the request runs.
    pub(crate) fn poll_related_videos(&mut self, ui: &mut dyn ShellUi) -> bool {
        let Some(pending) = self.related.as_ref() else {
            return false;
        };
        // Python `apply_related_videos_and_play` ignores results for an
        // older player generation.
        if pending.generation != self.application.player_session().generation()
            || !self.application.player_session().is_open()
        {
            self.related = None;
            return false;
        }
        let videos = match pending.receiver.try_recv() {
            Ok(result) => result.unwrap_or_default(),
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => Vec::new(),
        };
        let manual = pending.manual;
        self.related = None;
        if let Some(item) = self.application.apply_related_videos(videos) {
            self.start_sequence_media_item(ui, item, None);
            return false;
        }
        // Python `play_next_standard_fallback`.
        if manual {
            ui.set_status(&self.text("no_related_video"), true);
        } else {
            self.navigate_player_relative(ui, 1, true);
        }
        false
    }
}
