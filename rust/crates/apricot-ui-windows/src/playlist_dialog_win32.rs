//! Accessible modal dialogs used by user-playlist operations.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use apricot_core::shortcut::ShortcutChord;
use std::{ffi::c_void, mem::size_of};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus, VK_ESCAPE, VK_RETURN},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, ES_AUTOHSCROLL, GetClientRect, GetMessageW, GetParent,
                GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsChild,
                IsDialogMessageW, IsWindow, LB_ADDSTRING, LB_GETCURSEL, LB_SETCURSEL, LBN_DBLCLK,
                LBS_NOTIFY, LoadCursorW, MSG, MoveWindow, PostQuitMessage, RegisterClassW, SW_SHOW,
                SendMessageW, SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TranslateMessage,
                WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
                WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CHILD,
                WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const NAME_CLASS: PCWSTR = w!("ApricotPlayer2BetaPlaylistNameWindow");
const PICKER_CLASS: PCWSTR = w!("ApricotPlayer2BetaPlaylistPickerWindow");
const ID_VALUE: usize = 1701;
const ID_OK: usize = 1702;
const ID_CANCEL: usize = 1703;
/// `IsDialogMessageW` turns Enter and Escape into `IDOK` and `IDCANCEL`
/// before the focused list or edit field sees the key.
const IDOK_COMMAND: usize = 1;
const IDCANCEL_COMMAND: usize = 2;

#[derive(Default)]
pub struct PickerBehavior {
    pub initial_selection: usize,
    pub accept: Option<ShortcutChord>,
    pub back: Option<ShortcutChord>,
}

impl PickerBehavior {
    fn command(&self, chord: ShortcutChord) -> Option<usize> {
        if self.back == Some(chord) {
            Some(ID_CANCEL)
        } else if self.accept == Some(chord) {
            Some(ID_OK)
        } else {
            None
        }
    }
}

#[repr(C)]
struct DialogBase {
    previous_focus: HWND,
    prompt: HWND,
    value: HWND,
    ok: HWND,
    cancel: HWND,
}

#[repr(C)]
struct DialogState<T> {
    base: DialogBase,
    result: Option<T>,
    behavior: PickerBehavior,
}

pub fn register() -> Result<()> {
    // SAFETY: Window classes are registered once before the main message loop.
    unsafe {
        register_class(NAME_CLASS, Some(name_window_proc))?;
        register_class(PICKER_CLASS, Some(picker_window_proc))?;
    }
    Ok(())
}

/// Prompts for a playlist name with the blank edit control focused first.
///
/// # Errors
///
/// Returns a Win32 error when the modal window or a child control cannot be created.
pub fn prompt_name(
    owner: HWND,
    title: &str,
    prompt: &str,
    ok_label: &str,
    cancel_label: &str,
) -> Result<Option<String>> {
    prompt_name_with_initial(owner, title, prompt, "", ok_label, cancel_label)
}

/// Prompts for a name with a caller-provided initial edit value.
///
/// # Errors
///
/// Returns a Win32 error when the modal window or a child control cannot be created.
pub fn prompt_name_with_initial(
    owner: HWND,
    title: &str,
    prompt: &str,
    initial_value: &str,
    ok_label: &str,
    cancel_label: &str,
) -> Result<Option<String>> {
    // SAFETY: The nested modal loop owns its state and disables its owner until
    // the state allocation has been recovered.
    unsafe { prompt_name_win32(owner, title, prompt, initial_value, ok_label, cancel_label) }
}

/// Lets the user choose one playlist with the list focused first.
///
/// # Errors
///
/// Returns a Win32 error when the modal window or a child control cannot be created.
pub fn choose(
    owner: HWND,
    title: &str,
    prompt: &str,
    choices: &[String],
    ok_label: &str,
    cancel_label: &str,
) -> Result<Option<usize>> {
    // SAFETY: The nested modal loop owns its state and disables its owner until
    // the state allocation has been recovered.
    unsafe {
        choose_win32(
            owner,
            title,
            prompt,
            choices,
            PickerBehavior::default(),
            ok_label,
            cancel_label,
        )
    }
}

