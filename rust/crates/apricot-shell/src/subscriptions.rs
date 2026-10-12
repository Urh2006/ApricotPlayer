//! Channel subscriptions and the notification center (Windows
//! `show_subscriptions`, `refresh_subscriptions`, the subscription check
//! runtime, `show_notification_center` and its edits in `win32.rs`).

use std::collections::VecDeque;

use apricot_app::context_menu::{ContextCommand, ContextMenuEntry};
use apricot_app::{
    AppNotification, Subscription, SubscriptionAddOutcome, SubscriptionCheckResult,
    SubscriptionRemoveOutcome, YoutubeCollectionKind,
};
use apricot_core::{MediaItem, Route, RouteFrame};
use apricot_media::YoutubeBackend;
use apricot_platform::{YoutubeSearchService, YoutubeSearchServiceUpdate};

use crate::podcasts::format_timestamp;
use crate::youtube::components_directory;
use crate::{ChoiceDialog, ListScreen, ScreenControl, Shell, ShellUi, View, unix_timestamp};

/// Windows `PendingSubscriptionCheck`: the channels still to check, the one
/// being read and what came back.
struct PendingCheck {
    manual: bool,
    queue: VecDeque<(String, String)>,
    current: Option<(u64, String)>,
    results: Vec<SubscriptionCheckResult>,
    errors: Vec<String>,
}

/// The subscription check runs on its own search runtime, so it does not
/// disturb searches (Windows `youtube_subscriptions`).
#[derive(Default)]
pub(crate) struct SubscriptionState {
    search: YoutubeSearchService,
    pending: Option<PendingCheck>,
    next_token: u64,
}

/// The check interval in seconds (0.5 to 168 hours, six by default).
fn check_interval(hours: f64) -> f64 {
    if hours.is_finite() {
        hours.clamp(0.5, 168.0) * 60.0 * 60.0
    } else {
        6.0 * 60.0 * 60.0
    }
}

/// Windows `subscription_label`.
fn subscription_label(
    subscription: &Subscription,
    catalog: &apricot_core::TranslationCatalog,
) -> String {
    let mut parts = vec![subscription.title.clone()];
    if !subscription.category.trim().is_empty() {
        parts.push(
            catalog
                .text("category_value")
                .replace("{category}", &subscription.category),
        );
    }
    let checked = subscription
        .last_checked
        .filter(|timestamp| *timestamp > 0.0)
        .and_then(format_timestamp)
        .map_or_else(
            || catalog.text("subscription_never_checked").to_owned(),
            |time| {
                catalog
                    .text("subscription_last_checked")
                    .replace("{time}", &time)
            },
        );
    parts.push(checked);
    if subscription.last_new_count > 0 {
        parts.push(
            catalog
                .text("subscription_new_videos")
                .replace("{count}", &subscription.last_new_count.to_string())
                .replace("{title}", &subscription.title),
        );
    }
    parts.join(" | ")
}

/// Windows `notification_label`.
fn notification_label(
    notification: &AppNotification,
    catalog: &apricot_core::TranslationCatalog,
) -> String {
    let mut parts = Vec::new();
    if !notification.title.trim().is_empty() {
        parts.push(notification.title.clone());
    }
    if !notification.message.trim().is_empty() {
        parts.push(notification.message.clone());
    }
    if let Some(item) = &notification.item {
        if !item.title.trim().is_empty() {
            parts.push(item.title.clone());
        }
        if !item.channel.trim().is_empty() {
            parts.push(format!("{}: {}", catalog.text("channel"), item.channel));
        }
    }
    if notification.timestamp > 0.0
        && let Some(time) = format_timestamp(notification.timestamp)
    {
        parts.push(time);
    }
    parts.join(" | ")
}

