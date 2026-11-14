//! Windows host adapter; source behavior lives in the shared controller.
#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]
use super::{
    HWND, KillTimer, PathBuf, SetTimer, set_status, show_error_message, start_media_item, state,
    state_mut,
};
use apricot_shell::{
    FeatureForm, FormValues,
    sources::{SourceBrowser, SourceSelection, SourcesEnvironment, SourcesUi},
};
pub(super) const TIMER: usize = 8501;
struct Ui {
    window: HWND,
}
impl SourcesUi for Ui {
    fn browse(&mut self, browser: &SourceBrowser) -> Option<SourceSelection> {
        crate::sources_browser_win32::show(self.window, browser)
    }
    fn form(&mut self, form: &FeatureForm) -> Option<FormValues> {
        crate::feature_form_win32::show(self.window, form)
    }
    fn status(&mut self, message: &str, announce: bool) {
        if !message.is_empty() {
            unsafe {
                if let Some(state) = state(self.window) {
                    set_status(state, message, announce);
                }
            }
        }
    }
    fn error(&mut self, message: &str) {
        unsafe { show_error_message(self.window, message) }
    }
    fn play(&mut self, item: apricot_core::MediaItem) {
        unsafe { start_media_item(self.window, item, None) }
    }
    fn choose_file(&mut self, title: &str) -> Option<PathBuf> {
        crate::file_dialog_win32::choose_file_with_filters(
            self.window,
            title,
            &[
                (
                    "Playlists and programme guides".into(),
                    "*.m3u;*.m3u8;*.xml".into(),
                ),
                ("All files".into(), "*.*".into()),
            ],
        )
        .ok()
        .flatten()
    }
    fn choose_folder(&mut self, title: &str) -> Option<PathBuf> {
        crate::folder_dialog_win32::choose_media_folder(self.window, title)
    }
    fn copy(&mut self, text: &str) -> bool {
        crate::clipboard_win32::copy_text(self.window, text).is_ok()
    }
    fn cue(&mut self, started: bool) {
        apricot_platform::recording_cue::play(started);
    }
    fn ask_yes_no(&mut self, message: &str) -> bool {
        unsafe { super::ask_yes_no(self.window, message) }
    }
}
pub(super) unsafe fn environment(window: HWND) -> Option<SourcesEnvironment> {
    let state = state(window)?;
    let settings = state.application.settings();
    let directory = state.application.settings_file().parent()?.to_owned();
    let app_directory = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_owned));
    Some(SourcesEnvironment {
        directory,
        download_directory: settings.download_folder.clone().into(),
        ffmpeg: apricot_platform::ffmpeg_executable(
            &settings.ffmpeg_location,
            app_directory.as_deref(),
        ),
        proxy: settings.proxy.clone(),
        shortcuts: settings.keyboard_shortcuts.clone(),
        language: settings.language.clone(),
    })
}
pub(super) unsafe fn action(window: HWND, id: &str) -> bool {
    if !matches!(
        id,
        "open_radio"
            | "open_iptv"
            | "open_recording"
            | "recording_start"
            | "recording_pause"
            | "recording_stop"
            | "open_recordings_folder"
            | "radio_now_playing"
            | "recording_settings"
    ) {
        return false;
    }
    let Some(environment) = environment(window) else {
        return true;
    };
    let Some(state) = state_mut(window) else {
        return true;
    };
    let mut controller = std::mem::take(&mut state.source_tools);
    let handled = controller.action(&mut Ui { window }, &environment, id);
    let busy = controller.is_busy();
    if let Some(state) = state_mut(window) {
        state.source_tools = controller;
    }
    if busy {
        SetTimer(Some(window), TIMER, 50, None);
    }
    handled
}
/// The station or channel of the player, before it closes.
pub(super) unsafe fn player_source_ids(window: HWND) -> (Option<String>, Option<String>) {
    let Some(state) = state(window) else {
        return (None, None);
    };
    let session = state.application.player_session();
    let id = |key: &str| {
        session
            .current_item()
            .and_then(|item| item.metadata.get(key))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    (id("radio_station_id"), id("iptv_channel_id"))
}
/// One step back from a player started in Radio or IPTV: the list it was
/// played from, over the screen below. `true` when that list opened.
pub(super) unsafe fn resume_after_player(
    window: HWND,
    (station, channel): (Option<String>, Option<String>),
) -> bool {
    let Some(environment) = environment(window) else {
        return false;
    };
    let Some(state) = state_mut(window) else {
        return false;
    };
    let mut controller = std::mem::take(&mut state.source_tools);
    let resumed = controller.resume_after_player(
        &mut Ui { window },
        &environment,
        station.as_deref(),
        channel.as_deref(),
    );
    let busy = controller.is_busy();
    if let Some(state) = state_mut(window) {
        state.source_tools = controller;
    }
    if busy {
        SetTimer(Some(window), TIMER, 50, None);
    }
    resumed
}
/// Reads the Radio, IPTV and recording files once, for Settings; a damaged
/// file is reported as when Radio opens.
pub(super) unsafe fn ensure_loaded(window: HWND) {
    let Some(environment) = environment(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut controller = std::mem::take(&mut state.source_tools);
    controller.ensure_loaded(&mut Ui { window }, &environment);
    if let Some(state) = state_mut(window) {
        state.source_tools = controller;
    }
}
/// The player went to the main menu: nothing to go back to.
pub(super) unsafe fn forget_player_return(window: HWND) {
    if let Some(state) = state_mut(window) {
        state.source_tools.forget_player_return();
    }
}
pub(super) unsafe fn poll(window: HWND) {
    let Some(environment) = environment(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut controller = std::mem::take(&mut state.source_tools);
    let busy = controller.poll(&mut Ui { window }, &environment);
    if let Some(state) = state_mut(window) {
        state.source_tools = controller;
    }
    // A list shown while polling runs its own message loop, where this
    // timer polls the empty stand-in controller and stops; work the list
    // started afterwards needs the timer again.
    if busy {
        SetTimer(Some(window), TIMER, 50, None);
    } else {
        let _ = KillTimer(Some(window), TIMER);
    }
}
pub(super) unsafe fn metadata(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let session = state.application.player_session();
    let station = session
        .current_item()
        .and_then(|item| item.metadata.get("radio_station_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let title = session.media_info().stream_title.clone();
    let url = session
        .current_item()
        .and_then(|item| item.url.as_ref())
        .map(ToString::to_string);
    let iptv = session
        .current_item()
        .and_then(|item| item.metadata.get("iptv_channel_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let mut controller = std::mem::take(&mut state.source_tools);
    controller.stream_metadata(
        &mut Ui { window },
        station.as_deref(),
        title.as_deref(),
        url.as_deref(),
        iptv.as_deref(),
    );
    if let Some(state) = state_mut(window) {
        state.source_tools = controller;
    }
}
pub(super) unsafe fn playback_started(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut controller = std::mem::take(&mut state.source_tools);
    controller.playback_started(&mut Ui { window });
    if let Some(state) = state_mut(window) {
        state.source_tools = controller;
    }
}
