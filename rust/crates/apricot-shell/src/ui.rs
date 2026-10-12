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

    /// Opens the transcript window after the current event, modal over the
    /// window: the search field (focused), the cue list and its buttons. It
    /// polls [`crate::TranscriptDialog`], sends its buttons to
    /// [`crate::Shell::transcript_button`] and ends with
    /// [`crate::Shell::transcript_closed`].
    fn open_transcript(&mut self, dialog: crate::TranscriptDialog);

    /// Opens the comments window after the current event, modal over the
    /// window: the search field (focused), the sort choice, the list and its
    /// buttons; it starts the first page with
    /// [`crate::CommentsDialog::load_more`], sends its commands to
    /// [`crate::Shell::comments_command`] and ends with
    /// [`crate::Shell::comments_closed`].
    fn open_comments(&mut self, dialog: crate::CommentsDialog);

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

    /// Windows `GetOpenFileNameW` with a filter: one existing file with one
    /// of `extensions`, `None` when cancelled.
    fn choose_open_file(&mut self, title: &str, extensions: &[&str]) -> Option<PathBuf>;

    /// Windows `GetSaveFileNameW`: a save dialog in `directory` starting
    /// with `default_name`, `None` when cancelled.
    fn choose_save_file(
        &mut self,
        title: &str,
        directory: Option<&std::path::Path>,
        default_name: &str,
        extensions: &[&str],
    ) -> Option<PathBuf>;

    /// Windows `choose_download_folder`: a folder dialog starting in
    /// `initial`, `None` when cancelled.
    fn choose_download_folder(&mut self, title: &str, initial: &std::path::Path)
    -> Option<PathBuf>;

    /// Windows `MessageBoxW` with Yes and No: `true` for Yes. Focus returns
    /// to the control that had it.
    fn ask_yes_no(&mut self, message: &str, yes: &str, no: &str) -> bool;

    /// Windows `DownloadProgressWindow::create` or `show`: the modeless
    /// progress window, in front with focus on Hide. Details calls
    /// [`crate::Shell::download_progress_details`].
    fn show_download_progress(&mut self, progress: &crate::DownloadProgress);

    /// Windows `DownloadProgressWindow::set_progress`, without moving focus.
    fn update_download_progress(&mut self, percent: u32, message: &str);

    /// Windows `DownloadProgressWindow::destroy`.
    fn close_download_progress(&mut self);

    /// Opens the Settings window after the current event, modal over the
    /// window, with focus on its section list. It calls the shell's
    /// `settings_*` methods and ends with [`crate::Shell::settings_closed`].
    fn open_settings(&mut self);

    /// Opens the Action Finder after the current event, modal over the
    /// window, with focus on its search field; when it closed it calls
    /// [`crate::Shell::action_finder_closed`].
    fn open_action_finder(&mut self, model: apricot_app::ActionFinderModel);

    /// Windows `browser_cookies_exported`: an open Settings window shows
    /// the cookies file `path` and the browser choice `browser_index`.
    fn settings_cookie_source(&mut self, path: &str, browser_index: usize);

    /// Opens the file or folder converter after the current event, modal
    /// over the window; when it closed and focus is back where it was, it
    /// calls [`crate::Shell::converter_closed`].
    fn open_converter(&mut self, dialog: crate::ConverterDialog);

    /// Windows `ConversionProgressWindow::create`: the folder conversion
    /// progress with `total` files, which cannot be closed.
    fn show_conversion_progress(&mut self, title: &str, message: &str, total: usize);

    /// Windows `ConversionProgressWindow::update`.
    fn update_conversion_progress(&mut self, converted: usize, message: &str);

    /// Windows `ConversionProgressWindow::destroy`: focus returns where it
    /// was when the window had it.
    fn close_conversion_progress(&mut self);

    /// The `AudioVault` login dialog (Windows `audiovault_login_win32`):
    /// Email (focused), Password, Register, OK and Cancel; Return in a field
    /// is OK. The address and password, `None` when cancelled; focus
    /// returns where it was.
    fn audiovault_login(&mut self, login: &crate::AudiovaultLogin) -> Option<(String, String)>;

    /// Windows `spotify_login_win32::show`: the login dialog after the
    /// current event, with focus on its read-only status; its buttons call
    /// [`crate::Shell::spotify_login_action`] and closing calls
    /// [`crate::Shell::spotify_login_closed`].
    fn open_spotify_login(&mut self, texts: &crate::SpotifyLoginTexts);

    /// Windows `spotify_login_win32::update` of an open login dialog.
    fn spotify_login_update(&mut self, update: &crate::SpotifyLoginUpdate);

    /// Windows `spotify_settings_win32::show`: quality, normalisation,
    /// autoplay and library order; `None` when cancelled.
    fn spotify_settings(
        &mut self,
        texts: &crate::SpotifySettingsTexts,
        current: apricot_spotify::settings::SpotifySettings,
    ) -> Option<apricot_spotify::settings::SpotifySettings>;

    /// Windows `spotify_search_win32::show`: the query and the type index;
    /// `None` when cancelled. Accepting does not put the focus back where
    /// it was, because the results list takes it.
    fn spotify_search(
        &mut self,
        texts: &crate::SpotifySearchTexts,
        query: &str,
        kind: usize,
    ) -> Option<(String, usize)>;

    /// Windows `spotify_queue_win32::show` and `spotify_devices_win32::show`
    /// over the current screen, after the current event; its buttons and
    /// keys go to `Shell::spotify_dialog_action`, its context menu comes
    /// from `Shell::spotify_dialog_menu`, and closing it calls
    /// `Shell::spotify_dialog_closed`.
    fn open_spotify_dialog(&mut self, dialog: crate::SpotifyListDialog);

    /// New rows of the open Spotify dialog: `selected`, or with `None` the
    /// row with the same key as the selected one (the same position when
    /// it is gone). Focus stays where it is.
    fn spotify_dialog_update(
        &mut self,
        rows: Vec<crate::SpotifyDialogRow>,
        selected: Option<usize>,
    );

    /// Focus on the screen's list unless it has it.
    fn focus_list(&mut self);

    /// Replaces one row of the screen's list, keeping the selection
    /// (Windows `LB_DELETESTRING` and `LB_INSERTSTRING`).
    fn set_list_row(&mut self, index: usize, label: &str);

    /// Whether a window of the application is in front (Windows
    /// `GetForegroundWindow` belongs to the application).
    fn app_is_active(&self) -> bool;

    /// Windows `RSS_REFRESH_TIMER_ID`: calls
    /// [`crate::Shell::refresh_all_rss_feeds_background`] every
    /// `interval_seconds`, or stops with `None`.
    fn set_rss_timer(&mut self, interval_seconds: Option<f64>);

    /// Starts or stops polling [`crate::Shell::poll_background`] (Windows
    /// `LOCAL_FOLDER_TIMER_ID` and the other work timers).
    fn set_background_polling(&mut self, active: bool);

    /// Windows `restore_from_tray`: shows and activates the window.
    fn restore(&mut self);

    /// The Exit row: the application ends.
    fn exit(&mut self);
}