/// Windows `subscription_media_item`: the channel as an item.
pub(crate) fn subscription_media_item(subscription: &Subscription) -> Option<MediaItem> {
    let url = subscription.url.parse().ok()?;
    Some(MediaItem {
        id: apricot_core::MediaId(subscription.url.clone()),
        source: apricot_core::MediaSource::Youtube,
        kind: apricot_core::MediaKind::Channel,
        title: subscription.title.clone(),
        url: Some(url),
        stream_url: None,
        external_audio_url: None,
        local_path: None,
        channel: subscription.title.clone(),
        duration_seconds: None,
        metadata: std::collections::BTreeMap::new(),
    })
}

impl Shell {
    fn subscription_button(&self, id: &'static str, key: &str) -> ScreenControl {
        ScreenControl::Button {
            id,
            label: self.text(key),
        }
    }

    /// `open_subscriptions` and the main menu row.
    pub(crate) fn show_subscriptions(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("subscriptions");
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if self.application.current_route() != Route::Subscriptions {
            self.application.navigate_main_menu();
            self.application
                .navigate_to(RouteFrame::new(Route::Subscriptions));
        }
        self.view = View::Subscriptions;
        self.refresh_subscriptions(ui, true, None);
    }

    /// Python `refresh_subscriptions`: keeps the selected subscription, or
    /// the same row when it is gone, and only an empty list writes the
    /// status bar. The Windows Tab order: Back, Check now, Open videos, New
    /// videos, Remove, Filter category, Set category, the list.
    pub(crate) fn refresh_subscriptions(
        &mut self,
        ui: &mut dyn ShellUi,
        focus: bool,
        preferred_url: Option<&str>,
    ) {
        let catalog = self.catalog();
        let previous_row = (self.view == View::Subscriptions)
            .then(|| ui.selected_row())
            .flatten();
        let previous_url = preferred_url.map(str::to_owned).or_else(|| {
            self.selected_subscription(ui)
                .map(|subscription| subscription.url)
        });
        let visible = self.application.visible_subscription_indices();
        let (rows, selected) = if self.application.subscriptions().is_empty() {
            ui.set_status(catalog.text("subscription_empty"), false);
            (vec![catalog.text("subscription_empty").to_owned()], 0)
        } else if visible.is_empty() {
            ui.set_status(catalog.text("category_filter_empty"), false);
            (vec![catalog.text("category_filter_empty").to_owned()], 0)
        } else {
            let subscriptions = self.application.subscriptions();
            let rows = visible
                .iter()
                .filter_map(|index| subscriptions.get(*index))
                .map(|subscription| subscription_label(subscription, &catalog))
                .collect::<Vec<_>>();
            let selected = previous_url
                .as_deref()
                .and_then(|url| {
                    visible.iter().position(|index| {
                        subscriptions
                            .get(*index)
                            .is_some_and(|subscription| subscription.url == url)
                    })
                })
                .or(previous_row)
                .unwrap_or_default()
                .min(rows.len().saturating_sub(1));
            (rows, selected)
        };
        ui.show_list_screen(&ListScreen {
            list_name: catalog.text("subscriptions").to_owned(),
            rows,
            selected: Some(selected),
            controls: vec![
                self.subscription_button("back", "back"),
                self.subscription_button("subscription_check", "subscription_check_now"),
                self.subscription_button("open", "subscription_open_videos"),
                self.subscription_button("subscription_new", "subscription_new_videos_button"),
                self.subscription_button("collection_remove", "remove"),
                self.subscription_button("subscription_filter", "filter_category"),
                self.subscription_button("subscription_set_category", "set_category"),
                ScreenControl::List,
            ],
            focus_list: focus,
        });
        if let Some(error) = self.application.subscription_load_error() {
            ui.set_status(&format!("Subscriptions could not be loaded: {error}"), true);
        }
    }

    /// `new_subscription_videos` and the main menu row.
    pub(crate) fn show_notification_center(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("notification_center");
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if self.application.current_route() != Route::NotificationCenter {
            self.application.navigate_main_menu();
            self.application
                .navigate_to(RouteFrame::new(Route::NotificationCenter));
        }
        self.view = View::NotificationCenter;
        // Python `show_notification_center` only moves focus to the list.
        self.refresh_notification_center(ui, true, false, None);
    }

