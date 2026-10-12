//! The Settings window's decisions (Windows `settings_win32.rs`): how each
//! model control is bound to a setting, applying the visible values, Save,
//! Restore to defaults, resetting a section, capturing a shortcut, the
//! equalizer effects on a running player and refreshing the main window
//! when the window closes. The native window only shows the controls and
//! reports their values.

use apricot_app::equalizer::EqualizerGains;
use apricot_app::{
    SettingsChoiceOption, SettingsCommand, SettingsControl, SettingsScreenModel, SettingsValueType,
    ShortcutActionItem, SpotifyField,
};
use apricot_core::shortcut::{ShortcutChord, ShortcutKey};
use apricot_core::{SettingId, SettingsSection};

use crate::{MainMenuSelection, PlayerFocus, Shell, ShellUi, View};

/// What a control changes (Windows `ControlBinding`).
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsBinding {
    ReadOnly,
    Text(SettingId),
    Integer {
        setting: SettingId,
        minimum: i64,
        maximum: i64,
    },
    IntegerSlider {
        setting: SettingId,
        label: String,
        unit: &'static str,
    },
    Choice {
        setting: SettingId,
        value_type: SettingsValueType,
        values: Vec<String>,
    },
    Checkbox(SettingId),
    MenuItem(&'static str),
    EqualizerDevicePreset {
        device_id: String,
        values: Vec<String>,
    },
    EqualizerPresetName(String),
    EqualizerBand {
        preset_id: String,
        band_id: &'static str,
        label: String,
    },
    ShortcutActionList(Vec<ShortcutActionItem>),
    ShortcutCapture(String),
    Command(SettingsCommand),
    SpotifyChoice(SpotifyField),
    SpotifyNormalisation,
}

impl SettingsBinding {
    /// The setting of a plain control (Windows `binding_setting`).
    #[must_use]
    pub const fn setting(&self) -> Option<SettingId> {
        match self {
            Self::Text(setting)
            | Self::Checkbox(setting)
            | Self::Integer { setting, .. }
            | Self::IntegerSlider { setting, .. }
            | Self::Choice { setting, .. } => Some(*setting),
            _ => None,
        }
    }

    /// Windows `dependent_setting`: a change that rebuilds the section and
    /// focuses the control again.
    #[must_use]
    pub const fn dependent_setting(&self) -> Option<SettingId> {
        match self {
            Self::Checkbox(
                setting @ (SettingId::GlobalEqualizerEnabled
                | SettingId::ShowAdvancedNetworkSettings
                | SettingId::VolumeBoostByDefault),
            )
            | Self::Choice {
                setting: setting @ (SettingId::GlobalEqualizerPreset | SettingId::EqualizerDbRange),
                ..
            } => Some(*setting),
            _ => None,
        }
    }
}

/// The value a control shows (Windows `window_text`, `CB_GETCURSEL`,
/// `BM_GETCHECK` and `TBM_GETPOS`).
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsInput {
    Text(String),
    Selected(Option<usize>),
    Checked(bool),
    Position(i64),
    /// Buttons and lists change nothing by themselves.
    None,
}

/// What capturing a shortcut did (Windows `capture_shortcut`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShortcutCapture {
    /// A modifier alone or a key the shortcuts do not use.
    Ignored,
    /// Saved: the stored text and the announcement.
    Captured {
        shortcut: String,
        message: String,
    },
    /// In use by another action: a warning with this title, then the same
    /// sentence spoken.
    Conflict {
        title: String,
        message: String,
    },
    Failed(String),
}

/// Windows `shortcut_action_label`, with the macOS key names on a Mac.
#[must_use]
pub fn shortcut_action_label(action: &ShortcutActionItem) -> String {
    format!(
        "{}: {}",
        action.label,
        apricot_core::shortcut::display_shortcut(&action.shortcut)
    )
}

