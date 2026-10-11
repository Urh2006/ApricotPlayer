//! Channel and playlist contents on the search screen (Windows
//! `open_youtube_collection`, `start_youtube_collection_work`,
//! `finish_youtube_collection`, `refresh_youtube_collection`,
//! `append_youtube_collection_results`, `show_channel_options`,
//! `open_item_channel`, `play_youtube_playlist` and
//! `finish_youtube_playlist_playback`).

use apricot_app::{
    YoutubeCollectionApplyOutcome, YoutubeCollectionKind, YoutubeCollectionPhase,
    YoutubeCollectionWork, YoutubeCollectionWorkKind,
};
use apricot_core::{MediaItem, MediaKind, MediaSource, Route, RouteFrame};
use apricot_media::YoutubeBackend;

use crate::youtube::{PendingList, components_directory};
use crate::{ChoiceDialog, Shell, ShellUi, View, result_label};

impl Shell {
    /// Windows `activate_youtube_item`: a `SoundCloud` artist opens its
    /// tracks, a channel its options, a playlist its videos; other items
    /// play with shuffle off.
    pub(crate) fn activate_youtube_item(&mut self, ui: &mut dyn ShellUi, item: MediaItem) {
        match item.kind {
            MediaKind::Channel if item.source == MediaSource::Soundcloud => {
                self.open_youtube_collection(
                    ui,
                    &item,
                    YoutubeCollectionKind::SoundcloudArtistTracks,
                );
            }
            MediaKind::Channel => self.show_channel_options(ui, &item),
            MediaKind::Playlist => {
                self.open_youtube_collection(ui, &item, YoutubeCollectionKind::PlaylistVideos);
            }
            _ => self.start_media_item_with_options(ui, item, None, Some(false), true, None),
        }
    }

    /// Windows `activate_youtube_collection_selection`.
    pub(crate) fn activate_collection_selection(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = ui.selected_row() else {
            return;
        };
        let Some(item) = self.application.prepare_youtube_collection_playback(index) else {
            return;
        };
        self.activate_youtube_item(ui, item);
    }

    /// Windows `show_channel_options`: videos, playlists, live streams or
    /// popular videos of a channel.
    pub(crate) fn show_channel_options(&mut self, ui: &mut dyn ShellUi, item: &MediaItem) {
        let catalog = self.catalog();
        let choices = [
            "channel_videos",
            "channel_playlists",
            "channel_live_streams",
            "channel_popular",
        ]
        .map(|key| catalog.text(key).to_owned())
        .to_vec();
        let selection = ui.choose(&ChoiceDialog {
            title: catalog.text("channel_options").to_owned(),
            prompt: item.title.clone(),
            list_name: item.title.clone(),
            choices,
            initial: 0,
            ok: catalog.text("open").to_owned(),
            cancel: catalog.text("cancel").to_owned(),
        });
        let kind = match selection {
            Some(0) => YoutubeCollectionKind::ChannelVideos,
            Some(1) => YoutubeCollectionKind::ChannelPlaylists,
            Some(2) => YoutubeCollectionKind::ChannelStreams,
            Some(3) => YoutubeCollectionKind::ChannelPopular,
            _ => {
                ui.focus_primary_control();
                return;
            }
        };
        self.open_youtube_collection(ui, item, kind);
    }

    /// Windows `open_item_channel`: the uploading channel's videos of the
    /// active item, or of the playing item.
    pub(crate) fn open_item_channel(&mut self, ui: &mut dyn ShellUi) {
        let channel = self
            .active_media_item(ui)
            .as_ref()
            .and_then(apricot_app::context_menu::youtube_channel_item_for_video)
            .or_else(|| {
                self.application
                    .player_session()
                    .current_item()
                    .and_then(apricot_app::context_menu::youtube_channel_item_for_video)
            });
        match channel {
            Some(channel) => {
                self.open_youtube_collection(ui, &channel, YoutubeCollectionKind::ChannelVideos);
            }
            None => ui.set_status(&self.text("no_channel"), true),
        }
    }

