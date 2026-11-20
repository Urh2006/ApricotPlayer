//! Shared Spotify subscription forms and notification projection for both
//! native UIs.
use crate::{FeatureForm, FormField, FormValues};
use apricot_app::Application;
use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource, TranslationCatalog};
use apricot_spotify::playlist_subscriptions::{
    NotificationOptions, PlaylistChanges, SubscriptionCommand, SubscriptionDocument,
    SubscriptionReport,
};

#[derive(Clone, Debug)]
pub enum SubscriptionPanelAction {
    Command(SubscriptionCommand),
    Open(String),
    Changes(String),
    Options(String),
    Subscribe,
}

const PANEL_ACTIONS: [&str; 7] = [
    "spotify_sub_open",
    "spotify_sub_changes",
    "spotify_sub_options",
    "spotify_sub_unsubscribe",
    "spotify_sub_check_now",
    "spotify_sub_interval",
    "spotify_sub_subscribe_link",
];

/// A subscription message or error from the Spotify service in the
/// interface language; the service words its fixed texts as keys.
#[must_use]
pub fn report_text(catalog: &TranslationCatalog, text: &str) -> String {
    if let Some((key, detail)) = text.split_once(": ")
        && key.starts_with("spotify_sub_")
    {
        return format!("{} {detail}", catalog.text(key));
    }
    if text.starts_with("spotify_sub_") {
        catalog.text(text).to_owned()
    } else {
        text.to_owned()
    }
}

#[must_use]
pub fn panel(catalog: &TranslationCatalog, document: &SubscriptionDocument) -> FeatureForm {
    let mut form = FeatureForm::localized(
        catalog,
        catalog.text("spotify_sub_title"),
        vec![
            FormField::choice(
                "playlist",
                catalog.text("spotify_sub_playlist"),
                if document.entries.is_empty() {
                    vec![catalog.text("spotify_sub_none").to_owned()]
                } else {
                    document
                        .entries
                        .iter()
                        .map(|entry| entry.name.clone())
                        .collect()
                },
                0,
            ),
            FormField::choice(
                "action",
                catalog.text("playlist_editor_action"),
                PANEL_ACTIONS
                    .iter()
                    .map(|key| catalog.text(key).to_owned())
                    .collect(),
                0,
            ),
        ],
    );
    catalog
        .text("playlist_editor_do")
        .clone_into(&mut form.submit);
    form
}

/// What the panel asked for.
///
/// # Errors
///
/// Returns a translated message when no playlist is subscribed yet.
pub fn selection(
    catalog: &TranslationCatalog,
    document: &SubscriptionDocument,
    values: &FormValues,
) -> Result<SubscriptionPanelAction, String> {
    let selected = values
        .get("playlist")
        .and_then(crate::FormValue::choice)
        .unwrap_or(0);
    let uri = || {
        document
            .entries
            .get(selected)
            .map(|entry| entry.uri.clone())
            .ok_or_else(|| catalog.text("spotify_sub_none_selected").to_owned())
    };
    Ok(
        match values
            .get("action")
            .and_then(crate::FormValue::choice)
            .unwrap_or(0)
        {
            0 => SubscriptionPanelAction::Open(uri()?),
            1 => SubscriptionPanelAction::Changes(uri()?),
            2 => SubscriptionPanelAction::Options(uri()?),
            3 => SubscriptionPanelAction::Command(SubscriptionCommand::Unsubscribe(uri()?)),
            4 => SubscriptionPanelAction::Command(SubscriptionCommand::Check { force: true }),
            5 => SubscriptionPanelAction::Command(SubscriptionCommand::Interval(
                document.interval_seconds,
            )),
            _ => SubscriptionPanelAction::Subscribe,
        },
    )
}

#[must_use]
pub fn options_form(
    catalog: &TranslationCatalog,
    document: &SubscriptionDocument,
    uri: &str,
) -> Option<FeatureForm> {
    let entry = document.entries.iter().find(|entry| entry.uri == uri)?;
    Some(FeatureForm::localized(
        catalog,
        catalog
            .text("spotify_sub_options_title")
            .replace("{name}", &entry.name),
        vec![
            FormField::checkbox(
                "added",
                catalog.text("spotify_sub_notify_added"),
                entry.options.added,
            ),
            FormField::checkbox(
                "removed",
                catalog.text("spotify_sub_notify_removed"),
                entry.options.removed,
            ),
            FormField::checkbox(
                "reordered",
                catalog.text("spotify_sub_notify_reordered"),
                entry.options.reordered,
            ),
        ],
    ))
}