/// Windows `shortcut_from_virtual_key`: the stored text of a chord.
#[must_use]
pub fn shortcut_text(chord: ShortcutChord) -> Option<String> {
    let key = match chord.key {
        ShortcutKey::Enter => "Enter".to_owned(),
        ShortcutKey::Space => "Space".to_owned(),
        ShortcutKey::Escape => "Escape".to_owned(),
        ShortcutKey::Delete => "Delete".to_owned(),
        ShortcutKey::Backspace => "Backspace".to_owned(),
        ShortcutKey::Insert => "Insert".to_owned(),
        ShortcutKey::Home => "Home".to_owned(),
        ShortcutKey::End => "End".to_owned(),
        ShortcutKey::PageUp => "PageUp".to_owned(),
        ShortcutKey::PageDown => "PageDown".to_owned(),
        ShortcutKey::Left => "Left".to_owned(),
        ShortcutKey::Right => "Right".to_owned(),
        ShortcutKey::Up => "Up".to_owned(),
        ShortcutKey::Down => "Down".to_owned(),
        ShortcutKey::Applications => "Applications".to_owned(),
        ShortcutKey::LeftBracket => "LeftBracket".to_owned(),
        ShortcutKey::RightBracket => "RightBracket".to_owned(),
        ShortcutKey::Function(number) if (1..=24).contains(&number) => format!("F{number}"),
        ShortcutKey::Character(character) if character.is_ascii_alphanumeric() => {
            character.to_ascii_uppercase().to_string()
        }
        ShortcutKey::Function(_) | ShortcutKey::Character(_) => return None,
    };
    let mut parts = Vec::with_capacity(4);
    if chord.control {
        parts.push("Ctrl".to_owned());
    }
    if chord.shift {
        parts.push("Shift".to_owned());
    }
    if chord.alt {
        parts.push("Alt".to_owned());
    }
    parts.push(key);
    Some(parts.join("+"))
}

/// The binding of one model control (Windows `create_bound_control`).
#[must_use]
pub fn settings_binding(control: &SettingsControl) -> SettingsBinding {
    match control {
        SettingsControl::ReadOnlyText { .. } => SettingsBinding::ReadOnly,
        SettingsControl::Text { setting, .. } => SettingsBinding::Text(*setting),
        SettingsControl::Choice {
            setting,
            value_type,
            options,
            ..
        } => SettingsBinding::Choice {
            setting: *setting,
            value_type: *value_type,
            values: options.iter().map(|option| option.value.clone()).collect(),
        },
        SettingsControl::Checkbox { setting, .. } => SettingsBinding::Checkbox(*setting),
        SettingsControl::Integer {
            setting,
            minimum,
            maximum,
            ..
        } => SettingsBinding::Integer {
            setting: *setting,
            minimum: *minimum,
            maximum: *maximum,
        },
        SettingsControl::IntegerSlider {
            setting,
            label,
            unit,
            ..
        } => SettingsBinding::IntegerSlider {
            setting: *setting,
            label: label.clone(),
            unit,
        },
        SettingsControl::EqualizerDevicePresetChoice {
            device_id, options, ..
        } => SettingsBinding::EqualizerDevicePreset {
            device_id: device_id.clone(),
            values: options.iter().map(|option| option.value.clone()).collect(),
        },
        SettingsControl::EqualizerPresetName { preset_id, .. } => {
            SettingsBinding::EqualizerPresetName(preset_id.clone())
        }
        SettingsControl::EqualizerBandSlider {
            preset_id,
            band_id,
            label,
            ..
        } => SettingsBinding::EqualizerBand {
            preset_id: preset_id.clone(),
            band_id,
            label: label.clone(),
        },
        SettingsControl::ShortcutActionList { actions, .. } => {
            SettingsBinding::ShortcutActionList(actions.clone())
        }
        SettingsControl::ShortcutCapture { action_id, .. } => {
            SettingsBinding::ShortcutCapture(action_id.clone())
        }
        SettingsControl::MenuItemCheckbox { action_id, .. } => SettingsBinding::MenuItem(action_id),
        SettingsControl::Command { command, .. } => SettingsBinding::Command(*command),
        SettingsControl::SpotifyChoice { field, .. } => SettingsBinding::SpotifyChoice(*field),
        SettingsControl::SpotifyNormalisation { .. } => SettingsBinding::SpotifyNormalisation,
    }
}

/// Windows `cookie_profile_options` for the saved browser.
fn cookie_profile_options(
    settings: &apricot_storage::SettingsDocument,
) -> Vec<SettingsChoiceOption> {
    let catalog = apricot_app::embedded_catalog(&settings.language);
    let mut browser = settings.cookies_from_browser.trim().to_owned();
    if browser.is_empty() {
        "none".clone_into(&mut browser);
    }
    let discovered = apricot_platform::browser_cookies::discover_cookie_profiles(&browser);
    apricot_app::cookies::cookie_profile_choice_values(
        &discovered,
        &settings.cookies_browser_profile,
    )
    .into_iter()
    .map(|value| SettingsChoiceOption {
        label: apricot_app::cookies::cookie_profile_choice_label(&catalog, &value),
        value,
    })
    .collect()
}

