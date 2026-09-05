//! Native accessible playback-queue dialog.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use apricot_core::MediaItem;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, SetFocus, VK_ESCAPE, VK_RETURN},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                AppendMenuW, BS_DEFPUSHBUTTON, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW,
                DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GetClientRect,
                GetCursorPos, GetMessageW, GetParent, GetWindowLongPtrW, HMENU, IDC_ARROW,
                IsDialogMessageW, IsWindow, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT,
                LB_SETCURSEL, LBN_DBLCLK, LBS_NOTIFY, LoadCursorW, MF_STRING, MSG, MoveWindow,
                PostQuitMessage, RegisterClassW, SW_SHOW, SendMessageW, SetForegroundWindow,
                SetWindowLongPtrW, ShowWindow, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
                TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX,
                WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_KEYDOWN, WM_NCDESTROY,
                WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW,
                WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaPlaybackQueueWindow");
const ID_LIST: usize = 1501;
const ID_PLAY: usize = 1502;
const ID_MOVE_UP: usize = 1503;
const ID_MOVE_DOWN: usize = 1504;
const ID_REMOVE: usize = 1505;
const ID_CLEAR: usize = 1506;
const ID_BACK: usize = 1507;

pub struct PlaybackQueueDialogLabels {
    pub title: String,
    pub instructions: String,
    pub empty: String,
    pub play: String,
    pub move_up: String,
    pub move_down: String,
    pub remove: String,
    pub clear: String,
    pub back: String,
    pub channel: String,
    pub audio: String,
    pub video: String,
    pub live_stream: String,
    pub playlist: String,
    pub channel_kind: String,
    pub podcast_feed: String,
    pub podcast_episode: String,
    pub movie: String,
    pub tv_show: String,
    pub tv_episode: String,
    pub unknown: String,
}

pub struct PlaybackQueueDialogOutcome {
    pub items: Vec<MediaItem>,
    pub changed: bool,
    pub play: Option<MediaItem>,
}