    /// Windows `refresh_notification_center` with the Python Tab order:
    /// Back, Play, Clear notifications, the list.
    pub(crate) fn refresh_notification_center(
        &mut self,
        ui: &mut dyn ShellUi,
        focus: bool,
        announce_status: bool,
        selected_index: Option<usize>,
    ) {
        let catalog = self.catalog();
        let previous = (self.view == View::NotificationCenter)
            .then(|| ui.selected_row())
            .flatten();
        let notifications = self.application.notifications();
        let (rows, selected) = if notifications.is_empty() {
            ui.set_status(catalog.text("notification_center_empty"), announce_status);
            (
                vec![catalog.text("notification_center_empty").to_owned()],
                0,
            )
        } else {
            let rows = notifications
                .iter()
                .map(|notification| notification_label(notification, &catalog))
                .collect::<Vec<_>>();
            let selected = selected_index
                .or(previous)
                .unwrap_or_default()
                .min(rows.len() - 1);
            (rows, selected)
        };
        ui.show_list_screen(&ListScreen {
            list_name: catalog.text("notification_center").to_owned(),
            rows,
            selected: Some(selected),
            controls: vec![
                self.subscription_button("back", "back"),
                self.subscription_button("open", "play"),
                self.subscription_button("notification_clear", "clear_notifications"),
                ScreenControl::List,
            ],
            focus_list: focus,
        });
    }

    fn selected_subscription_index(&self, ui: &dyn ShellUi) -> Option<usize> {
        let selected = ui.selected_row()?;
        self.application
            .visible_subscription_indices()
            .get(selected)
            .copied()
    }

    fn selected_subscription(&self, ui: &dyn ShellUi) -> Option<Subscription> {
        if self.view != View::Subscriptions {
            return None;
        }
        let index = self.selected_subscription_index(ui)?;
        self.application.subscriptions().get(index).cloned()
    }

    /// The active item of both screens (Windows `active_media_item`).
    pub(crate) fn subscription_selected_media(&self, ui: &dyn ShellUi) -> Option<MediaItem> {
        match self.view {
            View::Subscriptions => self
                .selected_subscription(ui)
                .as_ref()
                .and_then(subscription_media_item),
            View::NotificationCenter => {
                let index = ui.selected_row()?;
                self.application
                    .notifications()
                    .get(index)
                    .and_then(|notification| notification.item.clone())
            }
            _ => None,
        }
    }

    /// Enter and Open on both screens.
    pub(crate) fn activate_subscription_selection(&mut self, ui: &mut dyn ShellUi) {
        if self.view == View::Subscriptions {
            self.open_selected_subscription_videos(ui);
            return;
        }
        // Windows `activate_notification_selection`.
        let Some(index) = ui.selected_row() else {
            return;
        };
        if let Some(item) = self.application.prepare_notification_playback(index) {
            self.start_media_item(ui, item, None);
        } else {
            ui.set_status(&self.text("notification_center_empty"), true);
        }
    }

    fn open_selected_subscription_videos(&mut self, ui: &mut dyn ShellUi) {
        let Some(item) = self
            .selected_subscription(ui)
            .as_ref()
            .and_then(subscription_media_item)
        else {
            // Python `self.message(self.t("no_selection"))`.
            ui.show_error(&self.text("no_selection"));
            return;
        };
        self.open_youtube_collection(ui, &item, YoutubeCollectionKind::ChannelVideos);
    }

    fn open_selected_subscription_new_videos(&mut self, ui: &mut dyn ShellUi) {
        let Some(subscription) = self.selected_subscription(ui) else {
            ui.show_error(&self.text("no_selection"));
            return;
        };
        let query = self
            .text("subscription_new_videos_title")
            .replace("{title}", &subscription.title);
        if subscription.last_new_items.is_empty()
            || !self
                .application
                .show_saved_subscription_results(query, subscription.last_new_items)
        {
            ui.set_status(&self.text("subscription_no_saved_new_videos"), true);
            return;
        }
        self.application
            .navigate_to(RouteFrame::new(Route::Results));
        self.view = View::Results;
        self.refresh_results(ui, true);
    }

