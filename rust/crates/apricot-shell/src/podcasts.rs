//! Podcasts and RSS feeds (Windows `show_rss_feeds`, `refresh_rss_feeds`,
//! `rss_feed_label`, `open_selected_rss_feed`, `refresh_rss_items`,
//! `maybe_extend_rss_items`, `rss_episode_label`, `play_selected_rss_episode`,
//! `prompt_add_rss_feed`, `prompt_podcast_search`, the podcast work and its
//! polling, the directory results and categories, category filter and
//! assignment, speed presets, played state and progress, OPML import and
//! export, the auto-refresh timer and the list context menus).

use std::collections::HashSet;
use std::io::Read;

use apricot_app::context_menu::{ContextCommand, ContextMenuEntry};
use apricot_app::{RssFeed, RssFeedAddOutcome};
use apricot_core::{MediaItem, MediaKind, Route, RouteFrame, TranslationCatalog};
use apricot_media::PodcastDirectoryItem;

use crate::podcast_work::{self, PendingPodcastWork, PodcastWorkResult};
use crate::{ChoiceDialog, ListScreen, ScreenControl, Shell, ShellUi, View, format_duration};

/// Windows `PODCAST_GENRES`.
const PODCAST_GENRES: [(&str, u32); 10] = [
    ("genre_arts", 1301),
    ("genre_business", 1304),
    ("genre_comedy", 1303),
    ("genre_education", 1307),
    ("genre_music", 1310),
    ("genre_news", 1311),
    ("genre_science", 1315),
    ("genre_sports", 1316),
    ("genre_technology", 1318),
    ("genre_true_crime", 1324),
];

/// Windows `PODCAST_SPEED_STEPS`.
const PODCAST_SPEED_STEPS: [f64; 19] = [
    0.25, 0.5, 0.6, 0.7, 0.75, 0.8, 0.9, 1.0, 1.1, 1.2, 1.25, 1.3, 1.4, 1.5, 1.75, 2.0, 2.5, 3.0,
    4.0,
];

/// The podcast screens' state (Windows `current_rss_feed_index`,
/// `current_rss_item_index`, `rss_visible_item_count`,
/// `podcast_search_query`, `podcast_search_results` and
/// `pending_podcast_work`).
#[derive(Default)]
pub(crate) struct PodcastState {
    feed: usize,
    item: usize,
    visible_items: usize,
    search_query: String,
    search_results: Vec<PodcastDirectoryItem>,
    pending: Option<PendingPodcastWork>,
}

fn unix_timestamp() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
}

