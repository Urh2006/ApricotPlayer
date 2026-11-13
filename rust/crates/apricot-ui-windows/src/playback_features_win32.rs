//! Windows host of the shared player-tool controller.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.
#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use super::{
    HWND, PLAYBACK_TIMER_ID, PLAYBACK_TIMER_INTERVAL_MS, PathBuf, PlaybackCommand, PlaybackRuntime,
    SetTimer, execute_player_command, refresh_player, state_mut,
};
use apricot_shell::playback_features::{self as features, PlaybackFeatureUi};

struct Ui<'a> {
    owner: HWND,
    announcer: &'a crate::announcement_win32::WindowsAnnouncer,
    sender: Option<apricot_playback::PlaybackCommandSender>,
    generation: u64,
}

impl PlaybackFeatureUi for Ui<'_> {
    fn form(&mut self, form: &apricot_shell::FeatureForm) -> Option<apricot_shell::FormValues> {
        crate::feature_form_win32::show(self.owner, form)
    }

    fn form_preview(
        &mut self,
        form: &apricot_shell::FeatureForm,
        preview: &mut dyn FnMut(&apricot_shell::FormValues) -> Vec<PlaybackCommand>,
    ) -> Option<apricot_shell::FormValues> {
        let sender = self.sender.clone();
        let generation = self.generation;
        crate::feature_form_win32::show_preview(self.owner, form, &mut |values| {
            if let Some(sender) = &sender {
                for command in preview(values) {
                    let _ = sender.execute(generation, command);
                }
            }
        })
    }

    fn announce(&mut self, text: &str) {
        unsafe { self.announcer.announce(text, true) };
    }

    fn speak_subtitle(&mut self, text: &str, interrupt: bool) {
        unsafe { self.announcer.announce(text, interrupt) };
    }

    fn open_file(&mut self, title: &str, extensions: &[&str]) -> Option<PathBuf> {
        let patterns = extensions
            .iter()
            .map(|extension| format!("*.{}", extension.trim_start_matches('.')))
            .collect::<Vec<_>>()
            .join(";");
        crate::file_dialog_win32::choose_file_with_filters(
            self.owner,
            title,
            &[
                (title.to_owned(), patterns),
                ("All files".to_owned(), "*.*".to_owned()),
            ],
        )
        .ok()
        .flatten()
    }
}

pub(super) unsafe fn dispatch(window: HWND, owner: HWND, action: &str) -> bool {
    let commands = {
        let Some(state) = state_mut(window) else {
            return false;
        };
        let previous_modal = state.modal_open;
        state.modal_open = true;
        let mut ui = Ui {
            owner,
            announcer: &state.announcer,
            sender: state.playback.as_ref().map(PlaybackRuntime::command_sender),
            generation: state.application.player_session().generation(),
        };
        let commands = features::dispatch_playback_feature(&mut state.application, &mut ui, action);
        state.modal_open = previous_modal;
        commands
    };
    let Some(commands) = commands else {
        return false;
    };
    for command in commands {
        let _ = execute_player_command(window, command);
    }
    if let Some(index) = state_mut(window).and_then(|state| {
        state
            .application
            .playback_features_mut()
            .pending_item_index
            .take()
    }) {
        super::navigate_player_to_index(window, index);
    }
    let exports = state_mut(window).map_or_else(Vec::new, |state| {
        std::mem::take(&mut state.application.playback_features_mut().pending_exports)
    });
    for export in exports {
        super::export_clip_range(
            window,
            super::DownloadChoice::Audio,
            export.start,
            export.end,
            export.audio_filter,
        );
    }
    if let Some(state) = state_mut(window) {
        if state.application.playback_features().timer.is_some() || state.playback.is_some() {
            let _ = SetTimer(
                Some(window),
                PLAYBACK_TIMER_ID,
                PLAYBACK_TIMER_INTERVAL_MS,
                None,
            );
        }
        if state.application.player_session().is_open() {
            refresh_player(window, state, false, false);
        }
    }
    true
}

pub(super) unsafe fn tick(window: HWND) {
    let commands = {
        let Some(state) = state_mut(window) else {
            return;
        };
        if state.modal_open {
            return;
        }
        let mut ui = Ui {
            owner: window,
            announcer: &state.announcer,
            sender: state.playback.as_ref().map(PlaybackRuntime::command_sender),
            generation: state.application.player_session().generation(),
        };
        features::tick_playback_features(&mut state.application, &mut ui)
    };
    for command in commands {
        let _ = execute_player_command(window, command);
    }
    // Crossfade: the next item starts before this one ends; the playback
    // runtime keeps the old player running while the volumes cross.
    let due = state_mut(window)
        .is_some_and(|state| features::automatic_crossfade_due(&mut state.application));
    if due {
        if let Some(state) = state_mut(window) {
            super::mark_current_podcast_episode_played(state);
        }
        super::navigate_player_relative_after_end(window);
    }
}

pub(super) unsafe fn ended(window: HWND) -> bool {
    let commands = state_mut(window)
        .and_then(|state| features::natural_playback_end(&mut state.application, false));
    let Some(commands) = commands else {
        return false;
    };
    for command in commands {
        let _ = execute_player_command(window, command);
    }
    true
}
