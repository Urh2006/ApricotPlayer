//! The playback queue window (Windows `show_playback_queue` with
//! `playback_queue_win32.rs`), adding and removing the active item
//! (`add_active_item_to_playback_queue`,
//! `remove_active_item_from_playback_queue`) and the context menus of the
//! lists and the player (`show_context_menu_for_active_view`,
//! `execute_context_command`).

use apricot_app::context_menu::{self, ContextCommand, ContextMenuContext, ContextMenuEntry};
use apricot_core::{MediaItem, MediaKind, TranslationCatalog};

use crate::{MainMenuSelection, Shell, ShellUi, View, player::QueueStartMode};

/// The playback queue window: its texts and a working copy of the queue.
/// Buttons in Tab order after the list: Play, Move up, Move down, Remove,
/// Clear and Back.
pub struct QueueDialog {
    pub title: String,
    pub instructions: String,
    pub play: String,
    pub move_up: String,
    pub move_down: String,
    pub remove: String,
    pub clear: String,
    pub back: String,
    items: Vec<MediaItem>,
    changed: bool,
    chosen: Option<MediaItem>,
    labels: QueueRowLabels,
}

/// What a button, key or menu item of the queue window did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueueChange {
    /// Rows to show with this row selected, and what to announce.
    Refresh {
        rows: Vec<String>,
        selected: usize,
        announcement: String,
    },
    /// The window closes (Play, or the queue became empty), after
    /// announcing the text when there is one.
    Close { announcement: Option<String> },
    /// Nothing changed (no row, or a move past an end).
    None,
}

struct QueueRowLabels {
    empty: String,
    channel: String,
    removed: String,
    reordered: String,
    cleared: String,
    kinds: [String; 11],
}

impl QueueRowLabels {
    fn new(catalog: &TranslationCatalog) -> Self {
        let text = |key: &str| catalog.text(key).to_owned();
        Self {
            empty: text("playback_queue_empty"),
            channel: text("channel"),
            removed: text("playback_queue_removed"),
            reordered: text("playback_queue_reordered"),
            cleared: text("playback_queue_cleared"),
            kinds: [
                text("download_audio_mode"),
                text("video"),
                text("live_stream"),
                text("playlist"),
                text("channel"),
                text("rss_feeds"),
                text("podcast_episode"),
                text("movie"),
                text("tv_show"),
                text("episode"),
                text("unknown"),
            ],
        }
    }

    /// Windows `kind_label`.
    fn kind(&self, kind: MediaKind) -> &str {
        let index = match kind {
            MediaKind::Audio => 0,
            MediaKind::Video => 1,
            MediaKind::LiveStream => 2,
            MediaKind::Playlist => 3,
            MediaKind::Channel => 4,
            MediaKind::PodcastFeed => 5,
            MediaKind::PodcastEpisode => 6,
            MediaKind::Movie => 7,
            MediaKind::TvShow => 8,
            MediaKind::TvEpisode => 9,
            MediaKind::Unknown => 10,
        };
        &self.kinds[index]
    }
}

impl QueueDialog {
    /// Windows `refresh_list`: numbered rows with channel and kind, or the
    /// empty text.
    #[must_use]
    pub fn rows(&self) -> Vec<String> {
        if self.items.is_empty() {
            return vec![self.labels.empty.clone()];
        }
        self.items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut parts = vec![format!("{}. {}", index + 1, item.title)];
                if !item.channel.is_empty() {
                    parts.push(format!("{}: {}", self.labels.channel, item.channel));
                }
                parts.push(self.labels.kind(item.kind).to_owned());
                parts.join(" | ")
            })
            .collect()
    }

    fn row(&self, selected: Option<usize>) -> Option<usize> {
        selected.filter(|index| *index < self.items.len())
    }

    /// Windows `play_selected`.
    pub fn play(&mut self, selected: Option<usize>) -> QueueChange {
        let Some(index) = self.row(selected) else {
            return QueueChange::None;
        };
        self.chosen = self.items.get(index).cloned();
        QueueChange::Close { announcement: None }
    }

    /// Windows `move_selected`.
    pub fn move_selected(&mut self, selected: Option<usize>, down: bool) -> QueueChange {
        let Some(index) = self.row(selected) else {
            return QueueChange::None;
        };
        let target = if down {
            index + 1
        } else {
            match index.checked_sub(1) {
                Some(target) => target,
                None => return QueueChange::None,
            }
        };
        if target >= self.items.len() {
            return QueueChange::None;
        }
        self.items.swap(index, target);
        self.changed = true;
        QueueChange::Refresh {
            rows: self.rows(),
            selected: target,
            announcement: self.labels.reordered.clone(),
        }
    }

    /// Windows `remove_selected`: the window closes once the queue is empty.
    pub fn remove(&mut self, selected: Option<usize>) -> QueueChange {
        let Some(index) = self.row(selected) else {
            return QueueChange::None;
        };
        let removed = self.items.remove(index);
        self.changed = true;
        let announcement = self.labels.removed.replace("{title}", &removed.title);
        if self.items.is_empty() {
            return QueueChange::Close {
                announcement: Some(announcement),
            };
        }
        QueueChange::Refresh {
            rows: self.rows(),
            selected: index.min(self.items.len() - 1),
            announcement,
        }
    }

    /// Windows `clear_queue`: the window closes after clearing.
    pub fn clear(&mut self) -> QueueChange {
        if self.items.is_empty() {
            return QueueChange::Close {
                announcement: Some(self.labels.empty.clone()),
            };
        }
        self.items.clear();
        self.changed = true;
        QueueChange::Close {
            announcement: Some(self.labels.cleared.clone()),
        }
    }
}

