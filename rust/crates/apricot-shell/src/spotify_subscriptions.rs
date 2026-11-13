//! Shared Spotify subscription forms and notification projection for both native UIs.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.
use crate::{FeatureForm, FormField, FormValues};
use apricot_app::Application;
use apricot_core::{MediaId, MediaItem, MediaKind, MediaSource};
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
pub fn panel(document: &SubscriptionDocument) -> FeatureForm {
    FeatureForm::new(
        "Spotify subscriptions",
        vec![
            FormField::choice(
                "playlist",
                "Playlist",
                if document.entries.is_empty() {
                    vec!["No subscribed playlists".into()]
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
                "Action",
                vec![
                    "Open playlist".into(),
                    "Show changes".into(),
                    "Notification options".into(),
                    "Unsubscribe".into(),
                    "Check now".into(),
                    "Check interval".into(),
                    "Subscribe using a link".into(),
                ],
                0,
            ),
        ],
    )
}
pub fn selection(
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
            .ok_or_else(|| "No subscribed playlist selected".to_owned())
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
pub fn options_form(document: &SubscriptionDocument, uri: &str) -> Option<FeatureForm> {
    let entry = document.entries.iter().find(|entry| entry.uri == uri)?;
    Some(FeatureForm::new(
        format!("Notifications: {}", entry.name),
        vec![
            FormField::checkbox("added", "Added tracks", entry.options.added),
            FormField::checkbox("removed", "Removed tracks", entry.options.removed),
            FormField::checkbox("reordered", "Track order changes", entry.options.reordered),
        ],
    ))
}
pub fn options(values: &FormValues) -> NotificationOptions {
    NotificationOptions {
        added: values
            .get("added")
            .and_then(crate::FormValue::checked)
            .unwrap_or(true),
        removed: values
            .get("removed")
            .and_then(crate::FormValue::checked)
            .unwrap_or(false),
        reordered: values
            .get("reordered")
            .and_then(crate::FormValue::checked)
            .unwrap_or(false),
    }
}
pub fn interval_form(document: &SubscriptionDocument) -> FeatureForm {
    FeatureForm::new(
        "Spotify subscription checks",
        vec![FormField::number(
            "minutes",
            "Check every (minutes)",
            document.interval_seconds as f64 / 60.0,
            1.0,
            10080.0,
        )],
    )
}
pub fn subscribe_form() -> FeatureForm {
    FeatureForm::new(
        "Subscribe to Spotify playlist",
        vec![FormField::text("uri", "Spotify playlist link or URI", "")],
    )
}
pub fn parse_uri(value: &str) -> Option<String> {
    let reference = apricot_core::SpotifyRef::parse(value.trim())?;
    (reference.kind == apricot_core::SpotifyEntityKind::Playlist).then(|| reference.to_uri())
}
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
pub fn item_changes(item: &MediaItem) -> Option<PlaylistChanges> {
    serde_json::from_value(item.metadata.get("spotify_subscription_change")?.clone()).ok()
}
/// Returns IDs safe to acknowledge only after durable notification insertion.
pub fn publish(
    application: &mut Application,
    report: &SubscriptionReport,
) -> Result<Vec<String>, String> {
    let mut acknowledged = Vec::new();
    for change in &report.document.outbox {
        let duplicate = application.notifications().iter().any(|notification| {
            notification
                .item
                .as_ref()
                .and_then(item_changes)
                .is_some_and(|existing| existing.id == change.id)
        });
        if !duplicate {
            let mut parts = Vec::new();
            if !change.added.is_empty() {
                parts.push(format!("{} added tracks", change.added.len()));
            }
            if !change.removed.is_empty() {
                parts.push(format!("{} removed tracks", change.removed.len()));
            }
            if change.reordered {
                parts.push("track order changed".to_owned());
            }
            let message = format!("{}: {}", change.name, parts.join(", "));
            application
                .add_notification(apricot_storage::AppNotification::new(
                    "spotify_playlist",
                    change.name.clone(),
                    message,
                    Some(notification_item(change, &report.account)),
                    change.timestamp as f64,
                ))
                .map_err(|e| e.to_string())?;
        }
        acknowledged.push(change.id.clone());
    }
    Ok(acknowledged)
}

impl crate::Shell {
    pub fn show_spotify_subscriptions(&mut self, ui: &mut dyn crate::ShellUi) {
        let Some(service) = self.spotify_service() else {
            return;
        };
        let document = match service.playlist_subscriptions() {
            Ok(doc) => doc,
            Err(error) => {
                ui.show_error(&error);
                return;
            }
        };
        let Some(values) = ui.feature_form(&panel(&document)) else {
            return;
        };
        let action = match selection(&document, &values) {
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
                    ui.set_status("No saved changes for this playlist", true);
                }
                return;
            }
            SubscriptionPanelAction::Options(uri) => {
                let Some(form) = options_form(&document, &uri) else {
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
                let Some(values) = ui.feature_form(&subscribe_form()) else {
                    return;
                };
                let Some(uri) = values
                    .get("uri")
                    .and_then(crate::FormValue::text)
                    .and_then(parse_uri)
                else {
                    ui.show_error("Enter a valid Spotify playlist link or URI");
                    return;
                };
                SubscriptionCommand::Subscribe(uri)
            }
            SubscriptionPanelAction::Command(SubscriptionCommand::Interval(_)) => {
                let Some(values) = ui.feature_form(&interval_form(&document)) else {
                    return;
                };
                SubscriptionCommand::Interval(
                    (values
                        .get("minutes")
                        .and_then(crate::FormValue::number)
                        .unwrap_or(60.0)
                        * 60.0)
                        .round() as u64,
                )
            }
            SubscriptionPanelAction::Command(command) => command,
        };
        let stamp = self.spotify.epochs.begin();
        service.subscription_operation(stamp, command);
        ui.set_status("Updating Spotify subscriptions", true);
        ui.set_background_polling(true);
    }
    pub(crate) fn spotify_subscription_report(
        &mut self,
        ui: &mut dyn crate::ShellUi,
        report: SubscriptionReport,
    ) {
        match publish(&mut self.application, &report) {
            Ok(ids) if !ids.is_empty() => {
                if let Some(service) = self.spotify_service() {
                    let stamp = self.spotify.epochs.begin();
                    service.subscription_operation(stamp, SubscriptionCommand::Acknowledge(ids));
                }
            }
            Err(error) => ui.show_error(&error),
            _ => {}
        }
        if !report.message.is_empty() {
            ui.set_status(&report.message, true);
        }
        if !report.errors.is_empty() {
            ui.show_error(&report.errors.join("\n"));
        }
    }
}
