//! Native comments dialog (Python `show_comments`) and the read-only comment
//! details dialog (Python `show_comment_details`). Loading runs on workers
//! supplied by the caller; the dialog only polls their results.
#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use crate::announcement_win32::WindowsAnnouncer;
use apricot_app::comments::{
    CommentSort, CommentsListView, CommentsPage, CommentsState, LoadFinished, LoadRequest,
    comment_copy_text, comment_details_text, comments_copy_text,
};
use apricot_core::{TranslationCatalog, shortcut::ShortcutChord};
use std::{
    mem::size_of,
    sync::mpsc::{Receiver, TryRecvError},
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{ClientToScreen, DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                EnableWindow, GetFocus, IsWindowEnabled, SetFocus, VK_ESCAPE, VK_TAB,
            },
            WindowsAndMessaging::{
                AppendMenuW, CB_ADDSTRING, CB_GETCURSEL, CB_GETDROPPEDSTATE, CB_SETCURSEL,
                CBN_SELCHANGE, CBS_DROPDOWNLIST, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, EN_CHANGE,
                ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, GetClientRect,
                GetCursorPos, GetMessageW, GetNextDlgTabItem, GetWindowLongPtrW,
                GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsChild, IsDialogMessageW,
                IsWindow, KillTimer, LB_ADDSTRING, LB_GETCURSEL, LB_GETITEMRECT, LB_RESETCONTENT,
                LB_SETCURSEL, LBN_DBLCLK, LBN_SELCHANGE, LBS_NOTIFY, LoadCursorW, MF_GRAYED,
                MF_SEPARATOR, MF_STRING, MSG, MoveWindow, PostQuitMessage, RegisterClassW, SW_SHOW,
                SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW, ShowWindow,
                TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
                WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
                WM_CONTEXTMENU, WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WM_TIMER, WNDCLASSW,
                WS_CHILD, WS_EX_CLIENTEDGE, WS_HSCROLL, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
                WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS: PCWSTR = w!("ApricotPlayer2BetaCommentsWindow");
const DETAILS_CLASS: PCWSTR = w!("ApricotPlayer2BetaCommentDetailsWindow");
const SEARCH: usize = 2201;
const SORT: usize = 2202;
const LIST: usize = 2203;
const OPEN: usize = 2204;
const COPY: usize = 2205;
const COPY_ALL: usize = 2206;
const AUTHOR: usize = 2207;
const MORE: usize = 2208;
const BACK: usize = 2209;
const DETAILS_TEXT: usize = 2251;
const DETAILS_BACK: usize = 2252;
const DETAILS_TEXT_ID: i32 = 2251;
const DETAILS_BACK_ID: i32 = 2252;
const POLL_TIMER: usize = 1;

/// Starts one page load on a worker and returns its result channel.
pub type CommentsLoader =
    Box<dyn FnMut(String) -> Receiver<std::result::Result<CommentsPage, String>>>;

pub struct CommentsDialogOptions {
    pub catalog: TranslationCatalog,
    pub loader: CommentsLoader,
    /// Python binds `open_selected` and `player_back` on the list only.
    pub accept: Option<ShortcutChord>,
    pub back: Option<ShortcutChord>,
}

struct State {
    /// Search, sort, list, open, copy, copy visible, author, more, back.
    controls: [HWND; 9],
    status: HWND,
    options: CommentsDialogOptions,
    comments: CommentsState,
    pending: Option<Receiver<std::result::Result<CommentsPage, String>>>,
    announcer: WindowsAnnouncer,
}

impl State {
    fn text(&self, key: &str) -> String {
        self.options.catalog.text(key).to_owned()
    }

    fn list(&self) -> HWND {
        self.controls[2]
    }

    fn control(&self, id: usize) -> HWND {
        self.controls[id - SEARCH]
    }

    unsafe fn announce(&self, text: &str) {
        self.announcer.announce(text, true);
    }

    unsafe fn selected_row(&self) -> Option<usize> {
        usize::try_from(SendMessageW(self.list(), LB_GETCURSEL, None, None).0).ok()
    }
}

/// Opens the modal comments dialog and starts loading the first page.
///
/// # Errors
/// Returns a Windows error if window creation or the message loop fails.
pub fn show(owner: HWND, options: CommentsDialogOptions) -> Result<()> {
    // SAFETY: State remains owned by this call throughout the nested message loop.
    unsafe { show_inner(owner, options) }
}

unsafe fn register(
    instance: HINSTANCE,
    class: PCWSTR,
    procedure: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) -> Result<()> {
    let class = WNDCLASSW {
        hInstance: instance,
        lpszClassName: class,
        lpfnWndProc: Some(procedure),
        cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size"),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        ..Default::default()
    };
    // Re-registering the same process-local class is harmless.
    let _ = RegisterClassW(&raw const class);
    Ok(())
}

// Keep window ownership and its nested message-loop cleanup together.
#[allow(clippy::too_many_lines)]
unsafe fn show_inner(owner: HWND, options: CommentsDialogOptions) -> Result<()> {
    let instance = HINSTANCE(GetModuleHandleW(None)?.0);
    register(instance, CLASS, window_proc)?;
    let title = wide(options.catalog.text("comments"));
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CLASS,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        860,
        560,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let mut state = Box::new(State {
        controls: [HWND::default(); 9],
        status: HWND::default(),
        options,
        comments: CommentsState::default(),
        pending: None,
        announcer: WindowsAnnouncer::new(HWND::default()),
    });
    if let Err(error) = create_controls(window, instance, &mut state) {
        let _ = DestroyWindow(window);
        return Err(error);
    }
    state.announcer = WindowsAnnouncer::new(state.status);
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), (&raw mut *state) as isize);
    // Python: the list starts with "Loading comments...", every button
    // disabled, then `load_more()` starts the first page.
    let loading = state.text("comments_loading");
    set_list(&state, &[loading], 0);
    for id in OPEN..=MORE {
        enable(&state, id, false);
    }
    load_more(window, &mut state);
    layout(window, &state);
    let previous = GetFocus();
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(window, SW_SHOW);
    // Python `wx.CallAfter(search_box.SetFocus)`.
    let _ = SetFocus(Some(state.controls[0]));
    let mut message = MSG::default();
    let mut error = None;
    while IsWindow(Some(window)).as_bool() {
        let result = GetMessageW(&raw mut message, None, 0, 0);
        if result.0 <= 0 {
            if result.0 < 0 {
                error = Some(windows::core::Error::from_thread());
            } else {
                PostQuitMessage(i32::try_from(message.wParam.0).unwrap_or_default());
            }
            let _ = DestroyWindow(window);
            break;
        }
        let ours = message.hwnd == window || IsChild(window, message.hwnd).as_bool();
        let on_list = GetFocus() == state.list();
        if ours
            && on_list
            && let Some(chord) = crate::shortcut_win32::chord_from_message(&message)
        {
            if state.options.accept == Some(chord) {
                command(window, &mut state, OPEN);
                continue;
            }
            if state.options.back == Some(chord) {
                let _ = DestroyWindow(window);
                continue;
            }
        }
        if ours
            && message.message == WM_KEYDOWN
            && message.wParam.0 == usize::from(VK_ESCAPE.0)
            && SendMessageW(state.controls[1], CB_GETDROPPEDSTATE, None, None).0 == 0
        {
            // Python's Back button is `wx.ID_CANCEL`.
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
    if IsWindow(Some(previous)).as_bool() {
        let _ = SetFocus(Some(previous));
    }
    error.map_or(Ok(()), Err)
}

unsafe fn create_controls(window: HWND, instance: HINSTANCE, state: &mut State) -> Result<()> {
    let specs: [(usize, PCWSTR, &str, u32); 9] = [
        (SEARCH, w!("EDIT"), "search_comments", ES_AUTOHSCROLL as u32),
        (
            SORT,
            w!("COMBOBOX"),
            "comments_sort",
            (CBS_DROPDOWNLIST as u32) | WS_VSCROLL.0,
        ),
        (
            LIST,
            w!("LISTBOX"),
            "comments",
            (LBS_NOTIFY as u32) | WS_VSCROLL.0,
        ),
        (OPEN, w!("BUTTON"), "open_comment", 0),
        (COPY, w!("BUTTON"), "copy_comment", 0),
        (COPY_ALL, w!("BUTTON"), "copy_visible_comments", 0),
        (AUTHOR, w!("BUTTON"), "open_comment_author_channel", 0),
        (MORE, w!("BUTTON"), "load_more_comments", 0),
        (BACK, w!("BUTTON"), "back", 0),
    ];
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for (index, (id, class, key, style)) in specs.into_iter().enumerate() {
        let label = state.text(key);
        let text = wide(if matches!(id, SEARCH | SORT | LIST) {
            ""
        } else {
            &label
        });
        let control = CreateWindowExW(
            if matches!(id, SEARCH | LIST) {
                WS_EX_CLIENTEDGE
            } else {
                WINDOW_EX_STYLE::default()
            },
            class,
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(style),
            0,
            0,
            100,
            if id == SORT { 200 } else { 28 },
            Some(window),
            Some(HMENU(id as *mut _)),
            Some(instance),
            None,
        )?;
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
        crate::accessibility_win32::annotate_control_name(control, &label);
        state.controls[index] = control;
    }
    for sort in CommentSort::ALL {
        let label = wide(state.options.catalog.text(sort.label_key()));
        SendMessageW(
            state.controls[1],
            CB_ADDSTRING,
            None,
            Some(LPARAM(label.as_ptr() as isize)),
        );
    }
    SendMessageW(state.controls[1], CB_SETCURSEL, Some(WPARAM(0)), None);
    // Hidden live-region text for screen readers without a direct API.
    state.status = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        w!(""),
        WS_CHILD,
        0,
        0,
        0,
        0,
        Some(window),
        None,
        Some(instance),
        None,
    )?;
    Ok(())
}

unsafe fn set_list(state: &State, labels: &[String], selection: usize) {
    let list = state.list();
    SendMessageW(list, LB_RESETCONTENT, None, None);
    for label in labels {
        let text = wide(label);
        SendMessageW(
            list,
            LB_ADDSTRING,
            None,
            Some(LPARAM(text.as_ptr() as isize)),
        );
    }
    SendMessageW(list, LB_SETCURSEL, Some(WPARAM(selection)), None);
}

/// wxWidgets moves focus on to the next control before disabling the
/// focused one, so keyboard navigation keeps working.
unsafe fn enable(state: &State, id: usize, enabled: bool) {
    let control = state.control(id);
    if !enabled
        && GetFocus() == control
        && let Ok(parent) = windows::Win32::UI::WindowsAndMessaging::GetParent(control)
        && let Ok(next) = GetNextDlgTabItem(parent, Some(control), false)
        && next != control
    {
        let _ = SetFocus(Some(next));
    }
    let _ = EnableWindow(control, enabled);
}

/// Python `refresh_comments` applied to the controls.
unsafe fn apply_view(state: &State, view: &CommentsListView) {
    set_list(state, &view.labels, view.selection);
    for id in [OPEN, COPY, COPY_ALL] {
        enable(state, id, view.has_comments);
    }
    enable(state, AUTHOR, view.author_enabled);
    enable(state, MORE, view.more_enabled);
}

unsafe fn refresh(state: &mut State, selection: usize) {
    let view = state.comments.refresh(&state.options.catalog, selection);
    apply_view(state, &view);
}

/// Python `load_more`.
unsafe fn load_more(window: HWND, state: &mut State) {
    match state.comments.request_load(&state.options.catalog) {
        LoadRequest::Ignored => {}
        LoadRequest::NoMore => state.announce(&state.text("no_more_comments")),
        LoadRequest::Start {
            page_token,
            loading_label,
        } => {
            enable(state, MORE, false);
            if let Some(label) = loading_label {
                set_list(state, &[label], 0);
            }
            state.pending = Some((state.options.loader)(page_token));
            let _ = SetTimer(Some(window), POLL_TIMER, 50, None);
        }
    }
}

/// Python `finish_load`.
unsafe fn poll(window: HWND, state: &mut State) {
    let Some(receiver) = state.pending.as_ref() else {
        let _ = KillTimer(Some(window), POLL_TIMER);
        return;
    };
    let result = match receiver.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Empty) => return,
        Err(TryRecvError::Disconnected) => Err(String::new()),
    };
    state.pending = None;
    let _ = KillTimer(Some(window), POLL_TIMER);
    match state.comments.finish_load(&state.options.catalog, result) {
        LoadFinished::Failed {
            label,
            announcement,
        } => {
            set_list(state, &[label], 0);
            enable(state, MORE, false);
            state.announce(&announcement);
        }
        LoadFinished::Loaded { view, announcement } => {
            apply_view(state, &view);
            state.announce(&announcement);
        }
    }
}

