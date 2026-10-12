//! Favorites and history (Windows `show_media_collection`,
//! `refresh_media_collection`, `media_collection_label`,
//! `activate_collection_selection`, `open_library_item`,
//! `add_active_favorite`, `remove_active_favorite`,
//! `remove_selected_collection_item` and `clear_history`).

use apricot_app::{CollectionAddOutcome, MediaCollectionControllerError, YoutubeCollectionKind};
use apricot_core::{MediaItem, MediaKind, MediaSource, Route, RouteFrame, TranslationCatalog};

use crate::{ListScreen, ScreenControl, Shell, ShellUi, View};

/// Windows `media_collection_label`: history rows start with the time and
/// the action.
#[must_use]
pub fn media_collection_label(
    item: &MediaItem,
    history: bool,
    catalog: &TranslationCatalog,
) -> String {
    let mut parts = Vec::new();
    if history {
        if let Some(timestamp) = item
            .metadata
            .get("timestamp")
            .and_then(serde_json::Value::as_f64)
            .and_then(|seconds| std::time::Duration::try_from_secs_f64(seconds).ok())
            .and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        {
            parts.push(
                timestamp
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string(),
            );
        }
        if let Some(action) = item.metadata.get("action").and_then(|value| value.as_str())
            && !action.trim().is_empty()
        {
            parts.push(action.to_owned());
        }
    }
    parts.push(item.title.clone());
    if !item.channel.is_empty() {
        parts.push(format!("{}: {}", catalog.text("channel"), item.channel));
    }
    parts.join(" | ")
}

impl Shell {
    /// Windows `show_media_collection`.
    pub(crate) fn show_media_collection(&mut self, ui: &mut dyn ShellUi, view: View) {
        let history = view == View::History;
        self.remember_menu_item(if history { "history" } else { "favorites" });
        if history && !self.application.settings().enable_history {
            self.show_main_menu(ui);
            return;
        }
        let route = if history {
            Route::History
        } else {
            Route::Favorites
        };
        ui.restore();
        self.stop_key_repeat(ui);
        self.cancel_youtube_work();
        self.cancel_local_folder_scan();
        if self.application.current_route() != route {
            self.application.navigate_main_menu();
            self.application.navigate_to(RouteFrame::new(route));
        }
        self.view = view;
        self.refresh_media_collection(ui, true, None);
    }

    /// Windows `refresh_media_collection`, with the Windows creation order
    /// list, Play, Back, Remove and (history) Clear history. `selected`
    /// restores a saved row (Windows `select_list_index`).
    pub(crate) fn refresh_media_collection(
        &mut self,
        ui: &mut dyn ShellUi,
        focus: bool,
        selected: Option<usize>,
    ) {
        let history = self.view == View::History;
        let catalog = self.catalog();
        let (name_key, empty_key, items) = if history {
            ("history", "history_empty", self.application.history())
        } else {
            ("favorites", "favorites_empty", self.application.favorites())
        };
        let rows = if items.is_empty() {
            ui.set_status(catalog.text(empty_key), false);
            vec![catalog.text(empty_key).to_owned()]
        } else {
            items
                .iter()
                .map(|item| media_collection_label(item, history, &catalog))
                .collect()
        };
        let selected = selected.unwrap_or(0).min(rows.len() - 1);
        let button = |id, key: &str| ScreenControl::Button {
            id,
            label: catalog.text(key).to_owned(),
        };
        let mut controls = vec![
            ScreenControl::List,
            button("open", "play"),
            button("back", "back"),
            button(
                "collection_remove",
                if history {
                    "remove_history_item"
                } else {
                    "remove_favorite"
                },
            ),
        ];
        if history {
            controls.push(button("history_clear", "clear_history"));
        }
        ui.show_list_screen(&ListScreen {
            list_name: catalog.text(name_key).to_owned(),
            rows,
            selected: Some(selected),
            controls,
            focus_list: focus,
        });
    }

