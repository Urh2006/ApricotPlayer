//! Native wrapper for the shared local file operations in the folder list.
use super::{MainView, refresh_local_folder, set_status, show_error_message, state_mut};
use apricot_shell::file_manager::{self, FileManagerUi, Operation};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{LB_GETCURSEL, SendMessageW},
};

/// The file operations' dialogs, owned by the main window.
struct FileUi {
    owner: HWND,
}

impl FileManagerUi for FileUi {
    fn form(&mut self, form: &apricot_shell::FeatureForm) -> Option<apricot_shell::FormValues> {
        crate::feature_form_win32::show(self.owner, form)
    }

    fn open_folder(&mut self, title: &str) -> Option<std::path::PathBuf> {
        crate::folder_dialog_win32::choose_media_folder(self.owner, title)
    }

    fn ask_yes_no(&mut self, message: &str) -> bool {
        // SAFETY: the owner is the live main window.
        unsafe { super::ask_yes_no(self.owner, message) }
    }
}

/// Delete, Shift+Delete and Manage files in the folder list; `None` opens
/// Manage files.
pub(super) unsafe fn run(window: HWND, operation: Option<Operation>) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.view != MainView::LocalFolder {
        return;
    }
    let Ok(row) = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let paths = file_manager::local_paths(state.application.local_folder_session().items());
    let previous_modal = state.modal_open;
    state.modal_open = true;
    let mut ui = FileUi { owner: window };
    let report = match operation {
        Some(operation) => paths
            .get(row)
            .cloned()
            .and_then(|path| file_manager::run(&catalog, operation, &[path], &mut ui)),
        None => file_manager::manage(&catalog, &paths, row, &mut ui),
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    state.modal_open = previous_modal;
    let Some(report) = report else {
        return;
    };
    state.application.remove_local_folder_files(&report.removed);
    refresh_local_folder(state, true, false);
    if !report.message.is_empty() {
        set_status(state, &report.message, true);
    }
    if let Some(failures) = &report.failures {
        show_error_message(window, failures);
    }
}

/// Manage files on the files marked while listening (Alt+Shift+K).
pub(super) unsafe fn manage_marked(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let paths: Vec<std::path::PathBuf> = state
        .application
        .playback_features()
        .marked_files
        .iter()
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_file())
        .collect();
    if paths.is_empty() {
        set_status(
            state,
            &catalog.text("files_marked_count").replace("{count}", "0"),
            true,
        );
        return;
    }
    let previous_modal = state.modal_open;
    state.modal_open = true;
    let mut ui = FileUi { owner: window };
    let report = file_manager::manage_marked(&catalog, &paths, &mut ui);
    let Some(state) = state_mut(window) else {
        return;
    };
    state.modal_open = previous_modal;
    let Some(report) = report else {
        return;
    };
    let removed: Vec<String> = report
        .removed
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    state
        .application
        .playback_features_mut()
        .marked_files
        .retain(|path| !removed.contains(path));
    if !report.message.is_empty() {
        set_status(state, &report.message, true);
    }
    if let Some(failures) = &report.failures {
        show_error_message(window, failures);
    }
}