unsafe fn command(window: HWND, state: &mut State, id: usize) {
    let row = state.selected_row();
    let selected = row.and_then(|row| state.comments.selected(row)).cloned();
    match id {
        OPEN => {
            if let Some(comment) = selected {
                let text = comment_details_text(&state.options.catalog, &comment);
                let title = state.text("comment_details");
                let back = state.text("back");
                if let Err(error) = show_details(window, &title, &back, &text) {
                    state.announce(&error.to_string());
                }
            }
        }
        COPY => {
            let Some(comment) = selected else {
                state.announce(&state.text(state.comments.empty_message_key()));
                return;
            };
            let text = comment_copy_text(&state.options.catalog, &comment, None);
            if crate::clipboard_win32::copy_text(window, &text).is_ok() {
                state.announce(&state.text("comment_copied"));
            }
        }
        COPY_ALL => {
            let visible = state.comments.visible_comments();
            if visible.is_empty() {
                state.announce(&state.text(state.comments.empty_message_key()));
                return;
            }
            let text = comments_copy_text(&state.options.catalog, &visible);
            let count = visible.len();
            if crate::clipboard_win32::copy_text(window, &text).is_ok() {
                state.announce(
                    &state
                        .text("comments_copied")
                        .replace("{count}", &count.to_string()),
                );
            }
        }
        AUTHOR => {
            let url = selected
                .map(|comment| comment.author_channel_url.trim().to_owned())
                .unwrap_or_default();
            let key = if open_http_url(&url) {
                "comment_author_channel_opened"
            } else {
                "comment_author_channel_unavailable"
            };
            state.announce(&state.text(key));
        }
        MORE => load_more(window, state),
        BACK => {
            let _ = DestroyWindow(window);
        }
        _ => {}
    }
}

