//! Accessible native dialog for playback-bookmark operations.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                EnableWindow, GetFocus, IsWindowEnabled, SetFocus, VK_DELETE, VK_ESCAPE, VK_F10,
                VK_RETURN, VK_SHIFT,
            },
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                AppendMenuW, BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GetClientRect,
                GetCursorPos, GetMessageW, GetParent, GetWindowLongPtrW, GetWindowRect, HMENU,
                IDC_ARROW, IsDialogMessageW, IsWindow, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT,
                LB_SETCURSEL, LBN_DBLCLK, LBS_NOTIFY, LoadCursorW, MF_GRAYED, MF_STRING, MSG,
                MoveWindow, PostQuitMessage, RegisterClassW, SW_SHOW, SendMessageW,
                SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TPM_LEFTALIGN, TPM_RETURNCMD,
                TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU,
                WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CHILD,
                WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaBookmarksWindow");
const ID_LIST: usize = 2_301;
const ID_ADD: usize = 2_302;
const ID_PLAY: usize = 2_303;
const ID_RENAME: usize = 2_304;
const ID_DELETE: usize = 2_305;
const ID_COPY: usize = 2_306;
const ID_CLOSE: usize = 2_307;
/// `IsDialogMessageW` turns Enter and Escape into `IDOK` and `IDCANCEL`
/// before the focused list sees the key.
const IDOK_COMMAND: usize = 1;
const IDCANCEL_COMMAND: usize = 2;

#[derive(Clone, Debug)]
pub struct BookmarkDialogEntry {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BookmarkDialogRequest {
    Add,
    Play(String),
    Rename(String),
    Delete(String),
    Copy(String),
}

#[derive(Clone, Debug)]
pub enum BookmarkDialogResponse {
    KeepOpen,
    Refresh {
        entries: Vec<BookmarkDialogEntry>,
        selected_id: Option<String>,
    },
    Close,
}

#[derive(Clone, Copy)]
pub struct BookmarkDialogLabels<'a> {
    pub title: &'a str,
    pub list_name: &'a str,
    pub empty: &'a str,
    pub add: &'a str,
    pub play: &'a str,
    pub rename: &'a str,
    pub delete: &'a str,
    pub copy: &'a str,
    pub close: &'a str,
}

type ActionHandler =
    Box<dyn FnMut(HWND, BookmarkDialogRequest) -> BookmarkDialogResponse + 'static>;

struct DialogState {
    previous_focus: HWND,
    list: HWND,
    add: HWND,
    play: HWND,
    rename: HWND,
    delete: HWND,
    copy: HWND,
    close: HWND,
    empty_label: String,
    entries: Vec<BookmarkDialogEntry>,
    handler: Option<ActionHandler>,
}

