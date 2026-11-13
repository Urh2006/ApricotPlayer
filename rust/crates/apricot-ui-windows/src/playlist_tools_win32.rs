//! Native wrapper for shared playlist editing and cancellable folder expansion.
#![allow(clippy::pedantic, clippy::style)] // 2.1 draft: fully linted when its feature unit is finished.
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
        Err(_) => Err("Playlist folder scan stopped unexpectedly".into()),
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
pub(super) unsafe fn edit(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let index = if state.view == MainView::UserPlaylists {
        usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).unwrap_or(0)
    } else {
        state.current_user_playlist_index
    };
    let result = apricot_shell::playlist_editor::edit(&mut state.application, index, |form| {
        crate::feature_form_win32::show(window, form)
    });
    match result {
        Ok(true) => {
            let on_items = state.view == MainView::UserPlaylistItems;
            if on_items {
                refresh_user_playlist_items(state, true, false);
            } else {
                refresh_user_playlists(state, true);
            }
            set_status(state, "Playlist saved", true);
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
    match apricot_shell::playlist_editor::save_current(&mut state.application, |form| {
        crate::feature_form_win32::show(window, form)
    }) {
        Ok(true) => set_status(state, "Playback list saved as a playlist", true),
        Ok(false) => {}
        Err(error) => show_error_message(window, &error),
    }
}
