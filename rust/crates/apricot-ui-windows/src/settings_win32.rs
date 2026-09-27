//! Native Win32 Settings window.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use apricot_app::{
    ActionFinderContext, Application, SettingsCommand, SettingsControl, SettingsScreenModel,
    SettingsValueType, ShortcutActionItem,
};
use apricot_core::{
    SettingId, SettingsSection,
    action::{ActionScope, RepeatPolicy},
    shortcut::{ShortcutContext, action_for_shortcut},
};
use apricot_platform::{ApplicationIdentity, sync_startup_registration};
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
                HMENU, IDC_ARROW, IsDialogMessageW, LB_ADDSTRING, LB_DELETESTRING, LB_GETCURSEL,
                LB_INSERTSTRING, LB_SETCURSEL, LBN_SELCHANGE, LBS_NOTIFY, LoadCursorW,
                MB_ICONERROR, MB_ICONQUESTION, MB_OK, MB_YESNO, MSG, MessageBoxW, MoveWindow,
                PostQuitMessage, RegisterClassW, SB_VERT, SCROLLINFO, SIF_PAGE, SIF_POS, SIF_RANGE,
                SW_SHOW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW,
                ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE,
                WM_CHAR, WM_CLOSE, WM_COMMAND, WM_HSCROLL, WM_KEYDOWN, WM_NCDESTROY, WM_SETFOCUS,
                WM_SETFONT, WM_SIZE, WM_VSCROLL, WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE, WS_GROUP,
                WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
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
const CBN_SELCHANGE: usize = 1;
const BN_CLICKED: usize = 0;
const EN_KILLFOCUS: usize = 0x0200;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: usize = 1;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const TBM_GETPOS: u32 = 0x0400;
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
    ShortcutCapture(String),
    Command(SettingsCommand),
}

#[derive(Clone, Debug)]
struct BoundControl {
    label: Option<HWND>,
    control: HWND,
    binding: ControlBinding,
}

struct SettingsWindowState {
    application: *mut Application,
    section_list: HWND,
    save: HWND,
    back: HWND,
    reset_all: HWND,
    selected_section: SettingsSection,
    controls: Vec<BoundControl>,
    scroll_offset: i32,
    content_height: i32,
    deferred_action: Option<&'static str>,
    announcer: crate::announcement_win32::WindowsAnnouncer,
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

pub unsafe fn show(owner: HWND, application: &mut Application) -> Result<Option<&'static str>> {
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
        WM_CLOSE => {
            cancel_and_close(window);
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
    let section_name = wide(&model.section_list_name);
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
    for section in &model.sections {
        add_list_string(section_list, &section.label);
    }
    SendMessageW(section_list, LB_SETCURSEL, Some(WPARAM(0)), None);
    if !SetWindowSubclass(section_list, Some(section_list_proc), 1, 0).as_bool() {
        return Err(windows::core::Error::from_thread());
    }

    let back = button(
        parent,
        instance,
        &model_text(application, "back"),
        ID_BACK,
        false,
    )?;
    let save = button(
        parent,
        instance,
        &model_text(application, "save"),
        ID_SAVE,
        true,
    )?;
    let reset_all = button(
        parent,
        instance,
        &model_text(application, "reset_all_settings"),
        ID_RESET_ALL,
        false,
    )?;
    apply_font(&[section_list, back, save, reset_all]);
    Ok(SettingsWindowState {
        application,
        section_list,
        save,
        back,
        reset_all,
        selected_section: SettingsSection::General,
        controls: Vec::new(),
        scroll_offset: 0,
        content_height: 0,
        deferred_action: None,
        announcer: crate::announcement_win32::WindowsAnnouncer::new(HWND::default()),
    })
}

unsafe fn handle_shortcut_message(window: HWND, message: &MSG) -> bool {
    let Some(chord) = crate::shortcut_win32::chord_from_message(message) else {
        return false;
    };
    let Some(state) = state(window) else {
        return false;
    };
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
    let model = (&*settings_state.application).action_finder_model(ActionFinderContext {
        scope: Some(ActionScope::Dialog),
        selection_available: false,
        player_active: false,
    });
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
    (&mut *state.application).cancel_settings();
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
    let model = (&*state.application).settings_model(state.selected_section);
    let instance = HINSTANCE(GetModuleHandleW(None)?.0);
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
    layout(window);
    Ok(())
}

#[allow(clippy::too_many_lines)]
unsafe fn create_bound_control(
    parent: HWND,
    instance: HINSTANCE,
    id: usize,
    model: SettingsControl,
) -> Result<BoundControl> {
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
            let accessible_label = equalizer_slider_name(&label, value_db);
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
            set_trackbar_range(control, minimum_db * 10, maximum_db * 10);
            SendMessageW(
                control,
                TBM_SETPOS,
                Some(WPARAM(1)),
                Some(LPARAM(
                    isize::try_from(db_to_tenths(value_db)).unwrap_or_default(),
                )),
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
    };
    if let Some(label) = label {
        apply_font(&[label]);
    }
    apply_font(&[control]);
    Ok(BoundControl {
        label,
        control,
        binding,
    })
}

unsafe fn handle_command(window: HWND, wparam: WPARAM) {
    let id = wparam.0 & 0xffff;
    let notification = (wparam.0 >> 16) & 0xffff;
    match id {
        ID_BACK => cancel_and_close(window),
        ID_SAVE => save_and_close(window),
        ID_RESET_ALL => reset_all(window),
        ID_SECTION_LIST
            if notification == usize::try_from(LBN_SELCHANGE).expect("notification fits") =>
        {
            change_section(window);
        }
        _ if id >= DYNAMIC_ID_START
            && (notification == BN_CLICKED
                || notification == CBN_SELCHANGE
                || notification == EN_KILLFOCUS) =>
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
            equalizer_slider_name(label, f64::from(tenths) / 10.0)
        }
        _ => return,
    };
    set_window_text(bound.control, &name);
}

