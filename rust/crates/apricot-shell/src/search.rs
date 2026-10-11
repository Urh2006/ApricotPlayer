//! The search screen and its results (Windows `show_search`,
//! `submit_search`, `finish_youtube_search`, `refresh_results`,
//! `append_results`, `result_selection_changed`,
//! `activate_result_selection`, `activate_search_button` and the provider
//! and type choices).

use apricot_app::{SearchApplyOutcome, SearchWorkKind};
use apricot_core::{MediaItem, MediaKind, MediaSource, Route, RouteFrame, TranslationCatalog};
use apricot_media::YoutubeSearchKind;

use crate::{ListScreen, ScreenControl, Shell, ShellUi, View, format_duration};

/// Windows `SEARCH_PROVIDER_KEYS`: `YouTube`, then `SoundCloud`.
const SEARCH_PROVIDER_KEYS: [&str; 2] = ["youtube", "soundcloud"];

/// Windows `search_type_definitions`.
fn search_type_definitions(provider_index: usize) -> &'static [(&'static str, YoutubeSearchKind)] {
    if provider_index == 1 {
        &[
            ("track", YoutubeSearchKind::SoundcloudTrack),
            ("playlist", YoutubeSearchKind::SoundcloudPlaylist),
            ("artist", YoutubeSearchKind::SoundcloudUser),
        ]
    } else {
        &[
            ("all", YoutubeSearchKind::All),
            ("video", YoutubeSearchKind::Video),
            ("playlist", YoutubeSearchKind::Playlist),
            ("channel", YoutubeSearchKind::Channel),
        ]
    }
}

/// Windows `playlist_count_text`.
fn playlist_count_text(item: &MediaItem, catalog: &TranslationCatalog) -> Option<String> {
    let raw = ["playlist_count", "n_entries", "video_count"]
        .iter()
        .filter_map(|key| item.metadata.get(*key))
        .find_map(|value| match value {
            serde_json::Value::Number(number) => Some(number.to_string()),
            serde_json::Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
            _ => None,
        })?;
    Some(match raw.replace(',', "").trim().parse::<i64>() {
        Ok(count) => catalog
            .text("playlist_video_count")
            .replace("{count}", &count.to_string()),
        Err(_) => raw,
    })
}

/// Windows `result_label_parts`.
fn result_label_parts(item: &MediaItem, catalog: &TranslationCatalog) -> Vec<String> {
    // Python `normalize_entry` names SoundCloud users and tracks differently.
    let soundcloud = item.source == MediaSource::Soundcloud;
    let kind = match item.kind {
        MediaKind::Playlist => catalog.text("playlist"),
        MediaKind::Channel if soundcloud => catalog.text("artist"),
        MediaKind::Channel => catalog.text("channel"),
        _ if soundcloud => catalog.text("track"),
        MediaKind::LiveStream => catalog.text("live_stream"),
        _ => catalog.text("video"),
    };
    if matches!(item.kind, MediaKind::Playlist | MediaKind::Channel) {
        let mut parts = vec![item.title.clone(), kind.to_owned()];
        if item.kind == MediaKind::Playlist
            && let Some(count) = playlist_count_text(item, catalog)
        {
            parts.push(count);
        }
        return parts;
    }
    let mut parts = vec![
        item.title.clone(),
        format!("{}: {}", catalog.text("channel"), item.channel),
    ];
    let views = apricot_app::player_information::display_count(&item.metadata, "views")
        .or_else(|| apricot_app::player_information::display_count(&item.metadata, "view_count"))
        .unwrap_or_default();
    parts.push(format!("{}: {views}", catalog.text("views")));
    let uploaded = apricot_app::player_information::display_upload_age(catalog, item);
    parts.push(if uploaded.is_empty() {
        catalog.text("uploaded_unknown").to_owned()
    } else {
        uploaded
    });
    if let Some(duration) = item.duration_seconds {
        parts.push(format_duration(duration));
    }
    parts.push(kind.to_owned());
    parts
}

/// Windows `queued_marker_key`.
const fn queued_marker_key(queued: &apricot_app::QueuedDownload) -> &'static str {
    use apricot_app::DownloadChoice;
    match (queued.item.kind, queued.choice) {
        (MediaKind::PodcastEpisode, _) => "podcast_audio_queued_marker",
        (MediaKind::Playlist | MediaKind::Channel, DownloadChoice::Audio) => {
            "collection_audio_queued_marker"
        }
        (MediaKind::Playlist | MediaKind::Channel, DownloadChoice::Video) => {
            "collection_video_queued_marker"
        }
        (_, DownloadChoice::Audio) => "audio_queued_marker",
        (_, DownloadChoice::Video) => "video_queued_marker",
        (_, DownloadChoice::Ask) => "selected_queued_marker",
    }
}

/// Windows `result_label`: the row of one search result.
#[must_use]
pub fn result_label(
    item: &MediaItem,
    catalog: &TranslationCatalog,
    downloads: &apricot_app::DownloadController,
) -> String {
    let mut parts = result_label_parts(item, catalog);
    if let Some(queued) = downloads.queued_item(item) {
        parts.push(catalog.text(queued_marker_key(queued)).to_owned());
    }
    parts.join(" | ")
}