/// The Settings window's state in the shell (Windows `settings_open`).
#[derive(Default)]
pub(crate) struct SettingsState {
    open: bool,
    /// Windows `saved_spotify_settings` when the window opened.
    spotify_before: Option<apricot_spotify::settings::SpotifySettings>,
}

impl Shell {
    /// Windows `open_settings`.
    pub(crate) fn open_settings(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("settings");
        self.stop_key_repeat(ui);
        if self.settings_window.open {
            return;
        }
        self.settings_window.open = true;
        self.settings_window.spotify_before = Some(self.application.saved_spotify_settings());
        ui.open_settings();
    }

    /// Windows `show_action_finder`.
    pub(crate) fn open_action_finder(&mut self, ui: &mut dyn ShellUi) {
        self.stop_key_repeat(ui);
        ui.open_action_finder(self.application.action_finder_model());
    }

    /// The Action Finder closed: the chosen action runs, then focus goes to
    /// the window's primary control.
    pub fn action_finder_closed(&mut self, ui: &mut dyn ShellUi, action_id: Option<&'static str>) {
        if let Some(action_id) = action_id {
            self.activate_action(ui, action_id);
        }
        ui.focus_primary_control();
    }

    /// The settings and Action Finder actions; `true` when `action_id` is
    /// one.
    pub(crate) fn settings_action(&mut self, ui: &mut dyn ShellUi, action_id: &str) -> bool {
        match action_id {
            "open_settings" => self.open_settings(ui),
            "open_action_finder" => self.open_action_finder(ui),
            _ => return false,
        }
        true
    }

    /// Windows `render_controls`: the model of `section`, with the cookie
    /// profiles of the saved browser.
    pub fn settings_screen(&mut self, section: SettingsSection) -> SettingsScreenModel {
        if section == SettingsSection::Cookies {
            // Python `configured_cookies_display_path` runs the legacy
            // migration.
            let _ = self
                .application
                .configured_cookies_display_path(&crate::youtube::documents_folders());
        }
        let mut model = self.application.settings_model(section);
        if section == SettingsSection::Cookies {
            model.replace_choice_options(
                SettingId::CookiesBrowserProfile,
                cookie_profile_options(self.application.settings()),
            );
        }
        model
    }

    /// Windows `sync_bound_control`: applies one visible value. `gains`
    /// are the visible equalizer sliders.
    ///
    /// # Errors
    ///
    /// Returns the message of a setting that could not be changed.
    pub fn settings_apply(
        &mut self,
        binding: &SettingsBinding,
        input: &SettingsInput,
        gains: &EqualizerGains,
    ) -> Result<(), String> {
        let app = &mut self.application;
        let selected = match input {
            SettingsInput::Selected(selected) => *selected,
            _ => None,
        };
        let text = match input {
            SettingsInput::Text(text) => text.as_str(),
            _ => "",
        };
        let checked = matches!(input, SettingsInput::Checked(true));
        let result = match binding {
            SettingsBinding::ReadOnly
            | SettingsBinding::Command(_)
            | SettingsBinding::ShortcutActionList(_) => Ok(()),
            SettingsBinding::Text(SettingId::CookiesFile) => app.apply_cookies_path_text(text),
            SettingsBinding::Text(SettingId::AudiovaultEmail) => {
                app.apply_audiovault_email_text(text)
            }
            SettingsBinding::Text(setting) => app.set_string_setting(*setting, text),
            SettingsBinding::Integer {
                setting,
                minimum,
                maximum,
            } => text.trim().parse::<i64>().map_or(Ok(()), |value| {
                app.set_integer_setting(*setting, value.clamp(*minimum, *maximum))
            }),
            SettingsBinding::IntegerSlider { setting, .. } => match input {
                SettingsInput::Position(position) => app.set_integer_setting(*setting, *position),
                _ => Ok(()),
            },
            SettingsBinding::Choice {
                setting,
                value_type,
                values,
            } => match selected.and_then(|index| values.get(index)) {
                None => Ok(()),
                Some(value) => match value_type {
                    SettingsValueType::String => app.set_string_setting(*setting, value),
                    SettingsValueType::Integer => value
                        .parse::<i64>()
                        .map_or(Ok(()), |parsed| app.set_integer_setting(*setting, parsed)),
                    SettingsValueType::Float => value
                        .parse::<f64>()
                        .map_or(Ok(()), |parsed| app.set_float_setting(*setting, parsed)),
                },
            },
            SettingsBinding::Checkbox(setting) => app.set_boolean_setting(*setting, checked),
            SettingsBinding::MenuItem(action_id) => {
                app.set_main_menu_item_visible(action_id, checked)
            }
            SettingsBinding::EqualizerDevicePreset { device_id, values } => selected
                .and_then(|index| values.get(index))
                .map_or(Ok(()), |preset_id| {
                    app.set_equalizer_device_preset(device_id, preset_id)
                }),
            SettingsBinding::EqualizerPresetName(preset_id) => {
                app.set_equalizer_preset_name(preset_id, text)
            }
            SettingsBinding::EqualizerBand { preset_id, .. } => {
                app.set_visible_equalizer_gains(preset_id, gains)
            }
            SettingsBinding::ShortcutCapture(action_id) => {
                app.set_keyboard_shortcut(action_id, text)
            }
            SettingsBinding::SpotifyChoice(field) => {
                if let Some(index) = selected {
                    app.set_spotify_choice(*field, index);
                }
                Ok(())
            }
            SettingsBinding::SpotifyNormalisation => {
                app.set_spotify_normalisation(checked);
                Ok(())
            }
        };
        result.map_err(|error| error.to_string())
    }

