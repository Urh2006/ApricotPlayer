//! The Spotify queue and device dialogs (Windows `spotify_queue_win32.rs`,
//! `spotify_devices_win32.rs` and their callers in `spotify_win32.rs`).
//! Both are a list over the current screen, so playback goes on; the shell
//! keeps their rows and edits, the platform shows them.

use apricot_app::context_menu::{ContextCommand, ContextMenuEntry};
use apricot_app::spotify::{SpotifyQueueRow, named, queue_menu, queue_rows};
use apricot_spotify::QueueSection;
use apricot_spotify::queue::QueueEdit;

use crate::{Shell, ShellUi};

/// A row of a Spotify dialog: the occurrence or device it stands for, so
/// an update keeps it selected, and its text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotifyDialogRow {
    pub key: Option<String>,
    pub label: String,
}

/// The buttons and menu entries of the Spotify dialogs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotifyDialogAction {
    Play,
    MoveUp,
    MoveDown,
    Remove,
    Clear,
    /// The device dialog's "Play here".
    Transfer,
}

impl SpotifyDialogAction {
    const ALL: [Self; 6] = [
        Self::Play,
        Self::MoveUp,
        Self::MoveDown,
        Self::Remove,
        Self::Clear,
        Self::Transfer,
    ];

    const fn id(self) -> &'static str {
        match self {
            Self::Play => "spotify_dialog_play",
            Self::MoveUp => "spotify_dialog_move_up",
            Self::MoveDown => "spotify_dialog_move_down",
            Self::Remove => "spotify_dialog_remove",
            Self::Clear => "spotify_dialog_clear",
            Self::Transfer => "spotify_dialog_transfer",
        }
    }

    /// The action of a context menu entry from [`Shell::spotify_dialog_menu`].
    pub fn from_command(command: ContextCommand) -> Option<Self> {
        let ContextCommand::PlayerAction(id) = command else {
            return None;
        };
        Self::ALL.into_iter().find(|action| action.id() == id)
    }
}

/// Which Spotify dialog is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotifyDialogKind {
    Queue,
    Devices,
}

/// A Spotify dialog to show: instructions, the list (named by the title,
/// focused) and the buttons in their Tab order, Back last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotifyListDialog {
    pub kind: SpotifyDialogKind,
    pub title: String,
    pub instructions: String,
    pub buttons: Vec<(SpotifyDialogAction, String)>,
    pub back: String,
    pub rows: Vec<SpotifyDialogRow>,
    pub selected: usize,
}

/// The rows the open dialog shows.
#[derive(Default)]
pub(crate) struct DialogRows {
    queue: Vec<SpotifyQueueRow>,
    devices: Vec<(String, String)>,
}

fn queue_dialog_rows(rows: &[SpotifyQueueRow]) -> Vec<SpotifyDialogRow> {
    rows.iter()
        .map(|row| SpotifyDialogRow {
            key: row.uid().map(str::to_owned),
            label: row.label().to_owned(),
        })
        .collect()
}

fn device_dialog_rows(rows: &[(String, String)]) -> Vec<SpotifyDialogRow> {
    rows.iter()
        .map(|(id, label)| SpotifyDialogRow {
            key: (!id.is_empty()).then(|| id.clone()),
            label: label.clone(),
        })
        .collect()
}

/// The first upcoming track; the playing one when nothing comes next.
fn initial_selection(rows: &[SpotifyQueueRow]) -> usize {
    rows.iter().position(|row| row.uid().is_some()).unwrap_or(0)
}

