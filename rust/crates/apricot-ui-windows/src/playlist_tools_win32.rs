//! Native wrapper for shared playlist editing and cancellable folder expansion.
use super::{
    MainView, layout_controls_state, refresh_user_playlist_items, refresh_user_playlists,
    set_status, show_error_message, state_mut,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{KillTimer, LB_GETCURSEL, SendMessageW, SetTimer},
};
pub(super) const TIMER_ID: usize = 113;
pub(super) struct PendingResolution {
    index: usize,
    expected: apricot_storage::UserPlaylist,
    receiver: Receiver<Result<Vec<apricot_core::MediaItem>, String>>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for PendingResolution {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
pub(super) unsafe fn start_resolution(window: HWND, index: usize) {
    let Some(state) = state_mut(window) else {
        return;
    };
    state.pending_playlist_resolution = None;
    let Some(expected) = state.application.user_playlists().get(index).cloned() else {
        return;
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let items = expected.items.clone();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = apricot_platform::playlist_file::resolve_folders(&items, &flag);
        let _ = sender.send(result);
    });
    state.pending_playlist_resolution = Some(PendingResolution {
        index,
        expected,
        receiver,
        cancelled,
    });
    let _ = SetTimer(Some(window), TIMER_ID, 100, None);
}
pub(super) unsafe fn poll(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(pending) = state.pending_playlist_resolution.as_ref() else {
        let _ = KillTimer(Some(window), TIMER_ID);
        return;
    };
    let result = match pending.receiver.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Empty) => return,
        Err(_) => Err(
            apricot_app::embedded_catalog(&state.application.settings().language)
                .text("playlist_folder_scan_failed")
                .to_owned(),
        ),
    };
    let Some(pending) = state.pending_playlist_resolution.take() else {
        return;
    };
    let _ = KillTimer(Some(window), TIMER_ID);
    match result {
        Ok(items) => {
            if state.application.install_user_playlist_resolution(
                pending.index,
                &pending.expected,
                items,
            ) && state.view == MainView::UserPlaylistItems
                && state.current_user_playlist_index == pending.index
            {
                refresh_user_playlist_items(state, false, false);
                layout_controls_state(window, state);
            }
        }
        Err(error) => {
            if state.view == MainView::UserPlaylistItems
                && state.current_user_playlist_index == pending.index
            {
                show_error_message(window, &error);
            }
        }
    }
}
/// The playlist editor's pickers on native dialogs owned by the main window.
struct EditorUi {
    owner: HWND,
}

fn pattern(extensions: &[&str]) -> String {
    extensions
        .iter()
        .map(|extension| format!("*.{extension}"))
        .collect::<Vec<_>>()
        .join(";")
}

impl apricot_shell::playlist_editor::PlaylistEditorUi for EditorUi {
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

    fn open_folder(&mut self, title: &str) -> Option<std::path::PathBuf> {
        crate::folder_dialog_win32::choose_media_folder(self.owner, title)
    }

    fn save_file(
        &mut self,
        title: &str,
        default_name: &str,
        extensions: &[&str],
    ) -> Option<std::path::PathBuf> {
        let directory = std::env::var_os("USERPROFILE")
            .map(|home| std::path::PathBuf::from(home).join("Music"))
            .unwrap_or_default();
        crate::file_dialog_win32::save_file_with_filters(
            self.owner,
            title,
            &[(title.to_owned(), pattern(extensions))],
            &directory,
            default_name,
            extensions.first().copied().unwrap_or("m3u8"),
        )
        .ok()
        .flatten()
    }
}

pub(super) unsafe fn edit(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let index = if state.view == MainView::UserPlaylists {
        usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).unwrap_or(0)
    } else {
        state.current_user_playlist_index
    };
    let previous_modal = state.modal_open;
    state.modal_open = true;
    let result = apricot_shell::playlist_editor::edit(
        &mut state.application,
        index,
        &mut EditorUi { owner: window },
    );
    state.modal_open = previous_modal;
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    match result {
        Ok(true) => {
            let on_items = state.view == MainView::UserPlaylistItems;
            if on_items {
                refresh_user_playlist_items(state, true, false);
            } else {
                refresh_user_playlists(state, true);
            }
            set_status(state, catalog.text("playlist_editor_saved"), true);
            layout_controls_state(window, state);
            if on_items {
                start_resolution(window, index);
            }
        }
        Ok(false) => {}
        Err(error) => show_error_message(window, &error),
    }
}
pub(super) unsafe fn save_current(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = apricot_app::embedded_catalog(&state.application.settings().language);
    match apricot_shell::playlist_editor::save_current(&mut state.application, |form| {
        crate::feature_form_win32::show(window, form)
    }) {
        Ok(true) => set_status(state, catalog.text("playlist_save_current_done"), true),
        Ok(false) => {}
        Err(error) => show_error_message(window, &error),
    }
}