    /// Windows `open_youtube_collection`.
    pub(crate) fn open_youtube_collection(
        &mut self,
        ui: &mut dyn ShellUi,
        item: &MediaItem,
        kind: YoutubeCollectionKind,
    ) {
        let Some(url) = item.url.as_ref().map(ToString::to_string) else {
            ui.set_status(&self.text("no_selection"), true);
            return;
        };
        // Python `open_channel_tab` loads the videos tab as `channel_uploads`.
        let kind = if kind == YoutubeCollectionKind::ChannelVideos {
            YoutubeCollectionKind::ChannelUploads
        } else {
            kind
        };
        let catalog = self.catalog();
        let suffix = |key: &str| format!("{} - {}", item.title, catalog.text(key));
        let title = match kind {
            YoutubeCollectionKind::PlaylistVideos
            | YoutubeCollectionKind::SoundcloudArtistTracks => item.title.clone(),
            YoutubeCollectionKind::ChannelVideos | YoutubeCollectionKind::ChannelUploads => {
                suffix("channel_videos")
            }
            YoutubeCollectionKind::ChannelPlaylists => suffix("channel_playlists"),
            YoutubeCollectionKind::ChannelStreams => suffix("channel_live_streams"),
            YoutubeCollectionKind::ChannelPopular => suffix("channel_popular"),
        };
        let work = match self
            .application
            .begin_youtube_collection(title.clone(), url, kind)
        {
            Ok(work) => work,
            Err(error) => {
                ui.set_status(&error.to_string(), true);
                return;
            }
        };
        // Python `open_soundcloud_artist_tracks` says `loading_channel`.
        let key = if kind == YoutubeCollectionKind::SoundcloudArtistTracks {
            "loading_channel"
        } else {
            "loading_playlist"
        };
        ui.set_status(&catalog.text(key).replace("{title}", &title), false);
        self.start_youtube_collection_work(ui, work);
    }

    /// Windows `start_youtube_collection_work`.
    pub(crate) fn start_youtube_collection_work(
        &mut self,
        ui: &mut dyn ShellUi,
        work: YoutubeCollectionWork,
    ) {
        let Some(components) = components_directory() else {
            self.report_collection_start_error(ui, &work, "Application path is unavailable");
            return;
        };
        let config = self.youtube_session_config();
        let started = self.youtube.search.start_collection(
            YoutubeBackend::YtDlp,
            &components,
            config,
            work.generation,
            work.url.clone(),
            work.kind,
            work.limit,
        );
        if let Err(error) = started {
            self.report_collection_start_error(ui, &work, &error.to_string());
            return;
        }
        let initial = work.work_kind == YoutubeCollectionWorkKind::Initial;
        self.youtube.pending_list = Some(PendingList::Collection(work));
        ui.set_background_polling(true);
        if !initial {
            return;
        }
        let route = if self
            .application
            .youtube_collection()
            .is_some_and(|collection| collection.kind() == YoutubeCollectionKind::PlaylistVideos)
        {
            Route::PlaylistResults
        } else {
            Route::ChannelResults
        };
        self.application.navigate_to(RouteFrame::new(route));
        self.view = View::Collection;
        self.refresh_youtube_collection(ui, true);
    }

    /// Windows `report_youtube_collection_start_error`.
    fn report_collection_start_error(
        &mut self,
        ui: &mut dyn ShellUi,
        work: &YoutubeCollectionWork,
        message: &str,
    ) {
        let _ = self
            .application
            .fail_youtube_collection(work.generation, message);
        let _ = self.application.pop_youtube_collection();
        ui.set_status(message, true);
        ui.show_error(message);
        ui.focus_primary_control();
    }

    /// Windows `finish_youtube_list`.
    pub(crate) fn finish_youtube_list(
        &mut self,
        ui: &mut dyn ShellUi,
        token: u64,
        items: Vec<MediaItem>,
        continuation: Option<String>,
    ) {
        match self.youtube.pending_list.as_ref() {
            Some(pending) if pending.token() != token => {}
            Some(PendingList::Search(_)) => {
                self.finish_youtube_search(ui, token, items, continuation);
            }
            Some(PendingList::Collection(_)) => self.finish_youtube_collection(ui, token, items),
            Some(PendingList::Trending(_)) => {
                self.finish_youtube_trending(ui, token, items, false);
            }
            Some(PendingList::PlaylistPlayback { shuffle, .. }) => {
                let shuffle = *shuffle;
                self.finish_playlist_playback(ui, token, items, shuffle);
            }
            None => {}
        }
    }

    /// Windows `finish_youtube_collection`.
    fn finish_youtube_collection(
        &mut self,
        ui: &mut dyn ShellUi,
        generation: u64,
        items: Vec<MediaItem>,
    ) {
        let work_kind = match self.youtube.pending_list.take() {
            Some(PendingList::Collection(work)) => Some(work.work_kind),
            _ => None,
        };
        let outcome = self
            .application
            .apply_youtube_collection_results(generation, items);
        match (work_kind, outcome) {
            (Some(YoutubeCollectionWorkKind::Initial), YoutubeCollectionApplyOutcome::Replaced) => {
                self.view = View::Collection;
                self.refresh_youtube_collection(ui, true);
            }
            (
                Some(YoutubeCollectionWorkKind::More),
                YoutubeCollectionApplyOutcome::Appended { added },
            ) => self.append_collection_results(ui, added),
            _ => {}
        }
    }

