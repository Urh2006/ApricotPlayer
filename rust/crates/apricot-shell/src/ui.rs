//! What a native UI offers the shell.

use apricot_app::MainMenuModel;

/// The native side of the main window. Every method runs on the UI thread.
pub trait ShellUi {
    /// Windows `set_status`: shows `message` on the status line and, when
    /// `announce` is set, speaks it once without moving focus.
    fn set_status(&mut self, message: &str, announce: bool);

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

    /// The row selected in the screen's list, if any.
    fn selected_row(&self) -> Option<usize>;

    /// The Exit row: the application ends.
    fn exit(&mut self);
}
