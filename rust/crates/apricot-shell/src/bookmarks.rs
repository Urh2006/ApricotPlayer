//! Playback bookmarks: the add prompt (Windows
//! `prompt_add_current_bookmark`), the bookmarks window
//! (`show_bookmarks_dialog` with `bookmark_dialog_win32.rs`) and its
//! requests (`handle_bookmark_dialog_request`, `prompt_rename_bookmark`,
//! `copy_bookmark_timestamp`, `play_bookmark`).

use apricot_core::{Route, RouteFrame};
use apricot_playback::PlaybackCommand;

use crate::{Shell, ShellUi, View, format_duration};

/// One row of the bookmarks list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookmarkEntry {
    pub id: String,
    pub label: String,
}

/// What a button, key or menu item of the bookmarks window asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BookmarkRequest {
    Add,
    Play(String),
    Rename(String),
    Delete(String),
    Copy(String),
}

/// What the window does after a request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BookmarkResponse {
    KeepOpen,
    /// New rows with `selected_id` selected (the first row without one);
    /// focus returns to the list.
    Refresh {
        entries: Vec<BookmarkEntry>,
        selected_id: Option<String>,
    },
    Close,
}

/// The bookmarks window: its texts, rows and whether Add is enabled.
/// An empty list shows `empty` as its only row with Play, Rename, Delete
/// and Copy disabled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BookmarkDialog {
    pub title: String,
    pub list_name: String,
    pub empty: String,
    pub add: String,
    pub play: String,
    pub rename: String,
    pub delete: String,
    pub copy: String,
    pub close: String,
    pub entries: Vec<BookmarkEntry>,
    pub can_add: bool,
    /// Only the bookmarks of the playing item (from the player).
    pub current_only: bool,
    /// Opened by going back to the bookmarks route: closing goes back again.
    pub return_on_close: bool,
}

fn unix_timestamp() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |duration| duration.as_secs_f64())
}

/// Windows `report_bookmark_error`.
fn report_bookmark_error(ui: &mut dyn ShellUi, detail: &str) {
    let message = format!("Bookmarks were not updated: {detail}");
    ui.set_status(&message, true);
    ui.show_error(&message);
}

impl Shell {
    /// Windows `show_add_current_bookmark_prompt` and
    /// `prompt_add_current_bookmark`: the name defaults to the time; the new
    /// bookmark's id is returned.
    pub(crate) fn prompt_add_current_bookmark(&mut self, ui: &mut dyn ShellUi) -> Option<String> {
        let session = self.application.player_session();
        let Some(item) = session.current_item().cloned() else {
            ui.set_status(&self.text("no_player"), true);
            return None;
        };
        let position = session.position_seconds().max(0.0);
        self.stop_key_repeat(ui);
        let default_name = self
            .text("bookmark_default_name")
            .replace("{time}", &format_duration(position));
        let name = ui.prompt_text(
            &self.text("add_bookmark"),
            &self.text("bookmark_name_prompt"),
            &default_name,
            &self.text("ok"),
            &self.text("cancel"),
        )?;
        let name = match name.trim() {
            "" => default_name,
            name => name.to_owned(),
        };
        match self
            .application
            .add_bookmark(&name, position, item, unix_timestamp())
        {
            Ok(Some(bookmark)) => {
                let message = self
                    .text("bookmark_added")
                    .replace("{name}", &bookmark.name)
                    .replace("{time}", &format_duration(bookmark.position));
                ui.set_status(&message, true);
                Some(bookmark.id)
            }
            Ok(None) => None,
            Err(error) => {
                report_bookmark_error(ui, &error.to_string());
                None
            }
        }
    }

    /// Windows `bookmark_dialog_entries`.
    fn bookmark_entries(&self, current_only: bool) -> Vec<BookmarkEntry> {
        let catalog = self.catalog();
        let bookmarks = if current_only {
            self.application
                .player_session()
                .current_item()
                .map_or_else(Vec::new, |item| self.application.bookmarks_for_item(item))
        } else {
            self.application.sorted_bookmarks()
        };
        bookmarks
            .into_iter()
            .enumerate()
            .map(|(index, bookmark)| {
                let name = if bookmark.name.trim().is_empty() {
                    catalog.text("bookmark")
                } else {
                    bookmark.name.trim()
                };
                let mut parts = vec![
                    format!("{}. {}", index + 1, format_duration(bookmark.position)),
                    name.to_owned(),
                ];
                if !current_only && !bookmark.media_title.trim().is_empty() {
                    parts.push(bookmark.media_title.trim().to_owned());
                }
                BookmarkEntry {
                    id: bookmark.id.clone(),
                    label: parts.join(" | "),
                }
            })
            .collect()
    }

    /// Windows `show_bookmarks_dialog`: the window opens after the current
    /// event and reports its requests to [`Shell::bookmark_request`].
    pub(crate) fn show_bookmarks_dialog(
        &mut self,
        ui: &mut dyn ShellUi,
        current_only: bool,
        return_on_close: bool,
    ) {
        self.stop_key_repeat(ui);
        if !current_only {
            self.remember_menu_item("bookmarks");
        }
        let session = self.application.player_session();
        let can_add = session.is_open()
            && session
                .current_item()
                .is_some_and(apricot_core::MediaItem::is_playable);
        ui.open_bookmarks(BookmarkDialog {
            title: self.text("bookmarks"),
            list_name: self.text("bookmarks"),
            empty: self.text("bookmarks_empty"),
            add: self.text("add_bookmark"),
            play: self.text("play"),
            rename: self.text("rename_bookmark"),
            delete: self.text("delete_bookmark"),
            copy: self.text("copy_timestamp_link"),
            close: self.text("back"),
            entries: self.bookmark_entries(current_only),
            can_add,
            current_only,
            return_on_close,
        });
    }