    /// Windows `equalizer_setting_changed` and `post_equalizer_apply`:
    /// the effects of a changed equalizer control on a running player.
    pub fn settings_equalizer_changed(
        &mut self,
        ui: &mut dyn ShellUi,
        binding: &SettingsBinding,
        gains: &EqualizerGains,
    ) {
        match binding {
            SettingsBinding::Checkbox(SettingId::GlobalEqualizerEnabled)
            | SettingsBinding::Choice {
                setting: SettingId::GlobalEqualizerPreset,
                ..
            } => {
                if self.application.player_session().is_open() {
                    self.application.set_player_equalizer(None);
                }
                self.settings_apply_equalizer(ui, false);
            }
            SettingsBinding::Checkbox(SettingId::EqualizerClippingProtection) => {
                self.settings_apply_equalizer(ui, true);
            }
            SettingsBinding::EqualizerDevicePreset { .. } => {
                self.settings_apply_equalizer(ui, false);
            }
            SettingsBinding::Choice {
                setting: SettingId::EqualizerDbRange,
                ..
            } => {
                let range = f64::from(
                    i32::try_from(self.application.settings().equalizer_db_range.clamp(6, 24))
                        .unwrap_or(12),
                );
                let preset = self.application.settings().global_equalizer_preset.clone();
                let gains = gains
                    .iter()
                    .map(|(band, gain)| (band.clone(), gain.clamp(-range, range)))
                    .collect();
                let _ = self
                    .application
                    .set_visible_equalizer_gains(&preset, &gains);
            }
            _ => {}
        }
    }

    /// Windows `WM_APPLY_GLOBAL_EQUALIZER`: `force` also reaches a player
    /// that uses its own F4 equalizer.
    pub fn settings_apply_equalizer(&mut self, ui: &mut dyn ShellUi, force: bool) {
        if !self.application.player_session().is_open() {
            return;
        }
        if force {
            self.apply_player_equalizer(ui);
        } else {
            self.apply_global_player_equalizer(ui);
        }
    }

    /// Windows `equalizer_slider_live_preview`: the global equalizer turns
    /// on and the visible sliders are heard. The caller applies them after
    /// `apricot_app::equalizer::APPLY_DELAY_MS` with
    /// [`Shell::settings_apply_equalizer`] (`force`).
    pub fn settings_equalizer_preview(&mut self, preset_id: &str, gains: &EqualizerGains) -> bool {
        if !self.application.player_session().is_open() {
            return false;
        }
        let _ = self
            .application
            .set_boolean_setting(SettingId::GlobalEqualizerEnabled, true);
        if apricot_app::equalizer::is_custom_preset(preset_id) {
            self.application.set_player_equalizer(None);
        } else {
            self.application
                .set_player_equalizer(Some(apricot_app::EqualizerSession {
                    enabled: true,
                    gains: gains.clone(),
                }));
        }
        true
    }