impl Shell {
    /// `spotify_queue` and the hub entry: the Spotify queue in a dialog
    /// over the current screen (Windows `show_queue`).
    pub(crate) fn show_spotify_queue(&mut self, ui: &mut dyn ShellUi) {
        self.stop_key_repeat(ui);
        if self.spotify.queue_open || self.spotify.devices_open {
            return;
        }
        let accounts = self
            .spotify_service()
            .map(|service| service.accounts())
            .unwrap_or_default();
        let Some(service) = self.spotify_service() else {
            return;
        };
        let texts = self.catalog();
        if apricot_app::spotify::active_account(&accounts).is_none() {
            ui.set_status(texts.text("spotify_log_in_first"), true);
            return;
        }
        if service.playback().is_none() {
            ui.set_status(texts.text("spotify_queue_inactive"), true);
            return;
        }
        let rows = queue_rows(&texts, &service.queue_now());
        let selected = initial_selection(&rows);
        let dialog = SpotifyListDialog {
            kind: SpotifyDialogKind::Queue,
            title: texts.text("spotify_queue").to_owned(),
            instructions: texts.text("playback_queue_instructions").to_owned(),
            buttons: vec![
                (SpotifyDialogAction::Play, texts.text("play").to_owned()),
                (
                    SpotifyDialogAction::MoveUp,
                    texts.text("move_up").to_owned(),
                ),
                (
                    SpotifyDialogAction::MoveDown,
                    texts.text("move_down").to_owned(),
                ),
                (
                    SpotifyDialogAction::Remove,
                    texts.text("spotify_queue_remove").to_owned(),
                ),
                (
                    SpotifyDialogAction::Clear,
                    texts.text("spotify_queue_clear").to_owned(),
                ),
            ],
            back: texts.text("back").to_owned(),
            rows: queue_dialog_rows(&rows),
            selected,
        };
        self.spotify.dialog_rows.queue = rows;
        self.spotify.queue_open = true;
        ui.open_spotify_dialog(dialog);
        // Titles not known yet arrive with the confirmed queue.
        self.spotify_reload_queue();
    }

    /// `spotify_devices` and the hub entry: the Connect devices in a
    /// dialog over the current screen (Windows `show_devices`).
    pub(crate) fn show_spotify_devices(&mut self, ui: &mut dyn ShellUi) {
        self.stop_key_repeat(ui);
        if self.spotify.queue_open || self.spotify.devices_open {
            return;
        }
        let accounts = self
            .spotify_service()
            .map(|service| service.accounts())
            .unwrap_or_default();
        let Some(service) = self.spotify_service() else {
            return;
        };
        let texts = self.catalog();
        if apricot_app::spotify::active_account(&accounts).is_none() {
            ui.set_status(texts.text("spotify_log_in_first"), true);
            return;
        }
        if service.devices_now().is_none() {
            ui.set_status(texts.text("spotify_not_connected"), true);
            return;
        }
        let rows = self.spotify_device_rows();
        let dialog = SpotifyListDialog {
            kind: SpotifyDialogKind::Devices,
            title: texts.text("spotify_devices").to_owned(),
            instructions: texts.text("spotify_devices_instructions").to_owned(),
            buttons: vec![(
                SpotifyDialogAction::Transfer,
                texts.text("spotify_devices_play").to_owned(),
            )],
            back: texts.text("back").to_owned(),
            rows: device_dialog_rows(&rows),
            selected: 0,
        };
        self.spotify.dialog_rows.devices = rows;
        self.spotify.devices_open = true;
        ui.open_spotify_dialog(dialog);
    }

    /// Windows `device_rows`: (device ID, label), or one row saying there
    /// are none.
    pub(crate) fn spotify_device_rows(&self) -> Vec<(String, String)> {
        let texts = self.catalog();
        let devices = self
            .spotify_service_ref()
            .and_then(apricot_spotify::SpotifyService::devices_now)
            .map(|(devices, _)| devices)
            .unwrap_or_default();
        if devices.is_empty() {
            return vec![(String::new(), texts.text("spotify_devices_none").to_owned())];
        }
        let labels = apricot_app::spotify::device_rows(&texts, &devices);
        devices
            .into_iter()
            .map(|device| device.id)
            .zip(labels)
            .collect()
    }