struct PlaybackQueueDialogState {
    labels: PlaybackQueueDialogLabels,
    items: Vec<MediaItem>,
    changed: bool,
    play: Option<MediaItem>,
    instructions: HWND,
    list: HWND,
    play_button: HWND,
    move_up: HWND,
    move_down: HWND,
    remove: HWND,
    clear: HWND,
    back: HWND,
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

pub unsafe fn show(
    owner: HWND,
    items: Vec<MediaItem>,
    labels: PlaybackQueueDialogLabels,
) -> Result<PlaybackQueueDialogOutcome> {
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
        760,
        520,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let state = match create_controls(window, instance, items, labels) {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let initial_focus = state.list;
    let state_pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), state_pointer as isize);
    refresh_list(window, 0);
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
    Ok(PlaybackQueueDialogOutcome {
        items: state.items,
        changed: state.changed,
        play: state.play,
    })
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
            if command == ID_PLAY
                || (command == ID_LIST
                    && notification == usize::try_from(LBN_DBLCLK).expect("notification fits"))
            {
                play_selected(window);
            } else if command == ID_MOVE_UP {
                move_selected(window, -1);
            } else if command == ID_MOVE_DOWN {
                move_selected(window, 1);
            } else if command == ID_REMOVE {
                remove_selected(window);
            } else if command == ID_CLEAR {
                clear_queue(window);
            } else if command == ID_BACK {
                let _ = DestroyWindow(window);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(window);
            LRESULT(0)
        }
        WM_CONTEXTMENU => {
            show_context_menu(window);
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
    items: Vec<MediaItem>,
    labels: PlaybackQueueDialogLabels,
) -> Result<PlaybackQueueDialogState> {
    let instructions = create_control(
        parent,
        instance,
        w!("STATIC"),
        &labels.instructions,
        WS_CHILD | WS_VISIBLE,
        WINDOW_EX_STYLE::default(),
        0,
    )?;
    let list = create_control(
        parent,
        instance,
        w!("LISTBOX"),
        &labels.title,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
        WS_EX_CLIENTEDGE,
        ID_LIST,
    )?;
    let play_button = create_button(parent, instance, &labels.play, ID_PLAY, true)?;
    let move_up = create_button(parent, instance, &labels.move_up, ID_MOVE_UP, false)?;
    let move_down = create_button(parent, instance, &labels.move_down, ID_MOVE_DOWN, false)?;
    let remove = create_button(parent, instance, &labels.remove, ID_REMOVE, false)?;
    let clear = create_button(parent, instance, &labels.clear, ID_CLEAR, false)?;
    let back = create_button(parent, instance, &labels.back, ID_BACK, false)?;
    if !SetWindowSubclass(list, Some(list_proc), 1, 0).as_bool() {
        return Err(windows::core::Error::from_thread());
    }
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in [
        instructions,
        list,
        play_button,
        move_up,
        move_down,
        remove,
        clear,
        back,
    ] {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    Ok(PlaybackQueueDialogState {
        labels,
        items,
        changed: false,
        play: None,
        instructions,
        list,
        play_button,
        move_up,
        move_down,
        remove,
        clear,
        back,
    })
}

unsafe fn create_button(
    parent: HWND,
    instance: HINSTANCE,
    label: &str,
    id: usize,
    default: bool,
) -> Result<HWND> {
    let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP;
    if default {
        style |= WINDOW_STYLE(BS_DEFPUSHBUTTON as u32);
    }
    create_control(
        parent,
        instance,
        w!("BUTTON"),
        label,
        style,
        WINDOW_EX_STYLE::default(),
        id,
    )
}

unsafe fn create_control(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    label: &str,
    style: WINDOW_STYLE,
    extended_style: WINDOW_EX_STYLE,
    id: usize,
) -> Result<HWND> {
    let label = wide(label);
    CreateWindowExW(
        extended_style,
        class,
        PCWSTR(label.as_ptr()),
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
        if wparam.0 == usize::from(VK_ESCAPE.0) {
            let _ = DestroyWindow(parent);
            return LRESULT(0);
        }
        if wparam.0 == usize::from(VK_RETURN.0) {
            play_selected(parent);
            return LRESULT(0);
        }
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(control, Some(list_proc), subclass_id);
    }
    DefSubclassProc(control, message, wparam, lparam)
}

unsafe fn selected_index(state: &PlaybackQueueDialogState) -> Option<usize> {
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    let selected = usize::try_from(selected).ok()?;
    (selected < state.items.len()).then_some(selected)
}

unsafe fn play_selected(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(index) = selected_index(state) else {
        return;
    };
    state.play = state.items.get(index).cloned();
    let _ = DestroyWindow(window);
}

unsafe fn move_selected(window: HWND, delta: i32) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(index) = selected_index(state) else {
        return;
    };
    let target = i64::try_from(index).ok().and_then(|index| {
        usize::try_from(index + i64::from(delta))
            .ok()
            .filter(|target| *target < state.items.len())
    });
    let Some(target) = target else {
        return;
    };
    state.items.swap(index, target);
    state.changed = true;
    refresh_list(window, target);
}

unsafe fn remove_selected(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(index) = selected_index(state) else {
        return;
    };
    state.items.remove(index);
    state.changed = true;
    refresh_list(window, index.min(state.items.len().saturating_sub(1)));
}

unsafe fn clear_queue(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.items.is_empty() {
        return;
    }
    state.items.clear();
    state.changed = true;
    refresh_list(window, 0);
}

unsafe fn show_context_menu(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    for (id, label) in [
        (ID_PLAY, &state.labels.play),
        (ID_MOVE_UP, &state.labels.move_up),
        (ID_MOVE_DOWN, &state.labels.move_down),
        (ID_REMOVE, &state.labels.remove),
        (ID_CLEAR, &state.labels.clear),
    ] {
        let label = wide(label);
        let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(label.as_ptr()));
    }
    let mut point = POINT::default();
    if GetCursorPos(&raw mut point).is_ok() {
        let selected = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
            point.x,
            point.y,
            None,
            window,
            None,
        );
        match usize::try_from(selected.0).unwrap_or_default() {
            ID_PLAY => play_selected(window),
            ID_MOVE_UP => move_selected(window, -1),
            ID_MOVE_DOWN => move_selected(window, 1),
            ID_REMOVE => remove_selected(window),
            ID_CLEAR => clear_queue(window),
            _ => {}
        }
    }
    let _ = DestroyMenu(menu);
}