/// Lets the user choose one item with a caller-provided initial selection.
///
/// # Errors
///
/// Returns a Win32 error when the modal window or a child control cannot be created.
pub fn choose_with_initial(
    owner: HWND,
    title: &str,
    prompt: &str,
    choices: &[String],
    initial_selection: usize,
    ok_label: &str,
    cancel_label: &str,
) -> Result<Option<usize>> {
    choose_configured(
        owner,
        title,
        prompt,
        choices,
        PickerBehavior {
            initial_selection,
            ..Default::default()
        },
        ok_label,
        cancel_label,
    )
}

/// Opens a picker honoring its caller's configured accept/back shortcuts.
///
/// # Errors
/// Returns a Win32 error if the dialog cannot be created.
pub fn choose_configured(
    owner: HWND,
    title: &str,
    prompt: &str,
    choices: &[String],
    behavior: PickerBehavior,
    ok_label: &str,
    cancel_label: &str,
) -> Result<Option<usize>> {
    // SAFETY: The nested modal loop owns its state and disables its owner until
    // the state allocation has been recovered.
    unsafe {
        choose_win32(
            owner,
            title,
            prompt,
            choices,
            behavior,
            ok_label,
            cancel_label,
        )
    }
}

unsafe fn register_class(
    name: PCWSTR,
    procedure: windows::Win32::UI::WindowsAndMessaging::WNDPROC,
) -> Result<()> {
    let module = GetModuleHandleW(None)?;
    let class = WNDCLASSW {
        cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size fits in i32"),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hInstance: HINSTANCE(module.0),
        lpszClassName: name,
        lpfnWndProc: procedure,
        ..Default::default()
    };
    if RegisterClassW(&raw const class) == 0 {
        return Err(windows::core::Error::from_thread());
    }
    Ok(())
}

unsafe fn prompt_name_win32(
    owner: HWND,
    title: &str,
    prompt: &str,
    initial_value: &str,
    ok_label: &str,
    cancel_label: &str,
) -> Result<Option<String>> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let title = wide(title);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        NAME_CLASS,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        520,
        210,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let state = create_common_controls::<String>(
        window,
        instance,
        prompt,
        w!("EDIT"),
        true,
        ok_label,
        cancel_label,
    )?;
    if !SetWindowSubclass(state.base.value, Some(name_value_proc), 1, 0).as_bool() {
        let _ = DestroyWindow(window);
        return Err(windows::core::Error::from_thread());
    }
    let initial_value = wide(initial_value);
    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(
        state.base.value,
        PCWSTR(initial_value.as_ptr()),
    );
    run_modal(window, owner, state, |state| state.result)
}

unsafe fn choose_win32(
    owner: HWND,
    title: &str,
    prompt: &str,
    choices: &[String],
    behavior: PickerBehavior,
    ok_label: &str,
    cancel_label: &str,
) -> Result<Option<usize>> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let title = wide(title);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        PICKER_CLASS,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        540,
        470,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let mut state = create_common_controls::<usize>(
        window,
        instance,
        prompt,
        w!("LISTBOX"),
        false,
        ok_label,
        cancel_label,
    )?;
    let initial_selection = behavior.initial_selection;
    state.behavior = behavior;
    for choice in choices {
        let choice = wide(choice);
        SendMessageW(
            state.base.value,
            LB_ADDSTRING,
            None,
            Some(LPARAM(choice.as_ptr() as isize)),
        );
    }
    if !choices.is_empty() {
        SendMessageW(
            state.base.value,
            LB_SETCURSEL,
            Some(WPARAM(initial_selection.min(choices.len() - 1))),
            None,
        );
    }
    if !SetWindowSubclass(state.base.value, Some(picker_value_proc), 1, 0).as_bool() {
        let _ = DestroyWindow(window);
        return Err(windows::core::Error::from_thread());
    }
    run_modal(window, owner, state, |state| state.result)
}