unsafe fn activate_dynamic_control(window: HWND, id: usize) {
    let index = id - DYNAMIC_ID_START;
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(bound) = state.controls.get(index).cloned() else {
        return;
    };
    if let ControlBinding::Command(command) = bound.binding {
        handle_settings_command(window, command, &window_text(bound.control));
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

unsafe fn handle_settings_command(window: HWND, command: SettingsCommand, label: &str) {
    if command == SettingsCommand::ResetSection {
        let Some(state) = state_mut(window) else {
            return;
        };
        if let Err(error) = (&mut *state.application).reset_settings_section(state.selected_section)
        {
            show_error(window, &error.to_string());
            return;
        }
        if let Err(error) = render_controls(window) {
            show_error(window, &error.to_string());
            return;
        }
        focus_first_control(window);
    } else if let Some(state) = state_mut(window) {
        // Python has every settings command. Until the Rust route exists, speak
        // the beta message and keep focus on the button.
        let catalog = apricot_app::embedded_catalog(&(*state.application).settings().language);
        let feature = label.replace('&', "");
        let message = apricot_app::unavailable_feature_message(&catalog, feature.trim());
        state.announcer.announce(&message, false);
    }
}

unsafe fn change_section(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if let Err(error) = sync_all_controls(state) {
        show_error(window, &error.to_string());
        return;
    }
    let selected = SendMessageW(state.section_list, LB_GETCURSEL, None, None).0;
    let Ok(index) = usize::try_from(selected) else {
        return;
    };
    let Some(section) = SettingsSection::ALL.get(index).copied() else {
        return;
    };
    state.selected_section = section;
    state.scroll_offset = 0;
    if let Err(error) = render_controls(window) {
        show_error(window, &error.to_string());
    }
    let _ = SetFocus(Some(state.section_list));
}

unsafe fn save_and_close(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if let Err(error) = sync_all_controls(state) {
        show_error(window, &error.to_string());
        return;
    }
    if let Err(error) = (&mut *state.application).save_settings() {
        show_error(window, &error.to_string());
        return;
    }
    let executable = match std::env::current_exe() {
        Ok(executable) => executable,
        Err(error) => {
            show_error(
                window,
                &format!("Could not locate the running executable: {error}"),
            );
            return;
        }
    };
    if let Err(error) = sync_startup_registration(
        ApplicationIdentity::RustBeta,
        &executable,
        (&*state.application).settings().start_with_windows,
    ) {
        show_error(window, &error.to_string());
        return;
    }
    let _ = DestroyWindow(window);
}

unsafe fn cancel_and_close(window: HWND) {
    if let Some(state) = state_mut(window) {
        (&mut *state.application).cancel_settings();
    }
    let _ = DestroyWindow(window);
}

unsafe fn reset_all(window: HWND) {
    let answer = MessageBoxW(
        Some(window),
        w!("Reset all ApricotPlayer settings to their defaults?"),
        w!("ApricotPlayer 2 Beta"),
        MB_YESNO | MB_ICONQUESTION,
    );
    if answer.0 != 6 {
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    (&mut *state.application).reset_all_settings();
    if let Err(error) = render_controls(window) {
        show_error(window, &error.to_string());
        return;
    }
    focus_first_control(window);
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

unsafe fn sync_bound_control(
    state: &mut SettingsWindowState,
    bound: &BoundControl,
) -> std::result::Result<(), apricot_app::SettingsControllerError> {
    let app = &mut *state.application;
    match &bound.binding {
        ControlBinding::ReadOnly
        | ControlBinding::Command(_)
        | ControlBinding::ShortcutActionList(_) => Ok(()),
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
        ControlBinding::EqualizerBand {
            preset_id, band_id, ..
        } => {
            let tenths = SendMessageW(bound.control, TBM_GETPOS, None, None).0;
            let tenths = i32::try_from(tenths).unwrap_or_default();
            app.set_equalizer_band_gain(preset_id, band_id, f64::from(tenths) / 10.0)
        }
        ControlBinding::ShortcutCapture(action_id) => {
            app.set_keyboard_shortcut(action_id, &window_text(bound.control))
        }
    }
}

unsafe extern "system" fn section_list_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    if message == WM_KEYDOWN
        && (wparam.0 == usize::from(VK_RETURN.0)
            || wparam.0 == usize::from(VK_TAB.0)
                && !GetKeyState(i32::from(VK_SHIFT.0)).is_negative())
        && let Ok(parent) = GetParent(window)
    {
        focus_first_control(parent);
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(window, Some(section_list_proc), subclass_id);
    }
    DefSubclassProc(window, message, wparam, lparam)
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
    if let Err(error) = (&mut *state.application).set_keyboard_shortcut(action_id, &shortcut) {
        show_error(parent, &error.to_string());
        let _ = SetFocus(Some(capture));
        return;
    }
    set_window_text(capture, &shortcut);
    update_shortcut_action_list(state, action_id, &shortcut);
    let _ = SetFocus(Some(capture));
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
    state.content_height = state.controls.iter().map(control_row_height).sum();
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

    let mut logical_y = 0;
    for bound in &state.controls {
        let y = top + logical_y - state.scroll_offset;
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
        logical_y += control_row_height(bound);
    }
}

fn control_window_height(binding: &ControlBinding) -> i32 {
    match binding {
        ControlBinding::Choice { .. } | ControlBinding::EqualizerDevicePreset { .. } => 420,
        ControlBinding::ShortcutActionList(_) => 260,
        _ => 30,
    }
}

fn control_row_height(bound: &BoundControl) -> i32 {
    match bound.binding {
        ControlBinding::ShortcutActionList(_) => 268,
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
    let control_top: i32 = state.controls[..index].iter().map(control_row_height).sum();
    let control_bottom = control_top + control_row_height(&state.controls[index]);
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

unsafe fn model_text(application: &Application, key: &str) -> String {
    let model = application.settings_model(SettingsSection::General);
    match key {
        "back" => apricot_app::embedded_catalog(&application.settings().language)
            .text("back")
            .to_owned(),
        "save" => apricot_app::embedded_catalog(&application.settings().language)
            .text("save")
            .to_owned(),
        "reset_all_settings" => apricot_app::embedded_catalog(&application.settings().language)
            .text("reset_all_settings")
            .to_owned(),
        _ => model.title,
    }
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

fn equalizer_slider_name(label: &str, value_db: f64) -> String {
    format!("{label}, {value_db:.1} dB")
}

#[allow(clippy::cast_possible_truncation)]
fn db_to_tenths(value_db: f64) -> i32 {
    (value_db.clamp(-24.0, 24.0) * 10.0).round() as i32
}

fn shortcut_action_label(action: &ShortcutActionItem) -> String {
    format!("{}: {}", action.label, action.shortcut)
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
    let message = wide(message);
    let _ = MessageBoxW(
        Some(window),
        PCWSTR(message.as_ptr()),
        w!("ApricotPlayer 2 Beta"),
        MB_OK | MB_ICONERROR,
    );
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
