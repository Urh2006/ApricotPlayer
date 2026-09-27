//! Native accessible read-only lyrics dialog (Python `show_lyrics`). Video
//! details are not a dialog; they live in the player page.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::mem::size_of;

use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::{GetModuleHandleW, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        UI::{
            Controls::EM_SCROLLCARET,
            Controls::RichEdit::{
                CFE_AUTOBACKCOLOR, CFM_BACKCOLOR, CHARFORMAT2W, CHARRANGE, EM_EXGETSEL,
                EM_EXSETSEL, EM_SETCHARFORMAT, SCF_SELECTION,
            },
            Input::KeyboardAndMouse::{EnableWindow, SetFocus, VK_ESCAPE},
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow,
                DispatchMessageW, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, GetClientRect,
                GetMessageW, GetWindowLongPtrW, HMENU, IDC_ARROW, IsDialogMessageW, IsWindow,
                KillTimer, LoadCursorW, MSG, MoveWindow, PostQuitMessage, RegisterClassW, SW_SHOW,
                SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowTextW,
                ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE,
                WM_CLOSE, WM_COMMAND, WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WM_TIMER,
                WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE, WS_HSCROLL, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
                WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaDetailsWindow");
const ID_TEXT: usize = 1601;
const ID_COPY: usize = 1602;
const ID_BACK: usize = 1603;

pub struct DetailsDialogLabels {
    pub title: String,
    pub copy: String,
    pub copied: String,
    pub back: String,
}

struct DetailsDialogState {
    timeline: Option<(
        apricot_media::lyrics::LyricsDocument,
        apricot_playback::PlaybackPositionReader,
        u64,
    )>,
    active_line: Option<usize>,
    pending: Option<AsyncText>,
    unavailable: Option<String>,
    text_value: String,
    copied_message: String,
    text: HWND,
    copy: HWND,
    back: HWND,
    status: HWND,
    announcer: crate::announcement_win32::WindowsAnnouncer,
}

pub struct AsyncText {
    pub receiver: std::sync::mpsc::Receiver<Option<apricot_media::lyrics::LyricsDocument>>,
    pub position: Option<(apricot_playback::PlaybackPositionReader, u64)>,
    pub unavailable: String,
    pub ready: String,
}

pub unsafe fn register() -> Result<()> {
    // Retain the system Rich Edit module for the lifetime of its window class.
    let _ = LoadLibraryExW(w!("Msftedit.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32)?;
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

pub unsafe fn show_async(
    owner: HWND,
    text_value: String,
    labels: &DetailsDialogLabels,
    pending: AsyncText,
) -> Result<()> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let title = wide(&labels.title);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CLASS_NAME,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        620,
        460,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let mut state = match create_controls(window, instance, text_value, labels) {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    state.unavailable = Some(pending.unavailable.clone());
    state.pending = Some(pending);
    let _ = SetTimer(Some(window), 1, 50, None);
    let initial_focus = state.text;
    let state_pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), state_pointer as isize);
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
        if message.message == WM_KEYDOWN && message.wParam.0 == usize::from(VK_ESCAPE.0) {
            let _ = DestroyWindow(window);
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
    drop(state);
    Ok(())
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TIMER if wparam.0 == 1 => {
            poll_text(window);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == 2 => {
            highlight_lyrics(window);
            LRESULT(0)
        }
        WM_SIZE => {
            layout(window);
            LRESULT(0)
        }
        WM_COMMAND => {
            match wparam.0 & 0xffff {
                ID_COPY => copy_details(window),
                ID_BACK => {
                    let _ = DestroyWindow(window);
                }
                _ => {}
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
    text_value: String,
    labels: &DetailsDialogLabels,
) -> Result<DetailsDialogState> {
    let text_wide = wide(&text_value);
    let text = create_control(
        parent,
        instance,
        w!("RICHEDIT50W"),
        PCWSTR(text_wide.as_ptr()),
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WINDOW_STYLE(ES_MULTILINE as u32)
            | WINDOW_STYLE(ES_READONLY as u32)
            | WINDOW_STYLE(ES_AUTOVSCROLL as u32)
            | WS_VSCROLL
            | WS_HSCROLL,
        WS_EX_CLIENTEDGE,
        ID_TEXT,
    )?;
    crate::accessibility_win32::annotate_control_name(text, &labels.title);
    let copy_label = wide(&labels.copy);
    let copy = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(copy_label.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        WINDOW_EX_STYLE::default(),
        ID_COPY,
    )?;
    let back_label = wide(&labels.back);
    let back = create_control(
        parent,
        instance,
        w!("BUTTON"),
        PCWSTR(back_label.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        WINDOW_EX_STYLE::default(),
        ID_BACK,
    )?;
    let status = create_control(
        parent,
        instance,
        w!("STATIC"),
        w!(""),
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in [text, copy, back, status] {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    Ok(DetailsDialogState {
        timeline: None,
        active_line: None,
        pending: None,
        unavailable: None,
        text_value,
        copied_message: labels.copied.clone(),
        text,
        copy,
        back,
        status,
        announcer: crate::announcement_win32::WindowsAnnouncer::new(status),
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
        0,
        0,
        Some(parent),
        Some(HMENU(id as *mut _)),
        Some(instance),
        None,
    )
}

unsafe fn copy_details(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if let Some(message) = state.unavailable.as_ref() {
        state.announcer.announce(message, true);
        return;
    }
    if crate::clipboard_win32::copy_text(window, &state.text_value).is_ok() {
        state.announcer.announce(&state.copied_message, true);
    }
}

unsafe fn poll_text(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(pending) = state.pending.as_ref() else {
        return;
    };
    let text = match pending.receiver.try_recv() {
        Ok(text) => text,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
    };
    let message = if let Some(document) = text.filter(|document| !document.text.trim().is_empty()) {
        state.text_value.clone_from(&document.text);
        if let Some((reader, generation)) = pending.position.as_ref() {
            state.timeline = Some((document, reader.clone(), *generation));
            let _ = SetTimer(Some(window), 2, 200, None);
        }
        state.unavailable = None;
        pending.ready.clone()
    } else {
        state.text_value.clone_from(&pending.unavailable);
        pending.unavailable.clone()
    };
    state.pending = None;
    let _ = KillTimer(Some(window), 1);
    let text = wide(&state.text_value);
    let _ = SetWindowTextW(state.text, PCWSTR(text.as_ptr()));
    let _ = SetFocus(Some(state.text));
    state.announcer.announce(&message, true);
}

unsafe fn highlight_lyrics(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some((document, reader, generation)) = &state.timeline else {
        return;
    };
    let Some(position) = reader.read(*generation) else {
        return;
    };
    let active = document.active_line(position);
    if active == state.active_line {
        return;
    }
    let mut saved = CHARRANGE::default();
    SendMessageW(
        state.text,
        EM_EXGETSEL,
        None,
        Some(LPARAM((&raw mut saved) as isize)),
    );
    for (index, enabled) in [(state.active_line, false), (active, true)] {
        let Some(line) = index.and_then(|index| document.timed_lines.get(index)) else {
            continue;
        };
        let range = CHARRANGE {
            cpMin: i32::try_from(line.rich_start).unwrap_or(i32::MAX),
            cpMax: i32::try_from(line.rich_end).unwrap_or(i32::MAX),
        };
        SendMessageW(
            state.text,
            EM_EXSETSEL,
            None,
            Some(LPARAM((&raw const range) as isize)),
        );
        let mut format = CHARFORMAT2W::default();
        format.Base.cbSize = u32::try_from(size_of::<CHARFORMAT2W>()).expect("format size");
        format.Base.dwMask = CFM_BACKCOLOR;
        if enabled {
            format.crBackColor = COLORREF(0x00e6_d8ad);
        } else {
            format.Base.dwEffects = CFE_AUTOBACKCOLOR;
        }
        SendMessageW(
            state.text,
            EM_SETCHARFORMAT,
            Some(WPARAM(SCF_SELECTION as usize)),
            Some(LPARAM((&raw const format) as isize)),
        );
        if enabled {
            SendMessageW(state.text, EM_SCROLLCARET, None, None);
        }
    }
    SendMessageW(
        state.text,
        EM_EXSETSEL,
        None,
        Some(LPARAM((&raw const saved) as isize)),
    );
    state.active_line = active;
}

unsafe fn layout(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let mut rect = RECT::default();
    if GetClientRect(window, &raw mut rect).is_err() {
        return;
    }
    let margin = 12;
    let gap = 8;
    let button_height = 30;
    let status_height = 24;
    let button_width = 130;
    let width = (rect.right - rect.left - margin * 2).max(1);
    let text_height =
        (rect.bottom - rect.top - margin * 2 - gap * 2 - button_height - status_height).max(1);
    let _ = MoveWindow(state.text, margin, margin, width, text_height, true);
    let button_y = margin + text_height + gap;
    let _ = MoveWindow(
        state.copy,
        margin,
        button_y,
        button_width,
        button_height,
        true,
    );
    let _ = MoveWindow(
        state.back,
        margin + button_width + gap,
        button_y,
        button_width,
        button_height,
        true,
    );
    let _ = MoveWindow(
        state.status,
        margin,
        button_y + button_height + gap,
        width,
        status_height,
        true,
    );
}

unsafe fn state(window: HWND) -> Option<&'static DetailsDialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0));
    (!pointer.eq(&0)).then(|| &*(pointer as *const DetailsDialogState))
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut DetailsDialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0));
    (!pointer.eq(&0)).then(|| &mut *(pointer as *mut DetailsDialogState))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Controls::RichEdit::EM_GETSELTEXT;

    #[test]
    fn native_lyrics_control_preserves_text_beyond_default_edit_limit() {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};
        let expected = "Long lyric line. ".repeat(8_000);
        // SAFETY: Hidden, test-owned control with buffers sized from its value.
        unsafe {
            let _ = LoadLibraryExW(w!("Msftedit.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).unwrap();
            let control = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("RICHEDIT50W"),
                w!("Loading"),
                WINDOW_STYLE((ES_MULTILINE | ES_READONLY) as u32),
                0,
                0,
                200,
                100,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let text = wide(&expected);
            SetWindowTextW(control, PCWSTR(text.as_ptr())).unwrap();
            let length = usize::try_from(GetWindowTextLengthW(control)).unwrap();
            let mut buffer = vec![0_u16; length + 1];
            let copied = usize::try_from(GetWindowTextW(control, &mut buffer)).unwrap();
            let actual = String::from_utf16_lossy(&buffer[..copied]);
            let _ = DestroyWindow(control);
            assert_eq!(actual.len(), expected.len());
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn lyric_ranges_select_exact_text_in_native_rich_edit() {
        let document = apricot_media::lyrics::LyricsDocument::parse(
            "Plain line\n[00:01.00] First \u{1f3b5}\n[00:02.00] Second \u{17e}",
            "Local",
        );
        // SAFETY: Hidden test-owned control; no user focus or window is touched.
        unsafe {
            let _ = LoadLibraryExW(w!("Msftedit.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).unwrap();
            let text = wide(&document.text);
            let control = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("RICHEDIT50W"),
                PCWSTR(text.as_ptr()),
                WINDOW_STYLE(ES_MULTILINE as u32),
                0,
                0,
                200,
                100,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let mut selected = Vec::new();
            for line in &document.timed_lines {
                let range = CHARRANGE {
                    cpMin: i32::try_from(line.rich_start).unwrap(),
                    cpMax: i32::try_from(line.rich_end).unwrap(),
                };
                SendMessageW(
                    control,
                    EM_EXSETSEL,
                    None,
                    Some(LPARAM((&raw const range) as isize)),
                );
                let mut buffer = [0_u16; 128];
                let copied = SendMessageW(
                    control,
                    EM_GETSELTEXT,
                    None,
                    Some(LPARAM(buffer.as_mut_ptr() as isize)),
                )
                .0;
                selected.push(String::from_utf16_lossy(
                    &buffer[..usize::try_from(copied).unwrap()],
                ));
            }
            let _ = DestroyWindow(control);
            assert_eq!(selected, ["First \u{1f3b5}", "Second \u{17e}"]);
        }
    }
}