    /// Windows `handle_bookmark_dialog_request`.
    pub fn bookmark_request(
        &mut self,
        ui: &mut dyn ShellUi,
        current_only: bool,
        request: BookmarkRequest,
    ) -> BookmarkResponse {
        let refresh = |shell: &Self, selected_id| BookmarkResponse::Refresh {
            entries: shell.bookmark_entries(current_only),
            selected_id,
        };
        match request {
            BookmarkRequest::Add => {
                let selected_id = self.prompt_add_current_bookmark(ui);
                refresh(self, selected_id)
            }
            BookmarkRequest::Rename(id) => {
                let selected_id = self.prompt_rename_bookmark(ui, &id).then_some(id);
                refresh(self, selected_id)
            }
            BookmarkRequest::Delete(id) => match self.application.delete_bookmark(&id) {
                Ok(true) => {
                    ui.set_status(&self.text("bookmark_deleted"), true);
                    refresh(self, None)
                }
                Ok(false) => BookmarkResponse::KeepOpen,
                Err(error) => {
                    report_bookmark_error(ui, &error.to_string());
                    BookmarkResponse::KeepOpen
                }
            },
            BookmarkRequest::Copy(id) => {
                self.copy_bookmark_timestamp(ui, &id);
                BookmarkResponse::KeepOpen
            }
            BookmarkRequest::Play(id) => {
                self.play_bookmark(ui, &id);
                BookmarkResponse::Close
            }
        }
    }

    /// The bookmarks window closed with focus back where it was; a
    /// bookmark that started playing puts focus on the player, and a
    /// window opened by going back goes back once more.
    pub fn bookmarks_closed(&mut self, ui: &mut dyn ShellUi, return_on_close: bool) {
        if return_on_close && self.application.current_route() == Route::Bookmarks {
            self.navigate_back(ui);
        } else if self.view == View::Player {
            ui.focus_primary_control();
        }
    }

    /// Windows `prompt_rename_bookmark`.
    fn prompt_rename_bookmark(&mut self, ui: &mut dyn ShellUi, id: &str) -> bool {
        let Some(current_name) = self
            .application
            .bookmark(id)
            .map(|bookmark| bookmark.name.clone())
        else {
            return false;
        };
        let Some(name) = ui
            .prompt_text(
                &self.text("rename_bookmark"),
                &self.text("bookmark_name_prompt"),
                &current_name,
                &self.text("ok"),
                &self.text("cancel"),
            )
            .filter(|name| !name.trim().is_empty())
        else {
            return false;
        };
        match self
            .application
            .rename_bookmark(id, &name, unix_timestamp())
        {
            Ok(true) => {
                let message = self.text("bookmark_renamed").replace("{name}", name.trim());
                ui.set_status(&message, true);
                true
            }
            Ok(false) => false,
            Err(error) => {
                report_bookmark_error(ui, &error.to_string());
                false
            }
        }
    }

    /// Windows `copy_bookmark_timestamp`.
    fn copy_bookmark_timestamp(&self, ui: &mut dyn ShellUi, id: &str) {
        let url = self.application.bookmark(id).and_then(|bookmark| {
            bookmark
                .media
                .youtube_url_at_timestamp(bookmark.position)
                .map(|url| url.to_string())
        });
        if let Some(url) = url {
            self.copy_text_and_announce(ui, &url, "timestamp_url_copied");
        } else {
            ui.set_status(&self.text("timestamp_url_unavailable"), true);
        }
    }

    /// Windows `play_bookmark`: a bookmark of the playing item seeks, any
    /// other starts its item at the position with Back returning to the
    /// bookmarks.
    fn play_bookmark(&mut self, ui: &mut dyn ShellUi, id: &str) {
        let Some(bookmark) = self.application.bookmark(id).cloned() else {
            return;
        };
        let same_item = self
            .application
            .player_session()
            .current_item()
            .is_some_and(|item| {
                self.application
                    .bookmarks_for_item(item)
                    .iter()
                    .any(|candidate| candidate.id == bookmark.id)
            });
        if same_item
            && self.execute_player_command(
                ui,
                PlaybackCommand::SeekAbsolute {
                    seconds: bookmark.position,
                    exact: true,
                },
            )
        {
            let name = if bookmark.name.trim().is_empty() {
                self.text("bookmark")
            } else {
                bookmark.name.trim().to_owned()
            };
            let message = self
                .text("bookmark_selected")
                .replace("{name}", &name)
                .replace("{time}", &format_duration(bookmark.position));
            ui.set_status(&message, true);
            return;
        }
        if self.application.current_route() == Route::Player {
            let _ = self.application.navigate_back();
        }
        if self.application.current_route() != Route::Bookmarks {
            self.application
                .navigate_to(RouteFrame::new(Route::Bookmarks));
        }
        self.start_media_item_at(ui, bookmark.media, bookmark.position);
    }
}
