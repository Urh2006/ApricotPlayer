//! Player page extras: the details panel (Windows `show_player_details`,
//! `hide_player_details`, `copy_player_details`, `sync_player_details`),
//! output devices (`show_output_devices` and the saved device check after
//! start) and full screen (`set_player_fullscreen`, `sync_window_fullscreen`).

use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

use apricot_app::SessionToggle;
use apricot_playback::{AudioOutputDevice, PlaybackCommand};

use crate::{ChoiceDialog, PlayerFocus, Shell, ShellUi, View};

/// Windows `AUDIO_DEVICE_CHECK_DELAY_MS` and `AUDIO_DEVICE_PROBE_TIMEOUT`.
const AUDIO_DEVICE_CHECK_DELAY: Duration = Duration::from_millis(6_500);
pub(crate) const AUDIO_DEVICE_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// The labels of the details panel.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetailsLabels {
    pub title: String,
    pub copy: String,
    pub back: String,
}

#[derive(Default)]
pub(crate) struct PlayerExtras {
    pub(crate) details_visible: bool,
    pending_device_check: Option<Receiver<Option<Vec<AudioOutputDevice>>>>,
}

impl Shell {
    fn details_labels(&self) -> DetailsLabels {
        DetailsLabels {
            title: self.text("video_details"),
            copy: self.text("copy_details"),
            back: self.text("back"),
        }
    }

    /// Windows `player_details_text`.
    fn player_details_text(&self) -> String {
        self.application
            .player_details_text()
            .unwrap_or_else(|| self.text("details_unavailable"))
    }

    /// Windows `show_current_player_screen`.
    pub(crate) fn show_current_player_screen(&mut self, ui: &mut dyn ShellUi) -> bool {
        if !self.application.player_session().is_open() {
            ui.set_status(&self.text("no_player"), true);
            self.show_main_menu(ui);
            return false;
        }
        if self.view != View::Player {
            self.cancel_local_folder_scan();
            self.application.navigate_to_player();
            self.view = View::Player;
        }
        self.refresh_player(ui, PlayerFocus::Initial);
        true
    }

    /// Windows `show_player_details`.
    pub(crate) fn show_player_details(&mut self, ui: &mut dyn ShellUi) {
        self.stop_key_repeat(ui);
        if self.view != View::Player
            && !(self.application.player_session().is_open() && self.show_current_player_screen(ui))
        {
            if !self.application.player_session().is_open() {
                ui.set_status(&self.text("no_player"), true);
            }
            return;
        }
        if self
            .application
            .player_session()
            .enabled_toggles()
            .contains(&SessionToggle::Fullscreen)
        {
            self.set_player_fullscreen(ui, false, false, false);
        }
        self.extras.details_visible = true;
        ui.show_player_details(&self.details_labels(), &self.player_details_text());
        ui.set_status(&self.text("video_details"), true);
    }

    /// Windows `hide_player_details`.
    pub(crate) fn hide_player_details(&mut self, ui: &mut dyn ShellUi) {
        if !self.extras.details_visible {
            return;
        }
        self.extras.details_visible = false;
        ui.hide_player_details();
        ui.set_status(&self.text("details_closed"), true);
    }

    /// Windows `copy_player_details`: copies freshly built text.
    pub(crate) fn copy_player_details(&mut self, ui: &mut dyn ShellUi) {
        if ui.copy_text(&self.player_details_text()) {
            ui.set_status(&self.text("details_copied"), true);
        }
    }

    /// Windows `sync_player_details` after speed, pitch or metadata changes.
    pub(crate) fn sync_player_details(&self, ui: &mut dyn ShellUi) {
        if self.view == View::Player && self.extras.details_visible {
            ui.set_player_details_text(&self.player_details_text());
        }
    }

    /// Windows `refresh_player` with `focus`: the details close, and open
    /// again when "show video details by default" is set.
    pub(crate) fn refresh_player_details(&mut self, ui: &mut dyn ShellUi, focus: PlayerFocus) {
        if focus != PlayerFocus::Initial {
            self.sync_player_details(ui);
            return;
        }
        if self.extras.details_visible {
            self.extras.details_visible = false;
            ui.hide_player_details();
        }
        if self.application.settings().show_video_details_by_default
            && !self
                .application
                .player_session()
                .enabled_toggles()
                .contains(&SessionToggle::Fullscreen)
        {
            self.extras.details_visible = true;
            ui.show_player_details(&self.details_labels(), &self.player_details_text());
            ui.set_status(&self.text("video_details"), true);
        }
    }

    /// Windows `handle_player_back_in_place`: Escape first hides the
    /// details, then leaves full screen. `true` when it did one of them.
    pub(crate) fn handle_player_back_in_place(&mut self, ui: &mut dyn ShellUi) -> bool {
        if self.view == View::Player && self.extras.details_visible {
            self.hide_player_details(ui);
            return true;
        }
        let fullscreen = self.view == View::Player
            && self
                .application
                .player_session()
                .enabled_toggles()
                .contains(&SessionToggle::Fullscreen);
        if fullscreen {
            self.set_player_fullscreen(ui, false, false, false);
        }
        fullscreen
    }

