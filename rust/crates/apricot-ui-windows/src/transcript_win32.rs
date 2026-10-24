//! Native transcript dialog. Playback operations remain owned by the caller.
#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use crate::{
    announcement_win32::WindowsAnnouncer,
    transcript_loader::{LoadedTranscript, TranscriptFailure},
};
use apricot_app::transcript::{TranscriptEntry, TranscriptView};
use std::{
    collections::BTreeMap,
    mem::size_of,
    sync::mpsc::{Receiver, TryRecvError},
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus, VK_ESCAPE, VK_RETURN},
            WindowsAndMessaging::{
                CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                EN_CHANGE, ES_AUTOHSCROLL, GetClientRect, GetMessageW, GetWindowLongPtrW,
                GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsChild, IsDialogMessageW,
                IsWindow, KillTimer, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, LB_SETCURSEL,
                LBN_DBLCLK, LBS_NOTIFY, LoadCursorW, MSG, MoveWindow, PostQuitMessage,
                RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow, SetTimer,
                SetWindowLongPtrW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_KEYDOWN,
                WM_NCDESTROY, WM_SETFONT, WM_SIZE, WM_TIMER, WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE,
                WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS: PCWSTR = w!("ApricotPlayer2Transcript");
const SEARCH: usize = 2101;
const LIST: usize = 2102;
const JUMP: usize = 2103;
const COPY_LINE: usize = 2104;
const COPY_ALL: usize = 2105;
const COPY_TIME: usize = 2106;
const BACK: usize = 2107;

pub enum TranscriptAction {
    Seek(TranscriptEntry),
    CopyTimestamp(TranscriptEntry),
}

pub struct TranscriptDialogOptions {
    pub labels: BTreeMap<String, String>,
    pub can_copy_timestamp: bool,
    pub receiver: Receiver<std::result::Result<LoadedTranscript, TranscriptFailure>>,
    pub accept: Option<apricot_core::shortcut::ShortcutChord>,
    pub back: Option<apricot_core::shortcut::ShortcutChord>,
}

struct State<'a> {
    controls: [HWND; 7],
    options: TranscriptDialogOptions,
    view: TranscriptView,
    loaded: Option<LoadedTranscript>,
    waiting: bool,
    announcer: WindowsAnnouncer,
    action: &'a mut dyn FnMut(TranscriptAction) -> String,
}

impl State<'_> {
    fn label(&self, key: &str) -> String {
        self.options
            .labels
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_owned())
    }

    /// Python `finish_load` shows `transcript_failed` with the error text;
    /// rate limiting is itself raised as the `transcript_rate_limited` text.
    fn failure_text(&self, failure: &TranscriptFailure) -> String {
        let error = match failure {
            TranscriptFailure::RateLimited => self.label("transcript_rate_limited"),
            TranscriptFailure::Failed(message) => message.clone(),
        };
        self.label("transcript_failed").replace("{error}", &error)
    }
}

/// Opens the modal native transcript view while a worker supplies its contents.
///
/// # Errors
/// Returns a Windows error if window creation or the message loop fails.
pub fn show(
    owner: HWND,
    options: TranscriptDialogOptions,
    action: &mut dyn FnMut(TranscriptAction) -> String,
) -> Result<Option<LoadedTranscript>> {
    // SAFETY: State remains owned by this call throughout the nested message loop.
    unsafe { show_inner(owner, options, action) }
}

