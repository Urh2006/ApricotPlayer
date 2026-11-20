//! The media controls form and the shared shell's media controls.

use apricot_app::integrations::IntegrationSettings;
use apricot_core::TranslationCatalog;

use crate::{FeatureForm, FormField, FormValue, FormValues};

/// Media controls: the system media controls and the media keys.
#[must_use]
pub fn integrations_form(
    catalog: &TranslationCatalog,
    settings: &IntegrationSettings,
) -> FeatureForm {
    FeatureForm::localized(
        catalog,
        catalog.text("integrations_settings"),
        vec![FormField::checkbox(
            "media_controls",
            catalog.text(if cfg!(target_os = "macos") {
                "integrations_media_controls_mac"
            } else {
                "integrations_media_controls"
            }),
            settings.media_controls,
        )],
    )
}

/// The settings chosen in [`integrations_form`].
#[must_use]
pub fn integrations_from_values(values: &FormValues) -> IntegrationSettings {
    IntegrationSettings {
        media_controls: values
            .get("media_controls")
            .and_then(FormValue::checked)
            .unwrap_or_default(),
    }
}

/// The shared shell's media controls (macOS). Media key events
/// wait in a queue that the playback poll empties.
#[derive(Default)]
pub(crate) struct IntegrationState {
    #[cfg(any(windows, target_os = "macos"))]
    media_controls: Option<apricot_platform::integrations::SystemMediaControls>,
    keys: std::sync::Arc<
        std::sync::Mutex<std::collections::VecDeque<apricot_platform::integrations::MediaKeyEvent>>,
    >,
}

impl crate::Shell {
    /// Starts or stops the media controls as the settings say.
    pub fn apply_integrations(&mut self) {
        let settings = IntegrationSettings::from_settings(self.application.settings());
        #[cfg(any(windows, target_os = "macos"))]
        if !settings.media_controls {
            self.integrations.media_controls = None;
        } else if self.integrations.media_controls.is_none() {
            let keys = std::sync::Arc::clone(&self.integrations.keys);
            self.integrations.media_controls =
                apricot_platform::integrations::SystemMediaControls::new(None, move |event| {
                    if let Ok(mut keys) = keys.lock() {
                        keys.push_back(event);
                    }
                })
                .ok();
        }
        self.update_integrations();
    }

    /// What the media controls show for the player.
    pub(crate) fn update_integrations(&mut self) {
        let session = self.application.player_session();
        let item = session.current_item().cloned();
        let paused = session.is_paused();
        let (position, duration) =
            apricot_app::playback_features::system_controls_times(&self.application);
        #[cfg(any(windows, target_os = "macos"))]
        if let Some(controls) = self.integrations.media_controls.as_mut() {
            match &item {
                Some(item) => {
                    controls.now_playing(&item.title, &item.channel, duration);
                    controls.playback(!paused, Some(position));
                }
                None => controls.stopped(),
            }
        }
    }

    /// The player closed: nothing plays any more.
    pub(crate) fn integrations_player_closed(&mut self) {
        #[cfg(any(windows, target_os = "macos"))]
        if let Some(controls) = self.integrations.media_controls.as_mut() {
            controls.stopped();
        }
    }

    /// Media keys pressed since the last playback poll.
    pub(crate) fn run_media_keys(&mut self, ui: &mut dyn crate::ShellUi) {
        use apricot_platform::integrations::MediaKeyEvent as Key;
        let keys: Vec<_> = self
            .integrations
            .keys
            .lock()
            .map(|mut keys| keys.drain(..).collect())
            .unwrap_or_default();
        for key in keys {
            if !self.application.player_session().is_open() {
                continue;
            }
            let paused = self.application.player_session().is_paused();
            let action = match key {
                Key::Toggle => Some("player_play_pause"),
                Key::Play if paused => Some("player_play_pause"),
                Key::Pause | Key::Stop if !paused => Some("player_play_pause"),
                Key::Next => Some("player_next"),
                Key::Previous => Some("player_previous"),
                // The step the system asks for (a headset's 15 seconds),
                // not the player's own (review 2.1).
                Key::SeekBy(seconds) if seconds.is_finite() && seconds != 0.0 => {
                    self.seek_player(ui, seconds);
                    None
                }
                // The position slider of the system's controls; silent, as a
                // slider is (review 2.1).
                Key::SetPosition(seconds) if seconds.is_finite() => {
                    let seconds = apricot_app::playback_features::system_seek_target(
                        &self.application,
                        seconds,
                    );
                    let _ = self.execute_player_command(
                        ui,
                        apricot_playback::PlaybackCommand::SeekAbsolute {
                            seconds,
                            exact: true,
                        },
                    );
                    None
                }
                _ => None,
            };
            if let Some(action) = action
                && self.application.player_session().is_open()
            {
                self.activate_action(ui, action);
            }
        }
    }

    /// Integrations from Settings or its own shortcut, saved at once.
    pub(crate) fn edit_integrations(&mut self, ui: &mut dyn crate::ShellUi) {
        let catalog = self.catalog();
        let settings = IntegrationSettings::from_settings(self.application.settings());
        let Some(values) = ui.feature_form(&integrations_form(&catalog, &settings)) else {
            return;
        };
        let settings = integrations_from_values(&values);
        self.application.set_feature_settings(
            apricot_app::integrations::INTEGRATIONS_KEY,
            serde_json::json!(settings),
        );
        match self.application.save_settings() {
            Ok(()) => ui.set_status(catalog.text("integrations_saved"), true),
            Err(error) => ui.show_error(
                &catalog
                    .text("settings_save_failed")
                    .replace("{error}", &error.to_string()),
            ),
        }
        self.apply_integrations();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_round_trips_the_switch() {
        let catalog = apricot_app::embedded_catalog("en");
        let settings = IntegrationSettings {
            media_controls: false,
        };
        let values: FormValues = integrations_form(&catalog, &settings)
            .fields
            .iter()
            .filter_map(|field| match field.kind {
                crate::FormFieldKind::Checkbox { checked } => {
                    Some((field.id.clone(), FormValue::Checkbox(checked)))
                }
                _ => None,
            })
            .collect();
        assert_eq!(integrations_from_values(&values), settings);
    }
}