#[must_use]
pub fn options(values: &FormValues) -> NotificationOptions {
    let checked = |id: &str, default: bool| {
        values
            .get(id)
            .and_then(crate::FormValue::checked)
            .unwrap_or(default)
    };
    NotificationOptions {
        added: checked("added", true),
        removed: checked("removed", false),
        reordered: checked("reordered", false),
    }
}

#[must_use]
pub fn interval_form(catalog: &TranslationCatalog, document: &SubscriptionDocument) -> FeatureForm {
    #[allow(clippy::cast_precision_loss)] // Seconds of at most a week.
    let minutes = document.interval_seconds as f64 / 60.0;
    FeatureForm::localized(
        catalog,
        catalog.text("spotify_sub_interval"),
        vec![FormField::number(
            "minutes",
            catalog.text("spotify_sub_interval_minutes"),
            minutes,
            1.0,
            10080.0,
        )],
    )
}

/// The interval chosen in [`interval_form`], in seconds.
#[must_use]
pub fn interval_seconds(values: &FormValues) -> u64 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 1 to 10080 minutes.
    let seconds = (values
        .get("minutes")
        .and_then(crate::FormValue::number)
        .unwrap_or(60.0)
        .clamp(1.0, 10080.0)
        * 60.0)
        .round() as u64;
    seconds
}

#[must_use]
pub fn subscribe_form(catalog: &TranslationCatalog) -> FeatureForm {
    FeatureForm::localized(
        catalog,
        catalog.text("spotify_sub_subscribe_title"),
        vec![FormField::text("uri", catalog.text("spotify_sub_link"), "")],
    )
}

#[must_use]
pub fn parse_uri(value: &str) -> Option<String> {
    let reference = apricot_core::SpotifyRef::parse(value.trim())?;
    (reference.kind == apricot_core::SpotifyEntityKind::Playlist).then(|| reference.to_uri())
}

/// The title of the list of a playlist's changes.
#[must_use]
pub fn changes_title(catalog: &TranslationCatalog, change: &PlaylistChanges) -> String {
    catalog
        .text("spotify_sub_changes_title")
        .replace("{name}", &change.name)
}

#[must_use]
pub fn notification_item(change: &PlaylistChanges, account: &str) -> MediaItem {
    MediaItem {
        id: MediaId(change.uri.clone()),
        source: MediaSource::Spotify,
        kind: MediaKind::Playlist,
        title: change.name.clone(),
        url: None,
        stream_url: None,
        external_audio_url: None,
        local_path: None,
        channel: String::new(),
        duration_seconds: None,
        metadata: std::collections::BTreeMap::from([
            (
                "spotify_subscription_change".into(),
                serde_json::to_value(change).unwrap_or_default(),
            ),
            (
                "spotify_subscription_account".into(),
                serde_json::Value::String(account.to_owned()),
            ),
        ]),
    }
}

#[must_use]
pub fn item_changes(item: &MediaItem) -> Option<PlaylistChanges> {
    serde_json::from_value(item.metadata.get("spotify_subscription_change")?.clone()).ok()
}

/// One summary line for a playlist's changes: "Mix: 3 tracks added, 1
/// removed".
#[must_use]
pub fn summary(catalog: &TranslationCatalog, change: &PlaylistChanges) -> String {
    if let Some(release) = &change.release {
        return release_summary(catalog, release);
    }
    let mut parts = Vec::new();
    if !change.added.is_empty() {
        parts.push(
            catalog
                .text("spotify_sub_added_count")
                .replace("{count}", &change.added.len().to_string()),
        );
    }
    if !change.removed.is_empty() {
        parts.push(
            catalog
                .text("spotify_sub_removed_count")
                .replace("{count}", &change.removed.len().to_string()),
        );
    }
    if change.reordered {
        parts.push(catalog.text("spotify_sub_reordered").to_owned());
    }
    catalog
        .text("spotify_sub_summary")
        .replace("{name}", &change.name)
        .replace("{changes}", &parts.join(", "))
}

/// The start of a new release's line, "New single from Artist"; the
/// notification adds the title from its item.
#[must_use]
pub fn release_summary(
    catalog: &TranslationCatalog,
    release: &apricot_spotify::playlist_subscriptions::ReleaseInfo,
) -> String {
    let key = match release.kind.as_str() {
        "SINGLE" => "spotify_release_single",
        "EP" => "spotify_release_ep",
        "COMPILATION" => "spotify_release_compilation",
        _ => "spotify_release_album",
    };
    catalog
        .text(key)
        .replace("{artists}", &release.artists.join(", "))
}

