//! Player equalizer dialog (F4) and the equalizer profile prompts shared with
//! Settings, following Python `show_player_equalizer` and its helpers.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of, path::PathBuf};

use apricot_app::{
    EqualizerSession,
    equalizer::{
        self, EqualizerGains, EqualizerSettings, RANGE_OPTIONS, SLIDER_LINE_STEP, SLIDER_PAGE_STEP,
    },
};
use apricot_core::{TranslationCatalog, audio::EQUALIZER_BANDS};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{
                TBM_SETLINESIZE, TBM_SETPAGESIZE, TBM_SETPOS, TBM_SETRANGEMAX, TBM_SETRANGEMIN,
                TRACKBAR_CLASSW,
            },
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus},
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CB_SETCURSEL,
                CBS_DROPDOWNLIST, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, ES_AUTOHSCROLL, GetClientRect, GetMessageW, GetParent,
                GetSystemMetrics, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU,
                IDC_ARROW, IDYES, IsDialogMessageW, IsWindow, KillTimer, LoadCursorW, MB_ICONERROR,
                MB_ICONQUESTION, MB_OK, MB_YESNO, MESSAGEBOX_STYLE, MSG, MessageBoxW, MoveWindow,
                PostQuitMessage, RegisterClassW, SM_CYMAXIMIZED, SW_HIDE, SW_SHOW, SendMessageW,
                SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowTextW, ShowWindow,
                TranslateMessage, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE,
                WM_COMMAND, WM_HSCROLL, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WM_TIMER, WNDCLASSW,
                WS_CHILD, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
                WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaEqualizerWindow");
/// `IsDialogMessageW` sends Enter and Escape as `IDOK` and `IDCANCEL`, so the
/// OK and Cancel buttons use those identifiers like `wx.ID_OK`/`wx.ID_CANCEL`.
const ID_OK: usize = 1;
const ID_CANCEL: usize = 2;
const ID_PRESET: usize = 3_001;
const ID_RANGE: usize = 3_002;
const ID_NAME: usize = 3_003;
const ID_BAND_START: usize = 3_010;
const ID_BUTTON_START: usize = 3_040;
const CBN_SELCHANGE: usize = 1;
const BN_CLICKED: usize = 0;
const APPLY_TIMER_ID: usize = 1;
const TBM_GETPOS: u32 = 0x0400;

/// What the dialog needs from the main window: settings, the player session
/// equalizer, applying it to mpv and speaking through the player announcer.
pub trait EqualizerDialogHost {
    fn catalog(&self) -> TranslationCatalog;
    fn settings(&self) -> EqualizerSettings;
    /// Writes the equalizer settings; `save` also writes `settings.json`.
    ///
    /// # Errors
    ///
    /// Returns a message for the user when the settings cannot be written.
    fn update_settings(
        &mut self,
        settings: &EqualizerSettings,
        save: bool,
    ) -> std::result::Result<(), String>;
    fn session_equalizer(&self) -> Option<EqualizerSession>;
    fn set_session_equalizer(&mut self, equalizer: Option<EqualizerSession>);
    /// Python `apply_equalizer_to_player`.
    fn apply_to_player(&mut self);
    /// Python `equalizer_current_device_key`.
    fn device_key(&self) -> String;
    /// Python `announce_player`.
    fn announce(&mut self, message: &str);
}

/// Buttons in Python's creation order, which is the Tab order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DialogButton {
    Ok,
    Cancel,
    Reset,
    SaveGlobal,
    AddProfile,
    DeleteProfile,
    ImportProfile,
    ExportProfile,
    Compare,
    SaveDevice,
    ClearDevice,
}

const BUTTONS: [(DialogButton, &str); 11] = [
    (DialogButton::Ok, "ok"),
    (DialogButton::Cancel, "cancel"),
    (DialogButton::Reset, "reset_equalizer"),
    (DialogButton::SaveGlobal, "save_equalizer_as_global"),
    (DialogButton::AddProfile, "add_equalizer_profile"),
    (DialogButton::DeleteProfile, "delete_equalizer_profile"),
    (DialogButton::ImportProfile, "import_equalizer_profile"),
    (DialogButton::ExportProfile, "export_equalizer_profile"),
    (DialogButton::Compare, "compare_equalizer_profile"),
    (DialogButton::SaveDevice, "save_equalizer_as_device_default"),
    (DialogButton::ClearDevice, "clear_equalizer_device_default"),
];

const fn button_id(button: DialogButton) -> usize {
    match button {
        DialogButton::Ok => ID_OK,
        DialogButton::Cancel => ID_CANCEL,
        other => ID_BUTTON_START + other as usize,
    }
}

fn button_from_id(id: usize) -> Option<DialogButton> {
    BUTTONS
        .iter()
        .map(|(button, _)| *button)
        .find(|button| button_id(*button) == id)
}

