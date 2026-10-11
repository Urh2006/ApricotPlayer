//! What a native UI offers the shell.

use std::path::PathBuf;

use apricot_app::PlayerScreenModel;
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

/// A modal list choice (Windows `playlist_dialog_win32::choose`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChoiceDialog {
    pub title: String,
    pub prompt: String,
    /// The list's accessible name.
    pub list_name: String,
    pub choices: Vec<String>,
    pub initial: usize,
    pub ok: String,
    pub cancel: String,
}

/// The native side of the main window. Every method runs on the UI thread.
pub trait ShellUi {
    /// Windows `set_status`: shows `message` on the status line and, when
    /// `announce` is set, speaks it once without moving focus.
    fn set_status(&mut self, message: &str, announce: bool);

    /// Windows `show_error_message`: a modal message; focus returns to the
    /// control that had it.
    fn show_error(&mut self, message: &str);

    /// A list screen (Windows `refresh_main_menu`, `refresh_local_folder`
    /// and `layout_controls_state`): the list with its name and rows, the
    /// row `selected`, the screen's buttons in Tab order; the list gets focus
    /// when `focus_list` is set.
    fn show_list_screen(&mut self, screen: &crate::ListScreen);

    /// Windows `PlayerControls::sync` and `layout_controls_state` for the
    /// player page.
    fn show_player(&mut self, model: &PlayerScreenModel, focus: PlayerFocus);

    /// Windows `PlayerControls::show_details`: the details label, read-only
    /// text (caret at the start, focused) and the Copy and Back buttons below
    /// the player controls, in Tab order after them.
    fn show_player_details(&mut self, labels: &crate::DetailsLabels, text: &str);

    /// Hides the details; focus goes to the player surface.
    fn hide_player_details(&mut self);

    /// Replaces the details text when it changed, keeping the reading
    /// position otherwise (Windows `set_details_text`).
    fn set_player_details_text(&mut self, text: &str);

    /// Focus on the player control `id`.
    fn focus_player_control(&mut self, id: &str);

    /// The window fills the screen (Windows `apply_window_fullscreen`).
    fn set_window_fullscreen(&mut self, fullscreen: bool);

    /// A modal list choice; `None` when cancelled.
    fn choose(&mut self, dialog: &ChoiceDialog) -> Option<usize>;

    /// The text of a screen's text field.
    fn screen_text(&self, id: &str) -> String;

    /// The selected row of a screen's drop-down list.
    fn screen_choice(&self, id: &str) -> Option<usize>;

    /// Replaces the rows of a screen's drop-down list.
    fn set_screen_choices(&mut self, id: &str, options: &[String], selected: usize);

    /// Moves focus to a field or choice of the screen.
    fn focus_screen_control(&mut self, id: &str);

    /// Replaces the rows of the screen's list without moving focus
    /// (Windows `LB_ADDSTRING` and `LB_SETCURSEL`).
    fn set_list_rows(&mut self, rows: Vec<String>, selected: usize);

    /// Opens the equalizer dialog after the current event, modal over the
    /// window. The dialog calls [`crate::Shell::equalizer_begin`], forwards
    /// its control changes and ends with [`crate::Shell::equalizer_finish`].
    fn open_equalizer(&mut self);

    /// Opens the bookmarks window after the current event, modal over the
    /// window, with focus on its list. Its requests go to
    /// [`crate::Shell::bookmark_request`]; when it closed and focus is back
    /// where it was, it calls [`crate::Shell::bookmarks_closed`].
    fn open_bookmarks(&mut self, dialog: crate::BookmarkDialog);

    /// Opens the lyrics window after the current event, modal over the
    /// window: the loading text focused, Copy and Back. The window polls
    /// [`crate::LyricsDialog`] and calls [`crate::Shell::lyrics_closed`]
    /// after it closed.
    fn open_lyrics(&mut self, dialog: crate::LyricsDialog);

    /// Opens the playback queue window after the current event, modal over
    /// the window: instructions, the list (focused, first row selected) and
    /// its buttons. Its keys and buttons call the [`crate::QueueDialog`]
    /// methods; after it closed it calls
    /// [`crate::Shell::playback_queue_closed`].
    fn open_playback_queue(&mut self, dialog: crate::QueueDialog);

    /// Shows a context menu at the focused control and returns the chosen
    /// command; focus returns to the control when it closes.
    fn show_context_menu(
        &mut self,
        entries: &[apricot_app::context_menu::ContextMenuEntry],
    ) -> Option<apricot_app::context_menu::ContextCommand>;

    /// Windows `prompt_name_with_initial`: a modal text prompt with
    /// `initial` in the field; `None` when cancelled. The text is returned
    /// as typed.
    fn prompt_text(
        &mut self,
        title: &str,
        prompt: &str,
        initial: &str,
        ok: &str,
        cancel: &str,
    ) -> Option<String>;

    /// Copies text to the clipboard; `true` on success.
    fn copy_text(&mut self, text: &str) -> bool;

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

    /// Windows `choose_media_folder`: a folder dialog, `None` when cancelled.
    fn choose_folder(&mut self, title: &str) -> Option<PathBuf>;

    /// Starts or stops polling [`crate::Shell::poll_background`] (Windows
    /// `LOCAL_FOLDER_TIMER_ID` and the other work timers).
    fn set_background_polling(&mut self, active: bool);

    /// Windows `restore_from_tray`: shows and activates the window.
    fn restore(&mut self);

    /// The Exit row: the application ends.
    fn exit(&mut self);
}