impl Shell {
    /// Windows `show_playback_queue`: says so when the queue is empty,
    /// otherwise opens the window after the current event.
    pub(crate) fn show_playback_queue(&mut self, ui: &mut dyn ShellUi) {
        self.remember_menu_item("playback_queue");
        self.stop_key_repeat(ui);
        if self.application.playback_queue().is_empty() {
            ui.set_status(&self.text("playback_queue_empty"), true);
            return;
        }
        let catalog = self.catalog();
        let text = |key: &str| catalog.text(key).to_owned();
        ui.open_playback_queue(QueueDialog {
            title: text("playback_queue"),
            instructions: text("playback_queue_instructions"),
            play: text("play"),
            move_up: text("move_up"),
            move_down: text("move_down"),
            remove: text("remove_from_playback_queue"),
            clear: text("clear_playback_queue"),
            back: text("back"),
            items: self.application.playback_queue().items().to_vec(),
            changed: false,
            chosen: None,
            labels: QueueRowLabels::new(&catalog),
        });
    }

    /// The end of Windows `show_playback_queue`: the edited queue is saved
    /// and a chosen item plays; focus returns to the screen otherwise.
    pub fn playback_queue_closed(&mut self, ui: &mut dyn ShellUi, dialog: QueueDialog) {
        if dialog.changed
            && let Err(error) = self.application.replace_playback_queue(dialog.items)
        {
            let message = format!("Playback queue was not updated: {error}");
            ui.set_status(&message, true);
            ui.show_error(&message);
            ui.focus_primary_control();
            return;
        }
        if self.view == View::MainMenu {
            let current = ui.selected_row();
            self.refresh_main_menu(ui, MainMenuSelection::Preserve, current, false);
        }
        if let Some(item) = dialog.chosen {
            self.start_media_item(ui, item, Some(QueueStartMode::Matching));
            return;
        }
        ui.focus_primary_control();
    }

