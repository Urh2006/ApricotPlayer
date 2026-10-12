//! The Trending screen (Windows `show_trending`, `load_trending_results`,
//! `start_youtube_trending_work`, `start_public_trending_work`,
//! `poll_youtube_trending_api`, `finish_youtube_trending`,
//! `finish_youtube_trending_error`, `restore_trending_view`) and the direct
//! link screen (`show_direct_link`, `activate_direct_link`).

use std::sync::mpsc::{self, Receiver, TryRecvError};

use apricot_app::{
    SearchApplyOutcome, YOUTUBE_TRENDING_CATEGORIES, YOUTUBE_TRENDING_COUNTRIES,
    YoutubeCollectionKind, YoutubeTrendingWork, youtube_trending_category_id,
    youtube_trending_public_url,
};
use apricot_core::{MediaItem, Route, RouteFrame};
use apricot_media::YoutubeBackend;

use crate::youtube::{PendingList, ResolvePurpose, components_directory};
use crate::{ListScreen, ScreenControl, Shell, ShellUi, View};

/// Windows `PendingYoutubeTrending`: the work and the official API's error
/// when the public chart is tried after it.
#[derive(Clone, Debug)]
pub(crate) struct PendingTrending {
    pub(crate) work: YoutubeTrendingWork,
    api_error: Option<String>,
}

/// Windows `PendingYoutubeTrendingApi`.
pub(crate) struct PendingTrendingApi {
    generation: u64,
    receiver: Receiver<Result<Vec<MediaItem>, String>>,
}

/// Windows `direct_link_with_scheme`: a link without a scheme gets
/// `https://`.
fn direct_link_with_scheme(value: &str) -> String {
    let value = value.trim();
    let has_scheme = value.split_once("://").is_some_and(|(scheme, _)| {
        let mut characters = scheme.chars();
        characters
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic())
            && characters.all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '+' | '.' | '-')
            })
    });
    if has_scheme {
        value.to_owned()
    } else {
        format!("https://{value}")
    }
}

impl Shell {
    /// The Trending screen in the Windows Tab order: country, category, the
    /// list, Back and Load trending.
    pub(crate) fn trending_screen(
        &self,
        rows: Vec<String>,
        selected: usize,
        choices: Option<(usize, usize)>,
    ) -> ListScreen {
        let catalog = self.catalog();
        let text = |key: &str| catalog.text(key).to_owned();
        ListScreen {
            list_name: text("trending"),
            rows,
            selected: Some(selected),
            controls: vec![
                ScreenControl::Choice {
                    id: "trending_country",
                    label: text("trending_country"),
                    options: YOUTUBE_TRENDING_COUNTRIES
                        .iter()
                        .map(|choice| choice.label_key.to_owned())
                        .collect(),
                    selected: choices.map(|(country, _)| country),
                },
                ScreenControl::Choice {
                    id: "trending_category",
                    label: text("trending_category"),
                    options: YOUTUBE_TRENDING_CATEGORIES
                        .iter()
                        .map(|choice| text(choice.label_key))
                        .collect(),
                    selected: choices.map(|(_, category)| category),
                },
                ScreenControl::List,
                ScreenControl::Button {
                    id: "back",
                    label: text("back"),
                },
                ScreenControl::Button {
                    id: "load_trending",
                    label: text("load_trending"),
                },
            ],
            focus_list: false,
        }
    }

    /// Windows `show_trending`: the empty list, focus on the country, and
    /// the chart loads at once.
    pub(crate) fn show_trending(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("trending");
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if !self.application.settings().enable_trending {
            ui.set_status(&self.text("trending_disabled"), true);
            self.show_main_menu(ui);
            return;
        }
        if self.application.current_route() != Route::Trending {
            self.application.navigate_main_menu();
            self.application
                .navigate_to(RouteFrame::new(Route::Trending));
        }
        self.view = View::Trending;
        let screen = self.trending_screen(vec![self.text("search_results_empty")], 0, None);
        ui.show_list_screen(&screen);
        ui.set_status(&self.text("ready"), false);
        ui.focus_screen_control("trending_country");
        self.load_trending_results(ui);
    }

