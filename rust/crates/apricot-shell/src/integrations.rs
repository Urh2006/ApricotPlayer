//! The Integrations form and the shared shell's media controls and Discord.

use apricot_app::integrations::{DISCORD_APPLICATION_ID, IntegrationSettings, PresenceActivity};
use apricot_core::TranslationCatalog;

use crate::{FeatureForm, FormField, FormValue, FormValues};

/// Integrations: the system media controls and Discord, with every shared
/// detail.
#[must_use]
pub fn integrations_form(
    catalog: &TranslationCatalog,
    settings: &IntegrationSettings,
) -> FeatureForm {
    let mut fields = vec![
        FormField::checkbox(
            "media_controls",
            catalog.text(if cfg!(target_os = "macos") {
                "integrations_media_controls_mac"
            } else {
                "integrations_media_controls"
            }),
            settings.media_controls,
        ),
        FormField::checkbox(
            "discord",
            catalog.text("integrations_discord"),
            settings.discord,
        ),
        FormField::checkbox(
            "discord_title",
            catalog.text("integrations_discord_title"),
            settings.discord_title,
        ),
        FormField::checkbox(
            "discord_artist",
            catalog.text("integrations_discord_artist"),
            settings.discord_artist,
        ),
        FormField::checkbox(
            "discord_artwork",
            catalog.text("integrations_discord_artwork"),
            settings.discord_artwork,
        ),
        FormField::checkbox(
            "discord_timing",
            catalog.text("integrations_discord_timing"),
            settings.discord_timing,
        ),
        FormField::checkbox(
            "discord_when_paused",
            catalog.text("integrations_discord_paused"),
            settings.discord_when_paused,
        ),
    ];
    if DISCORD_APPLICATION_ID.is_empty() {
        fields.push(FormField::read_only(
            "discord_note",
            catalog.text("integrations_discord_note_label"),
            catalog.text("integrations_discord_unavailable"),
        ));
    }
    FeatureForm::localized(catalog, catalog.text("integrations_settings"), fields)
}

/// The settings chosen in [`integrations_form`].
#[must_use]
pub fn integrations_from_values(values: &FormValues) -> IntegrationSettings {
    let checked = |id: &str| {
        values
            .get(id)
            .and_then(FormValue::checked)
            .unwrap_or_default()
    };
    IntegrationSettings {
        media_controls: checked("media_controls"),
        discord: checked("discord"),
        discord_title: checked("discord_title"),
        discord_artist: checked("discord_artist"),
        discord_artwork: checked("discord_artwork"),
        discord_timing: checked("discord_timing"),
        discord_when_paused: checked("discord_when_paused"),
    }
}

/// The platform's form of a presence.
#[must_use]
pub fn presence_for_platform(
    presence: Option<PresenceActivity>,
) -> Option<apricot_platform::integrations::PresenceActivity> {
    presence.map(
        |presence| apricot_platform::integrations::PresenceActivity {
            title: presence.title,
            artist: presence.artist,
            artwork: presence.artwork,
            started_ms: presence.started_ms,
            ends_ms: presence.ends_ms,
        },
    )
}

/// Unix milliseconds now.
#[must_use]
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

/// The shared shell's media controls and Discord (macOS). Media key events
/// wait in a queue that the playback poll empties.
#[derive(Default)]
pub(crate) struct IntegrationState {
    #[cfg(any(windows, target_os = "macos"))]
    media_controls: Option<apricot_platform::integrations::SystemMediaControls>,
    keys: std::sync::Arc<
        std::sync::Mutex<std::collections::VecDeque<apricot_platform::integrations::MediaKeyEvent>>,
    >,
    discord: Option<apricot_platform::integrations::DiscordPresence>,
}

impl crate::Shell {
    /// Starts or stops the media controls and Discord as Integrations says.
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
        if !settings.discord_active() {
            self.integrations.discord = None;
        } else if self.integrations.discord.is_none() {
            self.integrations.discord = Some(
                apricot_platform::integrations::DiscordPresence::start(DISCORD_APPLICATION_ID),
            );
        }
        self.update_integrations();
    }

    /// What the media controls and Discord show for the player.
    pub(crate) fn update_integrations(&mut self) {
        let session = self.application.player_session();
        let item = session.current_item().cloned();
        let paused = session.is_paused();
        let position = session.position_seconds();
        let duration = session
            .duration_seconds()
            .or_else(|| item.as_ref().and_then(|item| item.duration_seconds));
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
        if let Some(discord) = &self.integrations.discord {
            let settings = IntegrationSettings::from_settings(self.application.settings());
            discord.show(presence_for_platform(item.and_then(|item| {
                settings.presence(&item, position, duration, paused, now_ms())
            })));
        }
    }

    /// The player closed: nothing plays any more.
    pub(crate) fn integrations_player_closed(&mut self) {
        #[cfg(any(windows, target_os = "macos"))]
        if let Some(controls) = self.integrations.media_controls.as_mut() {
            controls.stopped();
        }
        if let Some(discord) = &self.integrations.discord {
            discord.show(None);
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
            let paused = self.application.player_session().is_paused();
            let action = match key {
                Key::Toggle => Some("player_play_pause"),
                Key::Play if paused => Some("player_play_pause"),
                Key::Pause | Key::Stop if !paused => Some("player_play_pause"),
                Key::Next => Some("player_next"),
                Key::Previous => Some("player_previous"),
                Key::SeekBy(seconds) if seconds >= 0.0 => Some("player_seek_forward"),
                Key::SeekBy(_) => Some("player_seek_back"),
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
    fn the_form_round_trips_every_switch() {
        let catalog = apricot_app::embedded_catalog("en");
        let settings = IntegrationSettings {
            media_controls: false,
            discord: true,
            discord_artwork: false,
            discord_when_paused: true,
            ..IntegrationSettings::default()
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