/// Adds one notification for each changed playlist. Returns the IDs safe to
/// acknowledge, which is only after the notification is stored.
///
/// # Errors
///
/// Returns the error when the notifications cannot be saved.
pub fn publish(
    application: &mut Application,
    report: &SubscriptionReport,
) -> Result<Vec<String>, String> {
    let catalog = apricot_app::embedded_catalog(&application.settings().language);
    let mut acknowledged = Vec::new();
    for change in &report.document.outbox {
        // The same change of another account is its own notification: one
        // account cannot open the other's (review 5, B21).
        let duplicate = application.notifications().iter().any(|notification| {
            notification.item.as_ref().is_some_and(|item| {
                item_changes(item).is_some_and(|existing| existing.id == change.id)
                    && item
                        .metadata
                        .get("spotify_subscription_account")
                        .and_then(serde_json::Value::as_str)
                        .is_none_or(|account| account == report.account)
            })
        });
        if !duplicate {
            #[allow(clippy::cast_precision_loss)] // Unix seconds.
            let timestamp = change.timestamp as f64;
            let (kind, title) = match &change.release {
                Some(_) => ("spotify_release", String::new()),
                None => ("spotify_playlist", change.name.clone()),
            };
            application
                .add_notification(apricot_storage::AppNotification::new(
                    kind,
                    title,
                    summary(&catalog, change),
                    Some(notification_item(change, &report.account)),
                    timestamp,
                ))
                .map_err(|error| error.to_string())?;
        }
        acknowledged.push(change.id.clone());
    }
    Ok(acknowledged)
}

impl crate::Shell {
    /// Settings, Spotify on macOS: new releases of all followed artists on
    /// or off; the list of single artists is the Windows window for now
    /// (2.1, RC 4).
    pub fn settings_spotify_releases(&mut self, ui: &mut dyn crate::ShellUi) {
        let catalog = self.catalog();
        let Some(service) = self
            .spotify_service()
            .filter(|service| service.connected_account().is_some())
        else {
            ui.show_error(catalog.text("spotify_not_connected"));
            return;
        };
        let mut options = match service.playlist_subscriptions() {
            Ok(document) => document.artists,
            Err(error) => {
                ui.show_error(&report_text(&catalog, &error));
                return;
            }
        };
        let form = FeatureForm::localized(
            &catalog,
            catalog.text("spotify_releases_title"),
            vec![FormField::choice(
                "all",
                catalog.text("spotify_releases_title"),
                vec![
                    catalog.text("spotify_releases_all_off").to_owned(),
                    catalog.text("spotify_releases_all_on").to_owned(),
                ],
                usize::from(options.all),
            )],
        );
        let Some(values) = ui.feature_form(&form) else {
            return;
        };
        let all = values
            .get("all")
            .and_then(crate::FormValue::choice)
            .is_some_and(|choice| choice == 1);
        options.set_all(all);
        options.revision =
            apricot_spotify::playlist_subscriptions::next_artist_revision(options.revision);
        let stamp = self.spotify.epochs.begin();
        service.subscription_operation(
            stamp,
            SubscriptionCommand::ArtistReleases {
                revision: options.revision,
                all: options.all,
                included: options.included,
                excluded: options.excluded,
            },
        );
        ui.set_status(
            catalog.text(if all {
                "spotify_releases_all_on_done"
            } else {
                "spotify_releases_all_off_done"
            }),
            true,
        );
    }

