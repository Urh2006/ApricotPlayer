//! The player equalizer dialog (F4) and its profile prompts, ported from
//! `equalizer_win32.rs` (Python `show_player_equalizer` and its helpers).
//!
//! The native dialog shows the controls and forwards every change here; the
//! shell keeps the dialog state and talks to the application. Changes are
//! heard at once: the edited equalizer becomes the session equalizer and is
//! applied after `APPLY_DELAY_MS`. OK keeps it for this player session,
//! Cancel, Escape and closing restore the previous one.

use std::path::PathBuf;

use apricot_app::{
    EqualizerSession,
    equalizer::{self, EqualizerGains, EqualizerSettings, RANGE_OPTIONS},
};
use apricot_core::audio::EQUALIZER_BANDS;
use apricot_playback::PlaybackCommand;

use crate::{ChoiceDialog, Shell, ShellUi};

/// The dialog buttons in Python's creation order, which is the Tab order,
/// with their label keys.
pub const EQUALIZER_BUTTONS: [(EqualizerButton, &str); 11] = [
    (EqualizerButton::Ok, "ok"),
    (EqualizerButton::Cancel, "cancel"),
    (EqualizerButton::Reset, "reset_equalizer"),
    (EqualizerButton::SaveGlobal, "save_equalizer_as_global"),
    (EqualizerButton::AddProfile, "add_equalizer_profile"),
    (EqualizerButton::DeleteProfile, "delete_equalizer_profile"),
    (EqualizerButton::ImportProfile, "import_equalizer_profile"),
    (EqualizerButton::ExportProfile, "export_equalizer_profile"),
    (EqualizerButton::Compare, "compare_equalizer_profile"),
    (
        EqualizerButton::SaveDevice,
        "save_equalizer_as_device_default",
    ),
    (
        EqualizerButton::ClearDevice,
        "clear_equalizer_device_default",
    ),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EqualizerButton {
    Ok,
    Cancel,
    Reset,
    SaveGlobal,
    AddProfile,
    DeleteProfile,
    ImportProfile,
    ExportProfile,
    Compare,
    SaveDevice,
    ClearDevice,
}

/// The texts of the equalizer dialog: the window title, the labels of the
/// preset, range and name controls, one per band and one per button (in
/// [`EQUALIZER_BUTTONS`] order).
pub struct EqualizerLabels {
    pub title: String,
    pub preset: String,
    pub range: String,
    pub name: String,
    pub bands: Vec<String>,
    pub buttons: Vec<String>,
}

/// The native equalizer dialog.
pub trait EqualizerView {
    /// The preset choice: labels and the selected row.
    fn set_presets(&mut self, labels: &[String], selected: usize);
    /// The selected preset row.
    fn selected_preset(&self) -> Option<usize>;
    /// The dB range choice row (in [`RANGE_OPTIONS`]).
    fn set_range(&mut self, index: usize);
    fn selected_range(&self) -> Option<usize>;
    /// The custom preset name field: its text and whether it (and its
    /// label) are shown; the Delete profile button follows `visible`.
    fn set_name(&mut self, text: &str);
    fn name(&self) -> String;
    fn set_name_visible(&mut self, visible: bool);
    /// One band slider: range ±`limit` tenths, the position, its name and
    /// value text; `notify` speaks the value change.
    fn set_slider(
        &mut self,
        index: usize,
        limit: i32,
        tenths: i32,
        label: &str,
        value: &str,
        notify: bool,
    );
    /// Schedules [`Shell::equalizer_apply_now`] after `delay_ms`, replacing
    /// a scheduled one.
    fn schedule_apply(&mut self, delay_ms: u32);
    fn cancel_apply(&mut self);
    /// Ends the dialog (OK, Cancel or closing).
    fn close(&mut self);
    /// A modal message with the dialog's title and `buttons`, the first one
    /// the default; returns the index of the pressed button.
    fn message(&mut self, title: &str, message: &str, buttons: &[&str]) -> usize;
    /// Windows `prompt_name`: a text prompt, `None` when cancelled.
    fn prompt_name(&mut self, title: &str, prompt: &str, ok: &str, cancel: &str) -> Option<String>;
    /// A list choice inside the dialog.
    fn choose(&mut self, dialog: &ChoiceDialog) -> Option<usize>;
    /// Windows `choose_json_file` and `save_json_file`.
    fn choose_json_file(&mut self, title: &str, type_label: &str) -> Option<PathBuf>;
    fn save_json_file(&mut self, title: &str, file_name: &str, type_label: &str)
    -> Option<PathBuf>;
}

/// Windows `DialogState` without the native controls.
pub(crate) struct EqualizerDialogState {
    preset_options: Vec<String>,
    visible_preset: String,
    gains: EqualizerGains,
    db_range: i64,
    original_equalizer: Option<EqualizerSession>,
    original_db_range: i64,
    compare_showing_original: bool,
    accepted: bool,
}

/// The slider range in tenths of a decibel, both ways.
fn slider_limit(db_range: i64) -> i32 {
    i32::try_from(db_range * 10).unwrap_or(120)
}

fn band_gain(gains: &EqualizerGains, index: usize) -> f64 {
    EQUALIZER_BANDS
        .get(index)
        .and_then(|band| gains.get(band.id).copied())
        .unwrap_or_default()
}

impl Shell {
    fn equalizer_settings(&self) -> EqualizerSettings {
        self.application.equalizer_settings()
    }

    /// Windows `PlayerEqualizerHost::update_settings`.
    fn update_equalizer_settings(
        &mut self,
        settings: &EqualizerSettings,
        save: bool,
    ) -> Result<(), String> {
        self.application
            .set_equalizer_settings(settings)
            .and_then(|()| {
                if save {
                    self.application.save_settings()
                } else {
                    Ok(())
                }
            })
            .map_err(|error| error.to_string())
    }

    /// Windows `PlayerEqualizerHost::apply_to_player` (`apply_player_equalizer`).
    pub(crate) fn apply_player_equalizer(&self, ui: &mut dyn ShellUi) {
        if !self.application.player_session().is_open() || !self.has_playback() {
            return;
        }
        let graph = self.application.player_equalizer_graph(None);
        let _ = self.execute_player_command(ui, PlaybackCommand::SetEqualizerFilter(graph));
    }

    /// Windows `show_player_equalizer`: silent without a running player.
    pub(crate) fn show_player_equalizer(&mut self, ui: &mut dyn ShellUi) {
        if !self.application.player_session().is_open() || !self.has_playback() {
            return;
        }
        self.stop_key_repeat(ui);
        ui.open_equalizer();
    }

    /// The texts of the dialog's window and controls.
    #[must_use]
    pub fn equalizer_labels(&self) -> EqualizerLabels {
        let catalog = self.catalog();
        EqualizerLabels {
            title: catalog.text("equalizer").to_owned(),
            preset: catalog.text("equalizer_preset").to_owned(),
            range: catalog.text("equalizer_db_range").to_owned(),
            name: catalog.text("equalizer_preset_name").to_owned(),
            bands: EQUALIZER_BANDS
                .iter()
                .map(|band| equalizer::band_gain_label(&catalog, band.id))
                .collect(),
            buttons: EQUALIZER_BUTTONS
                .iter()
                .map(|(_, key)| catalog.text(key).to_owned())
                .collect(),
        }
    }

    /// Windows `show_win32` and `initialize_controls`: the effective preset,
    /// the saved dB range and the sliders limited to it.
    pub fn equalizer_begin(&mut self, view: &mut dyn EqualizerView) {
        let settings = self.equalizer_settings();
        let device = self.application.equalizer_device_key();
        let original_equalizer = self
            .application
            .player_session()
            .audio()
            .and_then(|audio| audio.equalizer.clone());
        let (_, gains) = settings.base_state(original_equalizer.as_ref(), &device);
        let active_preset = settings.effective_preset(&device);
        let db_range = settings.db_range();
        self.equalizer = Some(EqualizerDialogState {
            preset_options: Vec::new(),
            visible_preset: active_preset.clone(),
            gains: equalizer::normalized_gains(&gains),
            db_range,
            original_equalizer,
            original_db_range: db_range,
            compare_showing_original: false,
            accepted: false,
        });
        self.fill_preset_choices(view, &settings, &active_preset);
        let range_index = RANGE_OPTIONS
            .iter()
            .position(|option| *option == db_range.to_string())
            .unwrap_or(1);
        view.set_range(range_index);
        view.set_name(&settings.custom_name(&active_preset));
        let catalog = self.catalog();
        if let Some(state) = self.equalizer.as_ref() {
            let limit = slider_limit(state.db_range);
            for (index, band) in EQUALIZER_BANDS.iter().enumerate() {
                let gain = state.gains.get(band.id).copied().unwrap_or_default();
                let tenths = equalizer::slider_from_gain(gain, state.db_range);
                view.set_slider(
                    index,
                    limit,
                    tenths,
                    &equalizer::band_gain_label(&catalog, band.id),
                    &equalizer::slider_value_text(tenths),
                    false,
                );
            }
        }
        self.update_custom_name_visibility(view);
    }

    fn fill_preset_choices(
        &mut self,
        view: &mut dyn EqualizerView,
        settings: &EqualizerSettings,
        selected: &str,
    ) {
        let options = settings.preset_options();
        let index = options
            .iter()
            .position(|option| option == selected)
            .unwrap_or(0);
        view.set_presets(&settings.preset_labels(&self.catalog()), index);
        if let Some(state) = self.equalizer.as_mut() {
            state.preset_options = options;
        }
    }

    /// Windows `show_gains`: the sliders show `gains` limited to the dialog
    /// range, and the dialog keeps the limited values.
    fn show_gains(&mut self, view: &mut dyn EqualizerView, gains: &EqualizerGains) {
        let catalog = self.catalog();
        let Some(state) = self.equalizer.as_mut() else {
            return;
        };
        let limit = slider_limit(state.db_range);
        for (index, band) in EQUALIZER_BANDS.iter().enumerate() {
            let tenths = equalizer::slider_from_gain(
                gains.get(band.id).copied().unwrap_or_default(),
                state.db_range,
            );
            state
                .gains
                .insert(band.id.to_owned(), equalizer::gain_from_slider(tenths));
            view.set_slider(
                index,
                limit,
                tenths,
                &equalizer::band_gain_label(&catalog, band.id),
                &equalizer::slider_value_text(tenths),
                false,
            );
        }
    }

    fn current_preset(&self, view: &dyn EqualizerView) -> String {
        let Some(state) = self.equalizer.as_ref() else {
            return equalizer::FLAT_PRESET.to_owned();
        };
        view.selected_preset()
            .and_then(|index| state.preset_options.get(index).cloned())
            .unwrap_or_else(|| equalizer::FLAT_PRESET.to_owned())
    }

    fn update_custom_name_visibility(&self, view: &mut dyn EqualizerView) {
        view.set_name_visible(equalizer::is_custom_preset(&self.current_preset(view)));
    }

    /// Windows `save_dialog_name`.
    fn save_dialog_name(&self, view: &dyn EqualizerView, settings: &mut EqualizerSettings) {
        let Some(state) = self.equalizer.as_ref() else {
            return;
        };
        let preset_id = settings.normalized_preset(&state.visible_preset);
        settings.set_custom_name(&preset_id, &view.name());
    }

    /// Windows `live_apply`.
    fn live_apply(&mut self, view: &mut dyn EqualizerView) {
        let Some(state) = self.equalizer.as_ref() else {
            return;
        };
        self.application
            .set_player_equalizer(Some(EqualizerSession {
                enabled: true,
                gains: equalizer::normalized_gains(&state.gains),
            }));
        view.schedule_apply(equalizer::APPLY_DELAY_MS);
    }

    /// The scheduled apply ran (Windows `APPLY_TIMER_ID`).
    pub fn equalizer_apply_now(&mut self, ui: &mut dyn ShellUi) {
        self.apply_player_equalizer(ui);
    }

    /// Windows `slider_changed`.
    pub fn equalizer_slider_changed(
        &mut self,
        view: &mut dyn EqualizerView,
        index: usize,
        tenths: i32,
    ) {
        let catalog = self.catalog();
        let Some(state) = self.equalizer.as_mut() else {
            return;
        };
        let Some(band) = EQUALIZER_BANDS.get(index) else {
            return;
        };
        let gain = equalizer::gain_from_slider(tenths);
        let previous = band_gain(&state.gains, index);
        state.gains.insert(band.id.to_owned(), gain);
        let limit = slider_limit(state.db_range);
        view.set_slider(
            index,
            limit,
            tenths,
            &equalizer::band_gain_label(&catalog, band.id),
            &equalizer::slider_value_text(tenths),
            (previous - gain).abs() >= 0.05,
        );
        self.live_apply(view);
    }

    /// Windows `load_preset`.
    fn load_preset(&mut self, view: &mut dyn EqualizerView, preset_id: &str) {
        let settings = self.equalizer_settings();
        let gains = settings.gains_for_preset(preset_id);
        if let Some(state) = self.equalizer.as_mut() {
            state.visible_preset = settings.normalized_preset(preset_id);
            state.gains = equalizer::normalized_gains(&gains);
        }
        self.show_gains(view, &gains);
        view.set_name(&settings.custom_name(preset_id));
        self.update_custom_name_visibility(view);
        self.live_apply(view);
    }

    /// Windows `preset_changed`.
    pub fn equalizer_preset_changed(&mut self, view: &mut dyn EqualizerView) {
        let mut settings = self.equalizer_settings();
        self.save_dialog_name(view, &mut settings);
        let _ = self.update_equalizer_settings(&settings, false);
        let preset = self.current_preset(view);
        self.load_preset(view, &preset);
    }

    /// Windows `range_changed`.
    pub fn equalizer_range_changed(&mut self, view: &mut dyn EqualizerView) {
        let Some(state) = self.equalizer.as_mut() else {
            return;
        };
        let value = view
            .selected_range()
            .and_then(|index| RANGE_OPTIONS.get(index))
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(state.original_db_range);
        state.db_range = value.clamp(6, 24);
        let gains = state.gains.clone();
        self.show_gains(view, &gains);
        self.live_apply(view);
    }

    /// Windows `refresh_preset_choices`.
    fn refresh_preset_choices(&mut self, view: &mut dyn EqualizerView, selected: &str) {
        let settings = self.equalizer_settings();
        if let Some(state) = self.equalizer.as_mut() {
            state.visible_preset = settings.normalized_preset(selected);
        }
        self.fill_preset_choices(view, &settings, selected);
        self.update_custom_name_visibility(view);
    }

    fn equalizer_error(&self, view: &mut dyn EqualizerView, message: &str) {
        view.message(&self.text("equalizer"), message, &[&self.text("ok")]);
    }

    /// A dialog button (Windows `button_clicked`).
    pub fn equalizer_button(
        &mut self,
        ui: &mut dyn ShellUi,
        view: &mut dyn EqualizerView,
        button: EqualizerButton,
    ) {
        match button {
            EqualizerButton::Ok => self.equalizer_accept(view),
            EqualizerButton::Cancel => view.close(),
            EqualizerButton::Reset => {
                let gains = equalizer::factory_gains(&self.current_preset(view));
                if let Some(state) = self.equalizer.as_mut() {
                    state.gains = equalizer::normalized_gains(&gains);
                }
                self.show_gains(view, &gains);
                self.live_apply(view);
            }
            EqualizerButton::SaveGlobal => self.equalizer_save_global(ui, view),
            EqualizerButton::AddProfile => {
                let gains = self.dialog_gains();
                if let Some(preset_id) = self.create_profile_with_prompt(ui, view, &gains) {
                    self.refresh_preset_choices(view, &preset_id);
                    view.set_name(&self.equalizer_settings().custom_name(&preset_id));
                    self.live_apply(view);
                }
            }
            EqualizerButton::DeleteProfile => {
                let preset_id = self.current_preset(view);
                if let Some(replacement) = self.delete_profile_with_prompt(ui, view, &preset_id) {
                    self.refresh_preset_choices(view, &replacement);
                    self.load_preset(view, &replacement);
                }
            }
            EqualizerButton::ImportProfile => {
                if let Some(preset_id) = self.import_profile_with_prompt(ui, view) {
                    self.refresh_preset_choices(view, &preset_id);
                    self.load_preset(view, &preset_id);
                }
            }
            EqualizerButton::ExportProfile => self.equalizer_export(ui, view),
            EqualizerButton::Compare => self.equalizer_compare(ui, view),
            EqualizerButton::SaveDevice => self.equalizer_save_device(ui, view),
            EqualizerButton::ClearDevice => self.equalizer_clear_device(ui, view),
        }
    }

    fn dialog_gains(&self) -> EqualizerGains {
        self.equalizer
            .as_ref()
            .map(|state| equalizer::normalized_gains(&state.gains))
            .unwrap_or_default()
    }

    /// Windows `accept`: the name and dB range are saved.
    fn equalizer_accept(&mut self, view: &mut dyn EqualizerView) {
        let mut settings = self.equalizer_settings();
        self.save_dialog_name(view, &mut settings);
        if let Some(state) = self.equalizer.as_ref() {
            settings.db_range = state.db_range;
        }
        if let Err(error) = self.update_equalizer_settings(&settings, true) {
            self.equalizer_error(view, &error);
            return;
        }
        if let Some(state) = self.equalizer.as_mut() {
            state.accepted = true;
        }
        view.close();
    }

    /// Windows `finish`: after the dialog closed.
    pub fn equalizer_finish(&mut self, ui: &mut dyn ShellUi) {
        let Some(state) = self.equalizer.take() else {
            return;
        };
        if state.accepted {
            ui.set_status(&self.text("equalizer_saved"), true);
            return;
        }
        let mut settings = self.equalizer_settings();
        settings.db_range = state.original_db_range;
        let _ = self.update_equalizer_settings(&settings, false);
        self.application
            .set_player_equalizer(state.original_equalizer);
        self.apply_player_equalizer(ui);
        ui.set_status(&self.text("equalizer_closed"), true);
    }

    fn equalizer_export(&mut self, ui: &mut dyn ShellUi, view: &mut dyn EqualizerView) {
        let mut settings = self.equalizer_settings();
        self.save_dialog_name(view, &mut settings);
        let _ = self.update_equalizer_settings(&settings, false);
        let preset_id = self.current_preset(view);
        let name = if equalizer::is_custom_preset(&preset_id) {
            view.name().trim().to_owned()
        } else {
            settings.preset_label(&self.catalog(), &preset_id)
        };
        let gains = self.dialog_gains();
        self.export_profile_with_prompt(ui, view, &name, &gains, &preset_id);
    }

    fn equalizer_compare(&mut self, ui: &mut dyn ShellUi, view: &mut dyn EqualizerView) {
        let Some(state) = self.equalizer.as_mut() else {
            return;
        };
        state.compare_showing_original = !state.compare_showing_original;
        if state.compare_showing_original {
            let original = state.original_equalizer.clone();
            view.cancel_apply();
            self.application.set_player_equalizer(original);
            self.apply_player_equalizer(ui);
            ui.set_status(&self.text("equalizer_compare_original"), true);
            return;
        }
        self.live_apply(view);
        ui.set_status(&self.text("equalizer_compare_current"), true);
    }

    fn equalizer_save_global(&mut self, ui: &mut dyn ShellUi, view: &mut dyn EqualizerView) {
        let mut settings = self.equalizer_settings();
        self.save_dialog_name(view, &mut settings);
        let _ = self.update_equalizer_settings(&settings, false);
        let gains = self.dialog_gains();
        let Some(preset_id) = self.choose_profile_for_save(ui, view, &gains) else {
            return;
        };
        let mut settings = self.equalizer_settings();
        settings.enabled = true;
        settings.preset.clone_from(&preset_id);
        if let Err(error) = self.update_equalizer_settings(&settings, true) {
            self.equalizer_error(view, &error);
            return;
        }
        self.refresh_preset_choices(view, &preset_id);
        ui.set_status(&self.text("equalizer_profile_saved"), true);
    }

    fn equalizer_save_device(&mut self, ui: &mut dyn ShellUi, view: &mut dyn EqualizerView) {
        let mut settings = self.equalizer_settings();
        self.save_dialog_name(view, &mut settings);
        let gains = self.dialog_gains();
        let mut preset_id = self.current_preset(view);
        if equalizer::is_custom_preset(&preset_id) {
            settings.set_preset_gains(&preset_id, &gains);
            let _ = self.update_equalizer_settings(&settings, false);
        } else {
            let _ = self.update_equalizer_settings(&settings, false);
            if !equalizer::gains_match(&gains, &settings.gains_for_preset(&preset_id)) {
                let Some(chosen) = self.choose_profile_for_save(ui, view, &gains) else {
                    return;
                };
                preset_id = chosen;
            }
        }
        let device = self.application.equalizer_device_key();
        let mut settings = self.equalizer_settings();
        settings.enabled = true;
        settings.set_device_preset(&device, &preset_id);
        if let Err(error) = self.update_equalizer_settings(&settings, true) {
            self.equalizer_error(view, &error);
            return;
        }
        view.cancel_apply();
        self.application.set_player_equalizer(None);
        self.apply_player_equalizer(ui);
        let message = self
            .text("equalizer_device_profile_saved")
            .replace("{device}", &equalizer::device_display_name(&device))
            .replace(
                "{preset}",
                &settings.preset_label(&self.catalog(), &preset_id),
            );
        ui.set_status(&message, true);
    }

    fn equalizer_clear_device(&mut self, ui: &mut dyn ShellUi, view: &mut dyn EqualizerView) {
        let device = self.application.equalizer_device_key();
        let mut settings = self.equalizer_settings();
        settings.set_device_preset(&device, "");
        if let Err(error) = self.update_equalizer_settings(&settings, true) {
            self.equalizer_error(view, &error);
            return;
        }
        view.cancel_apply();
        self.application.set_player_equalizer(None);
        self.apply_player_equalizer(ui);
        let message = self
            .text("equalizer_device_profile_cleared")
            .replace("{device}", &equalizer::device_display_name(&device));
        ui.set_status(&message, true);
    }

    /// Windows `create_profile_with_prompt`.
    pub(crate) fn create_profile_with_prompt(
        &mut self,
        ui: &mut dyn ShellUi,
        view: &mut dyn EqualizerView,
        gains: &EqualizerGains,
    ) -> Option<String> {
        let name = view.prompt_name(
            &self.text("add_equalizer_profile"),
            &self.text("equalizer_profile_name"),
            &self.text("ok"),
            &self.text("cancel"),
        )?;
        let mut settings = self.equalizer_settings();
        let preset_id = settings.create_profile(&name, gains);
        settings.preset.clone_from(&preset_id);
        if let Err(error) = self.update_equalizer_settings(&settings, true) {
            self.equalizer_error(view, &error);
            return None;
        }
        ui.set_status(&self.text("equalizer_profile_saved"), true);
        Some(preset_id)
    }

    /// Windows `choose_profile_for_save`.
    fn choose_profile_for_save(
        &mut self,
        ui: &mut dyn ShellUi,
        view: &mut dyn EqualizerView,
        gains: &EqualizerGains,
    ) -> Option<String> {
        let settings = self.equalizer_settings();
        let profile_ids = settings.custom_ids();
        let mut labels: Vec<String> = profile_ids
            .iter()
            .map(|id| settings.custom_name(id))
            .collect();
        labels.push(self.text("add_equalizer_profile"));
        let prompt = self.text("save_equalizer_as_global");
        let selection = view.choose(&ChoiceDialog {
            title: self.text("equalizer"),
            list_name: prompt.clone(),
            prompt,
            choices: labels,
            initial: 0,
            ok: self.text("ok"),
            cancel: self.text("cancel"),
        })?;
        if selection == profile_ids.len() {
            return self.create_profile_with_prompt(ui, view, gains);
        }
        let preset_id = profile_ids.get(selection)?.clone();
        let mut settings = self.equalizer_settings();
        settings.set_preset_gains(&preset_id, gains);
        if let Err(error) = self.update_equalizer_settings(&settings, true) {
            self.equalizer_error(view, &error);
            return None;
        }
        Some(preset_id)
    }

    /// Windows `import_profile_with_prompt`.
    pub(crate) fn import_profile_with_prompt(
        &mut self,
        ui: &mut dyn ShellUi,
        view: &mut dyn EqualizerView,
    ) -> Option<String> {
        let path = view.choose_json_file(
            &self.text("import_equalizer_profile"),
            &self.text("equalizer_profile_file"),
        )?;
        let mut settings = self.equalizer_settings();
        let invalid = self.text("equalizer_profile_invalid");
        let imported = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}'))
                    .map_err(|error| error.to_string())
            })
            .and_then(|payload| settings.profile_from_payload(&payload, &invalid))
            .and_then(|(name, gains)| {
                let preset_id = settings.create_profile(&name, &gains);
                settings.preset.clone_from(&preset_id);
                settings.global_gains = equalizer::normalized_gains(&gains);
                self.update_equalizer_settings(&settings, true)
                    .map(|()| (name, preset_id))
            });
        match imported {
            Ok((name, preset_id)) => {
                ui.set_status(
                    &self
                        .text("equalizer_profile_imported")
                        .replace("{name}", &name),
                    true,
                );
                Some(preset_id)
            }
            Err(error) => {
                let message = self
                    .text("equalizer_profile_import_failed")
                    .replace("{error}", &error);
                self.equalizer_error(view, &message);
                None
            }
        }
    }

    /// Windows `export_profile_with_prompt`.
    pub(crate) fn export_profile_with_prompt(
        &mut self,
        ui: &mut dyn ShellUi,
        view: &mut dyn EqualizerView,
        name: &str,
        gains: &EqualizerGains,
        preset_id: &str,
    ) {
        let Some(path) = view.save_json_file(
            &self.text("export_equalizer_profile"),
            &equalizer::export_file_name(name),
            &self.text("equalizer_profile_file"),
        ) else {
            return;
        };
        let path = equalizer::export_path(&path);
        let payload = self
            .equalizer_settings()
            .export_payload(name, gains, preset_id);
        let written = serde_json::to_string_pretty(&payload)
            .map_err(|error| error.to_string())
            .and_then(|text| std::fs::write(&path, text).map_err(|error| error.to_string()));
        match written {
            Ok(()) => ui.set_status(
                &self
                    .text("equalizer_profile_exported")
                    .replace("{path}", &path.to_string_lossy()),
                true,
            ),
            Err(error) => {
                let message = self
                    .text("equalizer_profile_export_failed")
                    .replace("{error}", &error);
                self.equalizer_error(view, &message);
            }
        }
    }

    /// Windows `delete_profile_with_prompt`.
    pub(crate) fn delete_profile_with_prompt(
        &mut self,
        ui: &mut dyn ShellUi,
        view: &mut dyn EqualizerView,
        preset_id: &str,
    ) -> Option<String> {
        let mut settings = self.equalizer_settings();
        if !equalizer::is_custom_preset(&settings.normalized_preset(preset_id)) {
            return None;
        }
        let (yes, no) = (self.text("yes"), self.text("no"));
        if view.message(
            &self.text("equalizer"),
            &self.text("equalizer_profile_delete_confirm"),
            &[&yes, &no],
        ) != 0
        {
            return None;
        }
        let replacement = settings.delete_profile(preset_id)?;
        if let Err(error) = self.update_equalizer_settings(&settings, true) {
            self.equalizer_error(view, &error);
            return None;
        }
        ui.set_status(&self.text("equalizer_profile_deleted"), true);
        Some(replacement)
    }
}