    /// Windows `show_output_devices`.
    pub(crate) fn show_output_devices(&mut self, ui: &mut dyn ShellUi) {
        let session = self.application.player_session();
        if !session.is_open() || !self.has_playback() {
            return;
        }
        let choices = apricot_app::audio_devices::player_device_choices(session.audio_devices());
        if choices.is_empty() {
            ui.set_status(&self.text("no_output_devices"), true);
            return;
        }
        let labels: Vec<String> = choices.iter().map(|choice| choice.label.clone()).collect();
        let prompt = self.text("select_output_device");
        let selected = ui.choose(&ChoiceDialog {
            title: self.text("output_devices"),
            list_name: prompt.clone(),
            prompt,
            choices: labels,
            initial: 0,
            ok: self.text("ok"),
            cancel: self.text("cancel"),
        });
        let Some(choice) = selected.and_then(|index| choices.get(index)) else {
            return;
        };
        match self
            .execute_player_command_result(PlaybackCommand::SetAudioDevice(choice.value.clone()))
        {
            Ok(()) => {
                self.application.set_player_output_device(&choice.value);
                self.apply_global_player_equalizer(ui);
                let message = self
                    .text("output_device_set")
                    .replace("{device}", &choice.label);
                ui.set_status(&message, true);
            }
            Err(error) => {
                let message = self.text("stream_url_failed").replace("{error}", &error);
                ui.set_status(&message, true);
            }
        }
    }