    /// Windows `restore_trending_view`: the choices of the route frame.
    pub(crate) fn restore_trending_view(&mut self, ui: &mut dyn ShellUi, frame: &RouteFrame) {
        let index = |key: &str, count: usize| {
            frame
                .parameters
                .get(key)
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default()
                .min(count.saturating_sub(1))
        };
        let choices = (
            index("country_index", YOUTUBE_TRENDING_COUNTRIES.len()),
            index("category_index", YOUTUBE_TRENDING_CATEGORIES.len()),
        );
        self.view = View::Trending;
        self.refresh_results_with(ui, true, Some(choices));
    }

    /// Windows `load_trending_results`.
    pub(crate) fn load_trending_results(&mut self, ui: &mut dyn ShellUi) {
        if self.view != View::Trending {
            return;
        }
        self.cancel_youtube_work();
        let country_index = ui
            .screen_choice("trending_country")
            .unwrap_or_default()
            .min(YOUTUBE_TRENDING_COUNTRIES.len().saturating_sub(1));
        let category_index = ui
            .screen_choice("trending_category")
            .unwrap_or_default()
            .min(YOUTUBE_TRENDING_CATEGORIES.len().saturating_sub(1));
        let country = YOUTUBE_TRENDING_COUNTRIES[country_index];
        let category = YOUTUBE_TRENDING_CATEGORIES[category_index];
        self.application.update_trending_route_context(
            country_index,
            category_index,
            country.code,
            category.code,
        );
        let work = match self
            .application
            .begin_youtube_trending(country.code, category.code)
        {
            Ok(work) => work,
            Err(error) => {
                ui.set_status(&error.to_string(), true);
                return;
            }
        };
        let message = self
            .text("trending_loading_official")
            .replace("{country}", country.label_key)
            .replace("{category}", &self.text(category.label_key));
        ui.set_status(&message, false);
        self.start_youtube_trending_work(ui, work);
    }

    /// Windows `start_youtube_trending_work`: the official API with a key,
    /// otherwise the public chart.
    fn start_youtube_trending_work(&mut self, ui: &mut dyn ShellUi, work: YoutubeTrendingWork) {
        let settings = self.application.settings();
        let api_key = settings.youtube_data_api_key.trim().to_owned();
        let proxy = Some(settings.proxy.trim().to_owned()).filter(|proxy| !proxy.is_empty());
        let (generation, country_code, category_code, limit) = (
            work.generation,
            work.country_code,
            work.category_code,
            work.limit,
        );
        self.youtube.pending_list = Some(PendingList::Trending(PendingTrending {
            work,
            api_error: None,
        }));
        if api_key.is_empty() {
            self.start_public_trending_work(ui);
            return;
        }
        let region = (country_code != "global").then_some(country_code);
        let category = youtube_trending_category_id(category_code);
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let result = apricot_platform::YoutubeDataApiClient::new(proxy.as_deref())
                .and_then(|client| client.fetch_trending(&api_key, region, category, limit))
                .map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        self.youtube.pending_trending_api = Some(PendingTrendingApi {
            generation,
            receiver,
        });
        ui.set_background_polling(true);
    }

    fn pending_trending(&self) -> Option<PendingTrending> {
        match self.youtube.pending_list.as_ref() {
            Some(PendingList::Trending(pending)) => Some(pending.clone()),
            _ => None,
        }
    }

