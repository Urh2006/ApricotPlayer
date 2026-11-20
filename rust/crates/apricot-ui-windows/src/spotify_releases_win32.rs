//! New releases of followed artists (2.1, RC 4): the account's followed
//! artists as a list of check boxes, loaded page by page as the selection
//! moves down, with Turn on for all and Turn off for all. Every change goes
//! to the playlist subscriptions document, whose check announces the
//! releases in the notification center.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{
    ffi::c_void,
    mem::size_of,
    sync::{Arc, mpsc::Receiver},
};

use apricot_spotify::{
    CatalogPage, SpotifyService,
    playlist_subscriptions::{ArtistReleaseOptions, SubscriptionCommand},
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{
                LVCF_WIDTH, LVCOLUMNW, LVIF_STATE, LVIF_TEXT, LVIS_FOCUSED, LVIS_SELECTED,
                LVIS_STATEIMAGEMASK, LVITEMW, LVM_INSERTCOLUMNW, LVM_INSERTITEMW,
                LVM_SETCOLUMNWIDTH, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMSTATE,
                LVN_ITEMCHANGED, LVS_EX_CHECKBOXES, LVS_EX_FULLROWSELECT, LVS_NOCOLUMNHEADER,
                LVS_REPORT, LVS_SHOWSELALWAYS, LVS_SINGLESEL, NMHDR, NMLISTVIEW,
            },
            Input::KeyboardAndMouse::{EnableWindow, SetFocus, VK_ESCAPE},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect,
                GetMessageW, GetParent, GetWindowLongPtrW, HMENU, IDC_ARROW, IsDialogMessageW,
                IsWindow, KillTimer, LoadCursorW, MSG, MoveWindow, PostQuitMessage, RegisterClassW,
                SW_SHOW, SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW,
                SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE,
                WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_KEYDOWN,
                WM_NCDESTROY, WM_NOTIFY, WM_SETFONT, WM_SIZE, WM_TIMER, WNDCLASSW, WS_CHILD,
                WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaSpotifyReleasesWindow");
const ID_LIST: usize = 1701;
const ID_ALL_ON: usize = 1702;
const ID_ALL_OFF: usize = 1703;
const ID_CLOSE: usize = 1704;
const IDCANCEL_COMMAND: usize = 2;
const LOAD_TIMER: usize = 1;
/// A new page loads when the selection is this close to the last row.
const LOAD_AHEAD: usize = 10;
/// State image indexes of a list view check box.
const UNCHECKED: u32 = 1 << 12;
const CHECKED: u32 = 2 << 12;

pub struct SpotifyReleasesLabels {
    pub title: String,
    pub instructions: String,
    pub list: String,
    pub all_on: String,
    pub all_off: String,
    pub close: String,
    pub all_on_done: String,
    pub all_off_done: String,
    pub loading: String,
    pub none: String,
    pub failed: String,
}

struct Artist {
    uri: String,
}

struct DialogState {
    labels: SpotifyReleasesLabels,
    service: Arc<SpotifyService>,
    send: Box<dyn Fn(SubscriptionCommand)>,
    options: ArtistReleaseOptions,
    announcer: crate::announcement_win32::WindowsAnnouncer,
    artists: Vec<Artist>,
    next_offset: Option<u64>,
    pending: Option<Receiver<std::result::Result<CatalogPage, String>>>,
    /// The list's check boxes are being set by the dialog, not the user.
    filling: bool,
    instructions: HWND,
    list: HWND,
    all_on: HWND,
    all_off: HWND,
    close: HWND,
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

/// Shows the dialog modally over `owner`. `send` saves a change.
pub unsafe fn show(
    owner: HWND,
    labels: SpotifyReleasesLabels,
    service: Arc<SpotifyService>,
    options: ArtistReleaseOptions,
    send: Box<dyn Fn(SubscriptionCommand)>,
) -> Result<()> {
    let previous_focus = windows::Win32::UI::Input::KeyboardAndMouse::GetFocus();
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
        560,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let state = match create_controls(window, instance, labels, service, options, send) {
        Ok(state) => state,
        Err(error) => {
            let _ = crate::accessibility_win32::destroy_modal(window);
            return Err(error);
        }
    };
    let list = state.list;
    let pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), pointer as isize);
    layout(window);
    crate::accessibility_win32::disable_owner(window, owner);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetFocus(Some(list));
    request_page(window, 0);

    let mut loop_error = None;
    let mut message = MSG::default();
    while IsWindow(Some(window)).as_bool() {
        let status = GetMessageW(&raw mut message, None, 0, 0);
        if status.0 == -1 {
            loop_error = Some(windows::core::Error::from_thread());
            let _ = crate::accessibility_win32::destroy_modal(window);
            break;
        }
        if status.0 == 0 {
            let _ = crate::accessibility_win32::destroy_modal(window);
            PostQuitMessage(0);
            break;
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
    // Back on the button that opened the list.
    if IsWindow(Some(previous_focus)).as_bool() {
        let _ = SetFocus(Some(previous_focus));
    }
    drop(Box::from_raw(pointer));
    loop_error.map_or(Ok(()), Err)
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
            match wparam.0 & 0xffff {
                ID_ALL_ON => set_all(window, true),
                ID_ALL_OFF => set_all(window, false),
                ID_CLOSE | IDCANCEL_COMMAND => {
                    let _ = crate::accessibility_win32::destroy_modal(window);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_NOTIFY => {
            let header = &*(lparam.0 as *const NMHDR);
            if header.idFrom == ID_LIST && header.code == LVN_ITEMCHANGED {
                item_changed(window, &*(lparam.0 as *const NMLISTVIEW));
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == LOAD_TIMER => {
            poll_page(window);
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = crate::accessibility_win32::destroy_modal(window);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            let _ = KillTimer(Some(window), LOAD_TIMER);
            SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), 0);
            DefWindowProcW(window, message, wparam, lparam)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn create_controls(
    parent: HWND,
    instance: HINSTANCE,
    labels: SpotifyReleasesLabels,
    service: Arc<SpotifyService>,
    options: ArtistReleaseOptions,
    send: Box<dyn Fn(SubscriptionCommand)>,
) -> Result<DialogState> {
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
        w!("SysListView32"),
        &labels.list,
        WS_CHILD
            | WS_VISIBLE
            | WS_TABSTOP
            | WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS | LVS_NOCOLUMNHEADER),
        WS_EX_CLIENTEDGE,
        ID_LIST,
    )?;
    let extended = LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT;
    SendMessageW(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(extended as usize)),
        Some(LPARAM(isize::try_from(extended).unwrap_or_default())),
    );
    let column = LVCOLUMNW {
        mask: LVCF_WIDTH,
        cx: 600,
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_INSERTCOLUMNW,
        Some(WPARAM(0)),
        Some(LPARAM(&raw const column as isize)),
    );
    if !SetWindowSubclass(list, Some(list_proc), 1, 0).as_bool() {
        return Err(windows::core::Error::from_thread());
    }
    crate::accessibility_win32::set_control_name(list, &labels.list);
    let all_on = create_button(parent, instance, &labels.all_on, ID_ALL_ON)?;
    let all_off = create_button(parent, instance, &labels.all_off, ID_ALL_OFF)?;
    let close = create_button(parent, instance, &labels.close, ID_CLOSE)?;
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in [instructions, list, all_on, all_off, close] {
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    Ok(DialogState {
        labels,
        service,
        send,
        options,
        announcer: crate::announcement_win32::WindowsAnnouncer::new(HWND::default()),
        artists: Vec::new(),
        next_offset: Some(0),
        pending: None,
        filling: false,
        instructions,
        list,
        all_on,
        all_off,
        close,
    })
}

/// Asks for the page of followed artists that starts at `offset`.
unsafe fn request_page(window: HWND, offset: u64) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.pending.is_some() || state.next_offset != Some(offset) {
        return;
    }
    state.pending = Some(state.service.followed_artists(offset));
    if offset == 0 {
        set_text(state.instructions, &state.labels.loading);
    }
    let _ = SetTimer(Some(window), LOAD_TIMER, 100, None);
}

unsafe fn poll_page(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(receiver) = state.pending.as_ref() else {
        let _ = KillTimer(Some(window), LOAD_TIMER);
        return;
    };
    let result = match receiver.try_recv() {
        Ok(result) => result,
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(String::new()),
    };
    state.pending = None;
    let _ = KillTimer(Some(window), LOAD_TIMER);
    let first = state.artists.is_empty();
    match result {
        Ok(page) => {
            state.next_offset = page.next_offset;
            let artists: Vec<_> = page
                .items
                .into_iter()
                .filter(|item| item.uri.starts_with("spotify:artist:"))
                .collect();
            state.filling = true;
            for artist in &artists {
                let index = state.artists.len();
                insert_row(
                    state.list,
                    index,
                    &artist.name,
                    state.options.is_enabled(&artist.uri),
                );
                state.artists.push(Artist {
                    uri: artist.uri.clone(),
                });
            }
            state.filling = false;
            if first {
                set_text(state.instructions, &state.labels.instructions);
                if state.artists.is_empty() {
                    let text = state.labels.none.clone();
                    set_text(state.instructions, &text);
                    state.announcer.announce(&text, true);
                } else {
                    select_row(state.list, 0);
                }
            }
            // A page of other library items only: the next one may have
            // artists.
            if artists.is_empty()
                && let Some(offset) = state.next_offset
            {
                request_page(window, offset);
            }
        }
        Err(error) => {
            let text = state.labels.failed.replace("{error}", &error);
            set_text(state.instructions, &text);
            state.announcer.announce(&text, true);
        }
    }
}

/// A check box changed, or the selection moved: a change is saved, and the
/// next page loads when the selection nears the end.
unsafe fn item_changed(window: HWND, change: &NMLISTVIEW) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Ok(index) = usize::try_from(change.iItem) else {
        return;
    };
    let image = |value: u32| value & LVIS_STATEIMAGEMASK.0;
    if !state.filling
        && change.uChanged.0 & LVIF_STATE.0 != 0
        && image(change.uNewState) != image(change.uOldState)
        && image(change.uOldState) != 0
        && let Some(artist) = state.artists.get(index)
    {
        let enabled = image(change.uNewState) == CHECKED;
        let uri = artist.uri.clone();
        state.options.set(&uri, enabled);
        save(state);
    }
    if change.uNewState & LVIS_FOCUSED.0 != 0
        && index + LOAD_AHEAD >= state.artists.len()
        && let Some(offset) = state.next_offset
    {
        request_page(window, offset);
    }
}

unsafe fn set_all(window: HWND, enabled: bool) {
    let Some(state) = state_mut(window) else {
        return;
    };
    state.options.set_all(enabled);
    save(state);
    state.filling = true;
    for index in 0..state.artists.len() {
        set_checked(state.list, index, enabled);
    }
    state.filling = false;
    let text = if enabled {
        state.labels.all_on_done.clone()
    } else {
        state.labels.all_off_done.clone()
    };
    state.announcer.announce(&text, true);
}

fn save(state: &mut DialogState) {
    state.options.revision =
        apricot_spotify::playlist_subscriptions::next_artist_revision(state.options.revision);
    (state.send)(SubscriptionCommand::ArtistReleases {
        revision: state.options.revision,
        all: state.options.all,
        included: state.options.included.clone(),
        excluded: state.options.excluded.clone(),
    });
}

unsafe fn insert_row(list: HWND, index: usize, name: &str, checked: bool) {
    let mut text = wide(name);
    let item = LVITEMW {
        mask: LVIF_TEXT,
        iItem: i32::try_from(index).unwrap_or(i32::MAX),
        pszText: windows::core::PWSTR(text.as_mut_ptr()),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_INSERTITEMW,
        Some(WPARAM(0)),
        Some(LPARAM(&raw const item as isize)),
    );
    set_checked(list, index, checked);
}

unsafe fn set_checked(list: HWND, index: usize, checked: bool) {
    let item = LVITEMW {
        stateMask: LVIS_STATEIMAGEMASK,
        state: windows::Win32::UI::Controls::LIST_VIEW_ITEM_STATE_FLAGS(if checked {
            CHECKED
        } else {
            UNCHECKED
        }),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_SETITEMSTATE,
        Some(WPARAM(index)),
        Some(LPARAM(&raw const item as isize)),
    );
}

unsafe fn select_row(list: HWND, index: usize) {
    let item = LVITEMW {
        stateMask: windows::Win32::UI::Controls::LIST_VIEW_ITEM_STATE_FLAGS(
            LVIS_SELECTED.0 | LVIS_FOCUSED.0,
        ),
        state: windows::Win32::UI::Controls::LIST_VIEW_ITEM_STATE_FLAGS(
            LVIS_SELECTED.0 | LVIS_FOCUSED.0,
        ),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_SETITEMSTATE,
        Some(WPARAM(index)),
        Some(LPARAM(&raw const item as isize)),
    );
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
        && wparam.0 == usize::from(VK_ESCAPE.0)
        && let Ok(parent) = GetParent(control)
    {
        let _ = crate::accessibility_win32::destroy_modal(parent);
        return LRESULT(0);
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(control, Some(list_proc), subclass_id);
    }
    DefSubclassProc(control, message, wparam, lparam)
}

unsafe fn create_button(parent: HWND, instance: HINSTANCE, label: &str, id: usize) -> Result<HWND> {
    create_control(
        parent,
        instance,
        w!("BUTTON"),
        label,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
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

unsafe fn set_text(control: HWND, text: &str) {
    let text = wide(text);
    let _ = SetWindowTextW(control, PCWSTR(text.as_ptr()));
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
    let height = (bounds.bottom - bounds.top).max(360);
    let margin = 12;
    let instructions_height = 52;
    let button_height = 34;
    let gap = 6;
    let button_width = ((width - margin * 2 - gap * 2) / 3).max(120);
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
    SendMessageW(
        state.list,
        LVM_SETCOLUMNWIDTH,
        Some(WPARAM(0)),
        Some(LPARAM(
            isize::try_from(width - margin * 2 - 30).unwrap_or(500),
        )),
    );
    for (index, control) in [state.all_on, state.all_off, state.close]
        .into_iter()
        .enumerate()
    {
        let left = margin + i32::try_from(index).unwrap_or_default() * (button_width + gap);
        let _ = MoveWindow(control, left, button_top, button_width, button_height, true);
    }
}

unsafe fn state(window: HWND) -> Option<&'static DialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const DialogState;
    pointer.as_ref()
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut DialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut DialogState;
    pointer.as_mut()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
