//! Edit mode of a local file (Windows `toggle_player_edit_mode`,
//! `save_edited_local_file`, `poll_edit_saves`) and the clip markers with
//! their preview (`toggle_player_clip_marker`, `preview_marked_clip`).

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
};

use apricot_app::PlaybackPhase;

use crate::{Shell, ShellUi, format_duration};

/// Windows `PendingEditSave`.
struct PendingEditSave {
    replace_original: bool,
    receiver: Receiver<Result<PathBuf, String>>,
}

#[derive(Default)]
pub(crate) struct EditSaves {
    pending: Vec<PendingEditSave>,
}

impl Shell {
    /// Windows `current_local_media_path`: the file of a playing local item.
    fn current_local_media_path(&self) -> Option<PathBuf> {
        self.application
            .player_session()
            .current_item()
            .filter(|item| item.is_local_media())
            .and_then(|item| item.local_path.as_deref())
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.is_file())
    }

    /// Windows `toggle_player_edit_mode`.
    pub(crate) fn toggle_player_edit_mode(&mut self, ui: &mut dyn ShellUi) {
        if !self.application.player_session().is_open() || !self.has_playback() {
            return;
        }
        let key = if self.current_local_media_path().is_none() {
            "edit_mode_local_only"
        } else if self.application.toggle_player_edit_mode() {
            "edit_mode_on"
        } else {
            "edit_mode_off"
        };
        ui.set_status(&self.text(key), true);
    }

    /// Windows `save_edited_local_file`: renders the speed, pitch and
    /// equalizer of edit mode into a copy ("name - edited") or over the
    /// original, in the background.
    pub(crate) fn save_edited_local_file(&mut self, ui: &mut dyn ShellUi, replace_original: bool) {
        if !self.application.player_session().edit_mode() {
            return;
        }
        let Some(source) = self.current_local_media_path() else {
            ui.set_status(&self.text("edit_mode_local_only"), true);
            return;
        };
        let Some(audio) = self.application.local_edit_audio() else {
            return;
        };
        let output = apricot_app::local_edit::edited_output_path(&source, replace_original);
        let temporary_output = if replace_original {
            apricot_app::local_edit::temporary_conversion_path(&output)
        } else {
            output.clone()
        };
        let mpv = apricot_platform::mpv_locations()
            .map(|(executable, _)| executable)
            .filter(|path| path.is_file());
        let ffmpeg = apricot_platform::ffmpeg_executable(
            &self.application.settings().ffmpeg_location,
            crate::player_tools::application_directory().as_deref(),
        );
        let render = mpv.zip(ffmpeg.clone()).map(|(mpv, ffmpeg)| {
            let pcm = apricot_app::local_edit::pcm_render_path(&temporary_output);
            apricot_platform::LocalEditRender {
                mpv,
                mpv_arguments: audio.mpv_render_arguments(&source, &pcm),
                ffmpeg,
                mux_arguments: audio.mux_arguments(&source, &pcm, &temporary_output),
                pcm,
            }
        });
        let fallback =
            ffmpeg.map(|ffmpeg| (ffmpeg, audio.ffmpeg_arguments(&source, &temporary_output)));
        let job = apricot_platform::LocalEditJob {
            source,
            output,
            temporary_output,
            render,
            fallback,
        };
        ui.set_status(&self.text("edit_save_started"), true);
        if replace_original {
            // Python `stop_player(silent=True)` releases the file and keeps
            // the player page open.
            self.close_player_runtime(ui);
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = sender.send(apricot_platform::save_local_edit(&job));
        });
        self.edit_saves.pending.push(PendingEditSave {
            replace_original,
            receiver,
        });
        ui.set_background_polling(true);
    }

    /// The end of Python `save_edited_local_file_worker`; `true` while
    /// saves run.
    pub(crate) fn poll_edit_saves(&mut self, ui: &mut dyn ShellUi) -> bool {
        if self.edit_saves.pending.is_empty() {
            return false;
        }
        let mut finished = Vec::new();
        self.edit_saves
            .pending
            .retain(|save| match save.receiver.try_recv() {
                Ok(result) => {
                    finished.push((save.replace_original, result));
                    false
                }
                Err(TryRecvError::Empty) => true,
                Err(TryRecvError::Disconnected) => {
                    finished.push((save.replace_original, Err("Edit worker stopped".to_owned())));
                    false
                }
            });
        let mut failures = Vec::new();
        for (replace_original, result) in finished {
            match result {
                Ok(path) => {
                    let key = if replace_original {
                        "edit_replace_done"
                    } else {
                        "edit_save_done"
                    };
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    ui.set_status(&self.text(key).replace("{title}", &name), true);
                }
                Err(error) => {
                    failures.push(self.text("edit_save_failed").replace("{error}", &error));
                }
            }
        }
        for message in failures {
            ui.show_error(&message);
        }
        !self.edit_saves.pending.is_empty()
    }

    /// Windows `toggle_player_clip_marker`: sets or clears the start or end
    /// marker at the position; a running preview stops first.
    pub(crate) fn toggle_player_clip_marker(&mut self, ui: &mut dyn ShellUi, start: bool) {
        if self.player.clip_preview.is_some() {
            if let Some(runtime) = self.player.playback.as_ref()
                && let Err(error) =
                    runtime.cancel_preview(self.application.player_session().generation())
            {
                ui.show_error(&format!("Player command failed: {error}"));
                return;
            }
            self.player.clip_preview = None;
        }
        let session = self.application.player_session();
        if !session.is_open() {
            return;
        }
        if matches!(
            session.phase(),
            PlaybackPhase::Starting | PlaybackPhase::Failed
        ) {
            ui.set_status(&self.text("timing_unavailable"), true);
            return;
        }
        let position = self.application.toggle_player_clip_marker(start);
        let key = match (start, position.is_some()) {
            (true, true) => "clip_start_marker_set",
            (true, false) => "clip_start_marker_cleared",
            (false, true) => "clip_end_marker_set",
            (false, false) => "clip_end_marker_cleared",
        };
        let mut message = self.text(key);
        if let Some(position) = position {
            message = message.replace("{time}", &format_duration(position));
        }
        ui.set_status(&message, true);
    }

    /// Windows `preview_marked_clip`: plays from the start marker to the end
    /// marker, then pauses.
    pub(crate) fn preview_marked_clip(&mut self, ui: &mut dyn ShellUi) {
        let session = self.application.player_session();
        let Some((start, end)) = session.clip_range() else {
            let key =
                if session.clip_start_seconds().is_some() && session.clip_end_seconds().is_some() {
                    "clip_marker_invalid"
                } else {
                    "clip_markers_missing"
                };
            ui.set_status(&self.text(key), true);
            return;
        };
        let generation = session.generation();
        let Some(runtime) = self.player.playback.as_ref() else {
            return;
        };
        if let Err(error) = runtime.preview(generation, start, end) {
            ui.show_error(&format!("Player command failed: {error}"));
            return;
        }
        self.player.clip_preview = Some(generation);
        let message = self
            .text("clip_preview_started")
            .replace("{start}", &format_duration(start))
            .replace("{end}", &format_duration(end));
        ui.set_status(&message, true);
    }
}
