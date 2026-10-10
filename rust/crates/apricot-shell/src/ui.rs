//! What a native UI offers the shell.

use std::path::PathBuf;

use apricot_app::{MainMenuModel, PlayerScreenModel};
use apricot_playback::MpvVideoMode;

/// Where focus goes when the player page is shown (Windows `refresh_player`
/// with `focus` and `preserve_selection`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerFocus {
    /// Background refresh: focus stays where it is.
    Keep,
    /// The page opens: focus goes to the model's initial control.
    Initial,
    /// A refresh after an action: the focused control keeps focus by id.
    Preserve,
}

/// The native side of the main window. Every method runs on the UI thread.
pub trait ShellUi {
    /// Windows `set_status`: shows `message` on the status line and, when
    /// `announce` is set, speaks it once without moving focus.
    fn set_status(&mut self, message: &str, announce: bool);

    /// Windows `show_error_message`: a modal message; focus returns to the
    /// control that had it.
    fn show_error(&mut self, message: &str);

    /// Windows `refresh_main_menu` and `layout_controls_state` for the main
    /// menu: the list gets the model's accessible name and rows, the row
    /// `selected` is selected, and the list gets focus when `focus` is set.
    /// `open_label` is the Open button's text (Windows `set_open_button_label`).
    fn show_main_menu(
        &mut self,
        model: &MainMenuModel,
        open_label: &str,
        selected: usize,
        focus: bool,
    );

    /// Windows `PlayerControls::sync` and `layout_controls_state` for the
    /// player page.
    fn show_player(&mut self, model: &PlayerScreenModel, focus: PlayerFocus);

    /// The window title (Windows `SetWindowTextW` on the main window).
    fn set_window_title(&mut self, title: &str);

    /// The row selected in the screen's list, if any.
    fn selected_row(&self) -> Option<usize>;

    /// The id of the focused player control (`video_host`, `play_pause`,
    /// `back_main`, ...), or `None` when focus is elsewhere.
    fn focused_player_control(&self) -> Option<&'static str>;

    /// Focus on the screen's main control (Windows `active_primary_control`).
    fn focus_primary_control(&mut self);

    /// Where libmpv shows video: a native child window or a render surface.
    fn video_mode(&self) -> MpvVideoMode;

    /// Starts or stops polling [`crate::Shell::poll_playback`] (Windows
    /// `PLAYBACK_TIMER_ID`, every 25 ms).
    fn set_playback_polling(&mut self, active: bool);

    /// Calls [`crate::Shell::key_repeat_tick`] once after `delay_ms`.
    fn schedule_key_repeat(&mut self, delay_ms: u32);

    /// Cancels a scheduled key repeat.
    fn cancel_key_repeat(&mut self);

    /// Python `play_default_sound`: the cue when speed or pitch reach 1.0.
    fn play_default_reached_sound(&mut self);

    /// Windows `choose_media_file`: an open dialog for one audio or video
    /// file, `Ok(None)` when cancelled.
    ///
    /// # Errors
    ///
    /// Returns a message when the dialog cannot be shown.
    fn choose_media_file(&mut self, title: &str) -> Result<Option<PathBuf>, String>;

    /// Windows `restore_from_tray`: shows and activates the window.
    fn restore(&mut self);

    /// The Exit row: the application ends.
    fn exit(&mut self);
}