    /// Windows `schedule_saved_audio_device_check` after a visible start: the
    /// saved device is probed once, in the background.
    pub fn schedule_saved_audio_device_check(&mut self, ui: &mut dyn ShellUi) {
        let saved = apricot_app::audio_devices::normalized_device(
            &self.application.settings().audio_output_device,
        );
        if saved.eq_ignore_ascii_case(apricot_app::audio_devices::AUTO_DEVICE) {
            return;
        }
        let library = apricot_platform::mpv_locations().map(|(_, library)| library);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            std::thread::sleep(AUDIO_DEVICE_CHECK_DELAY);
            let devices = library.and_then(|library| {
                apricot_playback::probe_audio_output_devices(
                    &library,
                    None,
                    AUDIO_DEVICE_PROBE_TIMEOUT,
                )
                .ok()
            });
            let _ = sender.send(devices);
        });
        self.extras.pending_device_check = Some(receiver);
        ui.set_background_polling(true);
    }

    /// Windows `check_saved_audio_device`; `true` while it still waits.
    pub(crate) fn poll_saved_audio_device_check(&mut self, ui: &mut dyn ShellUi) -> bool {
        let Some(pending) = self.extras.pending_device_check.as_ref() else {
            return false;
        };
        let devices = match pending.try_recv() {
            Ok(Some(devices)) => devices,
            Err(TryRecvError::Empty) => return true,
            Ok(None) | Err(TryRecvError::Disconnected) => {
                self.extras.pending_device_check = None;
                return false;
            }
        };
        self.extras.pending_device_check = None;
        let options = self.application.store_probed_audio_devices(&devices);
        if self.application.saved_audio_device_missing(&options) {
            self.prompt_for_new_default_audio_device(ui, &options);
        }
        false
    }

    /// Windows `prompt_for_new_default_audio_device`.
    fn prompt_for_new_default_audio_device(
        &mut self,
        ui: &mut dyn ShellUi,
        options: &[apricot_app::SettingsChoiceOption],
    ) {
        let message = self.text("audio_device_missing");
        ui.show_error(&message);
        let selected = ui.choose(&ChoiceDialog {
            title: self.text("default_audio_device"),
            prompt: message.clone(),
            list_name: message,
            choices: options.iter().map(|option| option.label.clone()).collect(),
            initial: 0,
            ok: self.text("ok"),
            cancel: self.text("cancel"),
        });
        let chosen =
            selected.and_then(|index| options.get(index).map(|option| option.value.clone()));
        let device = chosen
            .clone()
            .unwrap_or_else(|| apricot_app::audio_devices::AUTO_DEVICE.to_owned());
        if let Err(error) = self.application.set_default_audio_output_device(&device) {
            ui.set_status(&format!("Settings were not saved: {error}"), true);
            return;
        }
        if chosen.is_some() {
            ui.set_status(&self.text("settings_saved"), true);
        }
    }

    /// Windows `toggle_player_fullscreen`.
    pub(crate) fn toggle_player_fullscreen(
        &mut self,
        ui: &mut dyn ShellUi,
        focus_checkbox: bool,
        announce: bool,
    ) {
        let enabled = !self
            .application
            .player_session()
            .enabled_toggles()
            .contains(&SessionToggle::Fullscreen);
        self.set_player_fullscreen(ui, enabled, focus_checkbox, announce);
    }

    /// Windows `set_player_fullscreen`.
    pub(crate) fn set_player_fullscreen(
        &mut self,
        ui: &mut dyn ShellUi,
        enabled: bool,
        focus_checkbox: bool,
        announce: bool,
    ) {
        if enabled && !self.application.player_session().is_open() {
            ui.set_status(&self.text("no_player"), true);
            if self.view == View::Player {
                self.refresh_player(ui, PlayerFocus::Preserve);
            }
            return;
        }
        self.application
            .set_player_toggle(SessionToggle::Fullscreen, enabled);
        if self.view == View::Player {
            self.refresh_player(ui, PlayerFocus::Keep);
            ui.focus_player_control(if focus_checkbox {
                "fullscreen"
            } else {
                "video_host"
            });
        } else {
            self.show_current_player_screen(ui);
        }
        if announce {
            let key = if enabled {
                "fullscreen_on"
            } else {
                "fullscreen_off"
            };
            ui.set_status(&self.text(key), true);
        }
    }

    /// Windows `sync_window_fullscreen`: the window fills the screen while
    /// the player page is shown in full screen mode.
    pub(crate) fn sync_window_fullscreen(&self, ui: &mut dyn ShellUi) {
        let session = self.application.player_session();
        let wanted = self.view == View::Player
            && session.is_open()
            && session
                .enabled_toggles()
                .contains(&SessionToggle::Fullscreen);
        ui.set_window_fullscreen(wanted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_default_details_wait_for_explicit_f7() {
        use apricot_core::{setting::SettingId, shortcut::ShortcutChord};
        let mut shell = crate::tests::keyboard::shell();
        shell
            .application
            .set_boolean_setting(SettingId::ShowVideoDetailsByDefault, true)
            .unwrap();
        shell
            .application
            .start_player_item(crate::local_file_item(std::path::Path::new("fixture.mp4")));
        shell
            .application
            .set_player_toggle(SessionToggle::Fullscreen, true);
        shell.view = View::Player;
        let mut ui = crate::tests::equalizer::Ui::default();
        shell.refresh_player(&mut ui, PlayerFocus::Initial);
        assert!(ui.fullscreen);
        assert_eq!(
            ui.details_shown, 0,
            "automatic details must not take hidden focus"
        );
        let route = shell.route_key(
            ShortcutChord::parse("F7").unwrap(),
            false,
            crate::FocusKind::PlayerSurface,
        );
        assert!(matches!(route, crate::KeyRoute::Run("player_details")));
        shell.activate_action(&mut ui, "player_details");
        assert!(!ui.fullscreen);
        assert!(
            !shell
                .application
                .player_session()
                .enabled_toggles()
                .contains(&SessionToggle::Fullscreen)
        );
        assert_eq!(ui.details_shown, 1);
        assert!(shell.extras.details_visible);
        shell
            .application
            .set_player_toggle(SessionToggle::Fullscreen, false);
        shell.refresh_player(&mut ui, PlayerFocus::Initial);
        assert_eq!(
            ui.details_shown, 2,
            "ordinary player still opens default details"
        );
    }

    #[test]
    fn a_disconnected_device_probe_keeps_the_saved_output() {
        let mut shell = crate::tests::keyboard::shell();
        shell
            .application
            .set_default_audio_output_device("saved-device")
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        shell.extras.pending_device_check = Some(receiver);
        drop(sender);
        let mut ui = crate::tests::equalizer::Ui::default();
        assert!(!shell.poll_saved_audio_device_check(&mut ui));
        assert_eq!(
            shell.application.settings().audio_output_device,
            "saved-device"
        );
    }

    #[test]
    fn a_failed_device_probe_keeps_the_saved_output() {
        let mut shell = crate::tests::keyboard::shell();
        shell
            .application
            .set_default_audio_output_device("saved-device")
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        shell.extras.pending_device_check = Some(receiver);
        sender.send(None).unwrap();
        let mut ui = crate::tests::equalizer::Ui::default();
        assert!(!shell.poll_saved_audio_device_check(&mut ui));
        assert_eq!(
            shell.application.settings().audio_output_device,
            "saved-device"
        );
    }

    #[test]
    fn a_successful_probe_still_handles_a_missing_device() {
        let mut shell = crate::tests::keyboard::shell();
        shell
            .application
            .set_default_audio_output_device("saved-device")
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        shell.extras.pending_device_check = Some(receiver);
        sender.send(Some(Vec::new())).unwrap();
        let mut ui = crate::tests::equalizer::Ui::default();
        assert!(!shell.poll_saved_audio_device_check(&mut ui));
        assert_eq!(shell.application.settings().audio_output_device, "auto");
    }
}