// Keep window ownership and its nested message-loop cleanup together.
#[allow(clippy::too_many_lines)]
unsafe fn show_inner(
    owner: HWND,
    options: TranscriptDialogOptions,
    action: &mut dyn FnMut(TranscriptAction) -> String,
) -> Result<Option<LoadedTranscript>> {
    let instance = HINSTANCE(GetModuleHandleW(None)?.0);
    let class = WNDCLASSW {
        hInstance: instance,
        lpszClassName: CLASS,
        lpfnWndProc: Some(window_proc),
        cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size"),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        ..Default::default()
    };
    // Re-registering the same process-local class is harmless.
    let _ = RegisterClassW(&raw const class);
    let title = wide(
        options
            .labels
            .get("transcript")
            .map_or("Transcript", String::as_str),
    );
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CLASS,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        760,
        560,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let mut state = Box::new(State {
        controls: [HWND::default(); 7],
        options,
        view: TranscriptView::default(),
        loaded: None,
        waiting: true,
        announcer: WindowsAnnouncer::new(HWND::default()),
        action,
    });
    let result = create_controls(window, instance, &mut state);
    if let Err(error) = result {
        let _ = DestroyWindow(window);
        return Err(error);
    }
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), (&raw mut *state) as isize);
    let loading = state.label("transcript_loading");
    refresh(&mut state, Some(loading));
    layout(window, &state);
    let previous = GetFocus();
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetFocus(Some(state.controls[0]));
    let _ = SetTimer(Some(window), 1, 50, None);
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
        if ours && let Some(chord) = crate::shortcut_win32::chord_from_message(&message) {
            if state.options.back == Some(chord) {
                let _ = DestroyWindow(window);
                continue;
            }
            if state.options.accept == Some(chord)
                && [state.controls[0], state.controls[1]].contains(&GetFocus())
            {
                command(window, &mut state, JUMP);
                continue;
            }
        }
        if ours && message.message == WM_KEYDOWN {
            if message.wParam.0 == usize::from(VK_ESCAPE.0) {
                let _ = DestroyWindow(window);
                continue;
            }
            if message.wParam.0 == usize::from(VK_RETURN.0)
                && [state.controls[0], state.controls[1]].contains(&GetFocus())
            {
                command(window, &mut state, JUMP);
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
    if let Some(error) = error {
        return Err(error);
    }
    Ok(state.loaded.take())
}

unsafe fn create_controls(window: HWND, instance: HINSTANCE, state: &mut State<'_>) -> Result<()> {
    let specs = [
        (
            SEARCH,
            w!("EDIT"),
            "transcript_search",
            ES_AUTOHSCROLL as u32,
        ),
        (LIST, w!("LISTBOX"), "transcript", LBS_NOTIFY as u32),
        (JUMP, w!("BUTTON"), "play", 0),
        (COPY_LINE, w!("BUTTON"), "copy_transcript_line", 0),
        (COPY_ALL, w!("BUTTON"), "copy_transcript", 0),
        (COPY_TIME, w!("BUTTON"), "copy_timestamp_link", 0),
        (BACK, w!("BUTTON"), "back", 0),
    ];
    for (index, (id, class, key, style)) in specs.into_iter().enumerate() {
        let label = state.label(key);
        let text = wide(if id == SEARCH { "" } else { &label });
        let control = CreateWindowExW(
            if index < 2 {
                WS_EX_CLIENTEDGE
            } else {
                WINDOW_EX_STYLE::default()
            },
            class,
            PCWSTR(text.as_ptr()),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WINDOW_STYLE(style)
                | if id == LIST {
                    WS_VSCROLL
                } else {
                    WINDOW_STYLE::default()
                },
            0,
            0,
            100,
            28,
            Some(window),
            Some(HMENU(id as *mut _)),
            Some(instance),
            None,
        )?;
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize)),
            Some(LPARAM(1)),
        );
        crate::accessibility_win32::annotate_control_name(control, &label);
        state.controls[index] = control;
    }
    Ok(())
}

unsafe fn refresh(state: &mut State<'_>, placeholder: Option<String>) {
    let list = state.controls[1];
    SendMessageW(list, LB_RESETCONTENT, None, None);
    let labels: Vec<_> = state.view.visible_labels().map(str::to_owned).collect();
    let has_visible = !labels.is_empty();
    let labels = if has_visible {
        labels
    } else {
        vec![placeholder.unwrap_or_else(|| {
            state.label(if state.view.has_entries() {
                "transcript_no_search_results"
            } else {
                "no_transcript_available"
            })
        })]
    };
    for label in labels {
        let text = wide(&label);
        SendMessageW(
            list,
            LB_ADDSTRING,
            None,
            Some(LPARAM(text.as_ptr() as isize)),
        );
    }
    SendMessageW(list, LB_SETCURSEL, Some(WPARAM(0)), None);
    for index in [2, 3] {
        let _ = EnableWindow(state.controls[index], has_visible);
    }
    let _ = EnableWindow(state.controls[4], state.view.has_entries());
    let _ = EnableWindow(
        state.controls[5],
        has_visible && state.options.can_copy_timestamp,
    );
}