    fn collection_rows(&self) -> Vec<String> {
        let catalog = self.catalog();
        self.application
            .youtube_collection()
            .map(|collection| {
                collection
                    .items()
                    .iter()
                    .map(|item| result_label(item, &catalog, self.application.downloads()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Windows `refresh_youtube_collection`: the loading text, the rows or
    /// "No results" in the search screen's result list.
    pub(crate) fn refresh_youtube_collection(&mut self, ui: &mut dyn ShellUi, focus: bool) {
        let Some(collection) = self.application.youtube_collection() else {
            return;
        };
        let count = collection.items().len();
        let (rows, selected, status) =
            if collection.phase() == YoutubeCollectionPhase::LoadingInitial {
                let loading = self
                    .text("loading_playlist")
                    .replace("{title}", collection.title());
                (vec![loading.clone()], 0, loading)
            } else if count == 0 {
                let none = self.text("no_results");
                (vec![none.clone()], 0, none)
            } else {
                (
                    self.collection_rows(),
                    collection.selected_index().min(count - 1),
                    self.text("found").replace("{count}", &count.to_string()),
                )
            };
        let mut screen = self.search_screen(rows, selected, false);
        screen.focus_list = focus;
        ui.show_list_screen(&screen);
        ui.set_status(&status, false);
    }

    /// Windows `append_youtube_collection_results`.
    fn append_collection_results(&mut self, ui: &mut dyn ShellUi, added: usize) {
        if added == 0 {
            ui.set_status(&self.text("no_more_results"), false);
            return;
        }
        let Some(collection) = self.application.youtube_collection() else {
            return;
        };
        let count = collection.items().len();
        let selected = collection.selected_index().min(count.saturating_sub(1));
        ui.set_list_rows(self.collection_rows(), selected);
        let loaded = self
            .text("search_more_loaded")
            .replace("{count}", &count.to_string());
        ui.set_status(&loaded, false);
    }

    /// Windows `result_selection_changed` on a collection: the last row
    /// loads more.
    pub(crate) fn collection_selection_changed(&mut self, ui: &mut dyn ShellUi, index: usize) {
        if !self.application.select_youtube_collection_result(index) {
            return;
        }
        let last = self
            .application
            .youtube_collection()
            .is_some_and(|collection| index + 1 == collection.items().len());
        if last && let Some(work) = self.application.request_more_youtube_collection_results() {
            ui.set_status(&self.text("loading_more_results"), false);
            self.start_youtube_collection_work(ui, work);
        }
    }

    /// Windows `play_youtube_playlist`: every video of the playlist, then
    /// the first (or a shuffled one) plays.
    pub(crate) fn play_youtube_playlist(
        &mut self,
        ui: &mut dyn ShellUi,
        item: &MediaItem,
        shuffle: bool,
    ) {
        let Some(url) = item.url.as_ref().map(ToString::to_string) else {
            ui.set_status(&self.text("no_selection"), true);
            return;
        };
        self.cancel_youtube_work();
        let token = self.youtube.next_token();
        let Some(components) = components_directory() else {
            ui.show_error("Application path is unavailable");
            return;
        };
        let config = self.youtube_session_config();
        match self.youtube.search.start_collection_all(
            YoutubeBackend::YtDlp,
            &components,
            config,
            token,
            url,
            YoutubeCollectionKind::PlaylistVideos,
        ) {
            Ok(()) => {
                self.youtube.pending_list = Some(PendingList::PlaylistPlayback { token, shuffle });
                let message = self
                    .text("loading_playlist")
                    .replace("{title}", &item.title);
                ui.set_status(&message, false);
                ui.set_background_polling(true);
            }
            Err(error) => {
                let message = error.to_string();
                ui.set_status(&message, true);
                ui.show_error(&message);
                ui.focus_primary_control();
            }
        }
    }

    /// Windows `finish_youtube_playlist_playback`.
    fn finish_playlist_playback(
        &mut self,
        ui: &mut dyn ShellUi,
        token: u64,
        items: Vec<MediaItem>,
        shuffle: bool,
    ) {
        self.youtube.pending_list = None;
        if let Some(item) = self
            .application
            .prepare_youtube_playlist_playback(token, items, shuffle)
        {
            self.start_sequence_media_item(ui, item, None);
        } else {
            ui.set_status(&self.text("playlist_no_videos"), true);
            ui.focus_primary_control();
        }
    }
}