impl Shell {
    /// The search screen with its controls in the Windows Tab order: Back,
    /// the query, provider and type, the button row, then the result list.
    pub(crate) fn search_screen(
        &self,
        rows: Vec<String>,
        selected: usize,
        reset: bool,
    ) -> ListScreen {
        let catalog = self.catalog();
        let text = |key: &str| catalog.text(key).to_owned();
        let providers = SEARCH_PROVIDER_KEYS.iter().map(|key| text(key)).collect();
        let button = |id, key: &str| ScreenControl::Button {
            id,
            label: text(key),
        };
        ListScreen {
            list_name: text("result_list"),
            rows,
            selected: Some(selected),
            controls: vec![
                button("back", "back"),
                ScreenControl::TextField {
                    id: "search_query",
                    label: text("search_query"),
                    value: reset.then(String::new),
                },
                ScreenControl::Choice {
                    id: "search_provider",
                    label: text("search_provider"),
                    options: providers,
                    selected: reset.then_some(0),
                },
                ScreenControl::Choice {
                    id: "search_type",
                    label: text("type"),
                    options: self.search_kind_labels(if reset { 0 } else { self.search.provider }),
                    selected: reset.then_some(0),
                },
                button("search", "search"),
                button("search_play", "play"),
                button("search_download_audio", "download_audio"),
                button("search_download_video", "download_video"),
                button("search_add_favorite", "add_favorite"),
                ScreenControl::List,
            ],
            focus_list: false,
        }
    }

    fn search_kind_labels(&self, provider: usize) -> Vec<String> {
        let catalog = self.catalog();
        search_type_definitions(provider)
            .iter()
            .map(|(key, _)| catalog.text(key).to_owned())
            .collect()
    }

    /// Windows `show_search`: an empty query, the `YouTube` provider with its
    /// first type, and the empty result list; focus on the query.
    pub(crate) fn show_search(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("search");
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if self.application.current_route() != Route::Search {
            self.application.navigate_main_menu();
            self.application.navigate_to(RouteFrame::new(Route::Search));
        }
        self.view = View::Search;
        self.search.provider = 0;
        let screen = self.search_screen(vec![self.text("search_results_empty")], 0, true);
        ui.show_list_screen(&screen);
        ui.set_status(&self.text("ready"), false);
        ui.focus_screen_control("search_query");
    }

    /// Windows `search_provider_changed`: the type list follows the
    /// provider and starts at its first type.
    pub fn screen_choice_changed(&mut self, ui: &mut dyn ShellUi, id: &str) {
        if id == "search_provider"
            && matches!(self.view, View::Search | View::Results | View::Collection)
        {
            self.search.provider = ui.screen_choice("search_provider").unwrap_or(0);
            let labels = self.search_kind_labels(self.search.provider);
            ui.set_screen_choices("search_type", &labels, 0);
        }
    }

    /// Return in a text field of a screen (Windows `submit_primary_text`).
    pub fn screen_text_submitted(&mut self, ui: &mut dyn ShellUi, id: &str) {
        if id == "search_query"
            && matches!(self.view, View::Search | View::Results | View::Collection)
        {
            self.submit_search(ui);
        }
    }

    /// Windows `selected_search_kind`.
    fn selected_search_kind(ui: &dyn ShellUi) -> YoutubeSearchKind {
        let definitions = search_type_definitions(ui.screen_choice("search_provider").unwrap_or(0));
        ui.screen_choice("search_type")
            .and_then(|index| definitions.get(index))
            .unwrap_or(&definitions[0])
            .1
    }

    /// Windows `submit_search`.
    pub(crate) fn submit_search(&mut self, ui: &mut dyn ShellUi) {
        let query = ui.screen_text("search_query");
        // Python `search` shows `enter_query` and leaves a running search alone.
        if query.trim().is_empty() {
            ui.show_error(&self.text("enter_query"));
            return;
        }
        // A new search replaces a running one, whose results would be stale.
        self.cancel_youtube_work();
        let kind = Self::selected_search_kind(ui);
        let work = match self.application.begin_youtube_search(&query, kind) {
            Ok(work) => work,
            Err(error) => {
                ui.set_status(&error.to_string(), true);
                ui.focus_screen_control("search_query");
                return;
            }
        };
        let message = self
            .text("searching")
            .replace("{query}", self.application.search_session().query());
        ui.set_status(&message, false);
        self.start_youtube_search(ui, work);
    }