pub fn register() -> Result<()> {
    // SAFETY: Registration happens once before the main message loop.
    unsafe {
        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size fits i32"),
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

/// Shows a modal bookmark list with real native buttons and list semantics.
///
/// # Errors
///
/// Returns a Win32 error when the dialog or one of its controls cannot be created.
pub fn show(
    owner: HWND,
    labels: BookmarkDialogLabels<'_>,
    entries: Vec<BookmarkDialogEntry>,
    can_add: bool,
    handler: ActionHandler,
) -> Result<()> {
    // SAFETY: The nested modal loop owns its state and disables its owner until
    // the allocation is recovered after WM_NCDESTROY.
    unsafe { show_win32(owner, labels, entries, can_add, handler) }
}

unsafe fn show_win32(
    owner: HWND,
    labels: BookmarkDialogLabels<'_>,
    entries: Vec<BookmarkDialogEntry>,
    can_add: bool,
    handler: ActionHandler,
) -> Result<()> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let title = wide(labels.title);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CLASS_NAME,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        760,
        520,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let list_name = wide(labels.list_name);
    let list = create_control(
        window,
        instance,
        w!("LISTBOX"),
        PCWSTR(list_name.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
        WS_EX_CLIENTEDGE,
        ID_LIST,
    )?;
    if !SetWindowSubclass(list, Some(list_proc), 1, 0).as_bool() {
        let _ = DestroyWindow(window);
        return Err(windows::core::Error::from_thread());
    }
    let add = create_button(window, instance, ID_ADD, labels.add, false)?;
    let play = create_button(window, instance, ID_PLAY, labels.play, true)?;
    let rename = create_button(window, instance, ID_RENAME, labels.rename, false)?;
    let delete = create_button(window, instance, ID_DELETE, labels.delete, false)?;
    let copy = create_button(window, instance, ID_COPY, labels.copy, false)?;
    let close = create_button(window, instance, ID_CLOSE, labels.close, false)?;
    let _ = EnableWindow(add, can_add);
    apply_font(&[list, add, play, rename, delete, copy, close]);
    let state = DialogState {
        previous_focus: GetFocus(),
        list,
        add,
        play,
        rename,
        delete,
        copy,
        close,
        empty_label: labels.empty.to_owned(),
        entries,
        handler: Some(handler),
    };
    refresh_entries(&state, None);
    let pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), pointer as isize);
    layout(window);
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetFocus(Some(list));
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
    let state = Box::from_raw(pointer);
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
    if !state.previous_focus.0.is_null() {
        let _ = SetFocus(Some(state.previous_focus));
    }
    if let Some(error) = loop_error {
        return Err(error);
    }
    Ok(())
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
            let command = wparam.0 & 0xffff;
            let notification = (wparam.0 >> 16) & 0xffff;
            if command == ID_CLOSE || command == IDCANCEL_COMMAND {
                let _ = DestroyWindow(window);
            } else if command == IDOK_COMMAND {
                // Enter in the list plays the selected bookmark.
                dispatch_selected(window, ID_PLAY);
            } else if command == ID_ADD {
                dispatch_action(window, BookmarkDialogRequest::Add);
            } else if command == ID_PLAY
                || (command == ID_LIST
                    && notification == usize::try_from(LBN_DBLCLK).expect("notification fits"))
            {
                dispatch_selected(window, ID_PLAY);
            } else if matches!(command, ID_RENAME | ID_DELETE | ID_COPY) {
                dispatch_selected(window, command);
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

unsafe extern "system" fn list_proc(
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
            dispatch_selected(parent, ID_PLAY);
            return LRESULT(0);
        }
        if wparam.0 == usize::from(VK_DELETE.0) {
            dispatch_selected(parent, ID_DELETE);
            return LRESULT(0);
        }
        if wparam.0 == usize::from(VK_ESCAPE.0) {
            let _ = DestroyWindow(parent);
            return LRESULT(0);
        }
        if wparam.0 == usize::from(VK_F10.0)
            && windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(i32::from(VK_SHIFT.0))
                .is_negative()
        {
            show_context_menu(parent, true);
            return LRESULT(0);
        }
    }
    if message == WM_CONTEXTMENU
        && let Ok(parent) = GetParent(control)
    {
        show_context_menu(parent, lparam.0 == -1);
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(control, Some(list_proc), subclass_id);
    }
    DefSubclassProc(control, message, wparam, lparam)
}

unsafe fn show_context_menu(window: HWND, keyboard_invoked: bool) {
    let Some(state) = state(window) else {
        return;
    };
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    for (id, control) in [
        (ID_PLAY, state.play),
        (ID_ADD, state.add),
        (ID_RENAME, state.rename),
        (ID_DELETE, state.delete),
        (ID_COPY, state.copy),
    ] {
        let label = window_text(control);
        let label = wide(&label);
        let flags = if IsWindowEnabled(control).as_bool() {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        };
        let _ = AppendMenuW(menu, flags, id, PCWSTR(label.as_ptr()));
    }
    let mut point = POINT::default();
    let positioned = if keyboard_invoked {
        let mut bounds = RECT::default();
        GetWindowRect(state.list, &raw mut bounds).map(|()| {
            point.x = bounds.left + 16;
            point.y = bounds.top + 16;
        })
    } else {
        GetCursorPos(&raw mut point)
    };
    if positioned.is_ok() {
        let selected = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            point.x,
            point.y,
            None,
            window,
            None,
        );
        let selected = usize::try_from(selected.0).unwrap_or_default();
        if selected == ID_ADD {
            dispatch_action(window, BookmarkDialogRequest::Add);
        } else if matches!(selected, ID_PLAY | ID_RENAME | ID_DELETE | ID_COPY) {
            dispatch_selected(window, selected);
        }
    }
    let _ = DestroyMenu(menu);
}