unsafe fn refresh_list(window: HWND, selection: usize) {
    let Some(state) = state(window) else {
        return;
    };
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    if state.items.is_empty() {
        add_list_string(state.list, &state.labels.empty);
    } else {
        for (index, item) in state.items.iter().enumerate() {
            let mut parts = vec![format!("{}. {}", index + 1, item.title)];
            if !item.channel.is_empty() {
                parts.push(format!("{}: {}", state.labels.channel, item.channel));
            }
            parts.push(kind_label(item.kind, &state.labels).to_owned());
            add_list_string(state.list, &parts.join(" | "));
        }
    }
    let selection = selection.min(state.items.len().saturating_sub(1));
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selection)), None);
    let _ = SetFocus(Some(state.list));
}

fn kind_label(kind: apricot_core::MediaKind, labels: &PlaybackQueueDialogLabels) -> &str {
    match kind {
        apricot_core::MediaKind::Audio => &labels.audio,
        apricot_core::MediaKind::Video => &labels.video,
        apricot_core::MediaKind::LiveStream => &labels.live_stream,
        apricot_core::MediaKind::Playlist => &labels.playlist,
        apricot_core::MediaKind::Channel => &labels.channel_kind,
        apricot_core::MediaKind::PodcastFeed => &labels.podcast_feed,
        apricot_core::MediaKind::PodcastEpisode => &labels.podcast_episode,
        apricot_core::MediaKind::Movie => &labels.movie,
        apricot_core::MediaKind::TvShow => &labels.tv_show,
        apricot_core::MediaKind::TvEpisode => &labels.tv_episode,
        apricot_core::MediaKind::Unknown => &labels.unknown,
    }
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

unsafe fn layout(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let mut bounds = RECT::default();
    if GetClientRect(window, &raw mut bounds).is_err() {
        return;
    }
    let width = (bounds.right - bounds.left).max(620);
    let height = (bounds.bottom - bounds.top).max(360);
    let margin = 12;
    let instructions_height = 34;
    let button_height = 34;
    let gap = 6;
    let button_width = ((width - margin * 2 - gap * 5) / 6).max(88);
    let _ = MoveWindow(
        state.instructions,
        margin,
        margin,
        width - margin * 2,
        instructions_height,
        true,
    );
    let list_top = margin + instructions_height;
    let button_top = height - margin - button_height;
    let _ = MoveWindow(
        state.list,
        margin,
        list_top,
        width - margin * 2,
        button_top - list_top - margin,
        true,
    );
    for (index, control) in [
        state.play_button,
        state.move_up,
        state.move_down,
        state.remove,
        state.clear,
        state.back,
    ]
    .into_iter()
    .enumerate()
    {
        let left = margin + i32::try_from(index).unwrap_or_default() * (button_width + gap);
        let _ = MoveWindow(control, left, button_top, button_width, button_height, true);
    }
}

unsafe fn state(window: HWND) -> Option<&'static PlaybackQueueDialogState> {
    let pointer =
        GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const PlaybackQueueDialogState;
    pointer.as_ref()
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut PlaybackQueueDialogState> {
    let pointer =
        GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut PlaybackQueueDialogState;
    pointer.as_mut()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