/// Windows `format_timestamp`.
fn format_timestamp(timestamp: f64) -> Option<String> {
    std::time::Duration::try_from_secs_f64(timestamp)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|timestamp| {
            timestamp
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
}

fn metadata_bool(item: &MediaItem, key: &str) -> bool {
    item.metadata
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn metadata_number(item: &MediaItem, key: &str) -> Option<f64> {
    item.metadata.get(key).and_then(serde_json::Value::as_f64)
}

fn metadata_text(item: &MediaItem, key: &str) -> Option<String> {
    item.metadata
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Windows `format_rate`.
fn format_rate(rate: f64) -> String {
    let value = format!("{rate:.2}");
    value.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Windows `rss_feed_label`.
#[must_use]
pub fn rss_feed_label(feed: &RssFeed, catalog: &TranslationCatalog) -> String {
    let checked = feed
        .last_checked
        .and_then(format_timestamp)
        .unwrap_or_else(|| catalog.text("rss_feed_never_checked").to_owned());
    let played_count = feed
        .items
        .iter()
        .filter(|item| metadata_bool(item, "played"))
        .count();
    let mut parts = vec![if feed.title.trim().is_empty() {
        catalog.text("rss_unknown_feed_title").to_owned()
    } else {
        feed.title.clone()
    }];
    if !feed.category.trim().is_empty() {
        parts.push(
            catalog
                .text("category_value")
                .replace("{category}", &feed.category),
        );
    }
    parts.push(
        catalog
            .text("rss_feed_item_count")
            .replace("{count}", &feed.items.len().to_string()),
    );
    if played_count > 0 {
        parts.push(
            catalog
                .text("rss_feed_played_count")
                .replace("{count}", &played_count.to_string()),
        );
    }
    if let Some(speed) = feed.speed_preset {
        parts.push(
            catalog
                .text("podcast_speed_preset_marker")
                .replace("{speed}", &format_rate(speed)),
        );
    }
    parts.push(if feed.last_checked.is_some() {
        catalog
            .text("rss_feed_last_checked")
            .replace("{time}", &checked)
    } else {
        checked
    });
    parts.join(" | ")
}

/// Windows `rss_episode_label`.
#[must_use]
pub fn rss_episode_label(
    item: &MediaItem,
    catalog: &TranslationCatalog,
    resume_position: Option<f64>,
    queued: bool,
) -> String {
    let mut parts = vec![item.title.clone()];
    if metadata_bool(item, "played") {
        parts.push(catalog.text("played_marker").to_owned());
    }
    if let Some(position) = resume_position {
        parts.push(
            catalog
                .text("episode_resume_marker")
                .replace("{time}", &format_duration(position)),
        );
    }
    if let Some(timestamp) = metadata_number(item, "timestamp").and_then(format_timestamp) {
        parts.push(format!("{}: {timestamp}", catalog.text("published")));
    }
    if let Some(duration) = metadata_text(item, "duration")
        .filter(|duration| !duration.is_empty())
        .or_else(|| item.duration_seconds.map(format_duration))
    {
        parts.push(duration);
    }
    parts.push(catalog.text("podcast_episode").to_owned());
    if queued {
        parts.push(catalog.text("podcast_audio_queued_marker").to_owned());
    }
    parts.join(" | ")
}

/// Windows `podcast_directory_label`.
fn podcast_directory_label(item: &PodcastDirectoryItem, catalog: &TranslationCatalog) -> String {
    let mut parts = vec![item.title.clone()];
    if !item.author.is_empty() {
        parts.push(format!(
            "{}: {}",
            catalog.text("podcast_author"),
            item.author
        ));
    }
    if !item.genre.is_empty() {
        parts.push(format!("{}: {}", catalog.text("podcast_genre"), item.genre));
    }
    if item.episode_count > 0 {
        parts.push(
            catalog
                .text("podcast_episode_count")
                .replace("{count}", &item.episode_count.to_string()),
        );
    }
    parts.join(" | ")
}

impl Shell {
    fn podcast_button(&self, id: &'static str, key: &str) -> ScreenControl {
        ScreenControl::Button {
            id,
            label: self.text(key),
        }
    }

    fn unknown_feed_title(&self) -> String {
        self.text("rss_unknown_feed_title")
    }

    /// Windows `show_rss_feeds`.
    pub(crate) fn show_rss_feeds(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("rss_feeds");
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if !self.application.settings().enable_podcasts_rss {
            self.show_main_menu(ui);
            return;
        }
        if self.application.current_route() != Route::RssFeeds {
            self.application.navigate_main_menu();
            self.application
                .navigate_to(RouteFrame::new(Route::RssFeeds));
        }
        self.view = View::RssFeeds;
        self.refresh_rss_feeds(ui, true, None);
    }

    /// Windows `selected_rss_feed_index`.
    fn selected_rss_feed_index(&self, ui: &dyn ShellUi) -> Option<usize> {
        let selected = ui.selected_row()?;
        self.application
            .visible_rss_feed_indices()
            .get(selected)
            .copied()
    }

    /// Windows `refresh_rss_feeds`: only an empty list writes the status
    /// line. The Tab order is Back, Search podcasts, Podcast categories, Add
    /// feed, Refresh feeds, Open feed, Remove feed, Filter category, Set
    /// category, Import OPML, Export OPML, the list.
    pub(crate) fn refresh_rss_feeds(
        &mut self,
        ui: &mut dyn ShellUi,
        focus: bool,
        preferred_url: Option<&str>,
    ) {
        let previous_url = preferred_url.map(str::to_owned).or_else(|| {
            (self.view == View::RssFeeds)
                .then(|| self.selected_rss_feed_index(ui))
                .flatten()
                .and_then(|index| self.application.rss_feeds().get(index))
                .map(|feed| feed.url.clone())
        });
        let catalog = self.catalog();
        let visible = self.application.visible_rss_feed_indices();
        let (rows, selected) = if self.application.rss_feeds().is_empty() {
            ui.set_status(catalog.text("rss_feeds_empty"), false);
            (vec![catalog.text("rss_feeds_empty").to_owned()], 0)
        } else if visible.is_empty() {
            ui.set_status(catalog.text("category_filter_empty"), false);
            (vec![catalog.text("category_filter_empty").to_owned()], 0)
        } else {
            let feeds = self.application.rss_feeds();
            let rows = visible
                .iter()
                .filter_map(|index| feeds.get(*index))
                .map(|feed| rss_feed_label(feed, &catalog))
                .collect();
            let selected = previous_url
                .as_deref()
                .and_then(|url| {
                    visible.iter().position(|index| {
                        feeds
                            .get(*index)
                            .is_some_and(|feed| feed.url.eq_ignore_ascii_case(url))
                    })
                })
                .or_else(|| {
                    visible
                        .iter()
                        .position(|index| *index == self.podcasts.feed)
                })
                .unwrap_or_default();
            self.podcasts.feed = visible[selected];
            (rows, selected)
        };
        ui.show_list_screen(&ListScreen {
            list_name: catalog.text("rss_feeds").to_owned(),
            rows,
            selected: Some(selected),
            controls: vec![
                self.podcast_button("back", "back"),
                self.podcast_button("rss_search", "search_podcasts"),
                self.podcast_button("rss_categories", "podcast_categories"),
                self.podcast_button("rss_add", "add_rss_feed"),
                self.podcast_button("rss_refresh", "refresh_feeds"),
                self.podcast_button("open", "open_feed"),
                self.podcast_button("collection_remove", "remove_feed"),
                self.podcast_button("rss_filter", "filter_category"),
                self.podcast_button("rss_set_category", "set_category"),
                self.podcast_button("rss_import", "import_opml"),
                self.podcast_button("rss_export", "export_opml"),
                ScreenControl::List,
            ],
            focus_list: focus,
        });
        if let Some(error) = self.application.rss_feed_load_error() {
            ui.show_error(&format!("Podcast feeds could not be loaded: {error}"));
        }
    }

    /// Windows `open_selected_rss_feed`: a feed saved without all items is
    /// refreshed in the background.
    fn open_selected_rss_feed(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = self.selected_rss_feed_index(ui) else {
            return;
        };
        let refresh_legacy = self
            .application
            .rss_feeds()
            .get(index)
            .is_some_and(|feed| feed.items_complete != Some(true));
        self.podcasts.feed = index;
        self.podcasts.item = 0;
        self.podcasts.visible_items = 0;
        let mut frame = RouteFrame::new(Route::RssItems);
        frame
            .parameters
            .insert("feed_index".to_owned(), serde_json::Value::from(index));
        self.application.navigate_to(frame);
        self.view = View::RssItems;
        self.refresh_rss_items(ui, true, false, None);
        if refresh_legacy {
            self.refresh_rss_feed_background(ui, index);
        }
    }

    fn rss_episode_row(&self, item: &MediaItem, catalog: &TranslationCatalog) -> String {
        rss_episode_label(
            item,
            catalog,
            (!metadata_bool(item, "played"))
                .then(|| self.application.playback_resume_position(item))
                .flatten(),
            self.application.downloads().queued_item(item).is_some(),
        )
    }

    /// Windows `refresh_rss_items`: a batch of `rss_max_items` episodes,
    /// more on End or Down at the last row. The Tab order is Back, Refresh
    /// feed, Play episode, Download episode, Download feed, the list.
    pub(crate) fn refresh_rss_items(
        &mut self,
        ui: &mut dyn ShellUi,
        focus: bool,
        announce_status: bool,
        selected: Option<usize>,
    ) {
        let catalog = self.catalog();
        let feed = self.application.rss_feeds().get(self.podcasts.feed);
        let (rows, row, status) = match feed {
            Some(feed) if !feed.items.is_empty() => {
                let batch =
                    usize::try_from(self.application.settings().rss_max_items.clamp(25, 500))
                        .unwrap_or(100);
                self.podcasts.visible_items =
                    self.podcasts.visible_items.max(batch).min(feed.items.len());
                let rows = feed.items[..self.podcasts.visible_items]
                    .iter()
                    .map(|item| self.rss_episode_row(item, &catalog))
                    .collect();
                self.podcasts.item = selected
                    .unwrap_or(self.podcasts.item)
                    .min(self.podcasts.visible_items.saturating_sub(1));
                let status = format!(
                    "{}: {} of {}",
                    feed.title,
                    self.podcasts.visible_items,
                    feed.items.len()
                );
                (rows, self.podcasts.item, status)
            }
            _ => {
                self.podcasts.visible_items = 0;
                let empty = catalog.text("rss_items_empty").to_owned();
                (vec![empty.clone()], 0, empty)
            }
        };
        ui.show_list_screen(&ListScreen {
            list_name: catalog.text("rss_feed_items").to_owned(),
            rows,
            selected: Some(row),
            controls: vec![
                self.podcast_button("back", "back"),
                self.podcast_button("rss_refresh", "refresh_feed"),
                self.podcast_button("open", "play_episode"),
                self.podcast_button("rss_download_episode", "download_episode_audio"),
                self.podcast_button("rss_download_feed", "download_feed"),
                ScreenControl::List,
            ],
            focus_list: focus,
        });
        ui.set_status(&status, announce_status);
    }

    /// Windows `maybe_extend_rss_items`: End or Down at the last row loads
    /// the next batch.
    pub(crate) fn maybe_extend_rss_items(&mut self, ui: &mut dyn ShellUi) {
        let total = self
            .application
            .rss_feeds()
            .get(self.podcasts.feed)
            .map_or(0, |feed| feed.items.len());
        if self.podcasts.visible_items >= total {
            return;
        }
        let selected = ui.selected_row().unwrap_or_default();
        let batch = usize::try_from(self.application.settings().rss_max_items.clamp(25, 500))
            .unwrap_or(100);
        let before = self.podcasts.visible_items;
        self.podcasts.visible_items = self.podcasts.visible_items.saturating_add(batch);
        self.refresh_rss_items(ui, false, false, Some(selected));
        let added = self.podcasts.visible_items.saturating_sub(before);
        let message = self
            .text("podcast_more_episodes_loaded")
            .replace("{count}", &added.to_string());
        ui.set_status(&message, false);
    }

    /// Windows `selected_rss_episode`.
    pub(crate) fn selected_rss_episode(&self, ui: &dyn ShellUi) -> Option<MediaItem> {
        let selected = ui.selected_row()?;
        if selected >= self.podcasts.visible_items {
            return None;
        }
        self.application
            .rss_feeds()
            .get(self.podcasts.feed)?
            .items
            .get(selected)
            .cloned()
    }

    /// Windows `play_selected_rss_episode`.
    fn play_selected_rss_episode(&mut self, ui: &mut dyn ShellUi) {
        let Some(item_index) = ui
            .selected_row()
            .filter(|selected| *selected < self.podcasts.visible_items)
        else {
            return;
        };
        self.podcasts.item = item_index;
        match self
            .application
            .prepare_rss_episode_playback(self.podcasts.feed, item_index)
        {
            Some(item) => self.start_sequence_media_item(ui, item, None),
            None => ui.set_status(&self.text("no_selection"), true),
        }
    }

    /// Enter or Open on a podcast screen (Windows `activate_selection`).
    pub(crate) fn activate_podcast_selection(&mut self, ui: &mut dyn ShellUi) {
        match self.view {
            View::RssFeeds => self.open_selected_rss_feed(ui),
            View::RssItems => self.play_selected_rss_episode(ui),
            View::PodcastSearchResults => self.add_selected_podcast_result(ui),
            View::PodcastCategories => self.open_selected_podcast_category(ui),
            _ => {}
        }
    }

    /// Windows `result_selection_changed` on the episode list.
    pub(crate) fn podcast_selection_changed(&mut self, index: usize) {
        if self.view == View::RssItems && index < self.podcasts.visible_items {
            self.podcasts.item = index;
        } else if self.view == View::RssFeeds
            && let Some(feed) = self.application.visible_rss_feed_indices().get(index)
        {
            self.podcasts.feed = *feed;
        }
    }

    /// Windows `prompt_add_rss_feed`: a link without a scheme gets
    /// `https://`.
    fn prompt_add_rss_feed(&mut self, ui: &mut dyn ShellUi) {
        let Some(url) = ui.prompt_text(
            &self.text("add_rss_feed"),
            &self.text("rss_feed_url"),
            "",
            &self.text("ok"),
            &self.text("cancel"),
        ) else {
            return;
        };
        if url.trim().is_empty() {
            return;
        }
        let lowered = url.trim().to_ascii_lowercase();
        let url = if lowered.starts_with("http://") || lowered.starts_with("https://") {
            url.trim().to_owned()
        } else {
            format!("https://{}", url.trim())
        };
        let work = podcast_work::add_feed(
            url,
            self.application.settings().proxy.clone(),
            self.unknown_feed_title(),
            unix_timestamp(),
        );
        self.start_podcast_work(ui, work, &self.text("rss_refresh_started"));
    }

    /// Windows `prompt_podcast_search`.
    fn prompt_podcast_search(&mut self, ui: &mut dyn ShellUi) {
        let Some(query) = ui.prompt_text(
            &self.text("search_podcasts"),
            &self.text("podcast_search_query"),
            "",
            &self.text("search"),
            &self.text("cancel"),
        ) else {
            return;
        };
        let query = query.trim().to_owned();
        if query.is_empty() {
            return;
        }
        let settings = self.application.settings();
        let work = podcast_work::search_directory(
            query.clone(),
            settings.podcast_search_country.clone(),
            u32::try_from(settings.podcast_search_limit.clamp(1, 200)).unwrap_or(20),
            settings.proxy.clone(),
        );
        let message = self.text("podcast_searching").replace("{query}", &query);
        self.start_podcast_work(ui, work, &message);
    }

    /// Windows `start_podcast_work_with_message`.
    fn start_podcast_work(
        &mut self,
        ui: &mut dyn ShellUi,
        work: PendingPodcastWork,
        message: &str,
    ) {
        if self.podcasts.pending.is_some() {
            ui.set_status("A podcast operation is already in progress.", true);
            return;
        }
        self.podcasts.pending = Some(work);
        ui.set_status(message, true);
        ui.set_background_polling(true);
    }

    fn refresh_work(&self, feeds: Vec<(String, String)>, silent: bool) -> PendingPodcastWork {
        podcast_work::refresh_feeds(
            feeds,
            self.application.settings().proxy.clone(),
            self.unknown_feed_title(),
            unix_timestamp(),
            silent,
        )
    }

    /// Windows `refresh_rss_from_active_view`: every feed on the feed list,
    /// the open feed on the episode list.
    fn refresh_rss_from_active_view(&mut self, ui: &mut dyn ShellUi) {
        let feeds: Vec<(String, String)> = match self.view {
            View::RssItems => self
                .application
                .rss_feeds()
                .get(self.podcasts.feed)
                .map(|feed| vec![(feed.url.clone(), feed.url.clone())])
                .unwrap_or_default(),
            View::RssFeeds => self
                .application
                .rss_feeds()
                .iter()
                .map(|feed| (feed.url.clone(), feed.url.clone()))
                .collect(),
            _ => Vec::new(),
        };
        if feeds.is_empty() {
            let key = if self.view == View::RssFeeds {
                "rss_feeds_empty"
            } else {
                "rss_items_empty"
            };
            ui.set_status(&self.text(key), true);
            return;
        }
        let work = self.refresh_work(feeds, false);
        self.start_podcast_work(ui, work, &self.text("rss_refresh_started"));
    }

    /// Windows `refresh_selected_rss_feed`.
    fn refresh_selected_rss_feed(&mut self, ui: &mut dyn ShellUi) {
        let Some(url) = self
            .selected_rss_feed_index(ui)
            .and_then(|index| self.application.rss_feeds().get(index))
            .map(|feed| feed.url.clone())
        else {
            return;
        };
        let work = self.refresh_work(vec![(url.clone(), url)], false);
        self.start_podcast_work(ui, work, &self.text("rss_refresh_started"));
    }

    /// Windows `refresh_all_rss_feeds_background`: the timer and the start.
    pub fn refresh_all_rss_feeds_background(&mut self, ui: &mut dyn ShellUi) {
        if self.podcasts.pending.is_some()
            || !self.application.settings().enable_podcasts_rss
            || self.application.rss_feeds().is_empty()
        {
            return;
        }
        let feeds = self
            .application
            .rss_feeds()
            .iter()
            .map(|feed| (feed.url.clone(), feed.url.clone()))
            .collect();
        self.podcasts.pending = Some(self.refresh_work(feeds, true));
        ui.set_background_polling(true);
    }

    /// Windows `refresh_rss_feed_background`.
    fn refresh_rss_feed_background(&mut self, ui: &mut dyn ShellUi, feed_index: usize) {
        if self.podcasts.pending.is_some() {
            return;
        }
        let Some(url) = self
            .application
            .rss_feeds()
            .get(feed_index)
            .map(|feed| feed.url.clone())
        else {
            return;
        };
        self.podcasts.pending = Some(self.refresh_work(vec![(url.clone(), url)], true));
        ui.set_background_polling(true);
    }

    /// Windows `configure_rss_timer` and `refresh_rss_feeds_on_startup`.
    pub(crate) fn start_rss(&mut self, ui: &mut dyn ShellUi) {
        self.configure_rss_timer(ui);
        let settings = self.application.settings();
        if settings.enable_podcasts_rss && settings.rss_refresh_on_startup {
            self.refresh_all_rss_feeds_background(ui);
        }
    }

    /// Windows `configure_rss_timer`.
    pub(crate) fn configure_rss_timer(&self, ui: &mut dyn ShellUi) {
        let settings = self.application.settings();
        if !settings.enable_podcasts_rss || !settings.rss_auto_refresh_enabled {
            ui.set_rss_timer(None);
            return;
        }
        let hours = settings.rss_refresh_interval_hours;
        let seconds = if hours.is_finite() {
            hours.clamp(0.5, 168.0) * 60.0 * 60.0
        } else {
            12.0 * 60.0 * 60.0
        };
        ui.set_rss_timer(Some(seconds));
    }

    /// Windows `poll_podcast_work`; `true` while work runs.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn poll_podcast_work(&mut self, ui: &mut dyn ShellUi) -> bool {
        let Some(pending) = self.podcasts.pending.as_ref() else {
            return false;
        };
        let outcome = match pending.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                PodcastWorkResult::FeedAdded(Err("Podcast worker stopped unexpectedly".to_owned()))
            }
        };
        self.podcasts.pending = None;
        match outcome {
            PodcastWorkResult::FeedAdded(Ok(feed)) => {
                let title = feed.title.clone();
                match self.application.add_rss_feed(*feed) {
                    Ok(RssFeedAddOutcome::Added(_)) => {
                        let message = self.text("rss_feed_added").replace("{title}", &title);
                        ui.set_status(&message, true);
                        if self.view == View::RssFeeds {
                            self.refresh_rss_feeds(ui, false, None);
                        }
                    }
                    Ok(RssFeedAddOutcome::AlreadyPresent) => {
                        ui.set_status(&self.text("rss_feed_exists"), true);
                    }
                    Err(error) => ui.show_error(&error.to_string()),
                }
            }
            PodcastWorkResult::FeedAdded(Err(error)) => {
                let message = self.text("rss_refresh_failed").replace("{error}", &error);
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
            PodcastWorkResult::FeedsRefreshed { results, silent } => {
                match self
                    .application
                    .apply_rss_refreshes(results, unix_timestamp())
                {
                    Ok(summary) => {
                        let timestamp = unix_timestamp();
                        let catalog = self.catalog();
                        for (feed, item) in &summary.new_items {
                            let message = catalog
                                .text("notification_new_podcast")
                                .replace("{feed}", feed)
                                .replace("{title}", &item.title);
                            let _ = self.application.add_notification(
                                apricot_app::AppNotification::new(
                                    "podcast",
                                    catalog.text("rss_feeds"),
                                    &message,
                                    Some(item.clone()),
                                    timestamp,
                                ),
                            );
                            // The system notification arrives with the menu
                            // bar and notifications (unit M14).
                        }
                        if !silent {
                            let message = if summary.failures == 0 {
                                self.text("rss_refresh_done")
                            } else {
                                format!(
                                    "{} {} succeeded, {} failed.",
                                    self.text("rss_refresh_done"),
                                    summary.successes,
                                    summary.failures
                                )
                            };
                            ui.set_status(&message, true);
                        }
                        if self.view == View::RssFeeds {
                            self.refresh_rss_feeds(ui, false, None);
                        } else if self.view == View::RssItems {
                            let selected = ui.selected_row();
                            self.refresh_rss_items(ui, false, false, selected);
                        }
                    }
                    Err(error) => ui.show_error(&error.to_string()),
                }
            }
            PodcastWorkResult::DirectorySearched { query, result } => match result {
                Ok(results) => {
                    self.podcasts.search_query = query;
                    self.podcasts.search_results = results;
                    self.application
                        .navigate_to(RouteFrame::new(Route::PodcastSearchResults));
                    self.view = View::PodcastSearchResults;
                    self.refresh_podcast_directory_results(ui, true, None);
                }
                Err(error) => {
                    let message = self
                        .text("podcast_search_failed")
                        .replace("{error}", &error);
                    ui.set_status(&message, true);
                    ui.show_error(&message);
                }
            },
            PodcastWorkResult::CategoryLoaded { category, result } => match result {
                Ok(results) => {
                    self.podcasts.search_query = category;
                    self.podcasts.search_results = results;
                    let _ = self.application.navigate_back();
                    self.application
                        .navigate_to(RouteFrame::new(Route::PodcastSearchResults));
                    self.view = View::PodcastSearchResults;
                    self.refresh_podcast_directory_results(ui, true, None);
                }
                Err(error) => {
                    let message = self
                        .text("podcast_search_failed")
                        .replace("{error}", &error);
                    ui.set_status(&message, true);
                    ui.show_error(&message);
                }
            },
            PodcastWorkResult::FeedsImported { feeds, failures } => {
                match self.application.import_rss_feeds(feeds) {
                    Ok(summary) => {
                        if self.view == View::RssFeeds {
                            self.refresh_rss_feeds(ui, false, None);
                        }
                        let key = if failures == 0 {
                            "opml_import_done"
                        } else {
                            "opml_import_done_with_errors"
                        };
                        let message = self
                            .text(key)
                            .replace("{count}", &summary.added.to_string())
                            .replace("{imported}", &summary.added.to_string())
                            .replace("{failed}", &failures.to_string());
                        ui.set_status(&message, true);
                    }
                    Err(error) => ui.show_error(&error.to_string()),
                }
            }
        }
        false
    }

    /// Windows `refresh_podcast_directory_results`: Back, Add podcast, Open
    /// in browser, the list.
    fn refresh_podcast_directory_results(
        &mut self,
        ui: &mut dyn ShellUi,
        focus: bool,
        selected: Option<usize>,
    ) {
        let catalog = self.catalog();
        let (rows, status) = if self.podcasts.search_results.is_empty() {
            let empty = catalog.text("podcast_search_empty").to_owned();
            (vec![empty.clone()], empty)
        } else {
            (
                self.podcasts
                    .search_results
                    .iter()
                    .map(|item| podcast_directory_label(item, &catalog))
                    .collect(),
                catalog
                    .text("podcast_search_done")
                    .replace("{count}", &self.podcasts.search_results.len().to_string()),
            )
        };
        let selected = selected.unwrap_or(0).min(rows.len() - 1);
        ui.show_list_screen(&ListScreen {
            list_name: catalog.text("podcast_search_results").to_owned(),
            rows,
            selected: Some(selected),
            controls: vec![
                self.podcast_button("back", "back"),
                self.podcast_button("podcast_add", "add_podcast"),
                self.podcast_button("open_browser", "open_browser"),
                ScreenControl::List,
            ],
            focus_list: focus,
        });
        ui.set_status(&status, false);
    }

    /// Windows `add_selected_podcast_result`.
    fn add_selected_podcast_result(&mut self, ui: &mut dyn ShellUi) {
        let Some(url) = ui
            .selected_row()
            .and_then(|index| self.podcasts.search_results.get(index))
            .map(|item| item.feed_url.to_string())
        else {
            return;
        };
        let work = podcast_work::add_feed(
            url,
            self.application.settings().proxy.clone(),
            self.unknown_feed_title(),
            unix_timestamp(),
        );
        self.start_podcast_work(ui, work, &self.text("rss_refresh_started"));
    }

    /// Windows `show_podcast_categories`.
    fn show_podcast_categories(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("rss_feeds");
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if self.application.current_route() != Route::PodcastCategories {
            self.application
                .navigate_to(RouteFrame::new(Route::PodcastCategories));
        }
        self.view = View::PodcastCategories;
        self.refresh_podcast_categories(ui, true, None);
    }

    /// Windows `refresh_podcast_categories`: Back, Open, the list.
    fn refresh_podcast_categories(
        &mut self,
        ui: &mut dyn ShellUi,
        focus: bool,
        selected: Option<usize>,
    ) {
        let catalog = self.catalog();
        ui.show_list_screen(&ListScreen {
            list_name: catalog.text("podcast_categories_title").to_owned(),
            rows: PODCAST_GENRES
                .iter()
                .map(|(key, _)| catalog.text(key).to_owned())
                .collect(),
            selected: Some(selected.unwrap_or(0).min(PODCAST_GENRES.len() - 1)),
            controls: vec![
                self.podcast_button("back", "back"),
                self.podcast_button("open", "open"),
                ScreenControl::List,
            ],
            focus_list: focus,
        });
        ui.set_status(catalog.text("podcast_categories_title"), false);
    }

    /// Windows `open_selected_podcast_category`.
    fn open_selected_podcast_category(&mut self, ui: &mut dyn ShellUi) {
        let Some((key, genre_id)) = ui
            .selected_row()
            .and_then(|index| PODCAST_GENRES.get(index))
            .copied()
        else {
            return;
        };
        let work = podcast_work::load_category(
            self.text(key),
            genre_id,
            self.application.settings().proxy.clone(),
        );
        self.start_podcast_work(ui, work, &self.text("fetching_category_podcasts"));
    }

    /// Windows `choose_rss_category_filter`.
    fn choose_rss_category_filter(&mut self, ui: &mut dyn ShellUi) {
        let mut choices = vec![self.text("all_categories")];
        choices.extend(self.application.rss_categories());
        let current = self.application.rss_category_filter().to_owned();
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
        self.application.set_rss_category_filter(&category);
        self.refresh_rss_feeds(ui, true, None);
        let message = if category.is_empty() {
            self.text("category_filter_all")
        } else {
            self.text("category_filter_applied")
                .replace("{category}", &category)
        };
        ui.set_status(&message, true);
    }

    /// Windows `set_selected_rss_category`.
    fn set_selected_rss_category(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = self.selected_rss_feed_index(ui) else {
            return;
        };
        let Some(feed) = self.application.rss_feeds().get(index).cloned() else {
            return;
        };
        let Some(category) = ui.prompt_text(
            &self.text("set_category"),
            &self.text("category_prompt").replace("{title}", &feed.title),
            &feed.category,
            &self.text("ok"),
            &self.text("cancel"),
        ) else {
            ui.focus_primary_control();
            return;
        };
        let category = apricot_app::normalize_category(&category);
        match self.application.set_rss_feed_category(index, &category) {
            Ok(_) => {
                self.refresh_rss_feeds(ui, true, Some(&feed.url));
                let message = if category.is_empty() {
                    self.text("category_cleared")
                        .replace("{title}", &feed.title)
                } else {
                    self.text("category_assigned")
                        .replace("{title}", &feed.title)
                        .replace("{category}", &category)
                };
                ui.set_status(&message, true);
            }
            Err(error) => ui.show_error(&error.to_string()),
        }
    }

    /// Windows `remove_selected_rss_feed`: the next (or previous) feed is
    /// selected afterwards.
    pub(crate) fn remove_selected_rss_feed(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = self.selected_rss_feed_index(ui) else {
            return;
        };
        let visible = self.application.visible_rss_feed_indices();
        let Some(selected) = visible.iter().position(|candidate| *candidate == index) else {
            return;
        };
        let preferred = visible
            .get(selected + 1)
            .or_else(|| {
                selected
                    .checked_sub(1)
                    .and_then(|previous| visible.get(previous))
            })
            .and_then(|candidate| self.application.rss_feeds().get(*candidate))
            .map(|feed| feed.url.clone());
        match self.application.remove_rss_feed(index) {
            Ok(Some(_)) => {
                self.refresh_rss_feeds(ui, true, preferred.as_deref());
                ui.set_status(&self.text("rss_feed_removed"), true);
            }
            Ok(None) => {}
            Err(error) => ui.show_error(&error.to_string()),
        }
    }

    /// Windows `choose_rss_speed_preset`.
    fn choose_rss_speed_preset(&mut self, ui: &mut dyn ShellUi) {
        let index = if self.view == View::RssItems {
            Some(self.podcasts.feed)
        } else {
            self.selected_rss_feed_index(ui)
        };
        let Some(feed) = index.and_then(|index| self.application.rss_feeds().get(index).cloned())
        else {
            return;
        };
        let Some(index) = index else {
            return;
        };
        let mut choices = vec![self.text("podcast_speed_use_global")];
        choices.extend(PODCAST_SPEED_STEPS.iter().map(|speed| {
            self.text("playback_rate_x")
                .replace("{speed}", &format_rate(*speed))
        }));
        let initial = feed
            .speed_preset
            .and_then(|current| {
                PODCAST_SPEED_STEPS
                    .iter()
                    .position(|speed| (*speed - current).abs() < 0.001)
            })
            .map_or(0, |position| position + 1);
        let prompt = self
            .text("podcast_speed_preset_prompt")
            .replace("{title}", &feed.title);
        let Some(choice) = ui.choose(&ChoiceDialog {
            title: self.text("podcast_speed_preset"),
            list_name: prompt.clone(),
            prompt,
            choices,
            initial,
            ok: self.text("ok"),
            cancel: self.text("cancel"),
        }) else {
            ui.focus_primary_control();
            return;
        };
        let speed = choice
            .checked_sub(1)
            .and_then(|position| PODCAST_SPEED_STEPS.get(position))
            .copied();
        match self.application.set_rss_feed_speed(index, speed) {
            Ok(_) => {
                if self.view == View::RssFeeds {
                    self.refresh_rss_feeds(ui, true, Some(&feed.url));
                } else {
                    self.refresh_rss_items(ui, true, false, None);
                }
                let message = speed.map_or_else(
                    || {
                        self.text("podcast_speed_preset_cleared")
                            .replace("{title}", &feed.title)
                    },
                    |speed| {
                        self.text("podcast_speed_preset_saved")
                            .replace("{title}", &feed.title)
                            .replace("{speed}", &format_rate(speed))
                    },
                );
                ui.set_status(&message, true);
            }
            Err(error) => ui.show_error(&error.to_string()),
        }
    }

    /// Windows `active_rss_episode`: the selected episode, or the playing
    /// podcast episode.
    fn active_rss_episode(&self, ui: &dyn ShellUi) -> Option<(usize, usize, MediaItem)> {
        if self.view == View::RssItems {
            let item_index = ui.selected_row()?;
            return self
                .selected_rss_episode(ui)
                .map(|item| (self.podcasts.feed, item_index, item));
        }
        self.current_podcast_episode()
    }

    /// Windows `current_podcast_episode`.
    fn current_podcast_episode(&self) -> Option<(usize, usize, MediaItem)> {
        let item = self.application.player_session().current_item()?.clone();
        if item.kind != MediaKind::PodcastEpisode {
            return None;
        }
        let (feed_index, item_index) = self.application.rss_episode_location(&item)?;
        Some((feed_index, item_index, item))
    }

    /// Windows `mark_current_podcast_episode_played` at the end of an
    /// episode.
    pub(crate) fn mark_current_podcast_episode_played(&mut self, ui: &mut dyn ShellUi) {
        let Some((feed_index, item_index, item)) = self.current_podcast_episode() else {
            return;
        };
        if metadata_bool(&item, "played") {
            return;
        }
        match self.application.set_rss_episode_played(
            feed_index,
            item_index,
            true,
            unix_timestamp(),
        ) {
            Ok(Some(_)) => {
                let _ = self.application.clear_playback_position(&item);
            }
            Ok(None) => {}
            Err(error) => ui.set_status(
                &format!("Podcast played state was not saved: {error}"),
                false,
            ),
        }
    }

    /// Windows `toggle_selected_rss_played`.
    pub(crate) fn toggle_selected_rss_played(&mut self, ui: &mut dyn ShellUi) {
        let Some((feed_index, item_index, item)) = self.active_rss_episode(ui) else {
            return;
        };
        let played = !metadata_bool(&item, "played");
        match self.application.set_rss_episode_played(
            feed_index,
            item_index,
            played,
            unix_timestamp(),
        ) {
            Ok(Some(_)) => {
                if played {
                    let _ = self.application.clear_playback_position(&item);
                }
                if self.view == View::RssItems {
                    self.refresh_rss_items(ui, true, false, Some(item_index));
                }
                let key = if played {
                    "episode_marked_played"
                } else {
                    "episode_marked_unplayed"
                };
                ui.set_status(&self.text(key).replace("{title}", &item.title), true);
            }
            Ok(None) => {}
            Err(error) => ui.show_error(&error.to_string()),
        }
    }

    /// Windows `clear_selected_rss_progress`.
    pub(crate) fn clear_selected_rss_progress(&mut self, ui: &mut dyn ShellUi) {
        let Some((_, item_index, item)) = self.active_rss_episode(ui) else {
            return;
        };
        match self.application.clear_playback_position(&item) {
            Ok(apricot_app::PlaybackPositionUpdate::Cleared) => {
                if self.view == View::RssItems {
                    self.refresh_rss_items(ui, true, false, Some(item_index));
                }
                let message = self
                    .text("episode_progress_cleared")
                    .replace("{title}", &item.title);
                ui.set_status(&message, true);
            }
            Ok(_) => ui.set_status(&self.text("episode_progress_not_found"), true),
            Err(error) => ui.show_error(&error.to_string()),
        }
    }

    /// Windows `save_current_podcast_speed_preset`.
    pub(crate) fn save_current_podcast_speed_preset(&mut self, ui: &mut dyn ShellUi) {
        let found = self
            .application
            .player_session()
            .current_item()
            .filter(|item| item.kind == MediaKind::PodcastEpisode)
            .and_then(|item| self.application.rss_episode_location(item))
            .and_then(|(feed_index, _)| {
                let title = self.application.rss_feeds().get(feed_index)?.title.clone();
                let speed = self.application.player_session().audio()?.speed;
                Some((feed_index, title, speed))
            });
        let Some((feed_index, title, speed)) = found else {
            ui.set_status(&self.text("podcast_speed_preset_no_episode"), true);
            return;
        };
        match self.application.set_rss_feed_speed(feed_index, Some(speed)) {
            Ok(_) => {
                let message = self
                    .text("podcast_speed_preset_saved")
                    .replace("{title}", &title)
                    .replace("{speed}", &format_rate(speed));
                ui.set_status(&message, true);
            }
            Err(error) => ui.show_error(&error.to_string()),
        }
    }

    /// Windows `import_rss_opml`.
    fn import_rss_opml(&mut self, ui: &mut dyn ShellUi) {
        let Some(path) = ui.choose_open_file(&self.text("import_opml"), &["opml", "xml"]) else {
            return;
        };
        let parsed = (|| {
            let file = std::fs::File::open(&path).map_err(|error| error.to_string())?;
            let mut bytes = Vec::new();
            file.take((apricot_media::MAX_OPML_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            apricot_media::parse_opml(&bytes).map_err(|error| error.to_string())
        })();
        let feeds = match parsed {
            Ok(feeds) => feeds,
            Err(error) => {
                ui.show_error(
                    &self
                        .text("opml_import_failed_msg")
                        .replace("{error}", &error),
                );
                return;
            }
        };
        let existing = self
            .application
            .rss_feeds()
            .iter()
            .filter_map(|feed| apricot_app::canonical_feed_url(&feed.url))
            .map(|url| url.to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let entries = feeds
            .into_iter()
            .filter(|feed| {
                let identity = apricot_app::canonical_feed_url(feed.url.as_str())
                    .unwrap_or_else(|| feed.url.to_string())
                    .to_ascii_lowercase();
                !existing.contains(&identity)
            })
            .map(|feed| (feed.url.to_string(), feed.title))
            .collect::<Vec<_>>();
        if entries.is_empty() {
            ui.set_status(&self.text("opml_all_feeds_exist"), true);
            return;
        }
        let count = entries.len();
        let work = podcast_work::import_feeds(
            entries,
            self.application.settings().proxy.clone(),
            self.unknown_feed_title(),
            unix_timestamp(),
        );
        let message = self
            .text("opml_import_started")
            .replace("{count}", &count.to_string());
        self.start_podcast_work(ui, work, &message);
    }

    /// Windows `export_rss_opml`.
    fn export_rss_opml(&mut self, ui: &mut dyn ShellUi) {
        let feeds = self
            .application
            .rss_feeds()
            .iter()
            .filter_map(|feed| {
                Some(apricot_media::OpmlFeed {
                    title: feed.title.clone(),
                    url: feed.url.parse().ok()?,
                })
            })
            .collect::<Vec<_>>();
        if feeds.is_empty() {
            ui.set_status(&self.text("opml_no_feeds"), true);
            return;
        }
        let Some(path) =
            ui.choose_save_file(&self.text("export_opml"), "apricot_feeds.opml", &["opml"])
        else {
            return;
        };
        let result = apricot_media::write_opml(&feeds)
            .map_err(|error| error.to_string())
            .and_then(|bytes| std::fs::write(path, bytes).map_err(|error| error.to_string()));
        match result {
            Ok(()) => ui.set_status(&self.text("opml_export_success"), true),
            Err(error) => {
                ui.show_error(&self.text("opml_export_failed").replace("{error}", &error));
            }
        }
    }

    /// Windows `open_selected_podcast_in_browser`.
    fn open_selected_podcast_in_browser(&mut self, ui: &mut dyn ShellUi) {
        let url = match self.view {
            View::RssFeeds => self
                .selected_rss_feed_index(ui)
                .and_then(|index| self.application.rss_feeds().get(index))
                .map(|feed| {
                    if feed.site_url.trim().is_empty() {
                        feed.url.clone()
                    } else {
                        feed.site_url.clone()
                    }
                }),
            View::RssItems => self.selected_rss_episode(ui).and_then(|item| {
                metadata_text(&item, "webpage_url")
                    .or_else(|| item.url.as_ref().map(ToString::to_string))
            }),
            View::PodcastSearchResults => ui
                .selected_row()
                .and_then(|index| self.podcasts.search_results.get(index))
                .map(|item| item.webpage_url.to_string()),
            _ => None,
        };
        let Some(url) = url else {
            return;
        };
        if let Err(error) = apricot_platform::shell::open_url(&url) {
            ui.show_error(&format!("Could not open the browser: {error}"));
        }
    }

    /// The podcast actions of the action list; `true` when `action_id` is
    /// one.
    pub(crate) fn podcast_action(&mut self, ui: &mut dyn ShellUi, action_id: &str) -> bool {
        match action_id {
            "open_podcasts_rss" => self.show_rss_feeds(ui),
            "toggle_podcast_played" => self.toggle_selected_rss_played(ui),
            "clear_podcast_progress" => self.clear_selected_rss_progress(ui),
            "save_podcast_speed_preset" => self.save_current_podcast_speed_preset(ui),
            _ => return false,
        }
        true
    }

    /// A podcast screen button or context command (Windows
    /// `handle_window_command` and `execute_list_context_command`); `true`
    /// when `id` is one.
    pub(crate) fn podcast_command(&mut self, ui: &mut dyn ShellUi, id: &str) -> bool {
        match id {
            "rss_search" => self.prompt_podcast_search(ui),
            "rss_categories" => self.show_podcast_categories(ui),
            "rss_add" => self.prompt_add_rss_feed(ui),
            "rss_refresh" => self.refresh_rss_from_active_view(ui),
            "rss_refresh_selected" => self.refresh_selected_rss_feed(ui),
            "rss_filter" => self.choose_rss_category_filter(ui),
            "rss_set_category" => self.set_selected_rss_category(ui),
            "rss_import" => self.import_rss_opml(ui),
            "rss_export" => self.export_rss_opml(ui),
            "rss_speed" => self.choose_rss_speed_preset(ui),
            "rss_toggle_played" => self.toggle_selected_rss_played(ui),
            "rss_clear_progress" => self.clear_selected_rss_progress(ui),
            "rss_remove" => self.remove_selected_rss_feed(ui),
            "podcast_add" => self.add_selected_podcast_result(ui),
            "open_browser" => self.open_selected_podcast_in_browser(ui),
            "rss_download_episode" | "rss_download_feed" | "rss_queue_episode" => {
                // Downloads arrive with their unit (M9).
                let key = match id {
                    "rss_download_episode" => "download_episode_audio",
                    "rss_queue_episode" => "queue_episode_audio",
                    _ => "download_feed",
                };
                let message =
                    apricot_app::unavailable_feature_message(&self.catalog(), &self.text(key));
                ui.set_status(&message, true);
            }
            _ => return false,
        }
        true
    }

    /// Windows `list_context_entries` for the podcast screens, as context
    /// menu entries whose commands run [`Shell::podcast_command`] (and Play,
    /// Add to playlist, queue and copy as elsewhere).
    pub(crate) fn podcast_context_entries(&self) -> Option<Vec<ContextMenuEntry>> {
        let entry = |command: ContextCommand, key: &str| ContextMenuEntry::Command {
            label: self.text(key),
            command,
        };
        let podcast = |id: &'static str, key: &str| entry(ContextCommand::PlayerAction(id), key);
        Some(match self.view {
            View::RssFeeds => vec![
                entry(ContextCommand::Play, "open_feed"),
                podcast("rss_download_feed", "download_feed"),
                podcast("rss_speed", "podcast_speed_preset"),
                podcast("rss_refresh_selected", "refresh_feed"),
                podcast("rss_set_category", "set_category"),
                podcast("rss_filter", "filter_category"),
                entry(ContextCommand::CopyLocation, "copy_url"),
                podcast("rss_remove", "remove_feed"),
            ],
            View::RssItems => vec![
                entry(ContextCommand::Play, "play_episode"),
                podcast("rss_toggle_played", "mark_episode_played"),
                podcast("rss_clear_progress", "clear_episode_progress"),
                podcast("rss_download_episode", "download_episode_audio"),
                podcast("rss_queue_episode", "queue_episode_audio"),
                entry(ContextCommand::AddToPlaylist, "add_to_playlist"),
                entry(ContextCommand::AddToPlaybackQueue, "add_to_playback_queue"),
                entry(
                    ContextCommand::RemoveFromPlaybackQueue,
                    "remove_from_playback_queue",
                ),
                podcast("rss_download_feed", "download_feed"),
                podcast("open_browser", "open_episode_page"),
                entry(ContextCommand::CopyLocation, "copy_url"),
            ],
            View::PodcastSearchResults => vec![
                podcast("podcast_add", "add_podcast"),
                podcast("open_browser", "open_browser"),
                entry(ContextCommand::CopyLocation, "copy_url"),
            ],
            _ => return None,
        })
    }

    /// Windows `copy_context_location` on the podcast screens: the feed or
    /// directory URL.
    pub(crate) fn podcast_copy_location(&self, ui: &dyn ShellUi) -> Option<String> {
        match self.view {
            View::RssFeeds => self
                .selected_rss_feed_index(ui)
                .and_then(|index| self.application.rss_feeds().get(index))
                .map(|feed| feed.url.clone()),
            View::PodcastSearchResults => ui
                .selected_row()
                .and_then(|index| self.podcasts.search_results.get(index))
                .map(|item| item.feed_url.to_string()),
            _ => None,
        }
    }

    /// Back to a podcast screen (Windows `navigate_back`).
    pub(crate) fn restore_podcast_view(
        &mut self,
        ui: &mut dyn ShellUi,
        route: Route,
        saved_index: Option<usize>,
    ) {
        match route {
            Route::RssFeeds => {
                self.view = View::RssFeeds;
                self.refresh_rss_feeds(ui, true, None);
            }
            Route::RssItems => {
                self.view = View::RssItems;
                self.refresh_rss_items(ui, true, false, saved_index);
            }
            Route::PodcastSearchResults => {
                self.view = View::PodcastSearchResults;
                self.refresh_podcast_directory_results(ui, true, saved_index);
            }
            Route::PodcastCategories => {
                self.view = View::PodcastCategories;
                self.refresh_podcast_categories(ui, true, saved_index);
            }
            _ => {}
        }
    }
}