unsafe fn dispatch_selected(window: HWND, command: usize) {
    let Some(request) = state(window).and_then(|state| {
        let index = usize::try_from(SendMessageW(state.list, LB_GETCURSEL, None, None).0).ok()?;
        let id = state.entries.get(index)?.id.clone();
        let request = match command {
            ID_PLAY => BookmarkDialogRequest::Play(id.clone()),
            ID_RENAME => BookmarkDialogRequest::Rename(id.clone()),
            ID_DELETE => BookmarkDialogRequest::Delete(id.clone()),
            ID_COPY => BookmarkDialogRequest::Copy(id.clone()),
            _ => return None,
        };
        Some(request)
    }) else {
        return;
    };
    dispatch_action(window, request);
}

unsafe fn dispatch_action(window: HWND, request: BookmarkDialogRequest) {
    let Some(mut handler) = state_mut(window).and_then(|state| state.handler.take()) else {
        return;
    };
    let response = handler(window, request);
    if !IsWindow(Some(window)).as_bool() {
        return;
    }
    let Some(state) = state_mut(window) else {
        return;
    };
    state.handler = Some(handler);
    match response {
        BookmarkDialogResponse::KeepOpen => {}
        BookmarkDialogResponse::Refresh {
            entries,
            selected_id,
        } => {
            state.entries = entries;
            refresh_entries(state, selected_id.as_deref());
            let _ = SetFocus(Some(state.list));
        }
        BookmarkDialogResponse::Close => {
            let _ = DestroyWindow(window);
        }
    }
}

unsafe fn refresh_entries(state: &DialogState, selected_id: Option<&str>) {
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    if state.entries.is_empty() {
        add_list_string(state.list, &state.empty_label);
        SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(0)), None);
        for control in [state.play, state.rename, state.delete, state.copy] {
            let _ = EnableWindow(control, false);
        }
        return;
    }
    for entry in &state.entries {
        add_list_string(state.list, &entry.label);
    }
    let selected = selected_id
        .and_then(|id| state.entries.iter().position(|entry| entry.id == id))
        .unwrap_or(0);
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selected)), None);
    for control in [state.play, state.rename, state.delete, state.copy] {
        let _ = EnableWindow(control, true);
    }
}

unsafe fn layout(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let mut bounds = RECT::default();
    if GetClientRect(window, &raw mut bounds).is_err() {
        return;
    }
    let width = (bounds.right - bounds.left).max(560);
    let height = (bounds.bottom - bounds.top).max(320);
    let margin = 12;
    let gap = 8;
    let button_height = 34;
    let button_width = ((width - margin * 2 - gap * 5) / 6).max(80);
    let list_height = height - button_height - margin * 3;
    let _ = MoveWindow(
        state.list,
        margin,
        margin,
        width - margin * 2,
        list_height,
        true,
    );
    for (index, control) in [
        state.add,
        state.play,
        state.rename,
        state.delete,
        state.copy,
        state.close,
    ]
    .into_iter()
    .enumerate()
    {
        let x = margin + i32::try_from(index).unwrap_or_default() * (button_width + gap);
        let _ = MoveWindow(
            control,
            x,
            margin * 2 + list_height,
            button_width,
            button_height,
            true,
        );
    }
}

unsafe fn create_button(
    parent: HWND,
    instance: HINSTANCE,
    id: usize,
    label: &str,
    default: bool,
) -> Result<HWND> {
    let label = wide(label);
    let style = if default {
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32)
    } else {
        WS_CHILD | WS_VISIBLE | WS_TABSTOP
    };
    create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(label.as_ptr()),
        style,
        WINDOW_EX_STYLE::default(),
        id,
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

unsafe fn window_text(control: HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};

    let length = GetWindowTextLengthW(control);
    let mut value = vec![0_u16; usize::try_from(length).unwrap_or_default() + 1];
    let copied = GetWindowTextW(control, &mut value);
    String::from_utf16_lossy(&value[..usize::try_from(copied).unwrap_or_default()])
}

unsafe fn state(window: HWND) -> Option<&'static DialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const DialogState;
    pointer.as_ref()
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut DialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut DialogState;
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