/// Python `open_remote_url_in_browser(url, announce_error=False)`.
fn open_http_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some()
        && std::process::Command::new("explorer.exe")
            .arg(parsed.as_str())
            .spawn()
            .is_ok()
}

/// Python `show_comments_context_menu` on the list.
unsafe fn show_context_menu(window: HWND, state: &mut State, keyboard: bool) {
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let has_comments = !state.comments.visible_comments().is_empty();
    let author = state
        .selected_row()
        .and_then(|row| state.comments.selected(row))
        .is_some_and(|comment| !comment.author_channel_url.trim().is_empty());
    let entries = [
        (OPEN, "open_comment", has_comments),
        (COPY, "copy_comment", has_comments),
        (COPY_ALL, "copy_visible_comments", has_comments),
        (AUTHOR, "open_comment_author_channel", author),
        (0, "", false),
        (MORE, "load_more_comments", state.comments.more_enabled()),
    ];
    for (id, key, enabled) in entries {
        if id == 0 {
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
            continue;
        }
        let label = wide(&state.text(key));
        let flags = if enabled {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        };
        let _ = AppendMenuW(menu, flags, id, PCWSTR(label.as_ptr()));
    }
    let mut point = POINT::default();
    if keyboard {
        let mut item = RECT::default();
        let row = state.selected_row().unwrap_or(0);
        SendMessageW(
            state.list(),
            LB_GETITEMRECT,
            Some(WPARAM(row)),
            Some(LPARAM((&raw mut item) as isize)),
        );
        point.x = item.left + 16;
        point.y = item.bottom;
        let _ = ClientToScreen(state.list(), &raw mut point);
    } else {
        let _ = GetCursorPos(&raw mut point);
    }
    let selected = TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
        point.x,
        point.y,
        None,
        window,
        None,
    );
    let _ = DestroyMenu(menu);
    let selected = usize::try_from(selected.0).unwrap_or_default();
    if (OPEN..=MORE).contains(&selected) {
        command(window, state, selected);
    }
}

