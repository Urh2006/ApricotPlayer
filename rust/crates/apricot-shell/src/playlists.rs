//! User playlists (Windows `show_user_playlists`, `refresh_user_playlists`,
//! `show_user_playlist_items`, `refresh_user_playlist_items`,
//! `activate_user_playlist_item`, `create_user_playlist`,
//! `add_active_item_to_user_playlist`, `add_item_to_user_playlist`,
//! `remove_selected_user_playlist`, `remove_selected_user_playlist_item`,
//! `remove_active_item_from_user_playlist` and
//! `sync_user_playlist_item_selection`).

use apricot_app::{
    PlaybackSequenceSource, PlaylistAddOutcome, PlaylistCreateOutcome, UserPlaylistControllerError,
};
use apricot_core::{MediaItem, Route, RouteFrame};

use crate::{
    ChoiceDialog, ListScreen, ScreenControl, Shell, ShellUi, View, media_collection_label,
};

/// Windows `current_user_playlist_index` and `current_user_playlist_item_index`.
#[derive(Default)]
pub(crate) struct PlaylistState {
    pub(crate) playlist: usize,
    pub(crate) item: usize,
}

fn unix_timestamp() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
}

impl Shell {
    fn screen_button(&self, id: &'static str, key: &str) -> ScreenControl {
        ScreenControl::Button {
            id,
            label: self.text(key),
        }
    }

    /// Windows `show_user_playlists`.
    pub(crate) fn show_user_playlists(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("playlists");
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if self.application.current_route() != Route::UserPlaylists {
            self.application.navigate_main_menu();
            self.application
                .navigate_to(RouteFrame::new(Route::UserPlaylists));
        }
        self.view = View::UserPlaylists;
        self.refresh_user_playlists(ui, true);
    }

    /// Windows `refresh_user_playlists` with the Windows Tab order: Back,
    /// Create playlist, Open playlist, Download playlist, Remove playlist,
    /// the list.
    pub(crate) fn refresh_user_playlists(&mut self, ui: &mut dyn ShellUi, focus: bool) {
        let catalog = self.catalog();
        let playlists = self.application.user_playlists();
        let (rows, selected) = if playlists.is_empty() {
            ui.set_status(catalog.text("no_playlists"), false);
            (vec![catalog.text("no_playlists").to_owned()], 0)
        } else {
            let rows = playlists
                .iter()
                .map(|playlist| {
                    format!(
                        "{} | {} {}",
                        playlist.title,
                        playlist.items.len(),
                        catalog.text("video")
                    )
                })
                .collect();
            let selected = self.playlists.playlist.min(playlists.len() - 1);
            self.playlists.playlist = selected;
            (rows, selected)
        };
        ui.show_list_screen(&ListScreen {
            list_name: catalog.text("playlists").to_owned(),
            rows,
            selected: Some(selected),
            controls: vec![
                self.screen_button("back", "back"),
                self.screen_button("playlist_create", "create_playlist"),
                self.screen_button("open", "open_playlist"),
                self.screen_button("playlist_download", "download_user_playlist"),
                self.screen_button("collection_remove", "remove_playlist"),
                ScreenControl::List,
            ],
            focus_list: focus,
        });
    }

    /// Windows `open_selected_user_playlist`.
    fn open_selected_user_playlist(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = ui.selected_row() else {
            return;
        };
        if index >= self.application.user_playlists().len() {
            ui.set_status(&self.text("no_playlists"), true);
            return;
        }
        self.show_user_playlist_items(ui, index, true);
    }

    /// Windows `show_user_playlist_items`.
    pub(crate) fn show_user_playlist_items(
        &mut self,
        ui: &mut dyn ShellUi,
        playlist_index: usize,
        push_route: bool,
    ) {
        ui.restore();
        self.stop_key_repeat(ui);
        if playlist_index >= self.application.user_playlists().len() {
            self.show_user_playlists(ui);
            return;
        }
        self.playlists.playlist = playlist_index;
        self.playlists.item = 0;
        if push_route && self.application.current_route() != Route::UserPlaylistItems {
            let mut frame = RouteFrame::new(Route::UserPlaylistItems);
            frame
                .parameters
                .insert("playlist_index".to_owned(), playlist_index.into());
            self.application.navigate_to(frame);
        }
        self.view = View::UserPlaylistItems;
        self.refresh_user_playlist_items(ui, true, false);
    }