    /// The confirmed queue arrived. An unchanged list is left alone, so the
    /// screen reader does not repeat the row; otherwise the same occurrence
    /// stays selected.
    pub(crate) fn spotify_queue_arrived(
        &mut self,
        ui: &mut dyn ShellUi,
        rows: Vec<SpotifyQueueRow>,
    ) {
        if !self.spotify.queue_open || self.spotify.dialog_rows.queue == rows {
            return;
        }
        ui.spotify_dialog_update(queue_dialog_rows(&rows), None);
        self.spotify.dialog_rows.queue = rows;
    }

    /// The Connect devices changed while the device dialog is open.
    pub(crate) fn spotify_devices_changed(&mut self, ui: &mut dyn ShellUi) {
        if !self.spotify.devices_open {
            return;
        }
        let rows = self.spotify_device_rows();
        if self.spotify.dialog_rows.devices == rows {
            return;
        }
        ui.spotify_dialog_update(device_dialog_rows(&rows), None);
        self.spotify.dialog_rows.devices = rows;
    }

    /// The context menu of the selected row: only what it supports
    /// (Windows `show_context_menu` of both dialogs).
    pub fn spotify_dialog_menu(&self, selected: Option<usize>) -> Vec<ContextMenuEntry> {
        let texts = self.catalog();
        let entry = |action: SpotifyDialogAction, key: &str| ContextMenuEntry::Command {
            label: texts.text(key).to_owned(),
            command: ContextCommand::PlayerAction(action.id()),
        };
        if self.spotify.devices_open {
            let device = selected
                .and_then(|index| self.spotify.dialog_rows.devices.get(index))
                .is_some_and(|(id, _)| !id.is_empty());
            return if device {
                vec![entry(SpotifyDialogAction::Transfer, "spotify_devices_play")]
            } else {
                Vec::new()
            };
        }
        let menu = queue_menu(
            &self.spotify.dialog_rows.queue,
            selected.unwrap_or(usize::MAX),
        );
        [
            (SpotifyDialogAction::Play, "play", menu.play),
            (SpotifyDialogAction::MoveUp, "move_up", menu.move_up),
            (SpotifyDialogAction::MoveDown, "move_down", menu.move_down),
            (
                SpotifyDialogAction::Remove,
                "spotify_queue_remove",
                menu.remove,
            ),
            (
                SpotifyDialogAction::Clear,
                "spotify_queue_clear",
                menu.clear,
            ),
        ]
        .into_iter()
        .filter(|(_, _, shown)| *shown)
        .map(|(action, key, _)| entry(action, key))
        .collect()
    }

    /// A button, Enter, Delete or a menu entry of the open dialog on the
    /// selected row. `true` closes the dialog.
    pub fn spotify_dialog_action(
        &mut self,
        ui: &mut dyn ShellUi,
        action: SpotifyDialogAction,
        selected: Option<usize>,
    ) -> bool {
        match action {
            SpotifyDialogAction::Transfer => self.spotify_transfer_selected(ui, selected),
            SpotifyDialogAction::Play => self.spotify_queue_play(ui, selected),
            SpotifyDialogAction::MoveUp => {
                self.spotify_queue_move(ui, selected, true);
                false
            }
            SpotifyDialogAction::MoveDown => {
                self.spotify_queue_move(ui, selected, false);
                false
            }
            SpotifyDialogAction::Remove => {
                self.spotify_queue_remove(ui, selected);
                false
            }
            SpotifyDialogAction::Clear => {
                self.spotify_queue_clear(ui);
                false
            }
        }
    }

    /// The dialog closed; focus returns to the screen's control.
    pub fn spotify_dialog_closed(&mut self, ui: &mut dyn ShellUi) {
        self.spotify.queue_open = false;
        self.spotify.queue_load = None;
        self.spotify.devices_open = false;
        self.spotify.dialog_rows = DialogRows::default();
        ui.focus_primary_control();
    }