unsafe fn command(window: HWND, state: &mut State<'_>, id: usize) {
    if id == BACK {
        let _ = DestroyWindow(window);
        return;
    }
    let row = SendMessageW(state.controls[1], LB_GETCURSEL, None, None).0;
    let selected = usize::try_from(row)
        .ok()
        .and_then(|row| state.view.selected(row));
    let message = match id {
        _ if selected.is_none() && id != COPY_ALL => state.label("no_transcript_available"),
        JUMP | COPY_TIME => {
            let Some((_, entry, _)) = selected else {
                return;
            };
            let action = if id == JUMP {
                TranscriptAction::Seek(entry.clone())
            } else {
                TranscriptAction::CopyTimestamp(entry.clone())
            };
            (state.action)(action)
        }
        COPY_LINE | COPY_ALL => {
            let text = if id == COPY_ALL {
                state.view.full_text()
            } else {
                let Some((_, _, text)) = selected else {
                    return;
                };
                text.to_owned()
            };
            if text.is_empty() {
                state
                    .announcer
                    .announce(&state.label("no_transcript_available"), true);
                return;
            }
            match crate::clipboard_win32::copy_text(window, &text) {
                Ok(()) => state.label(if id == COPY_ALL {
                    "transcript_copied"
                } else {
                    "transcript_line_copied"
                }),
                Err(error) => error.to_string(),
            }
        }
        _ => return,
    };
    state.announcer.announce(&message, true);
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut State<'_>;
    if pointer.is_null() {
        return DefWindowProcW(window, message, wparam, lparam);
    }
    let state = &mut *pointer;
    match message {
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            let notification = (wparam.0 >> 16) & 0xffff;
            if id == SEARCH && notification == EN_CHANGE as usize {
                let mut text = vec![
                    0;
                    usize::try_from(GetWindowTextLengthW(state.controls[0]))
                        .unwrap_or_default()
                        + 1
                ];
                let count = GetWindowTextW(state.controls[0], &mut text);
                state.view.filter(&String::from_utf16_lossy(
                    &text[..usize::try_from(count).unwrap_or_default()],
                ));
                let placeholder = state.waiting.then(|| state.label("transcript_loading"));
                refresh(state, placeholder);
            } else if id == LIST && notification == LBN_DBLCLK as usize {
                command(window, state, JUMP);
            } else if (JUMP..=BACK).contains(&id) {
                command(window, state, id);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            let result = match state.options.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err(TranscriptFailure::Failed(String::new())))
                }
            };
            if let Some(result) = result {
                let _ = KillTimer(Some(window), 1);
                state.waiting = false;
                match result {
                    Ok(loaded) => {
                        let ready = if loaded.entries.is_empty() {
                            state.label("no_transcript_available")
                        } else {
                            state
                                .label("transcript_loaded_from_source")
                                .replace("{count}", &loaded.entries.len().to_string())
                                .replace("{source}", &state.label(loaded.source_key))
                        };
                        state.view = TranscriptView::new(loaded.entries.clone());
                        let mut query =
                            vec![
                                0;
                                usize::try_from(GetWindowTextLengthW(state.controls[0]))
                                    .unwrap_or_default()
                                    + 1
                            ];
                        let count = GetWindowTextW(state.controls[0], &mut query);
                        state.view.filter(&String::from_utf16_lossy(
                            &query[..usize::try_from(count).unwrap_or_default()],
                        ));
                        state.loaded = Some(loaded);
                        refresh(state, None);
                        state.announcer.announce(&ready, true);
                    }
                    Err(failure) => {
                        // Python caches a rate-limited video as checked, so
                        // reopening shows no transcript instead of retrying.
                        if failure == TranscriptFailure::RateLimited {
                            state.loaded = Some(LoadedTranscript {
                                entries: Vec::new(),
                                source_key: "",
                            });
                        }
                        let message = state.failure_text(&failure);
                        refresh(state, Some(message.clone()));
                        state.announcer.announce(&message, true);
                    }
                }
            }
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

unsafe fn layout(window: HWND, state: &State<'_>) {
    let mut rect = RECT::default();
    if GetClientRect(window, &raw mut rect).is_err() {
        return;
    }
    let width = (rect.right - 24).max(1);
    let height = (rect.bottom - 100).max(30);
    let _ = MoveWindow(state.controls[0], 12, 12, width, 28, true);
    let _ = MoveWindow(state.controls[1], 12, 48, width, height, true);
    let button_width = ((width - 32) / 5).max(1);
    for (offset, control) in state.controls[2..].iter().enumerate() {
        let offset = i32::try_from(offset).unwrap_or_default();
        let _ = MoveWindow(
            *control,
            12 + offset * (button_width + 8),
            56 + height,
            button_width,
            30,
            true,
        );
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}