    /// Windows `start_public_trending_work`.
    fn start_public_trending_work(&mut self, ui: &mut dyn ShellUi) {
        let Some(pending) = self.pending_trending() else {
            return;
        };
        let with_api_error = |message: &str| {
            let mut text = pending.api_error.clone().unwrap_or_default();
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(message);
            text
        };
        let Some(url) =
            youtube_trending_public_url(pending.work.country_code, pending.work.category_code)
        else {
            let message = with_api_error(&self.text("trending_api_key_required"));
            self.finish_youtube_trending_error(ui, pending.work.generation, &message);
            return;
        };
        let Some(components) = components_directory() else {
            self.finish_youtube_trending_error(
                ui,
                pending.work.generation,
                "Application path is unavailable",
            );
            return;
        };
        let config = self.youtube_session_config();
        match self.youtube.search.start_collection(
            YoutubeBackend::YtDlp,
            &components,
            config,
            pending.work.generation,
            url,
            YoutubeCollectionKind::PlaylistVideos,
            pending.work.limit,
        ) {
            Ok(()) => ui.set_background_polling(true),
            Err(error) => {
                let message = with_api_error(&error.to_string());
                self.finish_youtube_trending_error(ui, pending.work.generation, &message);
            }
        }
    }

    /// Windows `poll_youtube_trending_api`; `true` while the request runs.
    pub(crate) fn poll_youtube_trending_api(&mut self, ui: &mut dyn ShellUi) -> bool {
        let Some(pending) = self.youtube.pending_trending_api.as_ref() else {
            return false;
        };
        let generation = pending.generation;
        let outcome = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => {
                Err("YouTube Data API worker stopped unexpectedly".to_owned())
            }
        };
        self.youtube.pending_trending_api = None;
        if self
            .pending_trending()
            .is_none_or(|pending| pending.work.generation != generation)
        {
            return false;
        }
        match outcome {
            Ok(items) if !items.is_empty() => {
                self.finish_youtube_trending(ui, generation, items, true);
            }
            outcome => {
                let error = outcome
                    .err()
                    .unwrap_or_else(|| "Official YouTube trending returned no videos.".to_owned());
                if let Some(PendingList::Trending(pending)) = self.youtube.pending_list.as_mut() {
                    pending.api_error = Some(error);
                }
                self.start_public_trending_work(ui);
            }
        }
        false
    }

    /// Windows `finish_youtube_trending`.
    pub(crate) fn finish_youtube_trending(
        &mut self,
        ui: &mut dyn ShellUi,
        generation: u64,
        items: Vec<MediaItem>,
        used_api: bool,
    ) {
        let Some(pending) = self
            .pending_trending()
            .filter(|pending| pending.work.generation == generation)
        else {
            return;
        };
        if !used_api && items.is_empty() {
            let mut message = pending.api_error.unwrap_or_default();
            if !message.is_empty() {
                message.push_str("\n\n");
            }
            message.push_str("The public YouTube chart returned no videos.");
            self.finish_youtube_trending_error(ui, generation, &message);
            return;
        }
        let outcome = self
            .application
            .apply_search_results(generation, items, None);
        self.youtube.pending_list = None;
        self.youtube.pending_trending_api = None;
        if outcome == SearchApplyOutcome::Replaced {
            self.view = View::Trending;
            self.refresh_results(ui, true);
            let source_key = if used_api {
                "trending_source_api"
            } else {
                "trending_source_public"
            };
            ui.set_status(&self.text(source_key), false);
        }
    }

    /// Windows `finish_youtube_trending_error`: the message, then the main
    /// menu.
    pub(crate) fn finish_youtube_trending_error(
        &mut self,
        ui: &mut dyn ShellUi,
        generation: u64,
        error: &str,
    ) {
        if self
            .pending_trending()
            .is_none_or(|pending| pending.work.generation != generation)
        {
            return;
        }
        let _ = self.application.fail_search(generation, error);
        self.youtube.pending_list = None;
        self.youtube.pending_trending_api = None;
        let visible = self
            .text("trending_official_unavailable")
            .replace("{error}", error);
        ui.set_status(&visible, true);
        ui.show_error(&visible);
        ui.set_status(&self.text("trending_unavailable_returning"), true);
        self.show_main_menu(ui);
    }

    /// The direct link screen: the link field, then Back and the four
    /// buttons (Windows creation order).
    fn direct_link_screen(&self, reset: bool) -> ListScreen {
        let catalog = self.catalog();
        let button = |id, key: &str| ScreenControl::Button {
            id,
            label: catalog.text(key).to_owned(),
        };
        ListScreen {
            list_name: String::new(),
            rows: Vec::new(),
            selected: None,
            controls: vec![
                ScreenControl::TextField {
                    id: "direct_link",
                    label: catalog.text("direct_link_url").to_owned(),
                    value: reset.then(String::new),
                },
                button("back", "back"),
                button("direct_play", "play_direct_link"),
                button("direct_download_audio", "download_direct_audio"),
                button("direct_download_video", "download_direct_video"),
                button("direct_copy_stream", "copy_stream_url"),
            ],
            focus_list: false,
        }
    }

    /// Windows `show_direct_link`.
    pub(crate) fn show_direct_link(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("direct_link");
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if self.application.current_route() != Route::DirectLink {
            self.application.navigate_main_menu();
            self.application
                .navigate_to(RouteFrame::new(Route::DirectLink));
        }
        self.view = View::DirectLink;
        ui.show_list_screen(&self.direct_link_screen(true));
        ui.set_status(&self.text("ready"), false);
        ui.focus_screen_control("direct_link");
        self.prewarm_playback_resolver();
    }

    /// Back to the direct link screen keeps the typed link.
    pub(crate) fn restore_direct_link(&mut self, ui: &mut dyn ShellUi) {
        self.view = View::DirectLink;
        ui.show_list_screen(&self.direct_link_screen(false));
        ui.focus_screen_control("direct_link");
    }

    /// Windows `prewarm_playback_resolver`: yt-dlp starts while the link is
    /// typed.
    fn prewarm_playback_resolver(&mut self) {
        let Some(components) = components_directory() else {
            return;
        };
        let executable = apricot_platform::app_update::preferred_ytdlp_executable(&components);
        let config = self.youtube_session_config();
        std::thread::spawn(move || {
            apricot_platform::ytdlp_youtube::prewarm_playback_resolver(&executable, config);
        });
    }

    /// Windows `activate_direct_link`.
    pub(crate) fn activate_direct_link(&mut self, ui: &mut dyn ShellUi, action: &str) {
        let text = ui.screen_text("direct_link");
        // Python `direct_link_item` returns nothing for an empty field and
        // the action shows the `no_selection` message.
        if text.trim().is_empty() {
            ui.show_error(&self.text("no_selection"));
            return;
        }
        if self.spotify_play_link(ui, &text, action) {
            return;
        }
        let Some(item) = MediaItem::from_direct_link(&direct_link_with_scheme(&text)) else {
            ui.set_status(&self.text("direct_link_invalid"), true);
            ui.focus_screen_control("direct_link");
            return;
        };
        match action {
            "copy_stream_url" => {
                self.start_youtube_resolve(
                    ui,
                    &item,
                    ResolvePurpose::CopyStreamUrl,
                    None,
                    false,
                    None,
                );
            }
            "download_audio" => {
                self.start_download_item(ui, &item, apricot_app::DownloadChoice::Audio, false);
            }
            "download_video" => {
                self.start_download_item(ui, &item, apricot_app::DownloadChoice::Video, false);
            }
            _ => {
                self.application.prepare_standalone_playback();
                self.start_youtube_resolve(ui, &item, ResolvePurpose::Playback, None, false, None);
            }
        }
    }

    /// Return in the link field: Windows `submit_primary_text` with the
    /// configured `direct_link_enter_action`.
    pub(crate) fn submit_direct_link(&mut self, ui: &mut dyn ShellUi) {
        let action = self.application.settings().direct_link_enter_action.clone();
        self.activate_direct_link(ui, &action);
    }
}

#[cfg(test)]
mod tests {
    use super::direct_link_with_scheme;

    #[test]
    fn a_link_without_a_scheme_gets_https() {
        assert_eq!(
            direct_link_with_scheme(" youtu.be/x "),
            "https://youtu.be/x"
        );
        assert_eq!(direct_link_with_scheme("http://a.b/c"), "http://a.b/c");
        assert_eq!(direct_link_with_scheme("rtmp+s://a"), "rtmp+s://a");
    }
}