    /// Windows `add_active_item_to_playback_queue`.
    pub(crate) fn add_active_item_to_playback_queue(&mut self, ui: &mut dyn ShellUi) {
        let Some(item) = self.active_media_item(ui) else {
            return;
        };
        let title = item.title.clone();
        match self.application.add_to_playback_queue(item) {
            Ok(apricot_app::QueueAddOutcome::Added) => {
                let message = self.text("playback_queue_added").replace("{title}", &title);
                ui.set_status(&message, true);
            }
            Ok(apricot_app::QueueAddOutcome::AlreadyPresent) => {
                let message = self
                    .text("playback_queue_already_added")
                    .replace("{title}", &title);
                ui.set_status(&message, true);
            }
            Ok(apricot_app::QueueAddOutcome::Unplayable) => {
                ui.set_status(
                    &format!("{title} cannot be added to the playback queue"),
                    true,
                );
            }
            Err(error) => {
                let message = format!("Playback queue was not updated: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }

    /// Windows `remove_active_item_from_playback_queue`.
    pub(crate) fn remove_active_item_from_playback_queue(&mut self, ui: &mut dyn ShellUi) {
        let Some(item) = self.active_media_item(ui) else {
            return;
        };
        let title = item.title.clone();
        match self.application.remove_from_playback_queue(&item) {
            Ok(true) => {
                let message = self
                    .text("playback_queue_removed")
                    .replace("{title}", &title);
                ui.set_status(&message, true);
            }
            Ok(false) => ui.set_status(&self.text("playback_queue_not_found"), true),
            Err(error) => {
                let message = format!("Playback queue was not updated: {error}");
                ui.set_status(&message, true);
                ui.show_error(&message);
            }
        }
    }

    /// Windows `model_context_entries` for the screens the shell shows.
    fn context_entries(&self, item: Option<&MediaItem>) -> Option<Vec<ContextMenuEntry>> {
        let catalog = self.catalog();
        let titles = self
            .application
            .user_playlists()
            .iter()
            .map(|playlist| playlist.title.clone())
            .collect::<Vec<_>>();
        let context = ContextMenuContext {
            catalog: &catalog,
            settings: self.application.settings(),
            user_playlist_titles: &titles,
            queued_download_count: self.application.downloads().queued().len(),
        };
        match self.view {
            View::LocalFolder
            | View::Search
            | View::Results
            | View::Collection
            | View::Trending => Some(context_menu::results_context_menu(&context, item)),
            View::Player => Some(context_menu::player_context_menu(&context, item?)),
            View::Favorites => Some(context_menu::favorites_context_menu(&context, item)),
            View::History => Some(context_menu::history_context_menu(&context, item)),
            View::UserPlaylists => Some(context_menu::user_playlists_context_menu(&context)),
            View::UserPlaylistItems => Some(context_menu::user_playlist_items_context_menu(
                &context, item,
            )),
            View::RssFeeds | View::RssItems | View::PodcastSearchResults => {
                self.podcast_context_entries()
            }
            View::MainMenu | View::DirectLink | View::PodcastCategories => None,
        }
    }

    /// Windows `show_context_menu_for_active_view`: the menu of the list or
    /// the player for the active item; the chosen entry runs.
    pub(crate) fn show_context_menu_for_active_view(&mut self, ui: &mut dyn ShellUi) {
        let item = self.active_media_item(ui);
        let Some(entries) = self.context_entries(item.as_ref()) else {
            return;
        };
        self.stop_key_repeat(ui);
        if let Some(command) = ui.show_context_menu(&entries) {
            let label = find_label(&entries, command).unwrap_or_default();
            self.execute_context_command(ui, command, item, &label);
        }
    }

    /// Windows `execute_context_command` for what the macOS screens offer
    /// so far; the other commands say that they are not available yet.
    fn execute_context_command(
        &mut self,
        ui: &mut dyn ShellUi,
        command: ContextCommand,
        item: Option<MediaItem>,
        label: &str,
    ) {
        use ContextCommand as C;
        match command {
            C::Play | C::OpenUserPlaylist => self.activate_selection(ui),
            C::AddToPlaybackQueue => self.add_active_item_to_playback_queue(ui),
            C::RemoveFromPlaybackQueue => self.remove_active_item_from_playback_queue(ui),
            C::CopyStreamUrl => self.copy_active_stream_url(ui),
            C::CopyLocation => match self.podcast_copy_location(ui) {
                Some(url) => self.copy_text_and_announce(ui, &url, "url_copied"),
                None => self.copy_active_location(ui),
            },
            C::CopyTimestampLink => self.copy_current_timestamp_link(ui),
            C::PlayerAction(action_id) => {
                if !self.podcast_command(ui, action_id) {
                    self.activate_action(ui, action_id);
                }
            }
            C::ClosePlayer => self.close_current_player(ui),
            C::OpenChannel => self.open_item_channel(ui),
            C::AddFavorite => self.add_active_favorite(ui),
            C::RemoveFavorite => self.remove_active_favorite(ui),
            C::RemoveSelected => self.remove_selected_collection_item(ui),
            C::ClearHistory => self.clear_history(ui),
            C::CreatePlaylist => self.create_user_playlist(ui, None),
            C::AddToPlaylist => self.add_active_item_to_user_playlist(ui),
            C::AddToUserPlaylist(index) => self.add_active_item_to_chosen_user_playlist(ui, index),
            C::RemoveFromPlaylist => self.remove_active_item_from_user_playlist(ui),
            C::RemoveUserPlaylist => self.remove_selected_user_playlist(ui),
            C::ChannelOptions => {
                if let Some(item) = item {
                    self.show_channel_options(ui, &item);
                }
            }
            C::ChannelVideos | C::ChannelPopular | C::ChannelPlaylists | C::ChannelStreams => {
                if let Some(item) = item {
                    let kind = match command {
                        C::ChannelVideos => apricot_app::YoutubeCollectionKind::ChannelVideos,
                        C::ChannelPopular => apricot_app::YoutubeCollectionKind::ChannelPopular,
                        C::ChannelPlaylists => apricot_app::YoutubeCollectionKind::ChannelPlaylists,
                        _ => apricot_app::YoutubeCollectionKind::ChannelStreams,
                    };
                    self.open_youtube_collection(ui, &item, kind);
                }
            }
            C::PlayPlaylist | C::ShufflePlaylist => {
                if let Some(item) = item.as_ref() {
                    self.play_youtube_playlist(ui, item, command == C::ShufflePlaylist);
                }
            }
            C::OpenPlaylistVideos => {
                if let Some(item) = item {
                    self.open_youtube_collection(
                        ui,
                        &item,
                        apricot_app::YoutubeCollectionKind::PlaylistVideos,
                    );
                }
            }
            C::OpenInBrowser => {
                let Some(item) = item else {
                    return;
                };
                let Some(url) = context_menu::browser_url(&item) else {
                    // Python `validate_remote_http_url`.
                    ui.set_status("URL must use HTTP or HTTPS", true);
                    return;
                };
                if let Err(error) = apricot_platform::shell::open_url(&url) {
                    ui.set_status(&error.to_string(), true);
                }
            }
            _ => {
                let feature = label.split('\t').next().unwrap_or(label);
                let message = apricot_app::unavailable_feature_message(&self.catalog(), feature);
                ui.set_status(&message, true);
            }
        }
    }
}

/// The label of the entry that runs `command`.
fn find_label(entries: &[ContextMenuEntry], command: ContextCommand) -> Option<String> {
    entries.iter().find_map(|entry| match entry {
        ContextMenuEntry::Command {
            label,
            command: entry_command,
        } if *entry_command == command => Some(label.clone()),
        ContextMenuEntry::Submenu { entries, .. } => find_label(entries, command),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str) -> MediaItem {
        crate::local_file_item(std::path::Path::new(&format!("/tmp/{title}.mp3")))
    }

    fn dialog(titles: &[&str]) -> QueueDialog {
        let catalog = apricot_app::embedded_catalog("en");
        QueueDialog {
            title: String::new(),
            instructions: String::new(),
            play: String::new(),
            move_up: String::new(),
            move_down: String::new(),
            remove: String::new(),
            clear: String::new(),
            back: String::new(),
            items: titles.iter().map(|title| item(title)).collect(),
            changed: false,
            chosen: None,
            labels: QueueRowLabels::new(&catalog),
        }
    }

    #[test]
    fn rows_are_numbered_with_the_kind() {
        let dialog = dialog(&["one", "two"]);
        let rows = dialog.rows();
        assert!(rows[0].starts_with("1. one | "), "{rows:?}");
        assert!(rows[1].starts_with("2. two | "), "{rows:?}");
    }

    #[test]
    fn moving_reorders_and_stops_at_the_ends() {
        let mut dialog = dialog(&["one", "two", "three"]);
        assert_eq!(dialog.move_selected(Some(0), false), QueueChange::None);
        let QueueChange::Refresh { rows, selected, .. } = dialog.move_selected(Some(0), true)
        else {
            panic!("moved");
        };
        assert_eq!(selected, 1);
        assert!(rows[1].starts_with("2. one"));
        assert_eq!(dialog.move_selected(Some(2), true), QueueChange::None);
        assert!(dialog.changed);
    }

    #[test]
    fn removing_the_last_item_and_clearing_close_the_window() {
        let mut dialog = dialog(&["one", "two"]);
        let QueueChange::Refresh {
            selected,
            announcement,
            ..
        } = dialog.remove(Some(1))
        else {
            panic!("removed");
        };
        assert_eq!(selected, 0);
        assert!(announcement.contains("two"), "{announcement}");
        assert!(matches!(dialog.remove(Some(0)), QueueChange::Close { .. }));
        let mut dialog = super::tests::dialog(&["one"]);
        assert!(matches!(
            dialog.clear(),
            QueueChange::Close {
                announcement: Some(_)
            }
        ));
        assert!(dialog.items.is_empty());
    }

    #[test]
    fn play_chooses_the_selected_item_and_closes() {
        let mut dialog = dialog(&["one", "two"]);
        assert_eq!(dialog.play(None), QueueChange::None);
        assert_eq!(
            dialog.play(Some(1)),
            QueueChange::Close { announcement: None }
        );
        assert_eq!(
            dialog.chosen.as_ref().map(|item| item.title.as_str()),
            Some("two")
        );
    }
}