unsafe fn window_text(control: HWND) -> String {
    let mut text = vec![0; usize::try_from(GetWindowTextLengthW(control)).unwrap_or_default() + 1];
    let count = GetWindowTextW(control, &mut text);
    String::from_utf16_lossy(&text[..usize::try_from(count).unwrap_or_default()])
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut State;
    if pointer.is_null() {
        return DefWindowProcW(window, message, wparam, lparam);
    }
    let state = &mut *pointer;
    match message {
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            let notification = u32::try_from((wparam.0 >> 16) & 0xffff).unwrap_or_default();
            match (id, notification) {
                (SEARCH, EN_CHANGE) => {
                    let query = window_text(state.controls[0]);
                    state.comments.set_query(&query);
                    refresh(state, 0);
                }
                (SORT, CBN_SELCHANGE) => {
                    let index = SendMessageW(state.controls[1], CB_GETCURSEL, None, None).0;
                    if let Some(sort) = usize::try_from(index)
                        .ok()
                        .and_then(|index| CommentSort::ALL.get(index))
                    {
                        state.comments.set_sort(*sort);
                    }
                    refresh(state, 0);
                }
                (LIST, LBN_SELCHANGE) => {
                    let author = state
                        .selected_row()
                        .and_then(|row| state.comments.selected(row))
                        .is_some_and(|comment| !comment.author_channel_url.trim().is_empty());
                    enable(state, AUTHOR, author);
                }
                (LIST, LBN_DBLCLK) => command(window, state, OPEN),
                (OPEN..=BACK, _) if IsWindowEnabled(state.control(id)).as_bool() => {
                    command(window, state, id);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_CONTEXTMENU if HWND(wparam.0 as *mut _) == state.list() => {
            show_context_menu(window, state, lparam.0 == -1);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == POLL_TIMER => {
            poll(window, state);
            LRESULT(0)
        }
        WM_SIZE => {
            layout(window, state);
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

unsafe fn layout(window: HWND, state: &State) {
    let mut rect = RECT::default();
    if GetClientRect(window, &raw mut rect).is_err() {
        return;
    }
    let width = (rect.right - 16).max(1);
    let sort_width = 200.min(width / 3);
    let _ = MoveWindow(
        state.controls[0],
        8,
        8,
        (width - sort_width - 8).max(1),
        26,
        true,
    );
    let _ = MoveWindow(
        state.controls[1],
        width - sort_width + 8,
        8,
        sort_width,
        200,
        true,
    );
    let height = (rect.bottom - 90).max(30);
    let _ = MoveWindow(state.controls[2], 8, 42, width, height, true);
    let button_width = ((width - 40) / 6).max(1);
    for (offset, control) in state.controls[3..].iter().enumerate() {
        let offset = i32::try_from(offset).unwrap_or_default();
        let _ = MoveWindow(
            *control,
            8 + offset * (button_width + 8),
            50 + height,
            button_width,
            30,
            true,
        );
    }
}

/// Python `show_comment_details`: a read-only text and Back.
// Keep window ownership and its nested message-loop cleanup together.
#[allow(clippy::too_many_lines)]
unsafe fn show_details(owner: HWND, title: &str, back: &str, text: &str) -> Result<()> {
    let instance = HINSTANCE(GetModuleHandleW(None)?.0);
    register(instance, DETAILS_CLASS, details_proc)?;
    let title_text = wide(title);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        DETAILS_CLASS,
        PCWSTR(title_text.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        640,
        460,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let body = wide(text);
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let created = (|| -> Result<(HWND, HWND)> {
        // Rich Edit keeps long comments whole, like Python's `TE_RICH2`.
        let text_control = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("RICHEDIT50W"),
            PCWSTR(body.as_ptr()),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WS_VSCROLL
                | WS_HSCROLL
                | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32),
            0,
            0,
            100,
            100,
            Some(window),
            Some(HMENU(DETAILS_TEXT as *mut _)),
            Some(instance),
            None,
        )?;
        let back_label = wide(back);
        let back_button = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            PCWSTR(back_label.as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            0,
            100,
            30,
            Some(window),
            Some(HMENU(DETAILS_BACK as *mut _)),
            Some(instance),
            None,
        )?;
        Ok((text_control, back_button))
    })();
    let (text_control, back_button) = match created {
        Ok(controls) => controls,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    for control in [text_control, back_button] {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    crate::accessibility_win32::annotate_control_name(text_control, title);
    layout_details(window);
    let previous = GetFocus();
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetFocus(Some(text_control));
    let mut message = MSG::default();
    let mut error = None;
    while IsWindow(Some(window)).as_bool() {
        let result = GetMessageW(&raw mut message, None, 0, 0);
        if result.0 <= 0 {
            if result.0 < 0 {
                error = Some(windows::core::Error::from_thread());
            } else {
                PostQuitMessage(i32::try_from(message.wParam.0).unwrap_or_default());
            }
            let _ = DestroyWindow(window);
            break;
        }
        if message.message == WM_KEYDOWN {
            if message.wParam.0 == usize::from(VK_ESCAPE.0) {
                let _ = DestroyWindow(window);
                continue;
            }
            // A multi-line edit keeps Tab; wxWidgets moves on instead.
            if message.wParam.0 == usize::from(VK_TAB.0) && message.hwnd == text_control {
                let _ = SetFocus(Some(back_button));
                continue;
            }
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
    if IsWindow(Some(previous)).as_bool() {
        let _ = SetFocus(Some(previous));
    }
    error.map_or(Ok(()), Err)
}

unsafe extern "system" fn details_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND if wparam.0 & 0xffff == DETAILS_BACK => {
            let _ = DestroyWindow(window);
            LRESULT(0)
        }
        WM_SIZE => {
            layout_details(window);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(window);
            LRESULT(0)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn layout_details(window: HWND) {
    let mut rect = RECT::default();
    if GetClientRect(window, &raw mut rect).is_err() {
        return;
    }
    let width = (rect.right - 16).max(1);
    let height = (rect.bottom - 54).max(30);
    if let Ok(text) =
        windows::Win32::UI::WindowsAndMessaging::GetDlgItem(Some(window), DETAILS_TEXT_ID)
    {
        let _ = MoveWindow(text, 8, 8, width, height, true);
    }
    if let Ok(back) =
        windows::Win32::UI::WindowsAndMessaging::GetDlgItem(Some(window), DETAILS_BACK_ID)
    {
        let _ = MoveWindow(back, rect.right - 128, 16 + height, 120, 30, true);
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