    /// The preset shown in the Equalizer section (Windows
    /// `visible_equalizer_preset`).
    #[must_use]
    pub fn settings_visible_equalizer_preset(&self) -> String {
        self.application
            .equalizer_settings()
            .normalized_preset(&self.application.settings().global_equalizer_preset)
    }

    /// Windows `reset_equalizer_preset`: the factory gains of the visible
    /// preset, saved and heard; returns them and the announcement.
    ///
    /// # Errors
    ///
    /// Returns the message when the gains could not be saved.
    pub fn settings_reset_equalizer_preset(
        &mut self,
        ui: &mut dyn ShellUi,
    ) -> Result<(EqualizerGains, i64, String), String> {
        let preset = self.settings_visible_equalizer_preset();
        let gains = apricot_app::equalizer::factory_gains(&preset);
        self.application
            .set_visible_equalizer_gains(&preset, &gains)
            .map_err(|error| error.to_string())?;
        let range = self.application.equalizer_settings().db_range();
        if self.application.player_session().is_open() {
            if apricot_app::equalizer::is_custom_preset(&preset) {
                self.application.set_player_equalizer(None);
            } else {
                self.application
                    .set_player_equalizer(Some(apricot_app::EqualizerSession {
                        enabled: true,
                        gains: apricot_app::equalizer::normalized_gains(&gains),
                    }));
            }
            self.settings_apply_equalizer(ui, true);
        }
        Ok((gains, range, self.text("equalizer_saved")))
    }

    /// Windows `add_equalizer_profile`; `true` when the section is rebuilt
    /// with focus on the preset choice.
    pub fn settings_add_equalizer_profile(
        &mut self,
        ui: &mut dyn ShellUi,
        view: &mut dyn crate::EqualizerView,
        gains: &EqualizerGains,
    ) -> bool {
        self.create_profile_with_prompt(ui, view, gains).is_some()
    }

    /// Windows `import_equalizer_profile`.
    pub fn settings_import_equalizer_profile(
        &mut self,
        ui: &mut dyn ShellUi,
        view: &mut dyn crate::EqualizerView,
    ) -> bool {
        if self.import_profile_with_prompt(ui, view).is_some() {
            self.settings_apply_equalizer(ui, false);
            return true;
        }
        false
    }

    /// Windows `export_equalizer_profile`.
    pub fn settings_export_equalizer_profile(
        &mut self,
        ui: &mut dyn ShellUi,
        view: &mut dyn crate::EqualizerView,
        gains: &EqualizerGains,
    ) {
        let preset = self.settings_visible_equalizer_preset();
        let _ = self.application.set_visible_equalizer_gains(&preset, gains);
        let name = self
            .application
            .equalizer_settings()
            .preset_label(&self.catalog(), &preset);
        self.export_profile_with_prompt(ui, view, &name, gains, &preset);
    }

    /// Windows `delete_equalizer_profile`.
    pub fn settings_delete_equalizer_profile(
        &mut self,
        ui: &mut dyn ShellUi,
        view: &mut dyn crate::EqualizerView,
    ) -> bool {
        let preset = self.settings_visible_equalizer_preset();
        if self.delete_profile_with_prompt(ui, view, &preset).is_some() {
            self.settings_apply_equalizer(ui, false);
            return true;
        }
        false
    }