    /// Windows `refresh_user_playlist_items` with the Windows Tab order:
    /// Back, then Play, Download playlist and Remove from playlist when the
    /// playlist has items, the list.
    pub(crate) fn refresh_user_playlist_items(
        &mut self,
        ui: &mut dyn ShellUi,
        focus: bool,
        announce_status: bool,
    ) {
        let catalog = self.catalog();
        let mut controls = vec![self.screen_button("back", "back")];
        let playlist = self
            .application
            .user_playlists()
            .get(self.playlists.playlist);
        let (rows, selected, status) = match playlist {
            Some(playlist) if !playlist.items.is_empty() => {
                let selected = self.playlists.item.min(playlist.items.len() - 1);
                let rows = playlist
                    .items
                    .iter()
                    .map(|item| media_collection_label(item, false, &catalog))
                    .collect();
                let status = format!(
                    "{}: {} | {}",
                    catalog.text("playlist_items"),
                    playlist.title,
                    playlist.items.len()
                );
                controls.extend([
                    self.screen_button("open", "play"),
                    self.screen_button("playlist_download", "download_user_playlist"),
                    self.screen_button("collection_remove", "remove_from_playlist"),
                ]);
                (rows, selected, status)
            }
            _ => {
                let empty = catalog.text("playlist_empty").to_owned();
                (vec![empty.clone()], 0, empty)
            }
        };
        self.playlists.item = selected;
        controls.push(ScreenControl::List);
        ui.show_list_screen(&ListScreen {
            list_name: catalog.text("playlist_items").to_owned(),
            rows,
            selected: Some(selected),
            controls,
            focus_list: focus,
        });
        ui.set_status(&status, announce_status);
    }

    /// Windows `activate_user_playlist_item`: the playlist stays the
    /// sequence, so Next and autoplay go on in it.
    fn activate_user_playlist_item(&mut self, ui: &mut dyn ShellUi) {
        let Some(item_index) = ui.selected_row() else {
            ui.set_status(&self.text("playlist_empty"), true);
            return;
        };
        self.playlists.item = item_index;
        if let Some(item) = self
            .application
            .prepare_user_playlist_item_playback(self.playlists.playlist, item_index)
        {
            self.start_sequence_media_item(ui, item, None);
        }
    }

    /// Enter or Open on a playlist screen.
    pub(crate) fn activate_playlist_selection(&mut self, ui: &mut dyn ShellUi) {
        if self.view == View::UserPlaylists {
            self.open_selected_user_playlist(ui);
        } else {
            self.activate_user_playlist_item(ui);
        }
    }

    /// Windows `result_selection_changed` on the playlist screens.
    pub(crate) fn playlist_selection_changed(&mut self, index: usize) {
        let playlists = self.application.user_playlists();
        if self.view == View::UserPlaylists {
            if index < playlists.len() {
                self.playlists.playlist = index;
            }
        } else if playlists
            .get(self.playlists.playlist)
            .is_some_and(|playlist| index < playlist.items.len())
        {
            self.playlists.item = index;
        }
    }

    /// Windows `sync_user_playlist_item_selection`: the playing item of a
    /// playlist sequence becomes the selected row.
    pub(crate) fn sync_user_playlist_item_selection(&mut self, item: &MediaItem) {
        let Some(PlaybackSequenceSource::UserPlaylist { playlist_index }) =
            self.application.player_sequence_source()
        else {
            return;
        };
        let Some(identity) = item.stable_identity() else {
            return;
        };
        let Some(item_index) =
            self.application
                .user_playlists()
                .get(playlist_index)
                .and_then(|playlist| {
                    playlist.items.iter().position(|candidate| {
                        candidate.stable_identity().as_deref() == Some(&identity)
                    })
                })
        else {
            return;
        };
        self.playlists.playlist = playlist_index;
        self.playlists.item = item_index;
    }