    /// The selected upcoming track: its position, UID and row text.
    fn spotify_queue_entry(&self, selected: Option<usize>) -> Option<(usize, String, String)> {
        let index = selected?;
        let row = self.spotify.dialog_rows.queue.get(index)?;
        Some((index, row.uid()?.to_owned(), row.label().to_owned()))
    }

    /// The edit did not apply: the queue changed meanwhile. The list
    /// reloads.
    fn spotify_queue_changed(&mut self, ui: &mut dyn ShellUi) {
        ui.set_status(&self.text("spotify_queue_changed"), true);
        self.spotify_reload_queue();
    }

    fn spotify_queue_edit(&self, edit: &QueueEdit) -> bool {
        self.spotify_service_ref()
            .and_then(apricot_spotify::SpotifyService::playback)
            .is_some_and(|playback| playback.edit_queue(edit))
    }

    fn spotify_queue_play(&mut self, ui: &mut dyn ShellUi, selected: Option<usize>) -> bool {
        let Some((_, uid, _)) = self.spotify_queue_entry(selected) else {
            return false;
        };
        let played = self
            .spotify_service_ref()
            .and_then(apricot_spotify::SpotifyService::playback)
            .is_some_and(|playback| playback.play_queue_entry(&uid));
        if !played {
            self.spotify_queue_changed(ui);
        }
        // The player announces the new track once it plays.
        played
    }

    fn spotify_queue_move(&mut self, ui: &mut dyn ShellUi, selected: Option<usize>, up: bool) {
        let Some((index, uid, _)) = self.spotify_queue_entry(selected) else {
            return;
        };
        let menu = queue_menu(&self.spotify.dialog_rows.queue, index);
        if !(if up { menu.move_up } else { menu.move_down }) {
            return;
        }
        if !self.spotify_queue_edit(&QueueEdit::Move { uid, up }) {
            self.spotify_queue_changed(ui);
            return;
        }
        // Shown at once; the confirmed state replaces it with the same
        // order.
        let target = if up { index - 1 } else { index + 1 };
        self.spotify.dialog_rows.queue.swap(index, target);
        ui.spotify_dialog_update(
            queue_dialog_rows(&self.spotify.dialog_rows.queue),
            Some(target),
        );
        ui.set_status(&self.text("spotify_queue_moved"), true);
    }

    fn spotify_queue_remove(&mut self, ui: &mut dyn ShellUi, selected: Option<usize>) {
        let Some((index, uid, label)) = self.spotify_queue_entry(selected) else {
            return;
        };
        if !self.spotify_queue_edit(&QueueEdit::Remove(uid)) {
            self.spotify_queue_changed(ui);
            return;
        }
        let rows = &mut self.spotify.dialog_rows.queue;
        rows.remove(index);
        let selection = index.min(rows.len().saturating_sub(1));
        ui.spotify_dialog_update(queue_dialog_rows(rows), Some(selection));
        // The row without its section: "title, artists".
        let title = label
            .rsplit_once(", ")
            .map_or(label.as_str(), |(title, _)| title);
        let text = self.text("spotify_queue_removed").replace("{title}", title);
        ui.set_status(&text, true);
    }

    fn spotify_queue_clear(&mut self, ui: &mut dyn ShellUi) {
        if !queue_menu(&self.spotify.dialog_rows.queue, 0).clear {
            return;
        }
        if !self.spotify_queue_edit(&QueueEdit::ClearManual) {
            self.spotify_queue_changed(ui);
            return;
        }
        let rows = &mut self.spotify.dialog_rows.queue;
        rows.retain(|row| {
            !matches!(
                row,
                SpotifyQueueRow::Entry {
                    section: QueueSection::Manual,
                    ..
                }
            )
        });
        let selection = initial_selection(rows);
        ui.spotify_dialog_update(queue_dialog_rows(rows), Some(selection));
        ui.set_status(&self.text("spotify_queue_cleared"), true);
    }

