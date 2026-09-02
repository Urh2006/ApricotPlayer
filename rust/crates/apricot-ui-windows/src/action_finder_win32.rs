//! Native searchable Action Finder dialog.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use apricot_app::ActionFinderModel;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                EnableWindow, SetFocus, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP,
            },
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, EN_CHANGE, GetClientRect, GetMessageW, GetParent,
                GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW,
                IsDialogMessageW, IsWindow, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT,
                LB_SETCURSEL, LBN_DBLCLK, LBS_NOTIFY, LoadCursorW, MSG, MoveWindow,
                PostQuitMessage, RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow,
                SetWindowLongPtrW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_KEYDOWN,
                WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE,
                WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaActionFinderWindow");
const ID_QUERY: usize = 1401;
const ID_RESULTS: usize = 1402;
const ID_OPEN: usize = 1403;
const ID_CANCEL: usize = 1404;

struct ActionFinderState {
    model: ActionFinderModel,
    filtered: Vec<usize>,
    query_label: HWND,
    query: HWND,
    results: HWND,
    open: HWND,
    cancel: HWND,
    result: Option<&'static str>,
}

pub unsafe fn register() -> Result<()> {
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
    Ok(())
}

pub unsafe fn show(owner: HWND, model: ActionFinderModel) -> Result<Option<&'static str>> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let title = wide(&model.title);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CLASS_NAME,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        620,
        500,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let state = match create_controls(window, instance, model) {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let initial_focus = state.query;
    let state_pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), state_pointer as isize);
    refresh_results(window);
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
            PostQuitMessage(0);
            break;
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
    Ok(state.result)
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
            if command == ID_QUERY
                && notification == usize::try_from(EN_CHANGE).expect("notification fits")
            {
                refresh_results(window);
            } else if command == ID_OPEN
                || (command == ID_RESULTS
                    && notification == usize::try_from(LBN_DBLCLK).expect("notification fits"))
            {
                activate(window);
            } else if command == ID_CANCEL {
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

unsafe fn create_controls(
    parent: HWND,
    instance: HINSTANCE,
    model: ActionFinderModel,
) -> Result<ActionFinderState> {
    let query_label = wide(&model.query_label);
    let query_label = create_control(
        parent,
        instance,
        w!("STATIC"),
        PCWSTR(query_label.as_ptr()),
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let query = create_control(
        parent,
        instance,
        w!("EDIT"),
        w!(""),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        WS_EX_CLIENTEDGE,
        ID_QUERY,
    )?;
    let results_name = wide(&model.results_name);
    let results = create_control(
        parent,
        instance,
        w!("LISTBOX"),
        PCWSTR(results_name.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
        WS_EX_CLIENTEDGE,
        ID_RESULTS,
    )?;
    let open_label = wide(&model.open_label);
    let open = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(open_label.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE::default(),
        ID_OPEN,
    )?;
    let cancel_label = wide(&model.cancel_label);
    let cancel = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(cancel_label.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_CANCEL,
    )?;
    if !SetWindowSubclass(query, Some(control_proc), 1, 0).as_bool()
        || !SetWindowSubclass(results, Some(control_proc), 2, 0).as_bool()
    {
        return Err(windows::core::Error::from_thread());
    }
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in [query_label, query, results, open, cancel] {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    Ok(ActionFinderState {
        model,
        filtered: Vec::new(),
        query_label,
        query,
        results,
        open,
        cancel,
        result: None,
    })
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

unsafe extern "system" fn control_proc(
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
        let key = wparam.0;
        if key == usize::from(VK_ESCAPE.0) {
            let _ = DestroyWindow(parent);
            return LRESULT(0);
        }
        if key == usize::from(VK_RETURN.0) {
            activate(parent);
            return LRESULT(0);
        }
        if subclass_id == 1
            && matches!(key, value if value == usize::from(VK_DOWN.0) || value == usize::from(VK_UP.0))
            && let Some(state) = state(parent)
        {
            let _ = SetFocus(Some(state.results));
            return LRESULT(0);
        }
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(control, Some(control_proc), subclass_id);
    }
    DefSubclassProc(control, message, wparam, lparam)
}

unsafe fn refresh_results(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let query = window_text(state.query);
    state.filtered = state.model.filtered_indices(&query);
    SendMessageW(state.results, LB_RESETCONTENT, None, None);
    if state.filtered.is_empty() {
        let label = wide(&state.model.no_results_label);
        SendMessageW(
            state.results,
            LB_ADDSTRING,
            None,
            Some(LPARAM(label.as_ptr() as isize)),
        );
    } else {
        for index in &state.filtered {
            let label = wide(&state.model.items[*index].label);
            SendMessageW(
                state.results,
                LB_ADDSTRING,
                None,
                Some(LPARAM(label.as_ptr() as isize)),
            );
        }
    }
    SendMessageW(state.results, LB_SETCURSEL, Some(WPARAM(0)), None);
}

unsafe fn activate(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let selected = SendMessageW(state.results, LB_GETCURSEL, None, None).0;
    let Ok(selected) = usize::try_from(selected) else {
        return;
    };
    let Some(index) = state.filtered.get(selected) else {
        return;
    };
    state.result = Some(state.model.items[*index].action_id);
    let _ = DestroyWindow(window);
}

unsafe fn layout(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let mut bounds = RECT::default();
    if GetClientRect(window, &raw mut bounds).is_err() {
        return;
    }
    let width = (bounds.right - bounds.left).max(420);
    let height = (bounds.bottom - bounds.top).max(300);
    let margin = 12;
    let label_height = 22;
    let query_height = 30;
    let button_height = 34;
    let button_width = 110;
    let _ = MoveWindow(
        state.query_label,
        margin,
        margin,
        width - margin * 2,
        label_height,
        true,
    );
    let _ = MoveWindow(
        state.query,
        margin,
        margin + label_height,
        width - margin * 2,
        query_height,
        true,
    );
    let list_top = margin + label_height + query_height + 8;
    let _ = MoveWindow(
        state.results,
        margin,
        list_top,
        width - margin * 2,
        height - list_top - button_height - margin * 2,
        true,
    );
    let button_top = height - button_height - margin;
    let _ = MoveWindow(
        state.open,
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

unsafe fn state(window: HWND) -> Option<&'static ActionFinderState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const ActionFinderState;
    pointer.as_ref()
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut ActionFinderState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut ActionFinderState;
    pointer.as_mut()
}

unsafe fn window_text(control: HWND) -> String {
    let length = GetWindowTextLengthW(control);
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0_u16; usize::try_from(length).unwrap_or_default() + 1];
    let copied = GetWindowTextW(control, &mut buffer);
    String::from_utf16_lossy(&buffer[..usize::try_from(copied).unwrap_or_default()])
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