    /// Windows `create_user_playlist`.
    pub(crate) fn create_user_playlist(
        &mut self,
        ui: &mut dyn ShellUi,
        initial_item: Option<MediaItem>,
    ) {
        let Some(name) = ui.prompt_text(
            &self.text("create_playlist"),
            &self.text("playlist_name"),
            "",
            &self.text("ok"),
            &self.text("cancel"),
        ) else {
            return;
        };
        let timestamp = unix_timestamp();
        let outcome = match initial_item {
            Some(item) => self
                .application
                .create_user_playlist_with_item(&name, item, timestamp),
            None => self.application.create_user_playlist(&name, timestamp),
        };
        match outcome {
            Ok(PlaylistCreateOutcome::Created(index)) => {
                self.playlists.playlist = index;
                if self.view == View::UserPlaylists {
                    self.refresh_user_playlists(ui, true);
                } else if self.view == View::UserPlaylistItems {
                    self.refresh_user_playlist_items(ui, true, false);
                }
                let message = self
                    .text("playlist_created")
                    .replace("{title}", name.trim());
                ui.set_status(&message, true);
            }
            Ok(PlaylistCreateOutcome::AlreadyExists) => {
                ui.set_status(&self.text("playlist_exists"), true);
            }
            Ok(PlaylistCreateOutcome::EmptyName) => {}
            Ok(PlaylistCreateOutcome::UnsupportedItem) => {
                ui.set_status(&self.text("no_selection"), true);
            }
            Err(error) => {
                let message = format!("Playlist was not created: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }

    /// Windows `choose_user_playlist_from_indices` (all playlists for
    /// `choose_user_playlist`).
    fn choose_user_playlist(
        &self,
        ui: &mut dyn ShellUi,
        title_key: &str,
        indices: &[usize],
    ) -> Option<usize> {
        let choices: Vec<String> = indices
            .iter()
            .filter_map(|index| self.application.user_playlists().get(*index))
            .map(|playlist| playlist.title.clone())
            .collect();
        let prompt = self.text("select_playlist");
        let choice = ui.choose(&ChoiceDialog {
            title: self.text(title_key),
            list_name: prompt.clone(),
            prompt,
            choices,
            initial: 0,
            ok: self.text("ok"),
            cancel: self.text("cancel"),
        })?;
        indices.get(choice).copied()
    }

    /// Windows `add_active_item_to_user_playlist`: a new playlist when there
    /// is none, the only one, or a choice.
    pub(crate) fn add_active_item_to_user_playlist(&mut self, ui: &mut dyn ShellUi) {
        let Some(item) = self.active_media_item(ui).filter(MediaItem::is_playable) else {
            ui.set_status(&self.text("no_selection"), true);
            return;
        };
        let count = self.application.user_playlists().len();
        if count == 0 {
            self.create_user_playlist(ui, Some(item));
            return;
        }
        let playlist_index = if count == 1 {
            0
        } else {
            let all: Vec<usize> = (0..count).collect();
            let Some(index) = self.choose_user_playlist(ui, "add_to_playlist", &all) else {
                return;
            };
            index
        };
        self.add_item_to_user_playlist(ui, playlist_index, item);
    }

    /// Windows `add_active_item_to_chosen_user_playlist`.
    pub(crate) fn add_active_item_to_chosen_user_playlist(
        &mut self,
        ui: &mut dyn ShellUi,
        playlist_index: usize,
    ) {
        let Some(item) = self.active_media_item(ui).filter(MediaItem::is_playable) else {
            ui.set_status(&self.text("no_selection"), true);
            return;
        };
        self.add_item_to_user_playlist(ui, playlist_index, item);
    }

    /// Windows `add_item_to_user_playlist`.
    fn add_item_to_user_playlist(
        &mut self,
        ui: &mut dyn ShellUi,
        playlist_index: usize,
        item: MediaItem,
    ) {
        let title = item.title.clone();
        match self
            .application
            .add_item_to_user_playlist(playlist_index, item, unix_timestamp())
        {
            Ok(PlaylistAddOutcome::Added(_)) => {
                if self.view == View::UserPlaylistItems && self.playlists.playlist == playlist_index
                {
                    self.refresh_user_playlist_items(ui, false, false);
                }
                let playlist = self
                    .application
                    .user_playlists()
                    .get(playlist_index)
                    .map_or("", |playlist| playlist.title.as_str());
                let message = self
                    .text("added_to_playlist")
                    .replace("{playlist}", playlist)
                    .replace("{title}", &title);
                ui.set_status(&message, true);
            }
            Ok(PlaylistAddOutcome::AlreadyPresent) => {
                ui.set_status(&self.text("playlist_exists"), true);
            }
            Ok(PlaylistAddOutcome::MissingPlaylist | PlaylistAddOutcome::Unsupported) => {
                ui.set_status(&self.text("no_selection"), true);
            }
            Err(error) => {
                let message = format!("Playlist was not updated: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }

    /// Windows `remove_selected_user_playlist`.
    pub(crate) fn remove_selected_user_playlist(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = ui.selected_row() else {
            return;
        };
        match self.application.remove_user_playlist(index) {
            Ok(Some(_)) => {
                self.playlists.playlist =
                    index.min(self.application.user_playlists().len().saturating_sub(1));
                self.refresh_user_playlists(ui, true);
                ui.set_status(&self.text("playlist_removed"), true);
            }
            Ok(None) => ui.set_status(&self.text("no_playlists"), true),
            Err(error) => {
                let message = format!("Playlist was not removed: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }

    /// Windows `remove_selected_user_playlist_item`.
    pub(crate) fn remove_selected_user_playlist_item(&mut self, ui: &mut dyn ShellUi) {
        let Some(item_index) = ui.selected_row() else {
            ui.set_status(&self.text("playlist_empty"), true);
            return;
        };
        let result = self.application.remove_user_playlist_item(
            self.playlists.playlist,
            item_index,
            unix_timestamp(),
        );
        self.finish_user_playlist_item_removal(ui, result);
    }

    /// Windows `remove_active_item_from_user_playlist`.
    pub(crate) fn remove_active_item_from_user_playlist(&mut self, ui: &mut dyn ShellUi) {
        if self.view == View::UserPlaylistItems {
            self.remove_selected_user_playlist_item(ui);
            return;
        }
        let Some(item) = self.active_media_item(ui) else {
            return;
        };
        let matches = self.application.user_playlist_matches(&item);
        let playlist_index = match matches.as_slice() {
            [] => {
                ui.set_status(&self.text("not_in_playlist"), true);
                return;
            }
            [index] => *index,
            _ => {
                let Some(index) = self.choose_user_playlist(ui, "remove_from_playlist", &matches)
                else {
                    return;
                };
                index
            }
        };
        let identity = item.copy_location().or_else(|| item.stable_identity());
        let Some(item_index) = identity.and_then(|identity| {
            self.application
                .user_playlists()
                .get(playlist_index)?
                .items
                .iter()
                .position(|candidate| {
                    candidate
                        .copy_location()
                        .or_else(|| candidate.stable_identity())
                        .as_deref()
                        == Some(&identity)
                })
        }) else {
            return;
        };
        let result = self.application.remove_user_playlist_item(
            playlist_index,
            item_index,
            unix_timestamp(),
        );
        self.finish_user_playlist_item_removal(ui, result);
    }

    /// Windows `finish_user_playlist_item_removal`.
    fn finish_user_playlist_item_removal(
        &mut self,
        ui: &mut dyn ShellUi,
        result: Result<Option<MediaItem>, UserPlaylistControllerError>,
    ) {
        match result {
            Ok(Some(_)) => {
                if self.view == View::UserPlaylistItems {
                    self.refresh_user_playlist_items(ui, true, false);
                }
                ui.set_status(&self.text("removed_from_playlist"), true);
            }
            Ok(None) => {}
            Err(error) => {
                let message = format!("Playlist was not updated: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }
}
