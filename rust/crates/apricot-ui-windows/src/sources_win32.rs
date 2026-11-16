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
    /// This call showed the list in the main window, which stays between
    /// its actions and closes with it.
    showed_list: bool,
}
impl Ui {
    const fn new(window: HWND) -> Self {
        Self {
            window,
            showed_list: false,
        }
    }
    /// The window dialogs open over.
    const fn owner(&self) -> HWND {
        self.window
    }
}
impl Drop for Ui {
    fn drop(&mut self) {
        if self.showed_list {
            unsafe { super::sources_panel::close(self.window) };
        }
    }
}
impl SourcesUi for Ui {
    fn browse(&mut self, browser: &SourceBrowser) -> Option<SourceSelection> {
        self.showed_list = true;
        unsafe { super::sources_panel::browse(self.window, browser) }
    }
    fn form(&mut self, form: &FeatureForm) -> Option<FormValues> {
        crate::feature_form_win32::show(self.owner(), form)
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
        unsafe { show_error_message(self.owner(), message) }
    }
    fn play(&mut self, item: apricot_core::MediaItem) {
        // The player takes the place of the list.
        if std::mem::take(&mut self.showed_list) {
            unsafe { super::sources_panel::close(self.window) };
        }
        unsafe { start_media_item(self.window, item, None) }
    }
    fn choose_file(&mut self, title: &str) -> Option<PathBuf> {
        crate::file_dialog_win32::choose_file_with_filters(
            self.owner(),
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
        crate::folder_dialog_win32::choose_media_folder(self.owner(), title)
    }
    fn copy(&mut self, text: &str) -> bool {
        crate::clipboard_win32::copy_text(self.window, text).is_ok()
    }
    fn cue(&mut self, started: bool) {
        apricot_platform::recording_cue::play(started);
    }
    fn ask_yes_no(&mut self, message: &str) -> bool {
        unsafe { super::ask_yes_no(self.owner(), message) }
    }
    fn navigation_mark(&self) -> u64 {
        unsafe { state(self.window) }.map_or(0, |state| state.view_changes)
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
/// Takes the controller out for a call that may open a window.
fn lend(state: &mut super::WindowState) -> apricot_shell::sources::SourcesController {
    let controller = std::mem::take(&mut state.source_tools);
    if state.source_tools_out == 0 {
        state.lent_recording = controller.recording_controls();
    }
    state.source_tools_out += 1;
    controller
}

/// Puts the controller back; the outermost call ends the lending.
fn give_back(
    state: &mut super::WindowState,
    controller: apricot_shell::sources::SourcesController,
) {
    state.source_tools_out = state.source_tools_out.saturating_sub(1);
    // Only the outermost call holds the real controller.
    if state.source_tools_out == 0 {
        state.source_tools = controller;
        state.lent_recording = None;
    }
}

/// The global recording keys while the controller is out in an open
/// window: Stop and Pause reach the running recording, and Start never
/// begins a second one (review 2.1).
unsafe fn lent_recording_action(window: HWND, id: &str) -> bool {
    let Some(state) = state_mut(window) else {
        return false;
    };
    if state.source_tools_out == 0 {
        return false;
    }
    let key = match (id, state.lent_recording.clone()) {
        ("recording_stop", Some(controls)) => {
            controls.stop();
            "recording_finishing"
        }
        ("recording_pause", Some(controls)) => {
            if controls.toggle_pause() {
                "recording_paused"
            } else {
                "recording_resumed"
            }
        }
        ("recording_start", Some(_)) => "recording_already_active",
        ("recording_start", None) => "recording_close_window_first",
        ("recording_stop" | "recording_pause", None) => "recording_not_active",
        _ => return false,
    };
    let text = super::catalog_text(&state.application, key);
    super::set_status(state, &text, true);
    true
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
    if lent_recording_action(window, id) {
        return true;
    }
    let Some(environment) = environment(window) else {
        return true;
    };
    let Some(state) = state_mut(window) else {
        return true;
    };
    let mut controller = lend(state);
    let mut ui = Ui::new(window);
    let handled = controller.action(&mut ui, &environment, id);
    let busy = controller.is_busy();
    // While the next list loads, the shown list stays, so the focus does not
    // go back to the screen below and come again (beta 7).
    if busy {
        ui.showed_list = false;
    }
    drop(ui);
    if let Some(state) = state_mut(window) {
        give_back(state, controller);
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
    let mut controller = lend(state);
    let resumed = controller.resume_after_player(
        &mut Ui::new(window),
        &environment,
        station.as_deref(),
        channel.as_deref(),
    );
    let busy = controller.is_busy();
    if let Some(state) = state_mut(window) {
        give_back(state, controller);
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
    let mut controller = lend(state);
    controller.ensure_loaded(&mut Ui::new(window), &environment);
    if let Some(state) = state_mut(window) {
        give_back(state, controller);
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
    // Only the outermost poll holds the real controller; one inside an open
    // list polls the stand-in and leaves the list alone.
    let outermost = state.source_tools_out == 0;
    let mut controller = lend(state);
    let mut ui = Ui::new(window);
    let busy = controller.poll(&mut ui, &environment);
    // A list kept open while work loaded closes when the work ended without
    // showing another one, for example with an error.
    if busy {
        ui.showed_list = false;
    } else if outermost {
        ui.showed_list = true;
    }
    drop(ui);
    if let Some(state) = state_mut(window) {
        give_back(state, controller);
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
    let mut controller = lend(state);
    controller.stream_metadata(
        &mut Ui::new(window),
        station.as_deref(),
        title.as_deref(),
        url.as_deref(),
        iptv.as_deref(),
    );
    if let Some(state) = state_mut(window) {
        give_back(state, controller);
    }
}
pub(super) unsafe fn playback_started(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut controller = lend(state);
    controller.playback_started(&mut Ui::new(window));
    if let Some(state) = state_mut(window) {
        give_back(state, controller);
    }
}