    fn remove_selected_subscription(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = self.selected_subscription_index(ui) else {
            return;
        };
        let Some(title) = self
            .application
            .subscriptions()
            .get(index)
            .map(|subscription| subscription.title.clone())
        else {
            return;
        };
        match self.application.remove_subscription(index) {
            Ok(Some(_)) => {
                self.refresh_subscriptions(ui, true, None);
                let message = self.text("subscription_removed").replace("{title}", &title);
                ui.set_status(&message, true);
            }
            Err(error) => ui.show_error(&error.to_string()),
            Ok(None) => {}
        }
    }

    fn choose_subscription_category_filter(&mut self, ui: &mut dyn ShellUi) {
        let mut choices = vec![self.text("all_categories")];
        choices.extend(self.application.subscription_categories());
        let current = self.application.subscription_category_filter().to_owned();
        let initial = choices
            .iter()
            .position(|choice| !current.is_empty() && choice.eq_ignore_ascii_case(&current))
            .unwrap_or_default();
        let prompt = self.text("category_filter_prompt");
        let Some(index) = ui.choose(&ChoiceDialog {
            title: self.text("filter_category"),
            list_name: prompt.clone(),
            prompt,
            choices: choices.clone(),
            initial,
            ok: self.text("ok"),
            cancel: self.text("cancel"),
        }) else {
            ui.focus_primary_control();
            return;
        };
        let category = choices
            .get(index)
            .filter(|_| index > 0)
            .cloned()
            .unwrap_or_default();
        self.application.set_subscription_category_filter(&category);
        self.refresh_subscriptions(ui, true, None);
        let message = if category.is_empty() {
            self.text("category_filter_all")
        } else {
            self.text("category_filter_applied")
                .replace("{category}", &category)
        };
        ui.set_status(&message, true);
    }

    fn set_selected_subscription_category(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = self.selected_subscription_index(ui) else {
            return;
        };
        let Some(subscription) = self.application.subscriptions().get(index).cloned() else {
            return;
        };
        let Some(category) = ui.prompt_text(
            &self.text("set_category"),
            &self
                .text("category_prompt")
                .replace("{title}", &subscription.title),
            &subscription.category,
            &self.text("ok"),
            &self.text("cancel"),
        ) else {
            ui.focus_primary_control();
            return;
        };
        let category = apricot_app::normalize_category(&category);
        match self.application.set_subscription_category(index, &category) {
            Ok(_) => {
                self.refresh_subscriptions(ui, true, Some(&subscription.url));
                let message = if category.is_empty() {
                    self.text("category_cleared")
                        .replace("{title}", &subscription.title)
                } else {
                    self.text("category_assigned")
                        .replace("{title}", &subscription.title)
                        .replace("{category}", &category)
                };
                ui.set_status(&message, true);
            }
            Err(error) => ui.show_error(&error.to_string()),
        }
    }

