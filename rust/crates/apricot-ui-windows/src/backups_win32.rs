//! Native wrapper for the shared settings and bookmark import and export.
use super::{set_status, show_error_message, state_mut};
use apricot_shell::backups::{self, Backup, BackupUi};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::WindowsAndMessaging::PostMessageW,
};

/// The import and export dialogs, owned by the main window.
struct Dialogs {
    owner: HWND,
}

fn pattern(extensions: &[&str]) -> String {
    extensions
        .iter()
        .map(|extension| format!("*.{extension}"))
        .collect::<Vec<_>>()
        .join(";")
}

impl BackupUi for Dialogs {
    fn form(&mut self, form: &apricot_shell::FeatureForm) -> Option<apricot_shell::FormValues> {
        crate::feature_form_win32::show(self.owner, form)
    }

    fn open_file(&mut self, title: &str, extensions: &[&str]) -> Option<std::path::PathBuf> {
        crate::file_dialog_win32::choose_file_with_filters(
            self.owner,
            title,
            &[(title.to_owned(), pattern(extensions))],
        )
        .ok()
        .flatten()
    }

    fn save_file(
        &mut self,
        title: &str,
        default_name: &str,
        extensions: &[&str],
    ) -> Option<std::path::PathBuf> {
        let directory = std::env::var_os("USERPROFILE")
            .map(|home| std::path::PathBuf::from(home).join("Documents"))
            .unwrap_or_default();
        crate::file_dialog_win32::save_file_with_filters(
            self.owner,
            title,
            &[(title.to_owned(), pattern(extensions))],
            &directory,
            default_name,
            extensions.first().copied().unwrap_or("json"),
        )
        .ok()
        .flatten()
    }

    fn ask_yes_no(&mut self, message: &str) -> bool {
        // SAFETY: the owner is the live main window.
        unsafe { super::ask_yes_no(self.owner, message) }
    }
}

/// Export settings, Import settings, Export bookmarks and Import bookmarks.
pub(super) unsafe fn run(window: HWND, backup: Backup) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    let previous_modal = state.modal_open;
    state.modal_open = true;
    let mut dialogs = Dialogs { owner: window };
    let report = backups::run(&mut state.application, &catalog, backup, &mut dialogs);
    let Some(state) = state_mut(window) else {
        return;
    };
    state.modal_open = previous_modal;
    let Some(report) = report else {
        return;
    };
    if report.settings_imported {
        super::apply_integrations(window);
        let _ = super::apply_global_shortcuts(window);
        let _ = PostMessageW(
            Some(window),
            super::WM_APPLY_AUDIO_PROCESSING,
            WPARAM(0),
            LPARAM(0),
        );
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    if !report.message.is_empty() {
        set_status(state, &report.message, true);
    }
    if let Some(error) = &report.error {
        show_error_message(window, error);
    }
}