struct DialogState {
    host: Box<dyn EqualizerDialogHost>,
    catalog: TranslationCatalog,
    previous_focus: HWND,
    preset_label: HWND,
    preset: HWND,
    range_label: HWND,
    range: HWND,
    name_label: HWND,
    name: HWND,
    band_labels: Vec<HWND>,
    sliders: Vec<HWND>,
    buttons: Vec<(DialogButton, HWND)>,
    preset_options: Vec<String>,
    visible_preset: String,
    gains: EqualizerGains,
    db_range: i64,
    original_equalizer: Option<EqualizerSession>,
    original_db_range: i64,
    compare_showing_original: bool,
    accepted: bool,
}

pub fn register() -> Result<()> {
    // SAFETY: The class is registered once before the main message loop.
    unsafe {
        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size fits in i32"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hInstance: HINSTANCE(module.0),
            lpszClassName: CLASS_NAME,
            lpfnWndProc: Some(window_proc),
            ..Default::default()
        };
        if RegisterClassW(&raw const class) == 0 {
            return Err(windows::core::Error::from_thread());
        }
    }
    Ok(())
}

/// Python `show_player_equalizer`: a modal dialog whose changes are heard at
/// once. OK keeps the edited equalizer for this player session, Cancel,
/// Escape and closing the window restore the previous one.
///
/// # Errors
///
/// Returns a Win32 error when the window or one of its controls cannot be
/// created.
pub fn show(owner: HWND, host: Box<dyn EqualizerDialogHost>) -> Result<()> {
    // SAFETY: The nested modal loop owns the boxed state and re-enables its
    // owner before returning.
    unsafe { show_win32(owner, host) }
}