    /// Windows `render_controls` for the Playback section: a background
    /// device probe when the cached devices are old, `None` otherwise.
    #[must_use]
    pub fn settings_audio_device_probe(
        &self,
    ) -> Option<std::sync::mpsc::Receiver<Vec<apricot_playback::AudioOutputDevice>>> {
        if !self.application.audio_device_refresh_due() {
            return None;
        }
        let library = apricot_platform::mpv_locations().map(|(_, library)| library);
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let devices = library
                .and_then(|library| {
                    apricot_playback::probe_audio_output_devices(
                        &library,
                        None,
                        crate::player_extras::AUDIO_DEVICE_PROBE_TIMEOUT,
                    )
                    .ok()
                })
                .unwrap_or_default();
            let _ = sender.send(devices);
        });
        Some(receiver)
    }

    /// Windows `finish_audio_device_refresh`: the device choices, keeping
    /// `selected` (the shown value, else the saved device).
    pub fn settings_audio_devices(
        &mut self,
        devices: &[apricot_playback::AudioOutputDevice],
        selected: Option<&str>,
    ) -> (Vec<SettingsChoiceOption>, usize) {
        let options = self.application.store_probed_audio_devices(devices);
        let saved = apricot_app::audio_devices::normalized_device(
            &self.application.settings().audio_output_device,
        );
        let selected = selected.map_or(saved, str::to_owned);
        let options = apricot_app::audio_devices::refreshed_options_keeping(options, &selected);
        let position = options
            .iter()
            .position(|option| option.value == selected)
            .unwrap_or_default();
        (options, position)
    }

    /// Windows `save_and_register_startup`.
    fn settings_save_and_register(&mut self) -> Result<(), String> {
        self.application
            .save_settings()
            .map_err(|error| error.to_string())?;
        let executable = std::env::current_exe()
            .map_err(|error| format!("Could not locate the running executable: {error}"))?;
        apricot_platform::sync_startup_registration(
            apricot_platform::BUILD_IDENTITY,
            &executable,
            self.application.settings().start_with_windows,
        )
        .map_err(|error| error.to_string())
    }

    /// Windows `save_settings`: the announcement "Settings saved." and
    /// whether the language changed, which rebuilds the window.
    ///
    /// # Errors
    ///
    /// Returns the message when the settings were not saved.
    pub fn settings_save(&mut self, displayed_language: &str) -> Result<(String, bool), String> {
        self.settings_save_and_register()?;
        Ok((
            self.text("settings_saved"),
            self.application.settings().language != displayed_language,
        ))
    }

    /// Windows `restore_defaults`: no confirmation, saved at once.
    ///
    /// # Errors
    ///
    /// Returns the message when the settings were not saved.
    pub fn settings_restore_defaults(&mut self) -> Result<String, String> {
        self.application.reset_all_settings();
        crate::cookies::reset_repair_suppression();
        self.settings_save_and_register()?;
        Ok(self.text("defaults_restored"))
    }

    /// Windows `reset_section`: reset, save and the announcement.
    ///
    /// # Errors
    ///
    /// Returns the message when the section was not reset or saved.
    pub fn settings_reset_section(&mut self, section: SettingsSection) -> Result<String, String> {
        self.application
            .reset_settings_section(section)
            .map_err(|error| error.to_string())?;
        if section == SettingsSection::Cookies {
            crate::cookies::reset_repair_suppression();
        }
        self.settings_save_and_register()?;
        let label = self
            .application
            .settings_model(section)
            .sections
            .into_iter()
            .find(|item| item.section == section)
            .map(|item| item.label)
            .unwrap_or_default();
        Ok(self
            .text("section_settings_reset")
            .replace("{section}", &label))
    }

    /// Windows `browse_download_folder` after the folder was chosen: saved
    /// at once.
    ///
    /// # Errors
    ///
    /// Returns the message when the folder was not saved.
    pub fn settings_set_download_folder(&mut self, folder: &str) -> Result<(), String> {
        self.application
            .set_string_setting(SettingId::DownloadFolder, folder)
            .and_then(|()| self.application.save_settings())
            .map_err(|error| error.to_string())
    }

    /// Windows `capture_shortcut`. `action_name` names another action by
    /// its label in the action list.
    pub fn settings_capture_shortcut(
        &mut self,
        action_id: &str,
        chord: ShortcutChord,
        action_name: &dyn Fn(&str) -> String,
    ) -> ShortcutCapture {
        if chord.key == ShortcutKey::Space
            && !chord.control
            && !chord.shift
            && !chord.alt
            && action_id != "player_play_pause"
        {
            return ShortcutCapture::Ignored;
        }
        let Some(shortcut) = shortcut_text(chord) else {
            return ShortcutCapture::Ignored;
        };
        match self.application.set_keyboard_shortcut(action_id, &shortcut) {
            Ok(()) => ShortcutCapture::Captured {
                message: self.text("shortcut_captured").replace(
                    "{shortcut}",
                    &apricot_core::shortcut::display_shortcut(&shortcut),
                ),
                shortcut,
            },
            Err(apricot_app::SettingsControllerError::ShortcutConflict { shortcut, action }) => {
                ShortcutCapture::Conflict {
                    title: self.text("shortcut_in_use_title"),
                    message: self
                        .text("shortcut_in_use")
                        .replace(
                            "{shortcut}",
                            &apricot_core::shortcut::display_shortcut(&shortcut),
                        )
                        .replace("{action}", &action_name(&action)),
                }
            }
            Err(error) => ShortcutCapture::Failed(error.to_string()),
        }
    }

    /// Windows `open_youtube_api_key_page`: the announcement.
    ///
    /// # Errors
    ///
    /// Returns the message when the browser did not open.
    pub fn settings_open_api_key_page(&self) -> Result<String, String> {
        apricot_platform::shell::open_url("https://console.cloud.google.com/apis/credentials")
            .map(|_| self.text("youtube_api_key_page_opened"))
            .map_err(|error| {
                self.text("youtube_api_key_page_open_failed")
                    .replace("{error}", &error.to_string())
            })
    }

    /// A Settings command whose macOS part arrives with a later unit says
    /// so.
    #[must_use]
    pub fn settings_command_unavailable(&self, label: &str) -> String {
        apricot_app::unavailable_feature_message(&self.catalog(), label)
    }

    /// Windows `open_settings` after the window closed: the shown screen is
    /// refreshed in the new language without moving focus, then focus goes
    /// to its primary control and a chosen global action runs.
    pub(crate) const fn settings_window_open(&self) -> bool {
        self.settings_window.open
    }

    pub fn settings_closed(&mut self, ui: &mut dyn ShellUi, deferred_action: Option<&'static str>) {
        self.settings_window.open = false;
        if let Some(before) = self.settings_window.spotify_before.take() {
            self.spotify_settings_window_closed(before);
        }
        match self.view {
            View::MainMenu => {
                self.refresh_main_menu(ui, MainMenuSelection::LastActivated, None, false);
            }
            View::Results | View::Trending => self.refresh_results(ui, false),
            View::Collection => self.refresh_youtube_collection(ui, false),
            View::LocalFolder => self.refresh_local_folder(ui, false, false),
            View::Favorites | View::History => self.refresh_media_collection(ui, false, None),
            View::RssFeeds => self.refresh_rss_feeds(ui, false, None),
            View::RssItems => self.refresh_rss_items(ui, false, false, None),
            View::UserPlaylists => self.refresh_user_playlists(ui, false),
            View::UserPlaylistItems => self.refresh_user_playlist_items(ui, false, false),
            View::DownloadQueue => self.refresh_download_queue(ui, false, false),
            View::Subscriptions => self.refresh_subscriptions(ui, false, None),
            View::NotificationCenter => self.refresh_notification_center(ui, false, false, None),
            View::Player => self.refresh_player(ui, PlayerFocus::Keep),
            View::Search => {
                let mut screen =
                    self.search_screen(vec![self.text("search_results_empty")], 0, false);
                screen.focus_list = false;
                ui.show_list_screen(&screen);
            }
            View::DirectLink
            | View::PodcastSearchResults
            | View::PodcastCategories
            | View::AudiovaultMenu
            | View::AudiovaultSearch
            | View::AudiovaultResults
            | View::SpotifyHub
            | View::SpotifyAccounts
            | View::SpotifyBrowse => {}
        }
        ui.focus_primary_control();
        self.configure_subscription_timer(ui);
        self.configure_app_update_timer(ui);
        self.check_subscriptions_if_due(ui);
        self.configure_rss_timer(ui);
        self.process_activations(ui);
        if let Some(action_id) = deferred_action {
            self.activate_action(ui, action_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_are_stored_as_windows_text() {
        let chord = |text| ShortcutChord::parse(text).expect("chord");
        assert_eq!(
            shortcut_text(chord("Ctrl+Shift+A")).as_deref(),
            Some("Ctrl+Shift+A")
        );
        assert_eq!(shortcut_text(chord("Alt+F4")).as_deref(), Some("Alt+F4"));
        assert_eq!(shortcut_text(chord("PageUp")).as_deref(), Some("PageUp"));
    }

    #[test]
    fn the_equalizer_and_network_toggles_rebuild_the_section() {
        assert_eq!(
            SettingsBinding::Checkbox(SettingId::GlobalEqualizerEnabled).dependent_setting(),
            Some(SettingId::GlobalEqualizerEnabled)
        );
        assert_eq!(
            SettingsBinding::Checkbox(SettingId::ConfirmBeforeDownload).dependent_setting(),
            None
        );
    }
}