unsafe fn create_common_controls<T>(
    parent: HWND,
    instance: HINSTANCE,
    prompt: &str,
    value_class: PCWSTR,
    is_edit: bool,
    ok_label: &str,
    cancel_label: &str,
) -> Result<DialogState<T>> {
    let prompt_text = wide(prompt);
    let prompt_control = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(prompt_text.as_ptr()),
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let value_style = if is_edit {
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32)
    } else {
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32)
    };
    let value = create_control(
        parent,
        instance,
        value_class,
        PCWSTR(prompt_text.as_ptr()),
        value_style,
        WS_EX_CLIENTEDGE,
        ID_VALUE,
    )?;
    if is_edit {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(value, w!(""));
    }
    let ok_text = wide(ok_label);
    let ok = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(ok_text.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE::default(),
        ID_OK,
    )?;
    let cancel_text = wide(cancel_label);
    let cancel = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(cancel_text.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_CANCEL,
    )?;
    apply_font(&[prompt_control, value, ok, cancel]);
    Ok(DialogState {
        base: DialogBase {
            previous_focus: GetFocus(),
            prompt: prompt_control,
            value,
            ok,
            cancel,
        },
        result: None,
        behavior: PickerBehavior::default(),
    })
}

unsafe fn run_modal<T, R>(
    window: HWND,
    owner: HWND,
    state: DialogState<T>,
    take_result: impl FnOnce(DialogState<T>) -> R,
) -> Result<R> {
    let initial_focus = state.base.value;
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
        if (message.hwnd == window || IsChild(window, message.hwnd).as_bool())
            && let Some(chord) = crate::shortcut_win32::chord_from_message(&message)
            && let Some(command) = (*pointer).behavior.command(chord)
        {
            SendMessageW(window, WM_COMMAND, Some(WPARAM(command)), None);
            continue;
        }
        if message.message == WM_KEYDOWN && message.wParam.0 == usize::from(VK_ESCAPE.0) {
            let _ = DestroyWindow(window);
            continue;
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    let state = Box::from_raw(pointer);
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
    if !state.base.previous_focus.0.is_null() {
        let _ = SetFocus(Some(state.base.previous_focus));
    }
    if let Some(error) = loop_error {
        return Err(error);
    }
    Ok(take_result(*state))
}

unsafe extern "system" fn name_window_proc(
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
            let command = wparam.0 & 0xffff;
            if command == ID_OK || command == IDOK_COMMAND {
                accept_name(window);
            } else if command == ID_CANCEL || command == IDCANCEL_COMMAND {
                let _ = DestroyWindow(window);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(window);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), 0);
            DefWindowProcW(window, message, wparam, lparam)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe extern "system" fn picker_window_proc(
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
            let command = wparam.0 & 0xffff;
            let notification = (wparam.0 >> 16) & 0xffff;
            if command == ID_OK
                || command == IDOK_COMMAND
                || (command == ID_VALUE
                    && notification == usize::try_from(LBN_DBLCLK).expect("notification fits"))
            {
                accept_choice(window);
            } else if command == ID_CANCEL || command == IDCANCEL_COMMAND {
                let _ = DestroyWindow(window);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(window);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), 0);
            DefWindowProcW(window, message, wparam, lparam)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe extern "system" fn name_value_proc(
    control: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    if message == WM_KEYDOWN
        && let Ok(parent) = GetParent(control)
    {
        if wparam.0 == usize::from(VK_RETURN.0) {
            accept_name(parent);
            return LRESULT(0);
        }
        if wparam.0 == usize::from(VK_ESCAPE.0) {
            let _ = DestroyWindow(parent);
            return LRESULT(0);
        }
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(control, Some(name_value_proc), subclass_id);
    }
    DefSubclassProc(control, message, wparam, lparam)
}

unsafe extern "system" fn picker_value_proc(
    control: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    subclass_id: usize,
    _reference_data: usize,
) -> LRESULT {
    if message == WM_KEYDOWN
        && let Ok(parent) = GetParent(control)
    {
        if wparam.0 == usize::from(VK_RETURN.0) {
            accept_choice(parent);
            return LRESULT(0);
        }
        if wparam.0 == usize::from(VK_ESCAPE.0) {
            let _ = DestroyWindow(parent);
            return LRESULT(0);
        }
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(control, Some(picker_value_proc), subclass_id);
    }
    DefSubclassProc(control, message, wparam, lparam)
}

unsafe fn accept_name(window: HWND) {
    let Some(state) = state_mut::<String>(window) else {
        return;
    };
    let length = GetWindowTextLengthW(state.base.value);
    let mut value = vec![0_u16; usize::try_from(length).unwrap_or_default() + 1];
    let copied = GetWindowTextW(state.base.value, &mut value);
    let value = String::from_utf16_lossy(&value[..usize::try_from(copied).unwrap_or_default()]);
    state.result = Some(value);
    let _ = DestroyWindow(window);
}

unsafe fn accept_choice(window: HWND) {
    let Some(state) = state_mut::<usize>(window) else {
        return;
    };
    let selected = SendMessageW(state.base.value, LB_GETCURSEL, None, None).0;
    let Ok(index) = usize::try_from(selected) else {
        return;
    };
    state.result = Some(index);
    let _ = DestroyWindow(window);
}

unsafe fn layout(window: HWND) {
    let Some(state) = state_base(window) else {
        return;
    };
    let mut bounds = RECT::default();
    if GetClientRect(window, &raw mut bounds).is_err() {
        return;
    }
    let width = (bounds.right - bounds.left).max(360);
    let height = (bounds.bottom - bounds.top).max(160);
    let margin = 12;
    let prompt_height = 28;
    let button_height = 34;
    let button_width = 110;
    let _ = MoveWindow(
        state.prompt,
        margin,
        margin,
        width - margin * 2,
        prompt_height,
        true,
    );
    let value_top = margin + prompt_height;
    let value_height = height - value_top - button_height - margin * 2;
    let _ = MoveWindow(
        state.value,
        margin,
        value_top,
        width - margin * 2,
        value_height.max(30),
        true,
    );
    let button_top = height - button_height - margin;
    let _ = MoveWindow(
        state.ok,
        width - margin - button_width * 2 - 8,
        button_top,
        button_width,
        button_height,
        true,
    );
    let _ = MoveWindow(
        state.cancel,
        width - margin - button_width,
        button_top,
        button_width,
        button_height,
        true,
    );
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

unsafe fn state_base(window: HWND) -> Option<&'static DialogBase> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const DialogBase;
    pointer.as_ref()
}

unsafe fn state_mut<T>(window: HWND) -> Option<&'static mut DialogState<T>> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut DialogState<T>;
    pointer.as_mut()
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

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod shortcut_tests {
    use super::*;

    #[test]
    fn picker_honors_custom_bindings_without_capturing_other_player_keys() {
        let behavior = PickerBehavior {
            initial_selection: 2,
            accept: ShortcutChord::parse("Ctrl+J"),
            back: ShortcutChord::parse("Alt+Left"),
        };
        assert_eq!(
            behavior.command(ShortcutChord::parse("Ctrl+J").unwrap()),
            Some(ID_OK)
        );
        assert_eq!(
            behavior.command(ShortcutChord::parse("Alt+Left").unwrap()),
            Some(ID_CANCEL)
        );
        assert_eq!(behavior.command(ShortcutChord::parse("V").unwrap()), None);
        assert_eq!(
            PickerBehavior::default().command(ShortcutChord::parse("Ctrl+J").unwrap()),
            None
        );
    }
}