    fn remove_notification_at(&mut self, ui: &mut dyn ShellUi, index: usize) {
        match self.application.remove_notification(index) {
            // Python `clear_selected_notification` keeps the row index.
            Ok(Some(_)) => self.refresh_notification_center(ui, true, false, Some(index)),
            Ok(None) => {}
            Err(error) => {
                let message = format!("Notification was not removed: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }

    fn clear_notifications(&mut self, ui: &mut dyn ShellUi) {
        match self.application.clear_notifications() {
            Ok(_) => {
                // Python `clear_notifications` leaves focus where it was.
                self.refresh_notification_center(ui, false, false, None);
                ui.set_status(&self.text("notifications_cleared"), true);
            }
            Err(error) => {
                let message = format!("Notifications were not cleared: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }

    /// Delete and Remove on both screens.
    pub(crate) fn remove_selected_subscription_row(&mut self, ui: &mut dyn ShellUi) {
        if self.view == View::Subscriptions {
            self.remove_selected_subscription(ui);
        } else if let Some(index) = ui.selected_row() {
            self.remove_notification_at(ui, index);
        }
    }

    /// The buttons and menu entries of both screens; `false` for other ids.
    pub(crate) fn subscription_command(&mut self, ui: &mut dyn ShellUi, id: &str) -> bool {
        match id {
            "subscription_check" => self.check_subscriptions(ui, true),
            "subscription_new" => self.open_selected_subscription_new_videos(ui),
            "subscription_filter" => self.choose_subscription_category_filter(ui),
            "subscription_set_category" => self.set_selected_subscription_category(ui),
            "notification_clear" => self.clear_notifications(ui),
            _ => return false,
        }
        true
    }

    /// Windows `list_context_entries` of both screens.
    pub(crate) fn subscription_context_entries(&self) -> Option<Vec<ContextMenuEntry>> {
        let entry = |command: ContextCommand, key: &str| ContextMenuEntry::Command {
            label: self.text(key),
            command,
        };
        let own = |id: &'static str, key: &str| entry(ContextCommand::PlayerAction(id), key);
        Some(match self.view {
            View::NotificationCenter => vec![
                entry(ContextCommand::Play, "play"),
                entry(ContextCommand::CopyLocation, "copy_url"),
                own("notification_clear", "clear_notifications"),
            ],
            View::Subscriptions => vec![
                entry(ContextCommand::Play, "subscription_open_videos"),
                own("subscription_new", "subscription_new_videos_button"),
                own("subscription_check", "subscription_check_now"),
                own("subscription_set_category", "set_category"),
                own("subscription_filter", "filter_category"),
                entry(ContextCommand::CopyLocation, "copy_url"),
                entry(ContextCommand::UnsubscribeChannel, "unsubscribe_channel"),
                entry(ContextCommand::RemoveSelected, "remove"),
            ],
            _ => return None,
        })
    }

    /// `subscribe_channel` on the active item.
    pub(crate) fn subscribe_active_channel(&mut self, ui: &mut dyn ShellUi) {
        let Some(item) = self.active_media_item(ui) else {
            return;
        };
        let title = if item.channel.trim().is_empty() {
            item.title.clone()
        } else {
            item.channel.clone()
        };
        match self.application.subscribe_to_item(&item, unix_timestamp()) {
            Ok(SubscriptionAddOutcome::Added(_)) => {
                ui.set_status(
                    &self.text("subscription_added").replace("{title}", &title),
                    true,
                );
            }
            Ok(SubscriptionAddOutcome::AlreadyPresent) => {
                ui.set_status(
                    &self.text("subscription_exists").replace("{title}", &title),
                    true,
                );
            }
            Ok(SubscriptionAddOutcome::Unsupported) => {
                ui.set_status(&self.text("no_selection"), true);
            }
            Err(error) => ui.show_error(&error.to_string()),
        }
    }

    /// `unsubscribe_channel` on the active item; in the subscription list
    /// it removes the selected subscription.
    pub(crate) fn unsubscribe_active_channel(&mut self, ui: &mut dyn ShellUi) {
        if self.view == View::Subscriptions {
            self.remove_selected_subscription(ui);
            return;
        }
        let Some(item) = self.active_media_item(ui) else {
            return;
        };
        let title = if item.channel.trim().is_empty() {
            item.title.clone()
        } else {
            item.channel.clone()
        };
        match self.application.unsubscribe_from_item(&item) {
            Ok(SubscriptionRemoveOutcome::Removed) => {
                ui.set_status(
                    &self.text("subscription_removed").replace("{title}", &title),
                    true,
                );
            }
            Ok(SubscriptionRemoveOutcome::NotFound) => {
                let message = self
                    .text("subscription_not_found")
                    .replace("{title}", &title);
                ui.set_status(&message, true);
            }
            Ok(SubscriptionRemoveOutcome::Unsupported) => {
                ui.set_status(&self.text("no_selection"), true);
            }
            Err(error) => ui.show_error(&error.to_string()),
        }
    }

    /// Windows `configure_subscription_timer`.
    pub(crate) fn configure_subscription_timer(&self, ui: &mut dyn ShellUi) {
        let settings = self.application.settings();
        if !settings.subscription_check_enabled {
            ui.set_subscription_timer(None);
            return;
        }
        ui.set_subscription_timer(Some(check_interval(
            settings.subscription_check_interval_hours,
        )));
    }

    /// Windows `check_subscriptions_if_due`: at startup, after the
    /// settings and on the timer.
    pub fn check_subscriptions_if_due(&mut self, ui: &mut dyn ShellUi) {
        let settings = self.application.settings();
        if !settings.subscription_check_enabled || self.application.subscriptions().is_empty() {
            return;
        }
        let interval = check_interval(settings.subscription_check_interval_hours);
        let last = settings.last_subscription_check;
        if !last.is_finite() || unix_timestamp() - last.max(0.0) >= interval {
            self.check_subscriptions(ui, false);
        }
    }

    /// Windows `check_subscriptions`: every channel in turn, five newest
    /// videos each. A manual check says what it does.
    pub fn check_subscriptions(&mut self, ui: &mut dyn ShellUi, manual: bool) {
        if self.subscriptions.pending.is_some() {
            return;
        }
        let queue = self
            .application
            .subscriptions()
            .iter()
            .map(|subscription| (subscription.url.clone(), subscription.title.clone()))
            .collect::<VecDeque<_>>();
        if queue.is_empty() {
            if manual {
                ui.set_status(&self.text("subscription_empty"), true);
            }
            return;
        }
        self.subscriptions.pending = Some(PendingCheck {
            manual,
            queue,
            current: None,
            results: Vec::new(),
            errors: Vec::new(),
        });
        if manual {
            ui.set_status(&self.text("subscription_checking"), true);
        }
        ui.set_background_polling(true);
        self.start_next_subscription_check(ui);
    }

    /// Windows `start_next_subscription_check`.
    fn start_next_subscription_check(&mut self, ui: &mut dyn ShellUi) {
        loop {
            let Some((url, _title)) = self
                .subscriptions
                .pending
                .as_mut()
                .and_then(|pending| pending.queue.pop_front())
            else {
                self.finish_subscription_check(ui);
                return;
            };
            self.subscriptions.next_token = self.subscriptions.next_token.wrapping_add(1).max(1);
            let token = self.subscriptions.next_token;
            if let Some(pending) = self.subscriptions.pending.as_mut() {
                pending.current = Some((token, url.clone()));
            }
            let config = self.youtube_session_config();
            let started = match components_directory() {
                Some(components) => self
                    .subscriptions
                    .search
                    .start_collection(
                        YoutubeBackend::YtDlp,
                        &components,
                        config,
                        token,
                        url.clone(),
                        YoutubeCollectionKind::ChannelVideos,
                        5,
                    )
                    .map_err(|error| error.to_string()),
                None => Err("Application path is unavailable".to_owned()),
            };
            match started {
                Ok(()) => return,
                Err(message) => {
                    if let Some(pending) = self.subscriptions.pending.as_mut() {
                        pending.current = None;
                        pending.errors.push(message.clone());
                        pending.results.push(SubscriptionCheckResult {
                            url,
                            result: Err(message),
                        });
                    }
                }
            }
        }
    }

    /// Windows `poll_subscription_runtime`; `true` while a check runs.
    pub(crate) fn poll_subscription_check(&mut self, ui: &mut dyn ShellUi) -> bool {
        if self.subscriptions.pending.is_none() {
            return false;
        }
        let (token, result) = match self.subscriptions.search.poll() {
            Ok(None) => return true,
            Ok(Some(YoutubeSearchServiceUpdate::Results { token, items, .. })) => {
                (token, Ok(items))
            }
            Ok(Some(YoutubeSearchServiceUpdate::Failed { token, message })) => {
                (token, Err(message))
            }
            Ok(Some(
                YoutubeSearchServiceUpdate::Resolved { token, .. }
                | YoutubeSearchServiceUpdate::Hydrated { token, .. },
            )) => (
                token,
                Err("YouTube component returned an unexpected subscription response".to_owned()),
            ),
            Err(error) => (0, Err(error.to_string())),
        };
        self.finish_subscription_item(ui, token, result);
        self.subscriptions.pending.is_some()
    }

    /// Windows `finish_subscription_item`.
    fn finish_subscription_item(
        &mut self,
        ui: &mut dyn ShellUi,
        token: u64,
        result: Result<Vec<MediaItem>, String>,
    ) {
        let Some(pending) = self.subscriptions.pending.as_mut() else {
            return;
        };
        let Some((current, url)) = pending.current.take() else {
            return;
        };
        if token != 0 && current != token {
            pending.current = Some((current, url));
            return;
        }
        if let Err(error) = &result {
            pending.errors.push(error.clone());
        }
        pending
            .results
            .push(SubscriptionCheckResult { url, result });
        self.start_next_subscription_check(ui);
    }

    /// Windows `finish_subscription_check`: the results are saved, each new
    /// video goes to the notification center (and the system notifications
    /// when they are on), each channel's count is announced, and a manual
    /// check says how it ended.
    fn finish_subscription_check(&mut self, ui: &mut dyn ShellUi) {
        let Some(pending) = self.subscriptions.pending.take() else {
            return;
        };
        let timestamp = unix_timestamp();
        let summary = match self
            .application
            .apply_subscription_checks(pending.results, timestamp)
        {
            Ok(summary) => summary,
            Err(error) => {
                ui.show_error(&error.to_string());
                return;
            }
        };
        if summary.successes > 0
            && let Err(error) = self.application.record_subscription_check(timestamp)
        {
            ui.set_status(
                &format!("Subscription check time was not saved: {error}"),
                true,
            );
        }
        let catalog = self.catalog();
        let notification_title = catalog.text("notification_subscription_title").to_owned();
        let settings = self.application.settings();
        let system = settings.windows_notifications && settings.subscription_notifications;
        let mut channel_counts: Vec<(String, usize)> = Vec::new();
        for (channel, item) in summary.new_items {
            if let Some((_, count)) = channel_counts
                .iter_mut()
                .find(|(known, _)| known == &channel)
            {
                *count += 1;
            } else {
                channel_counts.push((channel.clone(), 1));
            }
            let message = catalog
                .text("notification_new_video")
                .replace("{channel}", &channel)
                .replace("{title}", &item.title);
            if let Err(error) = self.application.add_notification(AppNotification::new(
                "subscription",
                &notification_title,
                &message,
                Some(item),
                timestamp,
            )) {
                ui.set_status(
                    &format!("Subscription notification was not saved: {error}"),
                    true,
                );
            }
            if system {
                ui.show_system_notification(&notification_title, &message);
            }
        }
        for (channel, count) in channel_counts {
            let message = catalog
                .text("subscription_new_videos")
                .replace("{count}", &count.to_string())
                .replace("{title}", &channel);
            ui.set_status(&message, true);
        }
        if self.view == View::Subscriptions {
            // Focus stays where it is while the settings are open.
            let focus = !self.settings_window_open();
            self.refresh_subscriptions(ui, focus, None);
        }
        if pending.manual {
            let message = if summary.successes == 0 && summary.failures > 0 {
                catalog.text("subscription_check_failed").replace(
                    "{error}",
                    pending
                        .errors
                        .last()
                        .map_or("Unknown error", String::as_str),
                )
            } else if summary.total_new > 0 || summary.failures > 0 {
                catalog.text("subscription_check_complete").to_owned()
            } else {
                catalog.text("subscription_no_new").to_owned()
            };
            ui.set_status(&message, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_interval_stays_between_half_an_hour_and_a_week() {
        assert!((check_interval(0.1) - 1800.0).abs() < f64::EPSILON);
        assert!((check_interval(1000.0) - 168.0 * 3600.0).abs() < f64::EPSILON);
        assert!((check_interval(f64::NAN) - 6.0 * 3600.0).abs() < f64::EPSILON);
    }
}