    pub fn show_spotify_subscriptions(&mut self, ui: &mut dyn crate::ShellUi) {
        let Some(service) = self.spotify_service() else {
            return;
        };
        let catalog = self.catalog();
        let document = match service.playlist_subscriptions() {
            Ok(document) => document,
            Err(error) => {
                ui.show_error(&report_text(&catalog, &error));
                return;
            }
        };
        let Some(values) = ui.feature_form(&panel(&catalog, &document)) else {
            return;
        };
        let action = match selection(&catalog, &document, &values) {
            Ok(action) => action,
            Err(error) => {
                ui.show_error(&error);
                return;
            }
        };
        let command = match action {
            SubscriptionPanelAction::Open(uri) => {
                self.spotify_open_subscription_playlist(ui, uri);
                return;
            }
            SubscriptionPanelAction::Changes(uri) => {
                if let Some(change) = document.history.iter().find(|change| change.uri == uri) {
                    self.spotify_open_subscription_changes(ui, change.clone());
                } else {
                    ui.set_status(catalog.text("spotify_sub_no_changes"), true);
                }
                return;
            }
            SubscriptionPanelAction::Options(uri) => {
                let Some(form) = options_form(&catalog, &document, &uri) else {
                    return;
                };
                let Some(values) = ui.feature_form(&form) else {
                    return;
                };
                SubscriptionCommand::Options {
                    uri,
                    options: options(&values),
                }
            }
            SubscriptionPanelAction::Subscribe => {
                let Some(values) = ui.feature_form(&subscribe_form(&catalog)) else {
                    return;
                };
                let Some(uri) = values
                    .get("uri")
                    .and_then(crate::FormValue::text)
                    .and_then(parse_uri)
                else {
                    ui.show_error(catalog.text("spotify_sub_link_invalid"));
                    return;
                };
                SubscriptionCommand::Subscribe(uri)
            }
            SubscriptionPanelAction::Command(SubscriptionCommand::Interval(_)) => {
                let Some(values) = ui.feature_form(&interval_form(&catalog, &document)) else {
                    return;
                };
                SubscriptionCommand::Interval(interval_seconds(&values))
            }
            SubscriptionPanelAction::Command(command) => command,
        };
        let stamp = self.spotify.epochs.begin();
        service.subscription_operation(stamp, command);
        ui.set_status(catalog.text("spotify_sub_updating"), true);
        ui.set_background_polling(true);
    }

    pub(crate) fn spotify_subscription_report(
        &mut self,
        ui: &mut dyn crate::ShellUi,
        report: &SubscriptionReport,
    ) {
        let catalog = self.catalog();
        match publish(&mut self.application, report) {
            Ok(ids) if !ids.is_empty() => {
                if let Some(service) = self.spotify_service() {
                    let stamp = self.spotify.epochs.begin();
                    service.subscription_operation(stamp, SubscriptionCommand::Acknowledge(ids));
                }
            }
            Err(error) => ui.show_error(&error),
            _ => {}
        }
        self.sync_notification_center(ui);
        if !report.message.is_empty() {
            ui.set_status(&report_text(&catalog, &report.message), true);
        }
        if !report.errors.is_empty() {
            let errors: Vec<_> = report
                .errors
                .iter()
                .map(|error| report_text(&catalog, error))
                .collect();
            // A background check says its failure in the status line; only
            // Check now interrupts with a message box (review 2.1).
            if report.background {
                ui.set_status(&errors.join(" "), false);
            } else {
                ui.show_error(&errors.join("\n"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apricot_spotify::playlist_subscriptions::PlaylistChanges;

    /// Review 5, B21: the same release for a second account is its own
    /// notification; for the same account it is not repeated.
    #[test]
    fn each_account_gets_its_own_notification() {
        let mut shell = crate::tests::keyboard::shell();
        let mut report = SubscriptionReport {
            account: "a".into(),
            ..SubscriptionReport::default()
        };
        report.document.outbox.push(PlaylistChanges {
            id: "release:spotify:album:1".into(),
            uri: "spotify:album:1".into(),
            ..PlaylistChanges::default()
        });
        assert_eq!(publish(&mut shell.application, &report).unwrap().len(), 1);
        assert_eq!(publish(&mut shell.application, &report).unwrap().len(), 1);
        assert_eq!(shell.application.notifications().len(), 1);
        report.account = "b".into();
        publish(&mut shell.application, &report).unwrap();
        assert_eq!(shell.application.notifications().len(), 2);
    }

    #[test]
    fn the_summary_names_each_kind_of_change() {
        let catalog = apricot_app::embedded_catalog("en");
        let change = PlaylistChanges {
            name: "Mix".into(),
            reordered: true,
            ..PlaylistChanges::default()
        };
        assert_eq!(summary(&catalog, &change), "Mix: track order changed");
        assert_eq!(
            report_text(&catalog, "spotify_sub_subscribed"),
            "Subscribed to the playlist."
        );
        assert_eq!(report_text(&catalog, "raw error"), "raw error");
        let release = PlaylistChanges {
            name: "Far Away".into(),
            release: Some(apricot_spotify::playlist_subscriptions::ReleaseInfo {
                artists: vec!["Stereo Static".into(), "Udex".into()],
                kind: "SINGLE".into(),
                date: "2026-11-19".into(),
            }),
            ..PlaylistChanges::default()
        };
        assert_eq!(
            summary(&catalog, &release),
            "New single from Stereo Static, Udex"
        );
    }
}