unsafe fn show_win32(owner: HWND, host: Box<dyn EqualizerDialogHost>) -> Result<()> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let catalog = host.catalog();
    let settings = host.settings();
    let device = host.device_key();
    let original_equalizer = host.session_equalizer();
    let (_, gains) = settings.base_state(original_equalizer.as_ref(), &device);
    let active_preset = settings.effective_preset(&device);
    let db_range = settings.db_range();
    let title = wide(catalog.text("equalizer"));
    let height = (GetSystemMetrics(SM_CYMAXIMIZED) - 20).clamp(520, 1_020);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CLASS_NAME,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        760,
        height,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let mut state = match create_controls(window, instance, &catalog) {
        Ok(controls) => DialogState {
            host,
            catalog,
            previous_focus: GetFocus(),
            preset_label: controls.preset_label,
            preset: controls.preset,
            range_label: controls.range_label,
            range: controls.range,
            name_label: controls.name_label,
            name: controls.name,
            band_labels: controls.band_labels,
            sliders: controls.sliders,
            buttons: controls.buttons,
            preset_options: Vec::new(),
            visible_preset: active_preset.clone(),
            gains: equalizer::normalized_gains(&gains),
            db_range,
            original_equalizer,
            original_db_range: db_range,
            compare_showing_original: false,
            accepted: false,
        },
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    initialize_controls(&mut state, &settings, &active_preset);
    let initial_focus = state.preset;
    let pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), pointer as isize);
    layout(window);
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetFocus(Some(initial_focus));
    let mut loop_error = None;
    let mut message = MSG::default();
    while IsWindow(Some(window)).as_bool() {
        let status = GetMessageW(&raw mut message, None, 0, 0);
        if status.0 == -1 {
            loop_error = Some(windows::core::Error::from_thread());
            let _ = DestroyWindow(window);
            break;
        }
        if status.0 == 0 {
            let _ = DestroyWindow(window);
            PostQuitMessage(i32::try_from(message.wParam.0).unwrap_or_default());
            break;
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    let mut state = Box::from_raw(pointer);
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
    if !state.previous_focus.0.is_null() {
        let _ = SetFocus(Some(state.previous_focus));
    }
    finish(&mut state);
    if let Some(error) = loop_error {
        return Err(error);
    }
    Ok(())
}

/// Python's initial control values: the effective preset, the saved dB range
/// and the sliders limited to that range.
unsafe fn initialize_controls(
    state: &mut DialogState,
    settings: &EqualizerSettings,
    active_preset: &str,
) {
    fill_preset_choices(state, settings, active_preset);
    let range_index = RANGE_OPTIONS
        .iter()
        .position(|option| *option == state.db_range.to_string())
        .unwrap_or(1);
    SendMessageW(state.range, CB_SETCURSEL, Some(WPARAM(range_index)), None);
    set_text(state.name, &settings.custom_name(active_preset));
    for (index, band) in EQUALIZER_BANDS.iter().enumerate() {
        configure_slider(state, index);
        let gain = state.gains.get(band.id).copied().unwrap_or_default();
        set_slider(
            state,
            index,
            equalizer::slider_from_gain(gain, state.db_range),
            false,
        );
    }
    update_custom_name_visibility(state);
}

/// Python `set_dialog_slider_value` for every band: the sliders show `gains`
/// limited to the dialog range, and the dialog keeps the limited values.
unsafe fn show_gains(state: &mut DialogState, gains: &EqualizerGains) {
    for (index, band) in EQUALIZER_BANDS.iter().enumerate() {
        let tenths = equalizer::slider_from_gain(
            gains.get(band.id).copied().unwrap_or_default(),
            state.db_range,
        );
        state
            .gains
            .insert(band.id.to_owned(), equalizer::gain_from_slider(tenths));
        set_slider(state, index, tenths, false);
    }
}

/// Python's code after `dialog.ShowModal()`.
fn finish(state: &mut DialogState) {
    if state.accepted {
        state.host.announce(state.catalog.text("equalizer_saved"));
        return;
    }
    let mut settings = state.host.settings();
    settings.db_range = state.original_db_range;
    let _ = state.host.update_settings(&settings, false);
    state
        .host
        .set_session_equalizer(state.original_equalizer.clone());
    state.host.apply_to_player();
    state.host.announce(state.catalog.text("equalizer_closed"));
}

struct Controls {
    preset_label: HWND,
    preset: HWND,
    range_label: HWND,
    range: HWND,
    name_label: HWND,
    name: HWND,
    band_labels: Vec<HWND>,
    sliders: Vec<HWND>,
    buttons: Vec<(DialogButton, HWND)>,
}

unsafe fn create_controls(
    parent: HWND,
    instance: HINSTANCE,
    catalog: &TranslationCatalog,
) -> Result<Controls> {
    let mut all = Vec::new();
    let preset_label = static_label(parent, instance, catalog.text("equalizer_preset"))?;
    let preset = combo(parent, instance, ID_PRESET)?;
    let range_label = static_label(parent, instance, catalog.text("equalizer_db_range"))?;
    let range = combo(parent, instance, ID_RANGE)?;
    for option in RANGE_OPTIONS {
        add_combo_string(range, option);
    }
    let name_label = static_label(parent, instance, catalog.text("equalizer_preset_name"))?;
    let name = create_control(
        parent,
        instance,
        w!("EDIT"),
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        WS_EX_CLIENTEDGE,
        ID_NAME,
    )?;
    all.extend([preset_label, preset, range_label, range, name_label, name]);
    let mut band_labels = Vec::with_capacity(EQUALIZER_BANDS.len());
    let mut sliders = Vec::with_capacity(EQUALIZER_BANDS.len());
    for (index, band) in EQUALIZER_BANDS.iter().enumerate() {
        let label = equalizer::band_gain_label(catalog, band.id);
        let label_control = static_label(parent, instance, &label)?;
        let slider = create_control(
            parent,
            instance,
            TRACKBAR_CLASSW,
            &label,
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            WINDOW_EX_STYLE::default(),
            ID_BAND_START + index,
        )?;
        all.extend([label_control, slider]);
        band_labels.push(label_control);
        sliders.push(slider);
    }
    let mut buttons = Vec::with_capacity(BUTTONS.len());
    for (button, key) in BUTTONS {
        let style = if button == DialogButton::Ok {
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32)
        } else {
            WS_CHILD | WS_VISIBLE | WS_TABSTOP
        };
        let control = create_control(
            parent,
            instance,
            w!("BUTTON"),
            catalog.text(key),
            style,
            WINDOW_EX_STYLE::default(),
            button_id(button),
        )?;
        all.push(control);
        buttons.push((button, control));
    }
    apply_font(&all);
    Ok(Controls {
        preset_label,
        preset,
        range_label,
        range,
        name_label,
        name,
        band_labels,
        sliders,
        buttons,
    })
}