    /// Windows `transfer_to`: playback moves to the selected device; the
    /// dialog stays open (nothing plays, or the device already plays) after
    /// saying why.
    fn spotify_transfer_selected(&mut self, ui: &mut dyn ShellUi, selected: Option<usize>) -> bool {
        let Some(id) = selected
            .and_then(|index| self.spotify.dialog_rows.devices.get(index))
            .map(|(id, _)| id.clone())
            .filter(|id| !id.is_empty())
        else {
            return false;
        };
        let Some(service) = self.spotify_service() else {
            return false;
        };
        let texts = self.catalog();
        let Some((devices, playing)) = service.devices_now() else {
            return false;
        };
        let Some(device) = devices.into_iter().find(|device| device.id == id) else {
            return false;
        };
        if !playing {
            ui.set_status(texts.text("spotify_nothing_playing"), true);
            return false;
        }
        if device.active {
            ui.set_status(&named(&texts, "spotify_device_already", &device.name), true);
            return false;
        }
        let stamp = self.spotify.epochs.begin();
        self.spotify.transfer = Some((stamp, device.name));
        service.transfer_to(stamp, &id);
        true
    }
}

#[cfg(debug_assertions)]
impl Shell {
    /// Development self-test: the queue dialog with made-up rows (the
    /// playing track, two added tracks, one of the playlist).
    pub fn spotify_queue_dialog_self_test(&mut self, ui: &mut dyn ShellUi) {
        let entry = |uid: &str, section, label: &str| SpotifyQueueRow::Entry {
            uid: uid.to_owned(),
            section,
            label: label.to_owned(),
        };
        let rows = vec![
            SpotifyQueueRow::Current {
                label: "Now playing: One, Artist".to_owned(),
            },
            entry("a", QueueSection::Manual, "Two, Artist, Added manually"),
            entry("b", QueueSection::Manual, "Three, Artist, Added manually"),
            entry(
                "c",
                QueueSection::Context,
                "Four, Artist, Next from playlist",
            ),
        ];
        let texts = self.catalog();
        let dialog = SpotifyListDialog {
            kind: SpotifyDialogKind::Queue,
            title: texts.text("spotify_queue").to_owned(),
            instructions: texts.text("playback_queue_instructions").to_owned(),
            buttons: [
                (SpotifyDialogAction::Play, "play"),
                (SpotifyDialogAction::MoveUp, "move_up"),
                (SpotifyDialogAction::MoveDown, "move_down"),
                (SpotifyDialogAction::Remove, "spotify_queue_remove"),
                (SpotifyDialogAction::Clear, "spotify_queue_clear"),
            ]
            .into_iter()
            .map(|(action, key)| (action, texts.text(key).to_owned()))
            .collect(),
            back: texts.text("back").to_owned(),
            rows: queue_dialog_rows(&rows),
            selected: initial_selection(&rows),
        };
        self.spotify.dialog_rows.queue = rows;
        self.spotify.queue_open = true;
        ui.open_spotify_dialog(dialog);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_commands_map_back_to_their_actions() {
        for action in SpotifyDialogAction::ALL {
            assert_eq!(
                SpotifyDialogAction::from_command(ContextCommand::PlayerAction(action.id())),
                Some(action)
            );
        }
        assert_eq!(
            SpotifyDialogAction::from_command(ContextCommand::PlayerAction("player_fullscreen")),
            None
        );
    }

    #[test]
    fn device_rows_without_a_device_have_no_key() {
        let rows = device_dialog_rows(&[
            (String::new(), "No devices".to_owned()),
            ("abc".to_owned(), "Kitchen".to_owned()),
        ]);
        assert_eq!(rows[0].key, None);
        assert_eq!(rows[1].key.as_deref(), Some("abc"));
    }
}