    /// Windows `activate_collection_selection`.
    pub(crate) fn activate_library_selection(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = ui.selected_row() else {
            return;
        };
        let item = if self.view == View::History {
            self.application.prepare_history_playback(index)
        } else {
            self.application.prepare_favorite_playback(index)
        };
        if let Some(item) = item {
            self.open_library_item(ui, item);
        }
    }

    /// Windows `open_library_item`: a channel opens its videos, a playlist
    /// its videos, everything else plays with shuffle off.
    fn open_library_item(&mut self, ui: &mut dyn ShellUi, item: MediaItem) {
        match item.kind {
            MediaKind::Channel if item.source == MediaSource::Soundcloud => {
                self.open_youtube_collection(
                    ui,
                    &item,
                    YoutubeCollectionKind::SoundcloudArtistTracks,
                );
            }
            MediaKind::Channel => {
                self.open_youtube_collection(ui, &item, YoutubeCollectionKind::ChannelVideos);
            }
            MediaKind::Playlist => {
                self.open_youtube_collection(ui, &item, YoutubeCollectionKind::PlaylistVideos);
            }
            _ => self.start_media_item_with_options(ui, item, None, Some(false), true, None),
        }
    }

    /// Windows `add_active_favorite`.
    pub(crate) fn add_active_favorite(&mut self, ui: &mut dyn ShellUi) {
        let Some(item) = self.active_media_item(ui) else {
            return;
        };
        match self.application.add_favorite(item) {
            Ok(CollectionAddOutcome::Added) => ui.set_status(&self.text("favorite_added"), true),
            Ok(CollectionAddOutcome::AlreadyPresent) => {
                ui.set_status(&self.text("favorite_exists"), true);
            }
            Ok(CollectionAddOutcome::Unplayable) => {
                ui.set_status(&self.text("no_selection"), true);
            }
            Err(error) => {
                let message = format!("Favorites were not updated: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }

    /// Windows `remove_active_favorite`.
    pub(crate) fn remove_active_favorite(&mut self, ui: &mut dyn ShellUi) {
        let Some(item) = self.active_media_item(ui) else {
            return;
        };
        match self.application.remove_favorite_item(&item) {
            Ok(Some(_)) => {
                if self.view == View::Favorites {
                    self.refresh_media_collection(ui, true, None);
                }
                ui.set_status(&self.text("favorite_removed"), true);
            }
            Ok(None) => ui.set_status(&self.text("not_in_favorites"), true),
            Err(error) => {
                let message = format!("Favorites were not updated: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }

    /// Windows `remove_selected_collection_item` for favorites and history.
    pub(crate) fn remove_selected_collection_item(&mut self, ui: &mut dyn ShellUi) {
        let Some(index) = ui.selected_row() else {
            return;
        };
        let (result, key) = match self.view {
            View::Favorites => (self.application.remove_favorite(index), "favorite_removed"),
            View::History => (
                self.application.remove_history_item(index),
                "history_removed",
            ),
            View::UserPlaylists => {
                self.remove_selected_user_playlist(ui);
                return;
            }
            View::SpotifyAccounts => {
                self.spotify_remove_selected(ui);
                return;
            }
            View::UserPlaylistItems => {
                self.remove_selected_user_playlist_item(ui);
                return;
            }
            _ => return,
        };
        self.finish_collection_removal(ui, result, key);
    }

    /// Windows `finish_collection_removal`.
    fn finish_collection_removal(
        &mut self,
        ui: &mut dyn ShellUi,
        result: Result<Option<MediaItem>, MediaCollectionControllerError>,
        success_key: &str,
    ) {
        match result {
            Ok(Some(_)) => {
                self.refresh_media_collection(ui, true, None);
                ui.set_status(&self.text(success_key), true);
            }
            Ok(None) => {}
            Err(error) => {
                let message = format!("Media collection was not updated: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }

    /// Windows `clear_history`.
    pub(crate) fn clear_history(&mut self, ui: &mut dyn ShellUi) {
        match self.application.clear_history() {
            Ok(_) => {
                self.refresh_media_collection(ui, true, None);
                ui.set_status(&self.text("history_cleared"), true);
            }
            Err(error) => {
                let message = format!("History was not cleared: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }
}