unsafe fn fill_preset_choices(
    state: &mut DialogState,
    settings: &EqualizerSettings,
    selected: &str,
) {
    state.preset_options = settings.preset_options();
    SendMessageW(state.preset, CB_RESETCONTENT, None, None);
    for label in settings.preset_labels(&state.catalog) {
        add_combo_string(state.preset, &label);
    }
    let index = state
        .preset_options
        .iter()
        .position(|option| option == selected)
        .unwrap_or(0);
    SendMessageW(state.preset, CB_SETCURSEL, Some(WPARAM(index)), None);
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_SIZE => {
            layout(window);
            LRESULT(0)
        }
        WM_COMMAND => {
            handle_command(window, wparam);
            LRESULT(0)
        }
        WM_HSCROLL => {
            if lparam.0 != 0 {
                slider_changed(window, HWND(lparam.0 as *mut c_void));
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == APPLY_TIMER_ID => {
            let _ = KillTimer(Some(window), APPLY_TIMER_ID);
            if let Some(state) = state_mut(window) {
                state.host.apply_to_player();
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(window);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let _ = KillTimer(Some(window), APPLY_TIMER_ID);
            SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), 0);
            DefWindowProcW(window, message, wparam, lparam)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn handle_command(window: HWND, wparam: WPARAM) {
    let id = wparam.0 & 0xffff;
    let notification = (wparam.0 >> 16) & 0xffff;
    match id {
        ID_PRESET if notification == CBN_SELCHANGE => preset_changed(window),
        ID_RANGE if notification == CBN_SELCHANGE => range_changed(window),
        _ => {
            if (notification == BN_CLICKED || id == ID_OK || id == ID_CANCEL)
                && let Some(button) = button_from_id(id)
            {
                button_clicked(window, button);
            }
        }
    }
}

unsafe fn button_clicked(window: HWND, button: DialogButton) {
    match button {
        DialogButton::Ok => accept(window),
        DialogButton::Cancel => {
            let _ = DestroyWindow(window);
        }
        DialogButton::Reset => reset_preset(window),
        DialogButton::SaveGlobal => save_as_global(window),
        DialogButton::AddProfile => add_profile(window),
        DialogButton::DeleteProfile => delete_profile(window),
        DialogButton::ImportProfile => import_profile(window),
        DialogButton::ExportProfile => export_profile(window),
        DialogButton::Compare => compare(window),
        DialogButton::SaveDevice => save_device_default(window),
        DialogButton::ClearDevice => clear_device_default(window),
    }
}

/// OK: the name and dB range are saved, the edited equalizer stays active.
unsafe fn accept(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut settings = state.host.settings();
    save_dialog_name(state, &mut settings, None);
    settings.db_range = state.db_range;
    if let Err(error) = state.host.update_settings(&settings, true) {
        show_message(window, &state.catalog, &error, MB_ICONERROR);
        return;
    }
    state.accepted = true;
    let _ = DestroyWindow(window);
}

fn current_preset(state: &DialogState) -> String {
    // SAFETY: The combo box belongs to the live dialog on this thread.
    let index = unsafe { SendMessageW(state.preset, CB_GETCURSEL, None, None).0 };
    usize::try_from(index)
        .ok()
        .and_then(|index| state.preset_options.get(index).cloned())
        .unwrap_or_else(|| equalizer::FLAT_PRESET.to_owned())
}

/// Python `save_current_dialog_name`.
unsafe fn save_dialog_name(
    state: &DialogState,
    settings: &mut EqualizerSettings,
    preset_id: Option<&str>,
) {
    let preset_id = settings.normalized_preset(preset_id.unwrap_or(&state.visible_preset));
    settings.set_custom_name(&preset_id, &window_text(state.name));
}

/// Python `live_apply`: the dialog equalizer becomes the session equalizer
/// and is applied after `EQ_APPLY_DELAY_MS`.
unsafe fn live_apply(window: HWND, state: &mut DialogState) {
    state.host.set_session_equalizer(Some(EqualizerSession {
        enabled: true,
        gains: equalizer::normalized_gains(&state.gains),
    }));
    let _ = SetTimer(
        Some(window),
        APPLY_TIMER_ID,
        equalizer::APPLY_DELAY_MS,
        None,
    );
}

unsafe fn update_custom_name_visibility(state: &DialogState) {
    let visible = equalizer::is_custom_preset(&current_preset(state));
    let show = if visible { SW_SHOW } else { SW_HIDE };
    let _ = ShowWindow(state.name_label, show);
    let _ = ShowWindow(state.name, show);
    if let Some((_, delete)) = state
        .buttons
        .iter()
        .find(|(button, _)| *button == DialogButton::DeleteProfile)
    {
        let _ = EnableWindow(*delete, visible);
    }
    if let Ok(window) = GetParent(state.name) {
        layout(window);
    }
}

fn band_gain(gains: &EqualizerGains, index: usize) -> f64 {
    EQUALIZER_BANDS
        .get(index)
        .and_then(|band| gains.get(band.id).copied())
        .unwrap_or_default()
}

unsafe fn configure_slider(state: &DialogState, index: usize) {
    let slider = state.sliders[index];
    let limit = isize::try_from(state.db_range * 10).unwrap_or(120);
    SendMessageW(
        slider,
        TBM_SETRANGEMIN,
        Some(WPARAM(0)),
        Some(LPARAM(-limit)),
    );
    SendMessageW(
        slider,
        TBM_SETRANGEMAX,
        Some(WPARAM(1)),
        Some(LPARAM(limit)),
    );
    SendMessageW(
        slider,
        TBM_SETLINESIZE,
        None,
        Some(LPARAM(SLIDER_LINE_STEP as isize)),
    );
    SendMessageW(
        slider,
        TBM_SETPAGESIZE,
        None,
        Some(LPARAM(SLIDER_PAGE_STEP as isize)),
    );
}

/// Python `set_dialog_slider_value`: moves one slider, stores its gain and
/// refreshes its accessible value, announcing it only when `notify` is set.
unsafe fn set_slider(state: &DialogState, index: usize, tenths: i32, notify: bool) {
    let slider = state.sliders[index];
    SendMessageW(
        slider,
        TBM_SETPOS,
        Some(WPARAM(1)),
        Some(LPARAM(tenths as isize)),
    );
    annotate_band(state, index, tenths, notify);
}

unsafe fn annotate_band(state: &DialogState, index: usize, tenths: i32, notify: bool) {
    let Some(band) = EQUALIZER_BANDS.get(index) else {
        return;
    };
    crate::accessibility_win32::annotate_slider(
        state.sliders[index],
        &equalizer::band_gain_label(&state.catalog, band.id),
        &equalizer::slider_value_text(tenths),
        notify,
    );
}

fn slider_position(slider: HWND) -> i32 {
    // SAFETY: The trackbar belongs to the live dialog on this thread.
    let position = unsafe { SendMessageW(slider, TBM_GETPOS, None, None).0 };
    i32::try_from(position).unwrap_or_default()
}

/// Python `on_slider`.
unsafe fn slider_changed(window: HWND, slider: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(index) = state.sliders.iter().position(|control| *control == slider) else {
        return;
    };
    let tenths = slider_position(slider);
    let gain = equalizer::gain_from_slider(tenths);
    let previous = band_gain(&state.gains, index);
    state
        .gains
        .insert(EQUALIZER_BANDS[index].id.to_owned(), gain);
    annotate_band(state, index, tenths, (previous - gain).abs() >= 0.05);
    live_apply(window, state);
}

/// Python `load_preset_into_sliders`.
unsafe fn load_preset(window: HWND, state: &mut DialogState, preset_id: &str) {
    let settings = state.host.settings();
    state.visible_preset = settings.normalized_preset(preset_id);
    let gains = settings.gains_for_preset(preset_id);
    state.gains = equalizer::normalized_gains(&gains);
    show_gains(state, &gains);
    set_text(state.name, &settings.custom_name(preset_id));
    update_custom_name_visibility(state);
    live_apply(window, state);
}

/// Python `on_preset_changed`.
unsafe fn preset_changed(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut settings = state.host.settings();
    save_dialog_name(state, &mut settings, None);
    let _ = state.host.update_settings(&settings, false);
    let preset = current_preset(state);
    load_preset(window, state, &preset);
}

/// Python `on_range_changed` and `apply_dialog_db_range`.
unsafe fn range_changed(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let index = SendMessageW(state.range, CB_GETCURSEL, None, None).0;
    let value = usize::try_from(index)
        .ok()
        .and_then(|index| RANGE_OPTIONS.get(index))
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(state.original_db_range);
    state.db_range = value.clamp(6, 24);
    for index in 0..state.sliders.len() {
        configure_slider(state, index);
    }
    let gains = state.gains.clone();
    show_gains(state, &gains);
    live_apply(window, state);
}

/// Python `refresh_preset_choices`.
unsafe fn refresh_preset_choices(state: &mut DialogState, selected: &str) {
    let settings = state.host.settings();
    state.visible_preset = settings.normalized_preset(selected);
    fill_preset_choices(state, &settings, selected);
    update_custom_name_visibility(state);
}

/// Python `reset_dialog_equalizer`: factory values, or flat for a custom
/// profile.
unsafe fn reset_preset(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let gains = equalizer::factory_gains(&current_preset(state));
    state.gains = equalizer::normalized_gains(&gains);
    show_gains(state, &gains);
    live_apply(window, state);
}

/// Python `add_profile_from_dialog`.
unsafe fn add_profile(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let gains = equalizer::normalized_gains(&state.gains);
    let Some(preset_id) = create_profile_with_prompt(window, state.host.as_mut(), &gains) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    refresh_preset_choices(state, &preset_id);
    set_text(state.name, &state.host.settings().custom_name(&preset_id));
    live_apply(window, state);
}

/// Python `import_profile_from_dialog`.
unsafe fn import_profile(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(preset_id) = import_profile_with_prompt(window, state.host.as_mut()) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    refresh_preset_choices(state, &preset_id);
    load_preset(window, state, &preset_id);
}

/// Python `export_profile_from_dialog`.
unsafe fn export_profile(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut settings = state.host.settings();
    save_dialog_name(state, &mut settings, None);
    let _ = state.host.update_settings(&settings, false);
    let preset_id = current_preset(state);
    let name = if equalizer::is_custom_preset(&preset_id) {
        window_text(state.name).trim().to_owned()
    } else {
        settings.preset_label(&state.catalog, &preset_id)
    };
    let gains = equalizer::normalized_gains(&state.gains);
    export_profile_with_prompt(window, state.host.as_mut(), &name, &gains, &preset_id);
}

/// Python `compare_dialog_equalizer`.
unsafe fn compare(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    state.compare_showing_original = !state.compare_showing_original;
    if state.compare_showing_original {
        let _ = KillTimer(Some(window), APPLY_TIMER_ID);
        state
            .host
            .set_session_equalizer(state.original_equalizer.clone());
        state.host.apply_to_player();
        state
            .host
            .announce(state.catalog.text("equalizer_compare_original"));
        return;
    }
    live_apply(window, state);
    state
        .host
        .announce(state.catalog.text("equalizer_compare_current"));
}

/// Python `save_dialog_as_global`.
unsafe fn save_as_global(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut settings = state.host.settings();
    save_dialog_name(state, &mut settings, None);
    let _ = state.host.update_settings(&settings, false);
    let gains = equalizer::normalized_gains(&state.gains);
    let Some(preset_id) = choose_profile_for_save(window, state.host.as_mut(), &gains) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut settings = state.host.settings();
    settings.enabled = true;
    settings.preset.clone_from(&preset_id);
    if let Err(error) = state.host.update_settings(&settings, true) {
        show_message(window, &state.catalog, &error, MB_ICONERROR);
        return;
    }
    refresh_preset_choices(state, &preset_id);
    state
        .host
        .announce(state.catalog.text("equalizer_profile_saved"));
}

/// Python `save_dialog_as_device_default`.
unsafe fn save_device_default(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut settings = state.host.settings();
    save_dialog_name(state, &mut settings, None);
    let gains = equalizer::normalized_gains(&state.gains);
    let mut preset_id = current_preset(state);
    if equalizer::is_custom_preset(&preset_id) {
        settings.set_preset_gains(&preset_id, &gains);
        let _ = state.host.update_settings(&settings, false);
    } else {
        let _ = state.host.update_settings(&settings, false);
        if !equalizer::gains_match(&gains, &settings.gains_for_preset(&preset_id)) {
            let Some(chosen) = choose_profile_for_save(window, state.host.as_mut(), &gains) else {
                return;
            };
            preset_id = chosen;
        }
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    let device = state.host.device_key();
    let mut settings = state.host.settings();
    settings.enabled = true;
    settings.set_device_preset(&device, &preset_id);
    if let Err(error) = state.host.update_settings(&settings, true) {
        show_message(window, &state.catalog, &error, MB_ICONERROR);
        return;
    }
    let _ = KillTimer(Some(window), APPLY_TIMER_ID);
    state.host.set_session_equalizer(None);
    state.host.apply_to_player();
    let message = state
        .catalog
        .text("equalizer_device_profile_saved")
        .replace("{device}", &equalizer::device_display_name(&device))
        .replace(
            "{preset}",
            &settings.preset_label(&state.catalog, &preset_id),
        );
    state.host.announce(&message);
}

/// Python `clear_dialog_device_default`.
unsafe fn clear_device_default(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let device = state.host.device_key();
    let mut settings = state.host.settings();
    settings.set_device_preset(&device, "");
    if let Err(error) = state.host.update_settings(&settings, true) {
        show_message(window, &state.catalog, &error, MB_ICONERROR);
        return;
    }
    let _ = KillTimer(Some(window), APPLY_TIMER_ID);
    state.host.set_session_equalizer(None);
    state.host.apply_to_player();
    let message = state
        .catalog
        .text("equalizer_device_profile_cleared")
        .replace("{device}", &equalizer::device_display_name(&device));
    state.host.announce(&message);
}

/// Python `delete_profile_from_dialog`.
unsafe fn delete_profile(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let preset_id = current_preset(state);
    let Some(replacement) = delete_profile_with_prompt(window, state.host.as_mut(), &preset_id)
    else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    refresh_preset_choices(state, &replacement);
    load_preset(window, state, &replacement);
}

/// Python `create_equalizer_profile_dialog`: asks for a name, creates the
/// profile, makes it the global preset and saves.
pub unsafe fn create_profile_with_prompt(
    owner: HWND,
    host: &mut dyn EqualizerDialogHost,
    gains: &EqualizerGains,
) -> Option<String> {
    let catalog = host.catalog();
    let name = crate::playlist_dialog_win32::prompt_name(
        owner,
        catalog.text("add_equalizer_profile"),
        catalog.text("equalizer_profile_name"),
        catalog.text("ok"),
        catalog.text("cancel"),
    )
    .ok()
    .flatten()?;
    let mut settings = host.settings();
    let preset_id = settings.create_profile(&name, gains);
    settings.preset.clone_from(&preset_id);
    if let Err(error) = host.update_settings(&settings, true) {
        show_message(owner, &catalog, &error, MB_ICONERROR);
        return None;
    }
    host.announce(catalog.text("equalizer_profile_saved"));
    Some(preset_id)
}

/// Python `choose_equalizer_profile_for_save`: overwrite a custom profile or
/// add a new one.
pub unsafe fn choose_profile_for_save(
    owner: HWND,
    host: &mut dyn EqualizerDialogHost,
    gains: &EqualizerGains,
) -> Option<String> {
    let catalog = host.catalog();
    let settings = host.settings();
    let profile_ids = settings.custom_ids();
    let mut labels: Vec<String> = profile_ids
        .iter()
        .map(|id| settings.custom_name(id))
        .collect();
    labels.push(catalog.text("add_equalizer_profile").to_owned());
    let selection = crate::playlist_dialog_win32::choose(
        owner,
        catalog.text("equalizer"),
        catalog.text("save_equalizer_as_global"),
        &labels,
        catalog.text("ok"),
        catalog.text("cancel"),
    )
    .ok()
    .flatten()?;
    if selection == profile_ids.len() {
        return create_profile_with_prompt(owner, host, gains);
    }
    let preset_id = profile_ids.get(selection)?.clone();
    let mut settings = host.settings();
    settings.set_preset_gains(&preset_id, gains);
    if let Err(error) = host.update_settings(&settings, true) {
        show_message(owner, &catalog, &error, MB_ICONERROR);
        return None;
    }
    Some(preset_id)
}

/// Python `import_equalizer_profile_dialog`.
pub unsafe fn import_profile_with_prompt(
    owner: HWND,
    host: &mut dyn EqualizerDialogHost,
) -> Option<String> {
    let catalog = host.catalog();
    let path = crate::file_dialog_win32::choose_json_file(
        owner,
        catalog.text("import_equalizer_profile"),
        catalog.text("equalizer_profile_file"),
        catalog.text("all_files"),
    )
    .ok()
    .flatten()?;
    let mut settings = host.settings();
    let imported = std::fs::read_to_string(&path)
        .map_err(|error| error.to_string())
        .and_then(|text| {
            serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}'))
                .map_err(|error| error.to_string())
        })
        .and_then(|payload| {
            settings.profile_from_payload(&payload, catalog.text("equalizer_profile_invalid"))
        })
        .and_then(|(name, gains)| {
            let preset_id = settings.create_profile(&name, &gains);
            settings.preset.clone_from(&preset_id);
            settings.global_gains = equalizer::normalized_gains(&gains);
            host.update_settings(&settings, true)
                .map(|()| (name, preset_id))
        });
    match imported {
        Ok((name, preset_id)) => {
            host.announce(
                &catalog
                    .text("equalizer_profile_imported")
                    .replace("{name}", &name),
            );
            Some(preset_id)
        }
        Err(error) => {
            let message = catalog
                .text("equalizer_profile_import_failed")
                .replace("{error}", &error);
            show_message(owner, &catalog, &message, MB_ICONERROR);
            None
        }
    }
}

/// Python `export_equalizer_profile_dialog`.
pub unsafe fn export_profile_with_prompt(
    owner: HWND,
    host: &mut dyn EqualizerDialogHost,
    name: &str,
    gains: &EqualizerGains,
    preset_id: &str,
) {
    let catalog = host.catalog();
    let Ok(Some(path)) = crate::file_dialog_win32::save_json_file(
        owner,
        catalog.text("export_equalizer_profile"),
        &equalizer::export_file_name(name),
        catalog.text("equalizer_profile_file"),
        catalog.text("all_files"),
    ) else {
        return;
    };
    let path: PathBuf = equalizer::export_path(&path);
    let payload = host.settings().export_payload(name, gains, preset_id);
    let written = serde_json::to_string_pretty(&payload)
        .map_err(|error| error.to_string())
        .and_then(|text| std::fs::write(&path, text).map_err(|error| error.to_string()));
    match written {
        Ok(()) => host.announce(
            &catalog
                .text("equalizer_profile_exported")
                .replace("{path}", &path.to_string_lossy()),
        ),
        Err(error) => {
            let message = catalog
                .text("equalizer_profile_export_failed")
                .replace("{error}", &error);
            show_message(owner, &catalog, &message, MB_ICONERROR);
        }
    }
}

/// Python `delete_equalizer_profile(confirm=True)`: returns the flat
/// replacement preset after the user confirms.
pub unsafe fn delete_profile_with_prompt(
    owner: HWND,
    host: &mut dyn EqualizerDialogHost,
    preset_id: &str,
) -> Option<String> {
    let catalog = host.catalog();
    let mut settings = host.settings();
    if !equalizer::is_custom_preset(&settings.normalized_preset(preset_id)) {
        return None;
    }
    if show_message(
        owner,
        &catalog,
        catalog.text("equalizer_profile_delete_confirm"),
        MB_YESNO | MB_ICONQUESTION,
    ) != IDYES.0
    {
        return None;
    }
    let replacement = settings.delete_profile(preset_id)?;
    if let Err(error) = host.update_settings(&settings, true) {
        show_message(owner, &catalog, &error, MB_ICONERROR);
        return None;
    }
    host.announce(catalog.text("equalizer_profile_deleted"));
    Some(replacement)
}

/// Python `wx.MessageBox(..., self.t("equalizer"), ...)`.
unsafe fn show_message(
    owner: HWND,
    catalog: &TranslationCatalog,
    message: &str,
    style: MESSAGEBOX_STYLE,
) -> i32 {
    let message = wide(message);
    let title = wide(catalog.text("equalizer"));
    let style = if style.0 & MB_YESNO.0 == 0 {
        style | MB_OK
    } else {
        style
    };
    MessageBoxW(
        Some(owner),
        PCWSTR(message.as_ptr()),
        PCWSTR(title.as_ptr()),
        style,
    )
    .0
}

unsafe fn layout(window: HWND) {
    let Some(state) = state_ref(window) else {
        return;
    };
    let mut bounds = RECT::default();
    if GetClientRect(window, &raw mut bounds).is_err() {
        return;
    }
    let width = (bounds.right - bounds.left).max(400);
    let margin = 10;
    let inner = width - margin * 2;
    let label_height = 18;
    let mut top = margin;
    let place_pair = |label: HWND, control: HWND, height: i32, top: &mut i32| {
        let _ = MoveWindow(label, margin, *top, inner, label_height, true);
        let control_height = if height > 200 { 26 } else { height };
        let _ = MoveWindow(control, margin, *top + label_height, inner, height, true);
        *top += label_height + control_height + 6;
    };
    place_pair(state.preset_label, state.preset, 320, &mut top);
    place_pair(state.range_label, state.range, 200, &mut top);
    if equalizer::is_custom_preset(&current_preset(state)) {
        place_pair(state.name_label, state.name, 24, &mut top);
    }
    for (label, slider) in state.band_labels.iter().zip(&state.sliders) {
        place_pair(*label, *slider, 28, &mut top);
    }
    let button_width = 230;
    let button_height = 28;
    let columns = (inner / (button_width + 8)).max(1);
    for (index, (_, button)) in state.buttons.iter().enumerate() {
        let index = i32::try_from(index).unwrap_or_default();
        let _ = MoveWindow(
            *button,
            margin + (index % columns) * (button_width + 8),
            top + (index / columns) * (button_height + 6),
            button_width,
            button_height,
            true,
        );
    }
}

unsafe fn static_label(parent: HWND, instance: HINSTANCE, text: &str) -> Result<HWND> {
    create_control(
        parent,
        instance,
        w!("STATIC"),
        text,
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        0,
    )
}

unsafe fn combo(parent: HWND, instance: HINSTANCE, id: usize) -> Result<HWND> {
    create_control(
        parent,
        instance,
        w!("COMBOBOX"),
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
        WINDOW_EX_STYLE::default(),
        id,
    )
}

unsafe fn create_control(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    extended_style: WINDOW_EX_STYLE,
    id: usize,
) -> Result<HWND> {
    let text = wide(text);
    CreateWindowExW(
        extended_style,
        class,
        PCWSTR(text.as_ptr()),
        style,
        0,
        0,
        100,
        30,
        Some(parent),
        Some(HMENU(id as *mut c_void)),
        Some(instance),
        None,
    )
}

unsafe fn add_combo_string(control: HWND, text: &str) {
    let text = wide(text);
    SendMessageW(
        control,
        CB_ADDSTRING,
        None,
        Some(LPARAM(text.as_ptr() as isize)),
    );
}

unsafe fn set_text(control: HWND, text: &str) {
    let text = wide(text);
    let _ = SetWindowTextW(control, PCWSTR(text.as_ptr()));
}

unsafe fn window_text(control: HWND) -> String {
    let length = GetWindowTextLengthW(control);
    let mut value = vec![0_u16; usize::try_from(length).unwrap_or_default() + 1];
    let copied = GetWindowTextW(control, &mut value);
    String::from_utf16_lossy(&value[..usize::try_from(copied).unwrap_or_default()])
}

unsafe fn apply_font(controls: &[HWND]) {
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in controls {
        SendMessageW(
            *control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
}

unsafe fn state_ref(window: HWND) -> Option<&'static DialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const DialogState;
    pointer.as_ref()
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut DialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut DialogState;
    pointer.as_mut()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{BUTTONS, DialogButton, ID_CANCEL, ID_OK, button_from_id, button_id};

    #[test]
    fn buttons_follow_python_creation_order_and_standard_ids() {
        let order: Vec<_> = BUTTONS.iter().map(|(button, _)| *button).collect();
        assert_eq!(
            order,
            [
                DialogButton::Ok,
                DialogButton::Cancel,
                DialogButton::Reset,
                DialogButton::SaveGlobal,
                DialogButton::AddProfile,
                DialogButton::DeleteProfile,
                DialogButton::ImportProfile,
                DialogButton::ExportProfile,
                DialogButton::Compare,
                DialogButton::SaveDevice,
                DialogButton::ClearDevice,
            ]
        );
        assert_eq!(button_id(DialogButton::Ok), ID_OK);
        assert_eq!(button_id(DialogButton::Cancel), ID_CANCEL);
        for (button, _) in BUTTONS {
            assert_eq!(button_from_id(button_id(button)), Some(button));
        }
    }

    #[test]
    fn every_button_label_is_translated() {
        let catalog = apricot_app::embedded_catalog("sl");
        for (_, key) in BUTTONS {
            assert_ne!(catalog.text(key), key, "{key}");
        }
    }
}
