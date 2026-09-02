//! Native Win32 Settings window.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use apricot_app::{
    Application, SettingsCommand, SettingsControl, SettingsScreenModel, SettingsValueType,
};
use apricot_core::{SettingId, SettingsSection};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                EnableWindow, GetKeyState, SetFocus, VK_DOWN, VK_END, VK_HOME, VK_RETURN, VK_SHIFT,
                VK_TAB, VK_UP,
            },
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, CBS_DROPDOWNLIST, CW_USEDEFAULT,
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
                GetMessageW, GetParent, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
                HMENU, IDC_ARROW, IsDialogMessageW, LB_ADDSTRING, LB_GETCURSEL, LB_SETCURSEL,
                LBN_SELCHANGE, LBS_NOTIFY, LoadCursorW, MB_ICONERROR, MB_ICONINFORMATION,
                MB_ICONQUESTION, MB_OK, MB_YESNO, MSG, MessageBoxW, MoveWindow, RegisterClassW,
                SW_SHOW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow,
                TranslateMessage, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE,
                WM_COMMAND, WM_DESTROY, WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW,
                WS_CHILD, WS_EX_CLIENTEDGE, WS_GROUP, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
                WS_VSCROLL,
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

#[derive(Clone, Debug)]
enum ControlBinding {
    ReadOnly,
    Text(SettingId),
    Choice {
        setting: SettingId,
        value_type: SettingsValueType,
        values: Vec<String>,
    },
    Checkbox(SettingId),
    MenuItem(&'static str),
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
}

pub unsafe fn register() -> Result<()> {
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

pub unsafe fn show(owner: HWND, application: &mut Application) -> Result<()> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let model = application.settings_model(SettingsSection::General);
    let title = wide(&model.title);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("ApricotPlayer2BetaSettingsWindow"),
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
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
    SetWindowLongPtrW(
        window,
        WINDOW_LONG_PTR_INDEX(0),
        Box::into_raw(Box::new(state)) as isize,
    );
    render_controls(window)?;
    layout(window);
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetFocus(Some(initial_focus));

    let mut message = MSG::default();
    while windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(window)).as_bool() {
        let result = GetMessageW(&raw mut message, None, 0, 0);
        if result.0 == -1 {
            let _ = EnableWindow(owner, true);
            return Err(windows::core::Error::from_thread());
        }
        if result.0 == 0 {
            break;
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
    Ok(())
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
        WM_CLOSE => {
            cancel_and_close(window);
            LRESULT(0)
        }
        WM_DESTROY => {
            let pointer =
                GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut SettingsWindowState;
            if !pointer.is_null() {
                drop(Box::from_raw(pointer));
                SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), 0);
            }
            LRESULT(0)
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
    })
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
    if let Some(first) = state.controls.first()
        && !SetWindowSubclass(first.control, Some(settings_control_proc), 1, 0).as_bool()
    {
        return Err(windows::core::Error::from_thread());
    }
    for (index, bound) in state.controls.iter().enumerate() {
        if matches!(bound.binding, ControlBinding::MenuItem(_))
            && index != 0
            && !SetWindowSubclass(bound.control, Some(settings_control_proc), index + 1, 0)
                .as_bool()
        {
            return Err(windows::core::Error::from_thread());
        }
    }
    layout(window);
    Ok(())
}

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

unsafe fn activate_dynamic_control(window: HWND, id: usize) {
    let index = id - DYNAMIC_ID_START;
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(bound) = state.controls.get(index).cloned() else {
        return;
    };
    if let ControlBinding::Command(command) = bound.binding {
        handle_settings_command(window, command);
    } else if let Err(error) = sync_bound_control(state, &bound) {
        show_error(window, &error.to_string());
    }
}

unsafe fn handle_settings_command(window: HWND, command: SettingsCommand) {
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
    } else {
        let label = format!("{command:?} is not implemented in this internal build yet.");
        show_information(window, &label);
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
        ControlBinding::ReadOnly | ControlBinding::Command(_) => Ok(()),
        ControlBinding::Text(setting) => {
            app.set_string_setting(*setting, window_text(bound.control))
        }
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
    let row_height = 34;
    for (index, bound) in state.controls.iter().enumerate() {
        let y = top + i32::try_from(index).unwrap_or(i32::MAX / row_height) * row_height;
        if let Some(label) = bound.label {
            let _ = MoveWindow(label, control_x, y + 5, label_width, 24, true);
            let _ = MoveWindow(
                bound.control,
                control_x + label_width + 8,
                y,
                control_width - label_width - 8,
                420,
                true,
            );
        } else {
            let _ = MoveWindow(bound.control, control_x, y, control_width, 30, true);
        }
    }
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut SettingsWindowState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut SettingsWindowState;
    pointer.as_mut()
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

unsafe fn show_information(window: HWND, message: &str) {
    let message = wide(message);
    let _ = MessageBoxW(
        Some(window),
        PCWSTR(message.as_ptr()),
        w!("ApricotPlayer 2 Beta"),
        MB_OK | MB_ICONINFORMATION,
    );
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
