//! Native Win32 Settings window.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use apricot_app::{
    Application, SettingsCommand, SettingsControl, SettingsScreenModel, SettingsValueType,
    ShortcutActionItem,
};
use apricot_core::{
    SettingId, SettingsSection,
    action::{ActionScope, RepeatPolicy},
    shortcut::{ShortcutContext, action_for_shortcut},
};
use apricot_platform::{
    PlatformError, sync_startup_registration,
    windows_registration::{
        media_association_registration_complete, open_default_apps_settings,
        open_default_programs_control_panel, register_media_associations,
    },
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{
                InitCommonControls, SetScrollInfo, TBM_SETPOS, TBM_SETRANGEMAX, TBM_SETRANGEMIN,
                TRACKBAR_CLASSW,
            },
            Input::KeyboardAndMouse::{
                EnableWindow, GetKeyState, SetFocus, VK_CONTROL, VK_DOWN, VK_END, VK_HOME, VK_MENU,
                VK_RETURN, VK_SHIFT, VK_TAB, VK_UP,
            },
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, CBS_DROPDOWNLIST, CW_USEDEFAULT,
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
                GetMessageW, GetParent, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
                HMENU, IDC_ARROW, IDYES, IsDialogMessageW, KillTimer, LB_ADDSTRING,
                LB_DELETESTRING, LB_GETCURSEL, LB_INSERTSTRING, LB_RESETCONTENT, LB_SETCURSEL,
                LBN_SELCHANGE, LBS_NOTIFY, LoadCursorW, MB_ICONERROR, MB_ICONINFORMATION,
                MB_ICONWARNING, MB_OK, MB_YESNO, MESSAGEBOX_STYLE, MSG, MessageBoxW, MoveWindow,
                PostMessageW, PostQuitMessage, RegisterClassW, SB_VERT, SCROLLINFO, SIF_PAGE,
                SIF_POS, SIF_RANGE, SW_SHOW, SendMessageW, SetForegroundWindow, SetTimer,
                SetWindowLongPtrW, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_HSCROLL,
                WM_KEYDOWN, WM_NCDESTROY, WM_SETFOCUS, WM_SETFONT, WM_SIZE, WM_TIMER, WM_VSCROLL,
                WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE, WS_GROUP, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
                WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const ID_SAVE: usize = 1;
const ID_BACK: usize = 2;
const ID_RESET_ALL: usize = 1203;
const ID_SECTION_LIST: usize = 1204;
const DYNAMIC_ID_START: usize = 2000;
/// Python waits 140 ms after the last arrow key in the section list before it
/// renders the selected section.
const SECTION_RENDER_TIMER_ID: usize = 1;
const SECTION_RENDER_DELAY_MS: u32 = 140;
/// Polls the background device probe started by the Playback section.
const AUDIO_DEVICE_TIMER_ID: usize = 2;
const AUDIO_DEVICE_POLL_MS: u32 = 100;
/// Python `schedule_equalizer_apply()` after an equalizer slider moves.
const EQUALIZER_APPLY_TIMER_ID: usize = 3;
const TBM_SETPAGESIZE: u32 = 0x0415;
const TBM_SETLINESIZE: u32 = 0x0417;
const CB_RESETCONTENT: u32 = 0x014B;
const CBN_SELCHANGE: usize = 1;
const BN_CLICKED: usize = 0;
const EN_KILLFOCUS: usize = 0x0200;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: usize = 1;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const CB_DELETESTRING: u32 = 0x0144;
const CB_INSERTSTRING: u32 = 0x014A;
const TBM_GETPOS: u32 = 0x0400;
const WM_GETDLGCODE_MESSAGE: u32 = 0x0087;
const DLGC_WANTMESSAGE_CODE: isize = 0x0004;
const VK_ESCAPE_CODE: usize = 0x1B;
const VK_SPACE_CODE: usize = 0x20;
const VK_DELETE_CODE: usize = 0x2E;
const VK_BACK_CODE: usize = 0x08;
const VK_INSERT_CODE: usize = 0x2D;
const VK_PRIOR_CODE: usize = 0x21;
const VK_NEXT_CODE: usize = 0x22;
const VK_LEFT_CODE: usize = 0x25;
const VK_RIGHT_CODE: usize = 0x27;
const VK_APPS_CODE: usize = 0x5D;
const VK_OEM_4_CODE: usize = 0xDB;
const VK_OEM_6_CODE: usize = 0xDD;
const VK_F1_CODE: usize = 0x70;
const VK_F24_CODE: usize = 0x87;

#[derive(Clone, Debug)]
enum ControlBinding {
    ReadOnly,
    /// A field of Settings, Recording, saved to recording.json with Save.
    RecordingField(String, RecordingFieldKind),
    Text(SettingId),
    Integer {
        setting: SettingId,
        minimum: i64,
        maximum: i64,
    },
    IntegerSlider {
        setting: SettingId,
        label: String,
        unit: &'static str,
    },
    Choice {
        setting: SettingId,
        value_type: SettingsValueType,
        values: Vec<String>,
    },
    Checkbox(SettingId),
    MenuItem(&'static str),
    EqualizerDevicePreset {
        device_id: String,
        values: Vec<String>,
    },
    EqualizerPresetName(String),
    EqualizerBand {
        preset_id: String,
        band_id: &'static str,
        label: String,
    },
    ShortcutActionList(Vec<ShortcutActionItem>),
    ShortcutCategoryList(Vec<apricot_app::SettingsChoiceOption>),
    ShortcutSearch,
    ShortcutFilter,
    ShortcutCapture(String),
    Command(SettingsCommand),
    SpotifyChoice(apricot_app::SpotifyField),
    SpotifyNormalisation,
    SpotifyListeningHistory,
    SpotifyKeyCache,
}

#[derive(Clone, Debug)]
struct BoundControl {
    label: Option<HWND>,
    control: HWND,
    binding: ControlBinding,
    /// The shared binding F1 explains (`apricot_shell::settings_help`).
    help: apricot_shell::SettingsBinding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecordingFieldKind {
    Text,
    Choice,
    Checkbox,
}

struct SettingsWindowState {
    application: *mut Application,
    /// The main window's Radio, IPTV and recording state, for Recording.
    sources: *mut apricot_shell::sources::SourcesController,
    sources_environment: Option<apricot_shell::sources::SourcesEnvironment>,
    /// Recording fields changed since the last Save.
    recording_draft: apricot_shell::FormValues,
    /// The main window, which applies equalizer changes to a running player.
    owner: HWND,
    section_list: HWND,
    save: HWND,
    back: HWND,
    reset_all: HWND,
    selected_section: SettingsSection,
    pending_section: Option<SettingsSection>,
    displayed_language: String,
    controls: Vec<BoundControl>,
    /// The group boxes of the section: the box and the first and last of
    /// its controls (2.1.1).
    groups: Vec<(HWND, usize, usize)>,
    scroll_offset: i32,
    content_height: i32,
    deferred_action: Option<&'static str>,
    announcer: crate::announcement_win32::WindowsAnnouncer,
    pending_audio_devices:
        Option<std::sync::mpsc::Receiver<Vec<apricot_playback::AudioOutputDevice>>>,
}

pub unsafe fn register() -> Result<()> {
    InitCommonControls();
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let class = WNDCLASSW {
        cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size fits in i32"),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hInstance: instance,
        lpszClassName: w!("ApricotPlayer2BetaSettingsWindow"),
        lpfnWndProc: Some(settings_window_proc),
        ..Default::default()
    };
    if RegisterClassW(&raw const class) == 0 {
        return Err(windows::core::Error::from_thread());
    }
    Ok(())
}

pub unsafe fn show(
    owner: HWND,
    application: &mut Application,
    sources: &mut apricot_shell::sources::SourcesController,
    sources_environment: Option<apricot_shell::sources::SourcesEnvironment>,
) -> Result<Option<&'static str>> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let model = application.settings_model(SettingsSection::General);
    let title = wide(&model.title);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("ApricotPlayer2BetaSettingsWindow"),
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW | WS_VSCROLL,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        1_100,
        820,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let state = match create_base_controls(window, instance, application, &model) {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let initial_focus = state.section_list;
    let mut state = state;
    state.owner = owner;
    state.sources = std::ptr::from_mut(sources);
    state.sources_environment = sources_environment;
    let state_pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), state_pointer as isize);
    if let Err(error) = render_controls(window) {
        let _ = DestroyWindow(window);
        drop(Box::from_raw(state_pointer));
        return Err(error);
    }
    layout(window);
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetFocus(Some(initial_focus));

    let mut loop_error = None;
    let mut message = MSG::default();
    while windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(window)).as_bool() {
        let result = GetMessageW(&raw mut message, None, 0, 0);
        if result.0 == -1 {
            loop_error = Some(windows::core::Error::from_thread());
            let _ = DestroyWindow(window);
            break;
        }
        if result.0 == 0 {
            let _ = DestroyWindow(window);
            PostQuitMessage(0);
            break;
        }
        if handle_shortcut_message(window, &message) {
            continue;
        }
        if crate::accessibility_win32::tab_into_list(window, &message) {
            continue;
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
    let state = Box::from_raw(state_pointer);
    if let Some(error) = loop_error {
        return Err(error);
    }
    Ok(state.deferred_action)
}

unsafe extern "system" fn settings_window_proc(
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
            handle_slider_change(window, lparam);
            LRESULT(0)
        }
        WM_VSCROLL => {
            handle_vertical_scroll(window, wparam);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == SECTION_RENDER_TIMER_ID => {
            flush_section_render(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == AUDIO_DEVICE_TIMER_ID => {
            finish_audio_device_refresh(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == EQUALIZER_APPLY_TIMER_ID => {
            let _ = KillTimer(Some(window), EQUALIZER_APPLY_TIMER_ID);
            post_equalizer_apply(window, true);
            LRESULT(0)
        }
        WM_CLOSE => {
            close_without_saving(window);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), 0);
            DefWindowProcW(window, message, wparam, lparam)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn create_base_controls(
    parent: HWND,
    instance: HINSTANCE,
    application: &mut Application,
    model: &SettingsScreenModel,
) -> Result<SettingsWindowState> {
    let catalog = apricot_app::embedded_catalog(&application.settings().language);
    // Python adds the button row before the section list, so Back, Save and
    // Restore to defaults come first in the Tab order.
    let back = button(parent, instance, catalog.text("back"), ID_BACK, false)?;
    let save = button(parent, instance, catalog.text("save"), ID_SAVE, true)?;
    let reset_all = button(
        parent,
        instance,
        catalog.text("restore_defaults"),
        ID_RESET_ALL,
        false,
    )?;
    let section_name = wide(&model.section_list_name);
    let section_list_name = model.section_list_name.clone();
    let section_list = create_control(
        parent,
        instance,
        w!("LISTBOX"),
        PCWSTR(section_name.as_ptr()),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WS_GROUP
            | WS_VSCROLL
            | WINDOW_STYLE(LBS_NOTIFY as u32),
        WS_EX_CLIENTEDGE,
        ID_SECTION_LIST,
    )?;
    // The list's own text is not its name for a screen reader.
    crate::accessibility_win32::set_control_name(section_list, &section_list_name);
    for section in &model.sections {
        add_list_string(section_list, &section.label);
    }
    SendMessageW(section_list, LB_SETCURSEL, Some(WPARAM(0)), None);
    if !SetWindowSubclass(section_list, Some(section_list_proc), 1, 0).as_bool() {
        return Err(windows::core::Error::from_thread());
    }
    apply_font(&[back, save, reset_all, section_list]);
    Ok(SettingsWindowState {
        displayed_language: application.settings().language.clone(),
        application,
        sources: std::ptr::null_mut(),
        sources_environment: None,
        recording_draft: apricot_shell::FormValues::new(),
        owner: HWND::default(),
        section_list,
        save,
        back,
        reset_all,
        selected_section: SettingsSection::General,
        pending_section: None,
        controls: Vec::new(),
        scroll_offset: 0,
        content_height: 0,
        groups: Vec::new(),
        deferred_action: None,
        announcer: crate::announcement_win32::WindowsAnnouncer::new(HWND::default()),
        pending_audio_devices: None,
    })
}

unsafe fn handle_shortcut_message(window: HWND, message: &MSG) -> bool {
    let Some(chord) = crate::shortcut_win32::chord_from_message(message) else {
        return false;
    };
    let Some(state) = state(window) else {
        return false;
    };
    // Python `on_key_down` lets the shortcut capture field record any chord
    // before global navigation runs.
    if state.controls.iter().any(|bound| {
        bound.control == message.hwnd && matches!(bound.binding, ControlBinding::ShortcutCapture(_))
    }) {
        return false;
    }
    let application = &*state.application;
    let Some(action) = action_for_shortcut(
        &application.settings().keyboard_shortcuts,
        chord,
        ShortcutContext::new(ActionScope::Dialog, true),
    ) else {
        return false;
    };
    if !action.scopes.contains(&ActionScope::Global) {
        return false;
    }
    if crate::shortcut_win32::is_repeat(message) && action.repeat == RepeatPolicy::None {
        return true;
    }
    match action.id.as_str() {
        "open_action_finder" => show_action_finder(window),
        "open_settings" => {
            let _ = SetFocus(Some(state.section_list));
        }
        action_id => defer_global_action(window, action_id),
    }
    true
}

unsafe fn show_action_finder(window: HWND) {
    let Some(settings_state) = state(window) else {
        return;
    };
    let model = (&*settings_state.application).action_finder_model();
    match crate::action_finder_win32::show(window, model) {
        Ok(Some("open_settings" | "open_action_finder") | None) => {
            if let Some(state) = state(window) {
                let _ = SetFocus(Some(state.section_list));
            }
        }
        Ok(Some(action_id)) => defer_global_action(window, action_id),
        Err(error) => show_error(window, &error.to_string()),
    }
}

unsafe fn defer_global_action(window: HWND, action_id: &'static str) {
    let Some(state) = state_mut(window) else {
        return;
    };
    state.deferred_action = Some(action_id);
    close_settings_window(window);
}

/// Closes Settings with the main window enabled first, so Windows hands the
/// activation back to the main window, not to another program (beta 8).
unsafe fn close_settings_window(window: HWND) {
    if let Some(state) = state(window) {
        let _ = EnableWindow(state.owner, true);
    }
    let _ = DestroyWindow(window);
}

unsafe fn render_controls(window: HWND) -> Result<()> {
    let Some(state) = state_mut(window) else {
        return Ok(());
    };
    for bound in state.controls.drain(..) {
        if let Some(label) = bound.label {
            let _ = DestroyWindow(label);
        }
        let _ = DestroyWindow(bound.control);
    }
    for (group, _, _) in state.groups.drain(..) {
        let _ = DestroyWindow(group);
    }
    if state.selected_section == SettingsSection::Cookies {
        // Python `configured_cookies_display_path` runs the legacy migration.
        let _ = (&mut *state.application).configured_cookies_display_path(
            &apricot_platform::browser_cookies::windows_documents_folders(),
        );
    }
    let mut model = (&*state.application).settings_model(state.selected_section);
    if state.selected_section == SettingsSection::Recording && !state.sources.is_null() {
        let catalog = settings_catalog(state);
        model.controls = apricot_shell::recording_settings_controls(
            (*state.sources).recording_form(&catalog),
            &state.recording_draft,
        );
    }
    if state.selected_section == SettingsSection::Cookies {
        let settings = (&*state.application).settings();
        model.replace_choice_options(
            SettingId::CookiesBrowserProfile,
            cookie_profile_options(settings),
        );
    }
    let instance = HINSTANCE(GetModuleHandleW(None)?.0);
    let groups = std::mem::take(&mut model.groups);
    for (index, control) in model.controls.into_iter().enumerate() {
        let id = DYNAMIC_ID_START + index;
        state
            .controls
            .push(create_bound_control(window, instance, id, control)?);
    }
    for (index, bound) in state.controls.iter().enumerate() {
        if !SetWindowSubclass(bound.control, Some(settings_control_proc), index + 1, 0).as_bool() {
            return Err(windows::core::Error::from_thread());
        }
    }
    // A group box before every control in the window order, so NVDA finds
    // the box around a control and says "<name> grouping" when the focus
    // enters the group; it takes no focus itself (2.1.1, Urh).
    for group in groups {
        if group.last >= state.controls.len() {
            continue;
        }
        let label = wide(&group.label);
        let control = create_control(
            window,
            instance,
            w!("BUTTON"),
            PCWSTR(label.as_ptr()),
            WS_CHILD
                | WS_VISIBLE
                | WINDOW_STYLE(windows::Win32::UI::WindowsAndMessaging::BS_GROUPBOX as u32),
            windows::Win32::UI::WindowsAndMessaging::WS_EX_TRANSPARENT,
            0,
        )?;
        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
            control,
            Some(windows::Win32::UI::WindowsAndMessaging::HWND_TOP),
            0,
            0,
            0,
            0,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
        );
        state.groups.push((control, group.first, group.last));
    }
    layout(window);
    // Python `refresh_audio_output_devices_async` after building the
    // Playback section.
    if state.selected_section == SettingsSection::Playback
        && state.pending_audio_devices.is_none()
        && (&*state.application).audio_device_refresh_due()
    {
        state.pending_audio_devices = Some(crate::win32::spawn_audio_device_probe());
        let _ = SetTimer(
            Some(window),
            AUDIO_DEVICE_TIMER_ID,
            AUDIO_DEVICE_POLL_MS,
            None,
        );
    }
    Ok(())
}

/// Python `finish_audio_output_device_refresh`: caches the probe and, while
/// the Playback section is still shown, replaces the device choices in place
/// without moving focus and without losing the selected value.
unsafe fn finish_audio_device_refresh(window: HWND) {
    let Some(state) = state_mut(window) else {
        let _ = KillTimer(Some(window), AUDIO_DEVICE_TIMER_ID);
        return;
    };
    let Some(pending) = state.pending_audio_devices.as_ref() else {
        let _ = KillTimer(Some(window), AUDIO_DEVICE_TIMER_ID);
        return;
    };
    let devices = match pending.try_recv() {
        Ok(devices) => devices,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
    };
    let _ = KillTimer(Some(window), AUDIO_DEVICE_TIMER_ID);
    state.pending_audio_devices = None;
    let options = (&mut *state.application).store_probed_audio_devices(&devices);
    if state.selected_section != SettingsSection::Playback || state.pending_section.is_some() {
        return;
    }
    let saved = apricot_app::audio_devices::normalized_device(
        &(&*state.application).settings().audio_output_device,
    );
    for bound in &mut state.controls {
        let ControlBinding::Choice {
            setting: SettingId::AudioOutputDevice,
            values,
            ..
        } = &mut bound.binding
        else {
            continue;
        };
        let selected_index =
            usize::try_from(SendMessageW(bound.control, CB_GETCURSEL, None, None).0).ok();
        let selected = selected_index
            .and_then(|index| values.get(index).cloned())
            .unwrap_or(saved);
        let options = apricot_app::audio_devices::refreshed_options_keeping(options, &selected);
        SendMessageW(bound.control, CB_RESETCONTENT, None, None);
        for option in &options {
            add_combo_string(bound.control, &option.label);
        }
        let position = options
            .iter()
            .position(|option| option.value == selected)
            .unwrap_or_default();
        SendMessageW(bound.control, CB_SETCURSEL, Some(WPARAM(position)), None);
        *values = options.into_iter().map(|option| option.value).collect();
        return;
    }
}

#[allow(clippy::too_many_lines)]
unsafe fn create_bound_control(
    parent: HWND,
    instance: HINSTANCE,
    id: usize,
    model: SettingsControl,
) -> Result<BoundControl> {
    let help = apricot_shell::settings_binding(&model);
    let (label, control, binding) = match model {
        SettingsControl::ReadOnlyText {
            id: _,
            label,
            value,
        } => {
            let label = static_label(parent, instance, &label)?;
            let control = edit(parent, instance, &value, id, true, false)?;
            (Some(label), control, ControlBinding::ReadOnly)
        }
        SettingsControl::FieldText {
            field,
            label,
            value,
        } => {
            let label = static_label(parent, instance, &label)?;
            let control = edit(parent, instance, &value, id, false, false)?;
            (
                Some(label),
                control,
                ControlBinding::RecordingField(field, RecordingFieldKind::Text),
            )
        }
        SettingsControl::FieldChoice {
            field,
            label,
            options,
            selected,
        } => {
            let label = static_label(parent, instance, &label)?;
            let control = create_control(
                parent,
                instance,
                w!("COMBOBOX"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
                WINDOW_EX_STYLE::default(),
                id,
            )?;
            for option in &options {
                add_combo_string(control, option);
            }
            SendMessageW(control, CB_SETCURSEL, Some(WPARAM(selected)), None);
            (
                Some(label),
                control,
                ControlBinding::RecordingField(field, RecordingFieldKind::Choice),
            )
        }
        SettingsControl::FieldCheckbox {
            field,
            label,
            checked,
        } => {
            let control = checkbox(parent, instance, &label, id, checked)?;
            (
                None,
                control,
                ControlBinding::RecordingField(field, RecordingFieldKind::Checkbox),
            )
        }
        SettingsControl::Text {
            setting,
            label,
            value,
            secret,
        } => {
            let label = static_label(parent, instance, &label)?;
            let control = edit(parent, instance, &value, id, false, secret)?;
            (Some(label), control, ControlBinding::Text(setting))
        }
        SettingsControl::Choice {
            setting,
            label,
            value,
            value_type,
            options,
        } => {
            let label = static_label(parent, instance, &label)?;
            let control = create_control(
                parent,
                instance,
                w!("COMBOBOX"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
                WINDOW_EX_STYLE::default(),
                id,
            )?;
            let mut selected = 0;
            let mut values = Vec::with_capacity(options.len());
            for (index, option) in options.into_iter().enumerate() {
                if option.value == value {
                    selected = index;
                }
                add_combo_string(control, &option.label);
                values.push(option.value);
            }
            SendMessageW(control, CB_SETCURSEL, Some(WPARAM(selected)), None);
            (
                Some(label),
                control,
                ControlBinding::Choice {
                    setting,
                    value_type,
                    values,
                },
            )
        }
        SettingsControl::Checkbox {
            setting,
            label,
            checked,
        } => {
            let control = checkbox(parent, instance, &label, id, checked)?;
            (None, control, ControlBinding::Checkbox(setting))
        }
        SettingsControl::Integer {
            setting,
            label,
            value,
            minimum,
            maximum,
        } => {
            let label = static_label(parent, instance, &label)?;
            let control = edit(parent, instance, &value.to_string(), id, false, false)?;
            (
                Some(label),
                control,
                ControlBinding::Integer {
                    setting,
                    minimum,
                    maximum,
                },
            )
        }
        SettingsControl::IntegerSlider {
            setting,
            label,
            value,
            minimum,
            maximum,
            unit,
        } => {
            let accessible_label = if unit.is_empty() {
                format!("{label}, {value}")
            } else {
                format!("{label}, {value} {unit}")
            };
            let label_control = static_label(parent, instance, &label)?;
            let accessible_label = wide(&accessible_label);
            let control = create_control(
                parent,
                instance,
                TRACKBAR_CLASSW,
                PCWSTR(accessible_label.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                WINDOW_EX_STYLE::default(),
                id,
            )?;
            set_trackbar_range(control, minimum, maximum);
            SendMessageW(
                control,
                TBM_SETPOS,
                Some(WPARAM(1)),
                Some(LPARAM(isize::try_from(value).unwrap_or_default())),
            );
            (
                Some(label_control),
                control,
                ControlBinding::IntegerSlider {
                    setting,
                    label,
                    unit,
                },
            )
        }
        SettingsControl::EqualizerDevicePresetChoice {
            device_id,
            label,
            value,
            options,
        } => {
            let label = static_label(parent, instance, &label)?;
            let control = create_control(
                parent,
                instance,
                w!("COMBOBOX"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
                WINDOW_EX_STYLE::default(),
                id,
            )?;
            let mut selected = 0;
            let mut values = Vec::with_capacity(options.len());
            for (index, option) in options.into_iter().enumerate() {
                if option.value == value {
                    selected = index;
                }
                add_combo_string(control, &option.label);
                values.push(option.value);
            }
            SendMessageW(control, CB_SETCURSEL, Some(WPARAM(selected)), None);
            (
                Some(label),
                control,
                ControlBinding::EqualizerDevicePreset { device_id, values },
            )
        }
        SettingsControl::EqualizerPresetName {
            preset_id,
            label,
            value,
        } => {
            let label = static_label(parent, instance, &label)?;
            let control = edit(parent, instance, &value, id, false, false)?;
            (
                Some(label),
                control,
                ControlBinding::EqualizerPresetName(preset_id),
            )
        }
        SettingsControl::EqualizerBandSlider {
            preset_id,
            band_id,
            label,
            value_db,
            minimum_db,
            maximum_db,
        } => {
            let label_control = static_label(parent, instance, &label)?;
            let accessible_label = wide(&label);
            let control = create_control(
                parent,
                instance,
                TRACKBAR_CLASSW,
                PCWSTR(accessible_label.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                WINDOW_EX_STYLE::default(),
                id,
            )?;
            set_trackbar_range(control, minimum_db * 10, maximum_db * 10);
            // Python `configure_equalizer_slider_steps`.
            SendMessageW(
                control,
                TBM_SETLINESIZE,
                None,
                Some(LPARAM(apricot_app::equalizer::SLIDER_LINE_STEP as isize)),
            );
            SendMessageW(
                control,
                TBM_SETPAGESIZE,
                None,
                Some(LPARAM(apricot_app::equalizer::SLIDER_PAGE_STEP as isize)),
            );
            let tenths = apricot_app::equalizer::slider_from_gain(value_db, maximum_db);
            SendMessageW(
                control,
                TBM_SETPOS,
                Some(WPARAM(1)),
                Some(LPARAM(isize::try_from(tenths).unwrap_or_default())),
            );
            crate::accessibility_win32::annotate_slider(
                control,
                &label,
                &apricot_app::equalizer::slider_value_text(tenths),
                false,
            );
            (
                Some(label_control),
                control,
                ControlBinding::EqualizerBand {
                    preset_id,
                    band_id,
                    label,
                },
            )
        }
        SettingsControl::ShortcutSearch { label } => {
            let label = static_label(parent, instance, &label)?;
            let control = edit(parent, instance, "", id, false, false)?;
            (Some(label), control, ControlBinding::ShortcutSearch)
        }
        SettingsControl::ShortcutCategoryList { label, categories } => {
            let label = static_label(parent, instance, &label)?;
            let control = create_control(
                parent,
                instance,
                w!("LISTBOX"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
                WS_EX_CLIENTEDGE,
                id,
            )?;
            for category in &categories {
                add_list_string(control, &category.label);
            }
            SendMessageW(control, LB_SETCURSEL, Some(WPARAM(0)), None);
            (
                Some(label),
                control,
                ControlBinding::ShortcutCategoryList(categories),
            )
        }
        SettingsControl::ShortcutFilter { label, options } => {
            let label = static_label(parent, instance, &label)?;
            let control = create_control(
                parent,
                instance,
                w!("COMBOBOX"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
                WINDOW_EX_STYLE::default(),
                id,
            )?;
            for option in options {
                add_combo_string(control, &option);
            }
            SendMessageW(control, CB_SETCURSEL, Some(WPARAM(0)), None);
            (Some(label), control, ControlBinding::ShortcutFilter)
        }
        SettingsControl::ShortcutActionList { label, actions } => {
            let label = static_label(parent, instance, &label)?;
            let control = create_control(
                parent,
                instance,
                w!("LISTBOX"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
                WS_EX_CLIENTEDGE,
                id,
            )?;
            for action in &actions {
                add_list_string(control, &shortcut_action_label(action));
            }
            if !actions.is_empty() {
                SendMessageW(control, LB_SETCURSEL, Some(WPARAM(0)), None);
            }
            (
                Some(label),
                control,
                ControlBinding::ShortcutActionList(actions),
            )
        }
        SettingsControl::ShortcutCapture {
            label,
            action_id,
            value,
        } => {
            let label = static_label(parent, instance, &label)?;
            let control = edit(parent, instance, &value, id, false, false)?;
            (
                Some(label),
                control,
                ControlBinding::ShortcutCapture(action_id),
            )
        }
        SettingsControl::MenuItemCheckbox {
            action_id,
            label,
            checked,
        } => {
            let control = checkbox(parent, instance, &label, id, checked)?;
            (None, control, ControlBinding::MenuItem(action_id))
        }
        SettingsControl::Command { command, label } => {
            let control = button(parent, instance, &label, id, false)?;
            (None, control, ControlBinding::Command(command))
        }
        SettingsControl::SpotifyChoice {
            field,
            label,
            selected,
            options,
        } => {
            let label = static_label(parent, instance, &label)?;
            let control = create_control(
                parent,
                instance,
                w!("COMBOBOX"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
                WINDOW_EX_STYLE::default(),
                id,
            )?;
            for option in &options {
                add_combo_string(control, option);
            }
            SendMessageW(control, CB_SETCURSEL, Some(WPARAM(selected)), None);
            (Some(label), control, ControlBinding::SpotifyChoice(field))
        }
        SettingsControl::SpotifyNormalisation { label, checked } => {
            let control = checkbox(parent, instance, &label, id, checked)?;
            (None, control, ControlBinding::SpotifyNormalisation)
        }
        SettingsControl::SpotifyListeningHistory { label, checked } => {
            let control = checkbox(parent, instance, &label, id, checked)?;
            (None, control, ControlBinding::SpotifyListeningHistory)
        }
        SettingsControl::SpotifyKeyCache { label, checked } => {
            let control = checkbox(parent, instance, &label, id, checked)?;
            (None, control, ControlBinding::SpotifyKeyCache)
        }
    };
    if let Some(label) = label {
        apply_font(&[label]);
    }
    apply_font(&[control]);
    Ok(BoundControl {
        label,
        control,
        binding,
        help,
    })
}

unsafe fn handle_command(window: HWND, wparam: WPARAM) {
    let id = wparam.0 & 0xffff;
    let notification = (wparam.0 >> 16) & 0xffff;
    match id {
        ID_BACK => close_without_saving(window),
        ID_SAVE => save_settings(window),
        ID_RESET_ALL => restore_defaults(window),
        ID_SECTION_LIST
            if notification == usize::try_from(LBN_SELCHANGE).expect("notification fits") =>
        {
            schedule_section_render(window);
        }
        _ if id >= DYNAMIC_ID_START
            && (notification == BN_CLICKED
                || notification == CBN_SELCHANGE
                || notification == EN_KILLFOCUS
                || (notification == 0x0300
                    && state(window)
                        .and_then(|state| state.controls.get(id - DYNAMIC_ID_START))
                        .is_some_and(|bound| {
                            matches!(bound.binding, ControlBinding::ShortcutSearch)
                        }))) =>
        {
            activate_dynamic_control(window, id);
        }
        _ => {}
    }
}

unsafe fn handle_slider_change(window: HWND, lparam: LPARAM) {
    if lparam.0 == 0 {
        return;
    }
    let control = HWND(lparam.0 as *mut c_void);
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(bound) = state
        .controls
        .iter()
        .find(|bound| bound.control == control)
        .cloned()
    else {
        return;
    };
    if let Err(error) = sync_bound_control(state, &bound) {
        show_error(window, &error.to_string());
        return;
    }
    update_slider_accessible_name(&bound);
    if let ControlBinding::EqualizerBand { preset_id, .. } = &bound.binding {
        equalizer_slider_live_preview(window, preset_id);
    }
}

unsafe fn update_slider_accessible_name(bound: &BoundControl) {
    let position = SendMessageW(bound.control, TBM_GETPOS, None, None).0;
    let name = match &bound.binding {
        ControlBinding::IntegerSlider { label, unit, .. } => {
            if unit.is_empty() {
                format!("{label}, {position}")
            } else {
                format!("{label}, {position} {unit}")
            }
        }
        ControlBinding::EqualizerBand { label, .. } => {
            let tenths = i32::try_from(position).unwrap_or_default();
            crate::accessibility_win32::annotate_slider(
                bound.control,
                label,
                &apricot_app::equalizer::slider_value_text(tenths),
                true,
            );
            return;
        }
        _ => return,
    };
    set_window_text(bound.control, &name);
}

/// Python `on_equalizer_settings_slider` with a running player: the global
/// equalizer turns on and the visible sliders are heard, as the custom
/// profile itself or as a preview of the edited factory preset.
unsafe fn equalizer_slider_live_preview(window: HWND, preset_id: &str) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let app = &mut *state.application;
    if !app.player_session().is_open() {
        return;
    }
    let _ = app.set_boolean_setting(SettingId::GlobalEqualizerEnabled, true);
    if apricot_app::equalizer::is_custom_preset(preset_id) {
        app.set_player_equalizer(None);
    } else {
        let gains = visible_equalizer_gains(state);
        (*state.application).set_player_equalizer(Some(apricot_app::EqualizerSession {
            enabled: true,
            gains,
        }));
    }
    let _ = SetTimer(
        Some(window),
        EQUALIZER_APPLY_TIMER_ID,
        apricot_app::equalizer::APPLY_DELAY_MS,
        None,
    );
}

/// Python `visible_equalizer_gains_from_controls`.
unsafe fn visible_equalizer_gains(
    state: &SettingsWindowState,
) -> apricot_app::equalizer::EqualizerGains {
    state
        .controls
        .iter()
        .filter_map(|bound| match &bound.binding {
            ControlBinding::EqualizerBand { band_id, .. } => {
                let tenths = SendMessageW(bound.control, TBM_GETPOS, None, None).0;
                Some((
                    (*band_id).to_owned(),
                    apricot_app::equalizer::gain_from_slider(
                        i32::try_from(tenths).unwrap_or_default(),
                    ),
                ))
            }
            _ => None,
        })
        .collect()
}

/// Asks the main window to apply the equalizer to a running player. `force`
/// also reaches a player that uses its own F4 equalizer, as Python's
/// clipping and slider handlers do.
unsafe fn post_equalizer_apply(window: HWND, force: bool) {
    let Some(state) = state(window) else {
        return;
    };
    if (*state.application).player_session().is_open() && !state.owner.0.is_null() {
        let _ = PostMessageW(
            Some(state.owner),
            crate::win32::WM_APPLY_GLOBAL_EQUALIZER,
            WPARAM(usize::from(force)),
            LPARAM(0),
        );
    }
}

/// Python's equalizer handlers that follow a settings change while a player
/// is open.
unsafe fn equalizer_setting_changed(window: HWND, binding: &ControlBinding) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let app = &mut *state.application;
    match binding {
        // `on_global_equalizer_toggle` and `on_equalizer_settings_preset_changed`.
        ControlBinding::Checkbox(SettingId::GlobalEqualizerEnabled)
        | ControlBinding::Choice {
            setting: SettingId::GlobalEqualizerPreset,
            ..
        } => {
            if app.player_session().is_open() {
                app.set_player_equalizer(None);
            }
            post_equalizer_apply(window, false);
        }
        // `on_equalizer_clipping_protection_changed`; 2.1 bass boost
        // intensity is heard at once the same way.
        ControlBinding::Checkbox(SettingId::EqualizerClippingProtection)
        | ControlBinding::Choice {
            setting: SettingId::BassBoostIntensity,
            ..
        } => {
            post_equalizer_apply(window, true);
        }
        // `on_equalizer_device_preset_changed`.
        ControlBinding::EqualizerDevicePreset { .. } => post_equalizer_apply(window, false),
        // `on_equalizer_range_changed`: the visible gains are limited to the
        // new range and saved to the visible preset.
        ControlBinding::Choice {
            setting: SettingId::EqualizerDbRange,
            ..
        } => {
            let range = f64::from(
                i32::try_from(app.settings().equalizer_db_range.clamp(6, 24)).unwrap_or(12),
            );
            let preset = app.settings().global_equalizer_preset.clone();
            let gains = visible_equalizer_gains(state)
                .into_iter()
                .map(|(band, gain)| (band, gain.clamp(-range, range)))
                .collect();
            let _ = (*state.application).set_visible_equalizer_gains(&preset, &gains);
        }
        // `on_equalizer_settings_name_changed`: the preset choice shows the
        // new name at once.
        ControlBinding::EqualizerPresetName(preset_id) => {
            let label = app.equalizer_settings().custom_name(preset_id);
            if let Some(bound) = state.controls.iter().find(|bound| {
                matches!(
                    &bound.binding,
                    ControlBinding::Choice {
                        setting: SettingId::GlobalEqualizerPreset,
                        ..
                    }
                )
            }) && let ControlBinding::Choice { values, .. } = &bound.binding
                && let Some(index) = values.iter().position(|value| value == preset_id)
            {
                replace_combo_string(bound.control, index, &label);
            }
        }
        _ => {}
    }
}

unsafe fn activate_dynamic_control(window: HWND, id: usize) {
    let index = id - DYNAMIC_ID_START;
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(bound) = state.controls.get(index).cloned() else {
        return;
    };
    if matches!(
        bound.binding,
        ControlBinding::ShortcutCategoryList(_)
            | ControlBinding::ShortcutSearch
            | ControlBinding::ShortcutFilter
    ) {
        refresh_shortcut_browser(state);
        return;
    }
    if let ControlBinding::Command(command) = bound.binding {
        handle_settings_command(window, command);
        return;
    }
    if matches!(bound.binding, ControlBinding::ShortcutActionList(_)) {
        sync_shortcut_selection(state, &bound);
        return;
    }
    if let Err(error) = sync_bound_control(state, &bound) {
        show_error(window, &error.to_string());
        return;
    }
    equalizer_setting_changed(window, &bound.binding);
    if let Some(setting) = dependent_setting(&bound.binding) {
        if let Err(error) = render_controls(window) {
            show_error(window, &error.to_string());
            return;
        }
        focus_setting(window, setting);
    }
}

fn dependent_setting(binding: &ControlBinding) -> Option<SettingId> {
    match binding {
        ControlBinding::Checkbox(
            setting @ (SettingId::GlobalEqualizerEnabled
            | SettingId::ShowAdvancedNetworkSettings
            | SettingId::VolumeBoostByDefault),
        )
        | ControlBinding::Choice {
            setting: setting @ (SettingId::GlobalEqualizerPreset | SettingId::EqualizerDbRange),
            ..
        } => Some(*setting),
        _ => None,
    }
}

unsafe fn sync_shortcut_selection(state: &mut SettingsWindowState, source: &BoundControl) {
    let ControlBinding::ShortcutActionList(actions) = &source.binding else {
        return;
    };
    let selected = SendMessageW(source.control, LB_GETCURSEL, None, None).0;
    let Ok(index) = usize::try_from(selected) else {
        return;
    };
    let Some(action) = actions.get(index) else {
        return;
    };
    for bound in &mut state.controls {
        if let ControlBinding::ShortcutCapture(action_id) = &mut bound.binding {
            action_id.clone_from(&action.action_id);
            set_window_text(bound.control, &action.shortcut);
            break;
        }
    }
}

unsafe fn shortcut_browser_filters(
    state: &SettingsWindowState,
) -> (
    String,
    String,
    apricot_app::shortcut_catalog::ShortcutFilter,
) {
    let mut category = "all".to_owned();
    let mut query = String::new();
    let mut filter = apricot_app::shortcut_catalog::ShortcutFilter::All;
    for bound in &state.controls {
        match &bound.binding {
            ControlBinding::ShortcutCategoryList(categories) => {
                let index =
                    usize::try_from(SendMessageW(bound.control, LB_GETCURSEL, None, None).0)
                        .unwrap_or(0);
                categories
                    .get(index)
                    .map_or("all", |item| &item.value)
                    .clone_into(&mut category);
            }
            ControlBinding::ShortcutSearch => query = window_text(bound.control),
            ControlBinding::ShortcutFilter => {
                filter = apricot_app::shortcut_catalog::ShortcutFilter::from_index(
                    usize::try_from(SendMessageW(bound.control, CB_GETCURSEL, None, None).0)
                        .unwrap_or(0),
                );
            }
            _ => {}
        }
    }
    (category, query, filter)
}

unsafe fn selected_shortcut(state: &SettingsWindowState) -> Option<String> {
    state
        .controls
        .iter()
        .find_map(|bound| match &bound.binding {
            ControlBinding::ShortcutCapture(id) if !id.is_empty() => Some(id.clone()),
            _ => None,
        })
}

unsafe fn refresh_shortcut_browser(state: &mut SettingsWindowState) {
    let selected = selected_shortcut(state);
    let (category, query, filter) = shortcut_browser_filters(state);
    let actions = apricot_app::shortcut_catalog::filtered_actions(
        &settings_catalog(state),
        (&*state.application).settings(),
        &category,
        &query,
        filter,
    );
    let index = selected
        .as_ref()
        .and_then(|id| actions.iter().position(|action| &action.action_id == id))
        .unwrap_or(0);
    let enabled = !actions.is_empty();
    let mut source = None;
    for bound in &mut state.controls {
        match &mut bound.binding {
            ControlBinding::ShortcutActionList(current) => {
                SendMessageW(bound.control, LB_RESETCONTENT, None, None);
                for action in &actions {
                    add_list_string(bound.control, &shortcut_action_label(action));
                }
                if enabled {
                    SendMessageW(bound.control, LB_SETCURSEL, Some(WPARAM(index)), None);
                }
                current.clone_from(&actions);
                source = Some(bound.clone());
            }
            ControlBinding::ShortcutCapture(id) => {
                let _ = EnableWindow(bound.control, enabled);
                if !enabled {
                    id.clear();
                    set_window_text(bound.control, "");
                }
            }
            _ => {}
        }
    }
    if let Some(source) = source {
        sync_shortcut_selection(state, &source);
    }
}

unsafe fn shortcut_command(window: HWND, command: SettingsCommand) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let selected = selected_shortcut(state);
    let catalog = settings_catalog(state);
    let (category, _, _) = shortcut_browser_filters(state);
    if command == SettingsCommand::ResetShortcutCategory {
        let title = wide(catalog.text("shortcut_reset_category"));
        let message = wide(catalog.text("shortcut_reset_category_confirm"));
        if MessageBoxW(
            Some(window),
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONWARNING,
        ) != IDYES
        {
            return;
        }
    }
    let app = &mut *state.application;
    let result = match (command, selected.as_deref()) {
        (SettingsCommand::ResetShortcutCategory, _) => app.reset_shortcut_category(&category),
        (SettingsCommand::ClearShortcut, Some(id)) => app.clear_keyboard_shortcut(id),
        (SettingsCommand::ResetShortcut, Some(id)) => app.set_keyboard_shortcut(id, ""),
        (SettingsCommand::EditShortcutAlternatives, Some(id)) => {
            let current = app
                .settings()
                .keyboard_shortcuts
                .get(id)
                .cloned()
                .unwrap_or_default();
            let (primary, secondary) = current.split_once('|').unwrap_or((&current, ""));
            let form = apricot_shell::FeatureForm::localized(
                &catalog,
                catalog.text("shortcut_edit_alternatives").to_owned(),
                vec![
                    apricot_shell::FormField::text(
                        "primary",
                        catalog.text("shortcut_primary"),
                        primary.trim(),
                    ),
                    apricot_shell::FormField::text(
                        "secondary",
                        catalog.text("shortcut_secondary"),
                        secondary.trim(),
                    ),
                ],
            );
            let Some(values) = crate::feature_form_win32::show(window, &form) else {
                return;
            };
            let shortcut = ["primary", "secondary"]
                .iter()
                .filter_map(|key| values.get(*key).and_then(apricot_shell::FormValue::text))
                .map(apricot_shell::canonical_shortcut_text)
                .filter(|value| !value.is_empty())
                .collect::<Vec<_>>()
                .join(" | ");
            if shortcut.is_empty() {
                app.clear_keyboard_shortcut(id)
            } else {
                app.set_keyboard_shortcut(id, &shortcut)
            }
        }
        _ => return,
    };
    if let Err(error) = result {
        show_error(window, &error.to_string());
    }
    refresh_shortcut_browser(state);
    // Focus stays on the button, so speak the changed row.
    if let Some(row) = selected.as_deref().and_then(|id| {
        state
            .controls
            .iter()
            .find_map(|bound| match &bound.binding {
                ControlBinding::ShortcutActionList(actions) => actions
                    .iter()
                    .find(|action| action.action_id == id)
                    .map(shortcut_action_label),
                _ => None,
            })
    }) {
        state.announcer.announce(&row, true);
    }
}

/// Settings > Playback > Audio processing: the same form as F5 in the player,
/// kept in the unsaved settings until Save, like every other setting.
unsafe fn edit_audio_processing(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = settings_catalog(state);
    let app = &mut *state.application;
    let settings =
        apricot_app::playback_features::AudioProcessingSettings::from_settings(app.settings())
            .unwrap_or_default();
    let form = apricot_shell::playback_features::audio_processing_form(&catalog, &settings);
    let Some(values) = crate::feature_form_win32::show(window, &form) else {
        return;
    };
    let settings =
        apricot_shell::playback_features::audio_processing_from_values(&settings, &values);
    if let Err(key) = settings.filter() {
        show_error(window, catalog.text(&key));
        return;
    }
    app.set_feature_settings("audio_processing_v1", serde_json::json!(settings));
}

/// Settings > Playback > Playback transitions, kept in the unsaved settings.
unsafe fn edit_playback_transitions(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = settings_catalog(state);
    let app = &mut *state.application;
    let settings =
        apricot_app::playback_features::AudioProcessingSettings::from_settings(app.settings())
            .unwrap_or_default();
    let form = apricot_shell::playback_features::transitions_form(&catalog, &settings);
    let Some(values) = crate::feature_form_win32::show(window, &form) else {
        return;
    };
    let settings = apricot_shell::playback_features::transitions_from_values(&settings, &values);
    app.set_feature_settings("audio_processing_v1", serde_json::json!(settings));
}

unsafe fn edit_converter_options(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = settings_catalog(state);
    let app = &mut *state.application;
    let options = apricot_app::converter::ConversionOptions::from_settings(app.settings());
    let form = apricot_shell::converter_options_form(&catalog, &options);
    let Some(values) = crate::feature_form_win32::show(window, &form) else {
        return;
    };
    let options = apricot_shell::converter_options_from_values(&options, &values);
    app.set_feature_settings(
        apricot_app::converter::CONVERTER_OPTIONS_KEY,
        serde_json::json!(options),
    );
}

unsafe fn handle_settings_command(window: HWND, command: SettingsCommand) {
    match command {
        SettingsCommand::AudioProcessing => edit_audio_processing(window),
        SettingsCommand::PlaybackTransitions => edit_playback_transitions(window),
        SettingsCommand::ConverterOptions => edit_converter_options(window),
        SettingsCommand::RecordingSettings => defer_global_action(window, "recording_settings"),
        SettingsCommand::OpenIntegrations
        | SettingsCommand::GlobalShortcuts
        | SettingsCommand::ExportSettings
        | SettingsCommand::ImportSettings
        | SettingsCommand::ExportBookmarks
        | SettingsCommand::ImportBookmarks => {
            if let Some(action) = command.deferred_action() {
                defer_global_action(window, action);
            }
        }
        SettingsCommand::ResetShortcut
        | SettingsCommand::ClearShortcut
        | SettingsCommand::ResetShortcutCategory
        | SettingsCommand::EditShortcutAlternatives => shortcut_command(window, command),
        SettingsCommand::ResetSection => reset_section(window),
        SettingsCommand::SpotifyReleases => {
            if let Some(owner) = state(window).map(|state| state.owner) {
                crate::win32::open_spotify_releases(owner, window);
            }
        }
        SettingsCommand::BrowseDownloadFolder => browse_download_folder(window),
        SettingsCommand::SetDefaultPlayer => set_default_player(window),
        // Shown only on macOS.
        SettingsCommand::ImportWindowsData => {}
        SettingsCommand::ResetEqualizerPreset => reset_equalizer_preset(window),
        SettingsCommand::AddEqualizerProfile => add_equalizer_profile(window),
        SettingsCommand::ImportEqualizerProfile => import_equalizer_profile(window),
        SettingsCommand::ExportEqualizerProfile => export_equalizer_profile(window),
        SettingsCommand::DeleteEqualizerProfile => delete_equalizer_profile(window),
        SettingsCommand::ChooseCookiesFile => choose_cookies_file(window),
        SettingsCommand::OpenYoutubeLoginProfile => open_youtube_login_profile(window),
        SettingsCommand::ExportBrowserCookies => export_browser_cookies(window),
        SettingsCommand::ObtainYoutubeApiKey => open_youtube_api_key_page(window),
        SettingsCommand::CheckYtdlpUpdates => request_update_check(window, false),
        SettingsCommand::CheckAppUpdates => request_update_check(window, true),
        SettingsCommand::AudiovaultLogin => request_audiovault(window, true),
        SettingsCommand::AudiovaultLogout => request_audiovault(window, false),
        SettingsCommand::AudiovaultRegister => crate::win32::open_audiovault_registration(),
        SettingsCommand::CheckSubscriptions => {
            // Python checks with the saved settings, without applying the
            // visible controls; the main window runs the check.
            if let Some(state) = state(window) {
                let _ = PostMessageW(
                    Some(state.owner),
                    crate::win32::WM_UPDATE_REQUEST,
                    WPARAM(crate::win32::UPDATE_REQUEST_SUBSCRIPTIONS),
                    LPARAM(0),
                );
            }
        }
    }
}

/// Python `manual_ytdlp_update_check` applies the visible controls;
/// `manual_app_update_check` also saves them. The main window runs the check
/// and speaks its result, so focus stays on the button.
unsafe fn request_update_check(window: HWND, app: bool) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if let Err(error) = sync_all_controls(state) {
        show_error(window, &error.to_string());
        return;
    }
    if app && !save_and_register_startup(window, state) {
        return;
    }
    let request = if app {
        crate::win32::UPDATE_REQUEST_APP
    } else {
        crate::win32::UPDATE_REQUEST_YTDLP
    };
    let _ = PostMessageW(
        Some(state.owner),
        crate::win32::WM_UPDATE_REQUEST,
        WPARAM(request),
        LPARAM(0),
    );
}

/// Python `login_audiovault_from_settings` applies the visible controls and
/// shows the login over the settings; `logout_audiovault` forgets the login.
/// The main window owns the `AudioVault` session and runs both.
unsafe fn request_audiovault(window: HWND, login: bool) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if login && let Err(error) = sync_all_controls(state) {
        show_error(window, &error.to_string());
        return;
    }
    let request = if login {
        crate::win32::AUDIOVAULT_REQUEST_LOGIN
    } else {
        crate::win32::AUDIOVAULT_REQUEST_LOGOUT
    };
    let _ = PostMessageW(
        Some(state.owner),
        crate::win32::WM_AUDIOVAULT_REQUEST,
        WPARAM(request),
        LPARAM(window.0 as isize),
    );
}

/// Settings side of the shared equalizer profile prompts.
struct SettingsEqualizerHost {
    window: HWND,
}

impl crate::equalizer_win32::EqualizerDialogHost for SettingsEqualizerHost {
    fn catalog(&self) -> apricot_core::TranslationCatalog {
        // SAFETY: Used on the UI thread while the settings window lives.
        unsafe { state(self.window) }.map_or_else(
            || apricot_app::embedded_catalog("en"),
            |state| unsafe { settings_catalog(state) },
        )
    }

    fn settings(&self) -> apricot_app::equalizer::EqualizerSettings {
        // SAFETY: As above.
        unsafe { state(self.window) }.map_or_else(
            || {
                apricot_app::equalizer::EqualizerSettings::from_document(
                    &apricot_storage::SettingsDocument::default(),
                )
            },
            |state| unsafe { (*state.application).equalizer_settings() },
        )
    }

    fn update_settings(
        &mut self,
        settings: &apricot_app::equalizer::EqualizerSettings,
        save: bool,
    ) -> std::result::Result<(), String> {
        // SAFETY: As above.
        let Some(state) = (unsafe { state_mut(self.window) }) else {
            return Ok(());
        };
        let application = unsafe { &mut *state.application };
        application
            .set_equalizer_settings(settings)
            .and_then(|()| {
                if save {
                    application.save_settings()
                } else {
                    Ok(())
                }
            })
            .map_err(|error| error.to_string())
    }

    fn session_equalizer(&self) -> Option<apricot_app::EqualizerSession> {
        // SAFETY: As above.
        unsafe { state(self.window) }
            .and_then(|state| unsafe { (*state.application).player_session().audio() })
            .and_then(|audio| audio.equalizer.clone())
    }

    fn set_session_equalizer(&mut self, equalizer: Option<apricot_app::EqualizerSession>) {
        // SAFETY: As above.
        if let Some(state) = unsafe { state_mut(self.window) } {
            unsafe { (*state.application).set_player_equalizer(equalizer) };
        }
    }

    fn apply_to_player(&mut self) {
        // SAFETY: As above.
        unsafe { post_equalizer_apply(self.window, false) };
    }

    fn device_key(&self) -> String {
        // SAFETY: As above.
        unsafe { state(self.window) }.map_or_else(
            || "auto".to_owned(),
            |state| unsafe { (*state.application).equalizer_device_key() },
        )
    }

    fn announce(&mut self, message: &str) {
        // SAFETY: As above.
        if let Some(state) = unsafe { state_mut(self.window) } {
            unsafe { state.announcer.announce(message, false) };
        }
    }
}

/// The preset shown in the Equalizer section (Python `visible_equalizer_preset`).
unsafe fn visible_equalizer_preset(state: &SettingsWindowState) -> String {
    let application = &*state.application;
    application
        .equalizer_settings()
        .normalized_preset(&application.settings().global_equalizer_preset)
}

/// Python `reset_visible_equalizer_controls`.
unsafe fn reset_equalizer_preset(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let preset = visible_equalizer_preset(state);
    let gains = apricot_app::equalizer::factory_gains(&preset);
    if let Err(error) = (*state.application).set_visible_equalizer_gains(&preset, &gains) {
        show_error(window, &error.to_string());
        return;
    }
    let range = (*state.application).equalizer_settings().db_range();
    for bound in &state.controls {
        if let ControlBinding::EqualizerBand { band_id, label, .. } = &bound.binding {
            let tenths = apricot_app::equalizer::slider_from_gain(
                gains.get(*band_id).copied().unwrap_or_default(),
                range,
            );
            SendMessageW(
                bound.control,
                TBM_SETPOS,
                Some(WPARAM(1)),
                Some(LPARAM(isize::try_from(tenths).unwrap_or_default())),
            );
            crate::accessibility_win32::annotate_slider(
                bound.control,
                label,
                &apricot_app::equalizer::slider_value_text(tenths),
                false,
            );
        }
    }
    let application = &mut *state.application;
    if application.player_session().is_open() {
        if apricot_app::equalizer::is_custom_preset(&preset) {
            application.set_player_equalizer(None);
        } else {
            application.set_player_equalizer(Some(apricot_app::EqualizerSession {
                enabled: true,
                gains: apricot_app::equalizer::normalized_gains(&gains),
            }));
        }
        post_equalizer_apply(window, true);
    }
    let catalog = settings_catalog(state);
    state
        .announcer
        .announce(catalog.text("equalizer_saved"), false);
}

/// Python `add_equalizer_profile_from_settings`.
unsafe fn add_equalizer_profile(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let gains = visible_equalizer_gains(state);
    let mut host = SettingsEqualizerHost { window };
    if crate::equalizer_win32::create_profile_with_prompt(window, &mut host, &gains).is_some() {
        rerender_and_focus(window, SettingId::GlobalEqualizerPreset);
    }
}

/// Python `import_equalizer_profile_from_settings`.
unsafe fn import_equalizer_profile(window: HWND) {
    let mut host = SettingsEqualizerHost { window };
    if crate::equalizer_win32::import_profile_with_prompt(window, &mut host).is_some() {
        post_equalizer_apply(window, false);
        rerender_and_focus(window, SettingId::GlobalEqualizerPreset);
    }
}

/// Python `export_visible_equalizer_profile_from_settings`.
unsafe fn export_equalizer_profile(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let preset = visible_equalizer_preset(state);
    let gains = visible_equalizer_gains(state);
    let _ = (*state.application).set_visible_equalizer_gains(&preset, &gains);
    let catalog = settings_catalog(state);
    let name = (*state.application)
        .equalizer_settings()
        .preset_label(&catalog, &preset);
    let mut host = SettingsEqualizerHost { window };
    crate::equalizer_win32::export_profile_with_prompt(window, &mut host, &name, &gains, &preset);
}

/// Python `delete_visible_equalizer_profile_from_settings`.
unsafe fn delete_equalizer_profile(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let preset = visible_equalizer_preset(state);
    let mut host = SettingsEqualizerHost { window };
    if crate::equalizer_win32::delete_profile_with_prompt(window, &mut host, &preset).is_some() {
        post_equalizer_apply(window, false);
        rerender_and_focus(window, SettingId::GlobalEqualizerPreset);
    }
}

/// Python `render_settings_section_and_focus(key)`.
unsafe fn rerender_and_focus(window: HWND, setting: SettingId) {
    if let Err(error) = render_controls(window) {
        show_error(window, &error.to_string());
        return;
    }
    focus_setting(window, setting);
}

/// Python's `reset_settings_section`: reset, save, speak, then focus the first
/// control of the section.
unsafe fn reset_section(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let section = state.selected_section;
    if let Err(error) = (&mut *state.application).reset_settings_section(section) {
        show_error(window, &error.to_string());
        return;
    }
    if section == SettingsSection::Cookies {
        crate::cookies_win32::reset_repair_suppression();
    }
    if !save_and_register_startup(window, state) {
        return;
    }
    let catalog = settings_catalog(state);
    let label = (&*state.application)
        .settings_model(section)
        .sections
        .into_iter()
        .find(|item| item.section == section)
        .map(|item| item.label)
        .unwrap_or_default();
    let message = catalog
        .text("section_settings_reset")
        .replace("{section}", &label);
    state.announcer.announce(&message, true);
    if let Err(error) = render_controls(window) {
        show_error(window, &error.to_string());
        return;
    }
    focus_first_control(window);
}

/// Python's `choose_download_folder`: the chosen folder is saved at once.
unsafe fn browse_download_folder(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = settings_catalog(state);
    let initial = std::path::PathBuf::from(&(*state.application).settings().download_folder);
    let selected = crate::folder_dialog_win32::choose_download_folder(
        window,
        catalog.text("choose_download_folder"),
        &initial,
    );
    let Some(state) = state_mut(window) else {
        return;
    };
    let browse = state
        .controls
        .iter()
        .find(|bound| {
            matches!(
                bound.binding,
                ControlBinding::Command(SettingsCommand::BrowseDownloadFolder)
            )
        })
        .map(|bound| bound.control);
    if let Some(folder) = selected {
        let folder = folder.to_string_lossy().into_owned();
        let application = &mut *state.application;
        if let Err(error) = application
            .set_string_setting(SettingId::DownloadFolder, &folder)
            .and_then(|()| application.save_settings())
        {
            show_error(window, &error.to_string());
        }
        if let Some(bound) = state.controls.iter().find(|bound| {
            matches!(
                bound.binding,
                ControlBinding::Text(SettingId::DownloadFolder)
            )
        }) {
            set_window_text(bound.control, &folder);
        }
    }
    if let Some(browse) = browse {
        let _ = SetFocus(Some(browse));
    }
}

/// Python's `open_windows_default_apps_settings`.
unsafe fn set_default_player(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = settings_catalog(state);
    let opened = std::env::current_exe()
        .map_err(|error| PlatformError::Operation(error.to_string()))
        .and_then(|executable| {
            let identity = apricot_platform::BUILD_IDENTITY;
            if !media_association_registration_complete(identity, &executable) {
                register_media_associations(identity, &executable)?;
            }
            open_default_apps_settings()
        })
        .or_else(|error| open_default_programs_control_panel().map_err(|_| error));
    match opened {
        Ok(()) => state
            .announcer
            .announce(catalog.text("default_player_settings_opened"), true),
        Err(PlatformError::Operation(error)) => {
            let message = catalog
                .text("default_player_settings_failed")
                .replace("{error}", &error);
            show_error(window, &message);
        }
    }
}

/// Python `cookie_profile_choice_values` and `cookie_profile_choice_labels`
/// for the saved browser.
fn cookie_profile_options(
    settings: &apricot_storage::SettingsDocument,
) -> Vec<apricot_app::SettingsChoiceOption> {
    let catalog = apricot_app::embedded_catalog(&settings.language);
    let mut browser = settings.cookies_from_browser.trim().to_owned();
    if browser.is_empty() {
        "none".clone_into(&mut browser);
    }
    let discovered = apricot_platform::browser_cookies::discover_cookie_profiles(&browser);
    apricot_app::cookies::cookie_profile_choice_values(
        &discovered,
        &settings.cookies_browser_profile,
    )
    .into_iter()
    .map(|value| apricot_app::SettingsChoiceOption {
        label: apricot_app::cookies::cookie_profile_choice_label(&catalog, &value),
        value,
    })
    .collect()
}

/// The bound control for a setting or command.
fn bound_control(
    state: &SettingsWindowState,
    matches: impl Fn(&ControlBinding) -> bool,
) -> Option<HWND> {
    state
        .controls
        .iter()
        .find(|bound| matches(&bound.binding))
        .map(|bound| bound.control)
}

/// Python `choose_cookies_file`.
unsafe fn choose_cookies_file(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = settings_catalog(state);
    let button = bound_control(state, |binding| {
        matches!(
            binding,
            ControlBinding::Command(SettingsCommand::ChooseCookiesFile)
        )
    });
    let chosen =
        crate::file_dialog_win32::choose_cookies_file(window, catalog.text("choose_cookies_file"));
    let restore_focus = || {
        if let Some(button) = button {
            let _ = SetFocus(Some(button));
        }
    };
    let path = match chosen {
        Ok(Some(path)) => path,
        Ok(None) => {
            restore_focus();
            return;
        }
        Err(error) => {
            show_error(window, &error);
            restore_focus();
            return;
        }
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let result = (&mut *state.application).import_cookies_file(&path);
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let message = catalog.text("cookies_file_load_failed").replace(
                "{error}",
                &apricot_app::comments::friendly_error(&catalog, &error.localized(&catalog)),
            );
            show_message(
                window,
                &message,
                apricot_platform::product_name(),
                MB_ICONWARNING,
            );
            restore_focus();
            return;
        }
    };
    crate::cookies_win32::reset_repair_suppression();
    let path_text = path.to_string_lossy().into_owned();
    show_cookie_source(state, &path_text, 0);
    state.announcer.announce(
        &catalog
            .text(result.message_key())
            .replace("{path}", &path_text),
        true,
    );
    if result.has_login {
        state
            .announcer
            .announce(catalog.text("cookies_file_login_found"), false);
    } else {
        show_message(
            window,
            catalog.text("cookies_file_no_login_warning"),
            apricot_platform::product_name(),
            MB_ICONWARNING,
        );
    }
    restore_focus();
}

/// Python sets the cookies field and the browser choice after an import or
/// an export; an import also puts the profile back on Auto.
unsafe fn show_cookie_source(state: &SettingsWindowState, path: &str, browser_index: usize) {
    for bound in &state.controls {
        match &bound.binding {
            ControlBinding::Text(SettingId::CookiesFile) => set_window_text(bound.control, path),
            ControlBinding::Choice {
                setting: SettingId::CookiesFromBrowser,
                ..
            } => {
                SendMessageW(
                    bound.control,
                    CB_SETCURSEL,
                    Some(WPARAM(browser_index)),
                    None,
                );
            }
            ControlBinding::Choice {
                setting: SettingId::CookiesBrowserProfile,
                ..
            } if browser_index == 0 => {
                SendMessageW(bound.control, CB_SETCURSEL, Some(WPARAM(0)), None);
            }
            _ => {}
        }
    }
}

/// Python `finish_browser_cookies_export` for an open Settings window.
pub(crate) unsafe fn browser_cookies_exported(window: HWND, path: &str, browser: &str) {
    if let Some(state) = state(window) {
        let index = apricot_app::cookies::COOKIES_BROWSER_OPTIONS
            .iter()
            .position(|option| *option == browser)
            .unwrap_or_default();
        show_cookie_source(state, path, index);
    }
}

/// Applies the visible controls and returns the chosen browser, or shows
/// Python's `select_cookies_browser` message.
unsafe fn selected_cookie_browser(window: HWND) -> Option<String> {
    let state = state_mut(window)?;
    if let Err(error) = sync_all_controls(state) {
        show_error(window, &error.to_string());
        return None;
    }
    let browser =
        apricot_app::cookies::normalized_cookies_browser((&*state.application).settings());
    if browser.is_empty() {
        let catalog = settings_catalog(state);
        show_message(
            window,
            catalog.text("select_cookies_browser"),
            apricot_platform::product_name(),
            MB_ICONINFORMATION,
        );
        return None;
    }
    Some(browser)
}

/// Python `export_browser_cookies_from_settings`.
unsafe fn export_browser_cookies(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = settings_catalog(state);
    if !crate::cookies_win32::ytdlp_available() {
        show_error(window, catalog.text("missing_ytdlp"));
        return;
    }
    let Some(browser) = selected_cookie_browser(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let button = bound_control(state, |binding| {
        matches!(
            binding,
            ControlBinding::Command(SettingsCommand::ExportBrowserCookies)
        )
    });
    if apricot_platform::browser_cookies::cookie_browser_is_running(&browser) {
        let message = wide(
            &catalog
                .text("close_browser_for_cookie_export_message")
                .replace("{browser}", &apricot_app::cookies::browser_title(&browser)),
        );
        let title = wide(catalog.text("close_browser_for_cookie_export_title"));
        let answer = MessageBoxW(
            Some(window),
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONWARNING,
        );
        if let Some(button) = button {
            let _ = SetFocus(Some(button));
        }
        if answer != IDYES {
            return;
        }
        if apricot_platform::browser_cookies::close_cookie_browser_processes(&browser)
            && let Some(state) = state_mut(window)
        {
            state
                .announcer
                .announce(catalog.text("browser_closed_for_cookie_export"), true);
        }
        apricot_platform::browser_cookies::wait_for_cookie_browser_exit(
            &browser,
            std::time::Duration::from_secs(6),
        );
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    state
        .announcer
        .announce(catalog.text("exporting_browser_cookies"), true);
    let application = &*state.application;
    crate::cookies_win32::spawn_export(
        state.owner,
        crate::cookies_win32::CookieExportOrigin::Settings,
        application.settings().language.clone(),
        browser,
        application.settings().cookies_browser_profile.clone(),
        application.cached_cookies_file(),
    );
}

/// Python `open_youtube_login_profile_from_settings`.
unsafe fn open_youtube_login_profile(window: HWND) {
    let Some(browser) = selected_cookie_browser(window) else {
        return;
    };
    let Some(state) = state_mut(window) else {
        return;
    };
    let catalog = settings_catalog(state);
    let profile = (&*state.application)
        .settings()
        .cookies_browser_profile
        .clone();
    match crate::cookies_win32::open_youtube_login_profile(&browser, &profile) {
        Ok(()) => state
            .announcer
            .announce(catalog.text("youtube_profile_opened"), true),
        Err(error) => show_error(
            window,
            &catalog
                .text("youtube_profile_open_failed")
                .replace("{error}", &error),
        ),
    }
}

/// Python `open_youtube_api_key_page_from_settings`.
unsafe fn open_youtube_api_key_page(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if let Err(error) = sync_all_controls(state) {
        show_error(window, &error.to_string());
        return;
    }
    let catalog = settings_catalog(state);
    match apricot_platform::windows_registration::open_web_url(
        "https://console.cloud.google.com/apis/credentials",
    ) {
        Ok(()) => state
            .announcer
            .announce(catalog.text("youtube_api_key_page_opened"), true),
        Err(PlatformError::Operation(error)) => show_error(
            window,
            &catalog
                .text("youtube_api_key_page_open_failed")
                .replace("{error}", &error),
        ),
    }
}

/// Python applies the visible controls once, then renders the newly selected
/// section 140 ms after the last selection change.
unsafe fn schedule_section_render(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let selected = SendMessageW(state.section_list, LB_GETCURSEL, None, None).0;
    let Some(section) = usize::try_from(selected)
        .ok()
        .and_then(|index| SettingsSection::ALL.get(index).copied())
    else {
        return;
    };
    if state.pending_section.is_none() {
        if section == state.selected_section {
            return;
        }
        if let Err(error) = sync_all_controls(state) {
            show_error(window, &error.to_string());
            return;
        }
    }
    state.pending_section = Some(section);
    let _ = SetTimer(
        Some(window),
        SECTION_RENDER_TIMER_ID,
        SECTION_RENDER_DELAY_MS,
        None,
    );
}

/// Renders a pending section now. Tab, Enter and the timer all end here.
unsafe fn flush_section_render(window: HWND) {
    let _ = KillTimer(Some(window), SECTION_RENDER_TIMER_ID);
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(section) = state.pending_section.take() else {
        return;
    };
    state.selected_section = section;
    state.scroll_offset = 0;
    if let Err(error) = render_controls(window) {
        show_error(window, &error.to_string());
    }
}

/// Python's `save_settings_from_ui`: save, speak "Settings saved." and stay on
/// the Settings screen. Only a language change rebuilds the screen.
unsafe fn save_settings(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if let Err(error) = sync_all_controls(state) {
        show_error(window, &error.to_string());
        return;
    }
    // recording.json first: a rejected field keeps every other change
    // unsaved, as one Save.
    if let Err(error) = save_recording_draft(state) {
        show_error(window, &error);
        return;
    }
    if !save_and_register_startup(window, state) {
        return;
    }
    let catalog = settings_catalog(state);
    state
        .announcer
        .announce(catalog.text("settings_saved"), true);
    if (*state.application).settings().language != state.displayed_language {
        rebuild_screen(window);
    }
}

/// Saves the changed Settings, Recording fields to recording.json.
unsafe fn save_recording_draft(state: &mut SettingsWindowState) -> std::result::Result<(), String> {
    if state.recording_draft.is_empty() || state.sources.is_null() {
        return Ok(());
    }
    let Some(environment) = state.sources_environment.clone() else {
        return Ok(());
    };
    let catalog = settings_catalog(state);
    let form = (*state.sources).recording_form(&catalog);
    let values = apricot_shell::recording_settings_values(&form, &state.recording_draft)?;
    (*state.sources).apply_recording_values(&environment, &values)?;
    state.recording_draft.clear();
    Ok(())
}

unsafe fn save_and_register_startup(window: HWND, state: &mut SettingsWindowState) -> bool {
    let history_before = (&*state.application)
        .saved_spotify_settings()
        .listening_history;
    let processing_before = (
        apricot_shell::playback_features::processing_chain_filter(&*state.application),
        apricot_shell::playback_features::running_transition_options(&*state.application),
    );
    if let Err(error) = (&mut *state.application).save_settings() {
        show_error(window, &error.to_string());
        return false;
    }
    if (
        apricot_shell::playback_features::processing_chain_filter(&*state.application),
        apricot_shell::playback_features::running_transition_options(&*state.application),
    ) != processing_before
        && !state.owner.0.is_null()
    {
        let _ = PostMessageW(
            Some(state.owner),
            crate::win32::WM_APPLY_AUDIO_PROCESSING,
            WPARAM(0),
            LPARAM(0),
        );
    }
    if (&*state.application)
        .saved_spotify_settings()
        .listening_history
        != history_before
        && !state.owner.0.is_null()
    {
        let _ = PostMessageW(
            Some(state.owner),
            crate::win32::WM_APPLY_SPOTIFY_HISTORY,
            WPARAM(0),
            LPARAM(0),
        );
    }
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            show_error(
                window,
                &format!("Could not locate the running executable: {error}"),
            );
            return false;
        }
    };
    if let Err(error) = sync_startup_registration(
        apricot_platform::BUILD_IDENTITY,
        &executable,
        (&*state.application).settings().start_with_windows,
    ) {
        show_error(window, &error.to_string());
        return false;
    }
    true
}

/// Python's `show_settings` after Save with a new language or after Restore to
/// defaults: every text is rebuilt and focus returns to the section list.
unsafe fn rebuild_screen(window: HWND) {
    let _ = KillTimer(Some(window), SECTION_RENDER_TIMER_ID);
    // The window holds the focus while the list is filled again, so NVDA
    // reads the selected section once (beta 8).
    let _ = SetFocus(Some(window));
    let Some(state) = state_mut(window) else {
        return;
    };
    state.pending_section = None;
    state.scroll_offset = 0;
    let application = &*state.application;
    let catalog = apricot_app::embedded_catalog(&application.settings().language);
    let model = application.settings_model(state.selected_section);
    set_window_text(window, &model.title);
    let section_name = wide(&model.section_list_name);
    let _ = SetWindowTextW(state.section_list, PCWSTR(section_name.as_ptr()));
    crate::accessibility_win32::set_control_name(state.section_list, &model.section_list_name);
    SendMessageW(state.section_list, LB_RESETCONTENT, None, None);
    for section in &model.sections {
        add_list_string(state.section_list, &section.label);
    }
    let selected = SettingsSection::ALL
        .iter()
        .position(|section| *section == state.selected_section)
        .unwrap_or_default();
    SendMessageW(
        state.section_list,
        LB_SETCURSEL,
        Some(WPARAM(selected)),
        None,
    );
    state
        .displayed_language
        .clone_from(&application.settings().language);
    let section_list = state.section_list;
    let buttons = [
        (state.back, catalog.text("back").to_owned()),
        (state.save, catalog.text("save").to_owned()),
        (state.reset_all, catalog.text("restore_defaults").to_owned()),
    ];
    if let Err(error) = render_controls(window) {
        show_error(window, &error.to_string());
    }
    crate::accessibility_win32::focus_list(section_list);
    // Renamed after the focus left them, so NVDA does not read the focused
    // Save in the new language (beta 8).
    for (button, label) in buttons {
        set_window_text(button, &label);
    }
}

/// Python's `back_from_settings`: leave without saving. The main window then
/// drops every change made since the window opened (beta 8), where Python
/// kept the sections the user had left until the next save.
unsafe fn close_without_saving(window: HWND) {
    close_settings_window(window);
}

/// Python's `restore_default_settings`: no confirmation, save at once, speak
/// "Default settings restored." and show the Settings screen again.
unsafe fn restore_defaults(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    (&mut *state.application).reset_all_settings();
    crate::cookies_win32::reset_repair_suppression();
    if !save_and_register_startup(window, state) {
        return;
    }
    let catalog = settings_catalog(state);
    state
        .announcer
        .announce(catalog.text("defaults_restored"), true);
    rebuild_screen(window);
}

unsafe fn sync_all_controls(
    state: &mut SettingsWindowState,
) -> std::result::Result<(), apricot_app::SettingsControllerError> {
    let controls = state.controls.clone();
    for bound in &controls {
        sync_bound_control(state, bound)?;
    }
    Ok(())
}

/// Keeps a Settings, Recording field for Save.
unsafe fn sync_recording_field(
    state: &mut SettingsWindowState,
    control: HWND,
    field: &str,
    kind: RecordingFieldKind,
) {
    let value = match kind {
        RecordingFieldKind::Choice => {
            usize::try_from(SendMessageW(control, CB_GETCURSEL, None, None).0)
                .ok()
                .map(apricot_shell::FormValue::Choice)
        }
        RecordingFieldKind::Checkbox => Some(apricot_shell::FormValue::Checkbox(
            checkbox_is_checked(control),
        )),
        RecordingFieldKind::Text => Some(apricot_shell::FormValue::Text(window_text(control))),
    };
    if let Some(value) = value {
        state.recording_draft.insert(field.to_owned(), value);
    }
}

#[allow(clippy::too_many_lines)] // One arm per binding.
unsafe fn sync_bound_control(
    state: &mut SettingsWindowState,
    bound: &BoundControl,
) -> std::result::Result<(), apricot_app::SettingsControllerError> {
    if let ControlBinding::RecordingField(field, kind) = &bound.binding {
        sync_recording_field(state, bound.control, field, *kind);
        return Ok(());
    }
    let app = &mut *state.application;
    match &bound.binding {
        ControlBinding::ReadOnly
        | ControlBinding::RecordingField(..)
        | ControlBinding::Command(_)
        | ControlBinding::ShortcutCategoryList(_)
        | ControlBinding::ShortcutSearch
        | ControlBinding::ShortcutFilter
        | ControlBinding::ShortcutActionList(_) => Ok(()),
        ControlBinding::Text(SettingId::CookiesFile) => {
            app.apply_cookies_path_text(&window_text(bound.control))
        }
        ControlBinding::Text(SettingId::AudiovaultEmail) => {
            app.apply_audiovault_email_text(&window_text(bound.control))
        }
        ControlBinding::Text(setting) => {
            app.set_string_setting(*setting, window_text(bound.control))
        }
        ControlBinding::Integer {
            setting,
            minimum,
            maximum,
        } => window_text(bound.control)
            .parse::<i64>()
            .map_or(Ok(()), |value| {
                app.set_integer_setting(*setting, value.clamp(*minimum, *maximum))
            }),
        ControlBinding::IntegerSlider { setting, .. } => app.set_integer_setting(
            *setting,
            i64::try_from(SendMessageW(bound.control, TBM_GETPOS, None, None).0)
                .unwrap_or_default(),
        ),
        ControlBinding::Choice {
            setting,
            value_type,
            values,
        } => {
            let selected = SendMessageW(bound.control, CB_GETCURSEL, None, None).0;
            let Ok(index) = usize::try_from(selected) else {
                return Ok(());
            };
            let Some(value) = values.get(index) else {
                return Ok(());
            };
            match value_type {
                SettingsValueType::String => app.set_string_setting(*setting, value),
                SettingsValueType::Integer => value
                    .parse::<i64>()
                    .map_or(Ok(()), |parsed| app.set_integer_setting(*setting, parsed)),
                SettingsValueType::Float => value
                    .parse::<f64>()
                    .map_or(Ok(()), |parsed| app.set_float_setting(*setting, parsed)),
            }
        }
        ControlBinding::Checkbox(setting) => {
            app.set_boolean_setting(*setting, checkbox_is_checked(bound.control))
        }
        ControlBinding::MenuItem(action_id) => {
            app.set_main_menu_item_visible(action_id, checkbox_is_checked(bound.control))
        }
        ControlBinding::EqualizerDevicePreset { device_id, values } => {
            let selected = SendMessageW(bound.control, CB_GETCURSEL, None, None).0;
            let Ok(index) = usize::try_from(selected) else {
                return Ok(());
            };
            values.get(index).map_or(Ok(()), |preset_id| {
                app.set_equalizer_device_preset(device_id, preset_id)
            })
        }
        ControlBinding::EqualizerPresetName(preset_id) => {
            app.set_equalizer_preset_name(preset_id, &window_text(bound.control))
        }
        ControlBinding::EqualizerBand { preset_id, .. } => {
            let gains = visible_equalizer_gains(state);
            (*state.application).set_visible_equalizer_gains(preset_id, &gains)
        }
        ControlBinding::ShortcutCapture(action_id) => {
            app.set_keyboard_shortcut(action_id, &window_text(bound.control))
        }
        ControlBinding::SpotifyChoice(field) => {
            let selected = SendMessageW(bound.control, CB_GETCURSEL, None, None).0;
            if let Ok(index) = usize::try_from(selected) {
                app.set_spotify_choice(*field, index);
            }
            Ok(())
        }
        ControlBinding::SpotifyNormalisation => {
            app.set_spotify_normalisation(checkbox_is_checked(bound.control));
            Ok(())
        }
        ControlBinding::SpotifyListeningHistory => {
            sync_listening_history_control(state, bound);
            Ok(())
        }
        ControlBinding::SpotifyKeyCache => {
            app.set_spotify_key_cache(checkbox_is_checked(bound.control));
            Ok(())
        }
    }
}

unsafe fn sync_listening_history_control(state: &mut SettingsWindowState, bound: &BoundControl) {
    let app = &mut *state.application;
    let checked = checkbox_is_checked(bound.control);
    if checked && !app.spotify_settings().listening_history {
        let texts = apricot_app::embedded_catalog(&app.settings().language);
        let owner = windows::Win32::UI::WindowsAndMessaging::GetParent(bound.control)
            .unwrap_or(state.owner);
        if !crate::spotify_settings_win32::confirm_listening_history(
            owner,
            texts.text("spotify_settings"),
            texts.text("spotify_listening_history_warning"),
        ) {
            SendMessageW(bound.control, BM_SETCHECK, Some(WPARAM(0)), None);
            let _ = SetFocus(Some(bound.control));
            return;
        }
    }
    app.set_spotify_listening_history(checked);
}

unsafe extern "system" fn section_list_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    // IsDialogMessageW would otherwise move focus on Tab or press Save on Enter
    // before the list sees the key.
    if message == WM_GETDLGCODE_MESSAGE
        && lparam.0 != 0
        && is_section_list_commit_key(&*(lparam.0 as *const MSG))
    {
        let code = DefSubclassProc(window, message, wparam, lparam).0;
        return LRESULT(code | DLGC_WANTMESSAGE_CODE);
    }
    if message == WM_KEYDOWN
        && is_section_list_commit_key(&MSG {
            message,
            wParam: wparam,
            ..MSG::default()
        })
        && let Ok(parent) = GetParent(window)
    {
        flush_section_render(parent);
        focus_first_control(parent);
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(window, Some(section_list_proc), subclass_id);
    }
    DefSubclassProc(window, message, wparam, lparam)
}

/// Python renders the pending section and moves into it on Tab or Enter.
unsafe fn is_section_list_commit_key(message: &MSG) -> bool {
    message.message == WM_KEYDOWN
        && (message.wParam.0 == usize::from(VK_RETURN.0)
            || message.wParam.0 == usize::from(VK_TAB.0)
                && !GetKeyState(i32::from(VK_SHIFT.0)).is_negative())
}

unsafe extern "system" fn settings_control_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    if message == WM_SETFOCUS
        && let Ok(parent) = GetParent(window)
    {
        ensure_control_visible(parent, window);
    }
    // Python captures Enter and Escape too; only Tab leaves the capture field.
    if message == WM_GETDLGCODE_MESSAGE
        && lparam.0 != 0
        && let Ok(parent) = GetParent(window)
        && let Some(state) = state(parent)
        && state.controls.iter().any(|bound| {
            bound.control == window && matches!(bound.binding, ControlBinding::ShortcutCapture(_))
        })
    {
        let pending = &*(lparam.0 as *const MSG);
        let code = DefSubclassProc(window, message, wparam, lparam).0;
        if pending.message == WM_KEYDOWN && pending.wParam.0 != usize::from(VK_TAB.0) {
            return LRESULT(code | DLGC_WANTMESSAGE_CODE);
        }
        return LRESULT(code);
    }
    // 2.1: F1 says what the setting with the focus does.
    if message == WM_KEYDOWN
        && wparam.0 == 0x70
        && let Ok(parent) = GetParent(window)
        && let Some(state) = state(parent)
        && let Some(help) = state
            .controls
            .iter()
            .find(|bound| bound.control == window)
            .and_then(|bound| apricot_shell::settings_help(&settings_catalog(state), &bound.help))
    {
        state.announcer.announce(&help, true);
        return LRESULT(0);
    }
    if (message == WM_KEYDOWN || message == WM_CHAR)
        && let Ok(parent) = GetParent(window)
        && let Some(state) = state_mut(parent)
        && let Some(action_id) = state.controls.iter().find_map(|bound| {
            (bound.control == window).then(|| match &bound.binding {
                ControlBinding::ShortcutCapture(action_id) => Some(action_id.clone()),
                _ => None,
            })?
        })
    {
        if message == WM_CHAR {
            return LRESULT(0);
        }
        if wparam.0 != usize::from(VK_TAB.0) {
            capture_shortcut(parent, state, window, &action_id, wparam.0);
            return LRESULT(0);
        }
    }
    if message == WM_KEYDOWN
        && let Ok(parent) = GetParent(window)
        && let Some(state) = state_mut(parent)
    {
        if wparam.0 == usize::from(VK_TAB.0)
            && GetKeyState(i32::from(VK_SHIFT.0)).is_negative()
            && state
                .controls
                .first()
                .is_some_and(|first| first.control == window)
        {
            let _ = SetFocus(Some(state.section_list));
            return LRESULT(0);
        }
        if matches!(
            wparam.0,
            value if value == usize::from(VK_UP.0)
                || value == usize::from(VK_DOWN.0)
                || value == usize::from(VK_HOME.0)
                || value == usize::from(VK_END.0)
        ) && focus_adjacent_menu_checkbox(state, window, wparam.0)
        {
            return LRESULT(0);
        }
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(window, Some(settings_control_proc), subclass_id);
    }
    DefSubclassProc(window, message, wparam, lparam)
}

unsafe fn capture_shortcut(
    parent: HWND,
    state: &mut SettingsWindowState,
    capture: HWND,
    action_id: &str,
    key: usize,
) {
    if matches!(
        key,
        value if value == usize::from(VK_CONTROL.0)
            || value == usize::from(VK_SHIFT.0)
            || value == usize::from(VK_MENU.0)
    ) {
        return;
    }
    if key == VK_SPACE_CODE
        && !key_is_down(VK_CONTROL)
        && !key_is_down(VK_SHIFT)
        && !key_is_down(VK_MENU)
        && action_id != "player_play_pause"
    {
        return;
    }
    let Some(shortcut) = shortcut_from_virtual_key(key) else {
        return;
    };
    let catalog = settings_catalog(state);
    let shortcut =
        match (&mut *state.application).set_primary_keyboard_shortcut(action_id, &shortcut) {
            Ok(configured) => configured,
            Err(apricot_app::SettingsControllerError::ShortcutConflict { shortcut, action }) => {
                // Python names the other action by its translated label, shows a
                // warning and then speaks the same sentence.
                let message = catalog
                    .text("shortcut_in_use")
                    .replace("{shortcut}", &shortcut)
                    .replace("{action}", &shortcut_action_name(state, &action));
                show_message(
                    parent,
                    &message,
                    catalog.text("shortcut_in_use_title"),
                    MB_ICONWARNING,
                );
                state.announcer.announce(&message, true);
                let _ = SetFocus(Some(capture));
                return;
            }
            Err(error) => {
                show_error(parent, &error.to_string());
                let _ = SetFocus(Some(capture));
                return;
            }
        };
    set_window_text(capture, &shortcut);
    update_shortcut_action_list(state, action_id, &shortcut);
    let _ = SetFocus(Some(capture));
    let message = catalog
        .text("shortcut_captured")
        .replace("{shortcut}", &shortcut);
    state.announcer.announce(&message, true);
}

fn shortcut_action_name(state: &SettingsWindowState, action_id: &str) -> String {
    state
        .controls
        .iter()
        .find_map(|bound| match &bound.binding {
            ControlBinding::ShortcutActionList(actions) => actions
                .iter()
                .find(|action| action.action_id == action_id)
                .map(|action| action.label.clone()),
            _ => None,
        })
        .unwrap_or_else(|| action_id.to_owned())
}

unsafe fn update_shortcut_action_list(
    state: &mut SettingsWindowState,
    action_id: &str,
    shortcut: &str,
) {
    for bound in &mut state.controls {
        let ControlBinding::ShortcutActionList(actions) = &mut bound.binding else {
            continue;
        };
        let Some(index) = actions
            .iter()
            .position(|action| action.action_id == action_id)
        else {
            continue;
        };
        shortcut.clone_into(&mut actions[index].shortcut);
        let label = shortcut_action_label(&actions[index]);
        SendMessageW(bound.control, LB_DELETESTRING, Some(WPARAM(index)), None);
        let label = wide(&label);
        SendMessageW(
            bound.control,
            LB_INSERTSTRING,
            Some(WPARAM(index)),
            Some(LPARAM(label.as_ptr() as isize)),
        );
        SendMessageW(bound.control, LB_SETCURSEL, Some(WPARAM(index)), None);
        break;
    }
}

fn shortcut_from_virtual_key(key: usize) -> Option<String> {
    let key_name = shortcut_key_name(key)?;
    let mut parts = Vec::with_capacity(4);
    if key_is_down(VK_CONTROL) {
        parts.push("Ctrl".to_owned());
    }
    if key_is_down(VK_SHIFT) {
        parts.push("Shift".to_owned());
    }
    if key_is_down(VK_MENU) {
        parts.push("Alt".to_owned());
    }
    parts.push(key_name);
    Some(parts.join("+"))
}

fn key_is_down(key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
    unsafe { GetKeyState(i32::from(key.0)).is_negative() }
}

fn shortcut_key_name(key: usize) -> Option<String> {
    let name = match key {
        value if value == usize::from(VK_RETURN.0) => "Enter",
        VK_SPACE_CODE => "Space",
        VK_ESCAPE_CODE => "Escape",
        VK_DELETE_CODE => "Delete",
        VK_BACK_CODE => "Backspace",
        VK_INSERT_CODE => "Insert",
        value if value == usize::from(VK_HOME.0) => "Home",
        value if value == usize::from(VK_END.0) => "End",
        VK_PRIOR_CODE => "PageUp",
        VK_NEXT_CODE => "PageDown",
        VK_LEFT_CODE => "Left",
        VK_RIGHT_CODE => "Right",
        value if value == usize::from(VK_UP.0) => "Up",
        value if value == usize::from(VK_DOWN.0) => "Down",
        VK_APPS_CODE => "Applications",
        VK_OEM_4_CODE => "LeftBracket",
        VK_OEM_6_CODE => "RightBracket",
        VK_F1_CODE..=VK_F24_CODE => return Some(format!("F{}", key - VK_F1_CODE + 1)),
        0x30..=0x39 | 0x41..=0x5A => {
            return char::from_u32(u32::try_from(key).ok()?).map(|value| value.to_string());
        }
        _ => return None,
    };
    Some(name.to_owned())
}

unsafe fn focus_adjacent_menu_checkbox(
    state: &SettingsWindowState,
    current: HWND,
    key: usize,
) -> bool {
    let menu: Vec<_> = state
        .controls
        .iter()
        .filter(|bound| matches!(bound.binding, ControlBinding::MenuItem(_)))
        .map(|bound| bound.control)
        .collect();
    let Some(current_index) = menu.iter().position(|control| *control == current) else {
        return false;
    };
    let target = if key == usize::from(VK_HOME.0) {
        0
    } else if key == usize::from(VK_END.0) {
        menu.len().saturating_sub(1)
    } else if key == usize::from(VK_UP.0) {
        current_index.saturating_sub(1)
    } else {
        (current_index + 1).min(menu.len().saturating_sub(1))
    };
    let _ = SetFocus(Some(menu[target]));
    true
}

unsafe fn focus_first_control(window: HWND) {
    if let Some(state) = state_mut(window)
        && let Some(first) = state.controls.first()
    {
        let _ = SetFocus(Some(first.control));
    }
}

unsafe fn focus_setting(window: HWND, setting: SettingId) {
    if let Some(state) = state_mut(window)
        && let Some(bound) = state
            .controls
            .iter()
            .find(|bound| binding_setting(&bound.binding) == Some(setting))
    {
        let _ = SetFocus(Some(bound.control));
    }
}

fn binding_setting(binding: &ControlBinding) -> Option<SettingId> {
    match binding {
        ControlBinding::Text(setting)
        | ControlBinding::Checkbox(setting)
        | ControlBinding::Integer { setting, .. }
        | ControlBinding::IntegerSlider { setting, .. }
        | ControlBinding::Choice { setting, .. } => Some(*setting),
        _ => None,
    }
}

unsafe fn layout(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let mut bounds = RECT::default();
    if GetClientRect(window, &raw mut bounds).is_err() {
        return;
    }
    let width = (bounds.right - bounds.left).max(700);
    let height = (bounds.bottom - bounds.top).max(500);
    let margin = 12;
    let button_height = 32;
    let _ = MoveWindow(state.back, margin, margin, 150, button_height, true);
    let _ = MoveWindow(state.save, 172, margin, 120, button_height, true);
    let _ = MoveWindow(state.reset_all, 302, margin, 190, button_height, true);
    let top = margin * 2 + button_height;
    let section_width = 220;
    let _ = MoveWindow(
        state.section_list,
        margin,
        top,
        section_width,
        height - top - margin,
        true,
    );
    let control_x = margin * 2 + section_width;
    let control_width = width - control_x - margin;
    let label_width = (control_width / 2).min(330);
    let viewport_height = (height - top - margin).max(1);
    let (tops, content_height) = row_tops(state);
    state.content_height = content_height;
    let maximum_offset = (state.content_height - viewport_height).max(0);
    state.scroll_offset = state.scroll_offset.clamp(0, maximum_offset);
    let scroll_info = SCROLLINFO {
        cbSize: u32::try_from(size_of::<SCROLLINFO>()).expect("scroll info size fits"),
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: state.content_height.saturating_sub(1),
        nPage: u32::try_from(viewport_height).unwrap_or(u32::MAX),
        nPos: state.scroll_offset,
        nTrackPos: 0,
    };
    SetScrollInfo(window, SB_VERT, &raw const scroll_info, true);

    for (index, bound) in state.controls.iter().enumerate() {
        let y = top + tops[index] - state.scroll_offset;
        let height = control_window_height(&bound.binding);
        if let Some(label) = bound.label {
            let _ = MoveWindow(label, control_x, y + 5, label_width, 24, true);
            let _ = MoveWindow(
                bound.control,
                control_x + label_width + 8,
                y,
                control_width - label_width - 8,
                height,
                true,
            );
        } else {
            let _ = MoveWindow(bound.control, control_x, y, control_width, height, true);
        }
    }
    for &(group, first, last) in &state.groups {
        let group_top = top + tops[first] - GROUP_HEADER + 4 - state.scroll_offset;
        let group_bottom =
            top + tops[last] + control_row_height(&state.controls[last]) + 2 - state.scroll_offset;
        // Scrolled up, a box stays below the buttons at the top, so it
        // never takes in Back, Save or Restore to defaults.
        let group_top = group_top.max(top - 4);
        let group_bottom = group_bottom.max(group_top);
        let _ = MoveWindow(
            group,
            control_x - 8,
            group_top,
            control_width + 12,
            group_bottom - group_top,
            true,
        );
    }
}

/// Room above a group's first control for the group's name, and below its
/// last control for the box's edge.
const GROUP_HEADER: i32 = 28;
const GROUP_FOOTER: i32 = 10;

/// The top of every control's row below the section's first row, and the
/// height of all rows, group headers included.
fn row_tops(state: &SettingsWindowState) -> (Vec<i32>, i32) {
    let mut tops = Vec::with_capacity(state.controls.len());
    let mut y = 0;
    for (index, bound) in state.controls.iter().enumerate() {
        if state.groups.iter().any(|&(_, first, _)| first == index) {
            y += GROUP_HEADER;
        }
        tops.push(y);
        y += control_row_height(bound);
        if state.groups.iter().any(|&(_, _, last)| last == index) {
            y += GROUP_FOOTER;
        }
    }
    (tops, y)
}

fn control_window_height(binding: &ControlBinding) -> i32 {
    match binding {
        ControlBinding::Choice { .. }
        | ControlBinding::EqualizerDevicePreset { .. }
        | ControlBinding::SpotifyChoice(_) => 420,
        ControlBinding::ShortcutActionList(_) | ControlBinding::ShortcutCategoryList(_) => 260,
        _ => 30,
    }
}

fn control_row_height(bound: &BoundControl) -> i32 {
    match bound.binding {
        ControlBinding::ShortcutActionList(_) | ControlBinding::ShortcutCategoryList(_) => 268,
        _ => 34,
    }
}

unsafe fn handle_vertical_scroll(window: HWND, wparam: WPARAM) {
    let mut bounds = RECT::default();
    if GetClientRect(window, &raw mut bounds).is_err() {
        return;
    }
    let viewport_height = (bounds.bottom - bounds.top - 68).max(1);
    let Some(state) = state_mut(window) else {
        return;
    };
    let maximum = (state.content_height - viewport_height).max(0);
    let command = wparam.0 & 0xffff;
    let thumb = i32::try_from((wparam.0 >> 16) & 0xffff).unwrap_or_default();
    let next = match command {
        0 => state.scroll_offset - 34,
        1 => state.scroll_offset + 34,
        2 => state.scroll_offset - viewport_height,
        3 => state.scroll_offset + viewport_height,
        4 | 5 => thumb,
        6 => 0,
        7 => maximum,
        _ => state.scroll_offset,
    };
    state.scroll_offset = next.clamp(0, maximum);
    layout(window);
}

unsafe fn ensure_control_visible(window: HWND, control: HWND) {
    let mut bounds = RECT::default();
    if GetClientRect(window, &raw mut bounds).is_err() {
        return;
    }
    let viewport_height = (bounds.bottom - bounds.top - 68).max(1);
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(index) = state
        .controls
        .iter()
        .position(|bound| bound.control == control)
    else {
        return;
    };
    let (tops, _) = row_tops(state);
    // A group's first control shows its group's name with it.
    let control_top = if state.groups.iter().any(|&(_, first, _)| first == index) {
        tops[index] - GROUP_HEADER
    } else {
        tops[index]
    };
    let control_bottom = tops[index] + control_row_height(&state.controls[index]);
    if control_top < state.scroll_offset {
        state.scroll_offset = control_top;
    } else if control_bottom > state.scroll_offset + viewport_height {
        state.scroll_offset = control_bottom - viewport_height;
    } else {
        return;
    }
    layout(window);
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut SettingsWindowState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut SettingsWindowState;
    pointer.as_mut()
}

unsafe fn state(window: HWND) -> Option<&'static SettingsWindowState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const SettingsWindowState;
    pointer.as_ref()
}

unsafe fn settings_catalog(state: &SettingsWindowState) -> apricot_core::TranslationCatalog {
    apricot_app::embedded_catalog(&(*state.application).settings().language)
}

unsafe fn button(
    parent: HWND,
    instance: HINSTANCE,
    label: &str,
    id: usize,
    default: bool,
) -> Result<HWND> {
    let label = wide(label);
    let style = if default {
        WINDOW_STYLE(BS_DEFPUSHBUTTON as u32)
    } else {
        WINDOW_STYLE::default()
    };
    create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(label.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | style,
        WINDOW_EX_STYLE::default(),
        id,
    )
}

unsafe fn checkbox(
    parent: HWND,
    instance: HINSTANCE,
    label: &str,
    id: usize,
    checked: bool,
) -> Result<HWND> {
    let label = wide(label);
    let control = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(label.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
        WINDOW_EX_STYLE::default(),
        id,
    )?;
    SendMessageW(
        control,
        BM_SETCHECK,
        Some(WPARAM(if checked { BST_CHECKED } else { 0 })),
        None,
    );
    Ok(control)
}

unsafe fn edit(
    parent: HWND,
    instance: HINSTANCE,
    value: &str,
    id: usize,
    read_only: bool,
    secret: bool,
) -> Result<HWND> {
    let value = wide(value);
    let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP;
    if read_only {
        style |= WINDOW_STYLE(0x0800);
    }
    if secret {
        style |= WINDOW_STYLE(0x0020);
    }
    create_control(
        parent,
        instance,
        w!("EDIT"),
        PCWSTR(value.as_ptr()),
        style,
        WS_EX_CLIENTEDGE,
        id,
    )
}

unsafe fn static_label(parent: HWND, instance: HINSTANCE, value: &str) -> Result<HWND> {
    let value = wide(value);
    create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(value.as_ptr()),
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        0,
    )
}

unsafe fn create_control(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    name: PCWSTR,
    style: WINDOW_STYLE,
    extended_style: WINDOW_EX_STYLE,
    id: usize,
) -> Result<HWND> {
    CreateWindowExW(
        extended_style,
        class,
        name,
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

unsafe fn add_list_string(control: HWND, value: &str) {
    let value = wide(value);
    SendMessageW(
        control,
        LB_ADDSTRING,
        None,
        Some(LPARAM(value.as_ptr() as isize)),
    );
}

unsafe fn add_combo_string(control: HWND, value: &str) {
    let value = wide(value);
    SendMessageW(
        control,
        CB_ADDSTRING,
        None,
        Some(LPARAM(value.as_ptr() as isize)),
    );
}

unsafe fn set_trackbar_range(control: HWND, minimum: i64, maximum: i64) {
    SendMessageW(
        control,
        TBM_SETRANGEMIN,
        Some(WPARAM(1)),
        Some(LPARAM(isize::try_from(minimum).unwrap_or(isize::MIN))),
    );
    SendMessageW(
        control,
        TBM_SETRANGEMAX,
        Some(WPARAM(1)),
        Some(LPARAM(isize::try_from(maximum).unwrap_or(isize::MAX))),
    );
}

unsafe fn replace_combo_string(control: HWND, index: usize, label: &str) {
    let selected = SendMessageW(control, CB_GETCURSEL, None, None).0;
    let text = wide(label);
    SendMessageW(control, CB_DELETESTRING, Some(WPARAM(index)), None);
    SendMessageW(
        control,
        CB_INSERTSTRING,
        Some(WPARAM(index)),
        Some(LPARAM(text.as_ptr() as isize)),
    );
    if let Ok(selected) = usize::try_from(selected) {
        SendMessageW(control, CB_SETCURSEL, Some(WPARAM(selected)), None);
    }
}

fn shortcut_action_label(action: &ShortcutActionItem) -> String {
    let shortcut = if action.shortcut.trim().is_empty() {
        &action.unassigned
    } else {
        &action.shortcut
    };
    format!("{}: {shortcut}. {}", action.label, action.scope)
}

unsafe fn set_window_text(control: HWND, value: &str) {
    let value = wide(value);
    let _ = SetWindowTextW(control, PCWSTR(value.as_ptr()));
}

unsafe fn checkbox_is_checked(control: HWND) -> bool {
    SendMessageW(control, BM_GETCHECK, None, None).0 == BST_CHECKED.cast_signed()
}

unsafe fn window_text(control: HWND) -> String {
    let length = GetWindowTextLengthW(control);
    let mut value = vec![0_u16; usize::try_from(length).unwrap_or_default() + 1];
    let copied = GetWindowTextW(control, &mut value);
    String::from_utf16_lossy(&value[..usize::try_from(copied).unwrap_or_default()])
}

unsafe fn apply_font(controls: &[HWND]) {
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let font_param = Some(WPARAM(font.0 as usize));
    for control in controls {
        SendMessageW(*control, WM_SETFONT, font_param, Some(LPARAM(1)));
    }
}

unsafe fn show_error(window: HWND, message: &str) {
    show_message(
        window,
        message,
        apricot_platform::product_name(),
        MB_ICONERROR,
    );
}

unsafe fn show_message(window: HWND, message: &str, title: &str, icon: MESSAGEBOX_STYLE) {
    let message = wide(message);
    let title = wide(title);
    let _ = MessageBoxW(
        Some(window),
        PCWSTR(message.as_ptr()),
        PCWSTR(title.as_ptr()),
        MB_OK | icon,
    );
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