    /// Windows `finish_youtube_search`.
    pub(crate) fn finish_youtube_search(
        &mut self,
        ui: &mut dyn ShellUi,
        generation: u64,
        items: Vec<MediaItem>,
        continuation: Option<String>,
    ) {
        let work_kind = match self.youtube.pending_list.take() {
            Some(crate::youtube::PendingList::Search(work)) => Some(work.work_kind),
            _ => None,
        };
        let outcome = self
            .application
            .apply_search_results(generation, items, continuation);
        match (work_kind, outcome) {
            (Some(SearchWorkKind::Initial), SearchApplyOutcome::Replaced) => {
                // Python `search` shows the results on the search screen, so
                // Back returns to the main menu.
                self.application.navigate_main_menu();
                self.application
                    .navigate_to(RouteFrame::new(Route::Results));
                self.view = View::Results;
                self.refresh_results(ui, true);
            }
            (Some(SearchWorkKind::More), SearchApplyOutcome::Appended { added }) => {
                self.append_results(ui, added);
            }
            _ => {}
        }
    }

    fn result_rows(&self) -> Vec<String> {
        let catalog = self.catalog();
        self.application
            .search_session()
            .items()
            .iter()
            .map(|item| result_label(item, &catalog, self.application.downloads()))
            .collect()
    }

    /// Windows `refresh_results`: the rows, the selected result and "Found"
    /// on the status line; the query and choices stay as typed.
    pub(crate) fn refresh_results(&mut self, ui: &mut dyn ShellUi, focus: bool) {
        let session = self.application.search_session();
        let count = session.items().len();
        let (rows, selected, status) = if count == 0 {
            let none = self.text("no_results");
            (vec![none.clone()], 0, none)
        } else {
            (
                self.result_rows(),
                session.selected_index().min(count - 1),
                self.text("found").replace("{count}", &count.to_string()),
            )
        };
        let mut screen = self.search_screen(rows, selected, false);
        screen.focus_list = focus;
        ui.show_list_screen(&screen);
        ui.set_status(&status, false);
    }

    /// Windows `append_results`.
    fn append_results(&mut self, ui: &mut dyn ShellUi, added: usize) {
        if added == 0 {
            ui.set_status(&self.text("no_more_results"), false);
            return;
        }
        let session = self.application.search_session();
        let count = session.items().len();
        let selected = session.selected_index().min(count.saturating_sub(1));
        ui.set_list_rows(self.result_rows(), selected);
        let loaded = self
            .text("search_more_loaded")
            .replace("{count}", &count.to_string());
        ui.set_status(&loaded, false);
    }

    /// Windows `result_selection_changed` for the screens the shell shows:
    /// the last result loads more.
    pub fn list_selection_changed(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = ui.selected_row() else {
            return;
        };
        match self.view {
            View::LocalFolder => {
                let _ = self.application.select_local_folder_item(index);
            }
            View::Results => {
                if !self.application.select_search_result(index) {
                    return;
                }
                if index + 1 == self.application.search_session().items().len()
                    && let Some(work) = self.application.request_more_search_results()
                {
                    ui.set_status(&self.text("loading_more_results"), false);
                    self.start_youtube_search(ui, work);
                }
            }
            View::Collection => self.collection_selection_changed(ui, index),
            View::MainMenu | View::Search | View::Player => {}
        }
    }

    /// Windows `activate_result_selection`.
    pub(crate) fn activate_result_selection(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = ui.selected_row() else {
            return;
        };
        let Some(item) = self.application.prepare_search_playback(index) else {
            return;
        };
        self.activate_youtube_item(ui, item);
    }

    /// Windows `activate_search_button`: without a selected result each
    /// button says `no_selection` in a message.
    pub(crate) fn activate_search_button(&mut self, ui: &mut dyn ShellUi, id: &str) {
        if id == "search" {
            self.submit_search(ui);
            return;
        }
        if self.active_media_item(ui).is_none() {
            ui.show_error(&self.text("no_selection"));
            return;
        }
        let key = match id {
            "search_play" => {
                self.activate_selection(ui);
                return;
            }
            "search_download_audio" => "download_audio",
            "search_download_video" => "download_video",
            _ => "add_favorite",
        };
        // Downloads and favorites arrive with their units (M7 and M9).
        let message = apricot_app::unavailable_feature_message(&self.catalog(), &self.text(key));
        ui.set_status(&message, true);
    }
}

/// The search screen's provider, kept while results replace the list.
#[derive(Default)]
pub(crate) struct SearchState {
    provider: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_rows_follow_the_windows_label() {
        let catalog = apricot_app::embedded_catalog("en");
        let mut item = crate::local_file_item(std::path::Path::new("/tmp/Song.mp4"));
        item.source = MediaSource::Youtube;
        item.kind = MediaKind::Video;
        item.metadata.clear();
        item.channel = "Band".to_owned();
        item.duration_seconds = Some(65.0);
        let label = result_label(&item, &catalog, &apricot_app::DownloadController::default());
        assert!(
            label.starts_with("Song | Channel: Band | Views: "),
            "{label}"
        );
        assert!(label.ends_with(" | 1:05 | Video"), "{label}");
    }

    #[test]
    fn soundcloud_types_follow_the_provider() {
        assert_eq!(search_type_definitions(0).len(), 4);
        assert_eq!(
            search_type_definitions(1)[2].1,
            YoutubeSearchKind::SoundcloudUser
        );
    }
}
