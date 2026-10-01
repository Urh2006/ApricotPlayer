//! Spotify queue dialog (`docs/SPOTIFY_PLAN.md` 4.2, 5.1), laid out like the
//! Apricot playback-queue dialog: list, Play, Move up, Move down, Remove,
//! Clear and Back. Connect owns the queue: every edit goes to it and the list
//! shows only the confirmed state, which [`update`] brings in, also for
//! changes made on another device. The focus never moves on an update.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of, sync::Arc};

use apricot_app::spotify::{SpotifyQueueRow, queue_menu};
use apricot_spotify::{QueueEdit, SpotifyPlayback};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, SetFocus, VK_DELETE, VK_ESCAPE, VK_RETURN},
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

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaSpotifyQueueWindow");
const ID_LIST: usize = 1601;
const ID_PLAY: usize = 1602;
const ID_MOVE_UP: usize = 1603;
const ID_MOVE_DOWN: usize = 1604;
const ID_REMOVE: usize = 1605;
const ID_CLEAR: usize = 1606;
const ID_BACK: usize = 1607;
/// `IsDialogMessageW` turns Enter and Escape into `IDOK` and `IDCANCEL`.
const IDOK_COMMAND: usize = 1;
const IDCANCEL_COMMAND: usize = 2;

pub struct SpotifyQueueDialogLabels {
    pub title: String,
    pub instructions: String,
    pub play: String,
    pub move_up: String,
    pub move_down: String,
    pub remove: String,
    pub clear: String,
    pub back: String,
    pub removed: String,
    pub moved: String,
    pub cleared: String,
    pub changed: String,
}

/// What the dialog needs from the Spotify session.
pub struct SpotifyQueueActions {
    pub playback: Arc<SpotifyPlayback>,
    /// Asks for the confirmed queue; it arrives through [`update`].
    pub reload: Box<dyn Fn()>,
}

struct SpotifyQueueDialogState {
    labels: SpotifyQueueDialogLabels,
    actions: SpotifyQueueActions,
    announcer: crate::announcement_win32::WindowsAnnouncer,
    rows: Vec<SpotifyQueueRow>,
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

/// Shows the dialog modally. `opened` receives its window, so the owner can
/// send it updates while it is open.
pub unsafe fn show(
    owner: HWND,
    rows: Vec<SpotifyQueueRow>,
    labels: SpotifyQueueDialogLabels,
    actions: SpotifyQueueActions,
    opened: impl FnOnce(HWND),
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
        760,
        520,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let state = match create_controls(window, instance, rows, labels, actions) {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let initial_focus = state.list;
    let state_pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), state_pointer as isize);
    fill_list(window, initial_selection(window));
    layout(window);
    opened(window);
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
    drop(Box::from_raw(state_pointer));
    loop_error.map_or(Ok(()), Err)
}

/// The confirmed queue changed. The same occurrence stays selected, or the
/// same position when it is gone; an unchanged list is left alone, so the
/// screen reader does not repeat the row.
pub unsafe fn update(window: HWND, rows: Vec<SpotifyQueueRow>) {
    let Some(state) = state_mut(window) else {
        return;
    };
    if state.rows == rows {
        return;
    }
    let index = selected_index(state).unwrap_or(0);
    let selected = state.rows.get(index).cloned();
    state.rows = rows;
    let selection = match selected {
        Some(SpotifyQueueRow::Entry { uid, .. }) => state
            .rows
            .iter()
            .position(|row| row.uid() == Some(uid.as_str()))
            .unwrap_or(index),
        _ => index,
    };
    fill_list(window, selection);
}

/// The first upcoming track; the playing one when nothing comes next.
unsafe fn initial_selection(window: HWND) -> usize {
    state(window)
        .and_then(|state| state.rows.iter().position(|row| row.uid().is_some()))
        .unwrap_or(0)
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
                || command == IDOK_COMMAND
                || (command == ID_LIST
                    && notification == usize::try_from(LBN_DBLCLK).expect("notification fits"))
            {
                play_selected(window);
            } else if command == ID_MOVE_UP {
                move_selected(window, true);
            } else if command == ID_MOVE_DOWN {
                move_selected(window, false);
            } else if command == ID_REMOVE {
                remove_selected(window);
            } else if command == ID_CLEAR {
                clear_manual(window);
            } else if command == ID_BACK || command == IDCANCEL_COMMAND {
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
    rows: Vec<SpotifyQueueRow>,
    labels: SpotifyQueueDialogLabels,
    actions: SpotifyQueueActions,
) -> Result<SpotifyQueueDialogState> {
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
    crate::accessibility_win32::set_control_name(list, &labels.title);
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
    Ok(SpotifyQueueDialogState {
        labels,
        actions,
        announcer: crate::announcement_win32::WindowsAnnouncer::new(HWND::default()),
        rows,
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
        if wparam.0 == usize::from(VK_DELETE.0) {
            remove_selected(parent);
            return LRESULT(0);
        }
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(control, Some(list_proc), subclass_id);
    }
    DefSubclassProc(control, message, wparam, lparam)
}

unsafe fn selected_index(state: &SpotifyQueueDialogState) -> Option<usize> {
    let selected = SendMessageW(state.list, LB_GETCURSEL, None, None).0;
    let selected = usize::try_from(selected).ok()?;
    (selected < state.rows.len()).then_some(selected)
}

/// The selected upcoming track: its UID and row text.
unsafe fn selected_entry(window: HWND) -> Option<(usize, String, String)> {
    let state = state(window)?;
    let index = selected_index(state)?;
    let row = state.rows.get(index)?;
    Some((index, row.uid()?.to_owned(), row.label().to_owned()))
}

/// The edit did not apply: the queue changed meanwhile. The list reloads.
unsafe fn report_changed(window: HWND) {
    announce(window, |labels| labels.changed.clone());
    if let Some(state) = state(window) {
        (state.actions.reload)();
    }
}

unsafe fn play_selected(window: HWND) {
    let Some((_, uid, _)) = selected_entry(window) else {
        return;
    };
    let played = state(window).is_some_and(|state| state.actions.playback.play_queue_entry(&uid));
    if played {
        // The player announces the new track once it plays.
        let _ = DestroyWindow(window);
    } else {
        report_changed(window);
    }
}

unsafe fn move_selected(window: HWND, up: bool) {
    let Some((index, uid, _)) = selected_entry(window) else {
        return;
    };
    let allowed = state(window).is_some_and(|state| {
        let menu = queue_menu(&state.rows, index);
        if up { menu.move_up } else { menu.move_down }
    });
    if !allowed {
        return;
    }
    let edit = QueueEdit::Move { uid, up };
    if !state(window).is_some_and(|state| state.actions.playback.edit_queue(&edit)) {
        report_changed(window);
        return;
    }
    // Shown at once; the confirmed state replaces it with the same order.
    if let Some(state) = state_mut(window) {
        let target = if up { index - 1 } else { index + 1 };
        state.rows.swap(index, target);
        fill_list(window, target);
    }
    announce(window, |labels| labels.moved.clone());
}

unsafe fn remove_selected(window: HWND) {
    let Some((index, uid, label)) = selected_entry(window) else {
        return;
    };
    if !state(window)
        .is_some_and(|state| state.actions.playback.edit_queue(&QueueEdit::Remove(uid)))
    {
        report_changed(window);
        return;
    }
    if let Some(state) = state_mut(window) {
        state.rows.remove(index);
        let selection = index.min(state.rows.len().saturating_sub(1));
        fill_list(window, selection);
    }
    // The row without its section: "title, artists".
    let title = label
        .rsplit_once(", ")
        .map_or(label.as_str(), |(title, _)| title);
    announce(window, |labels| labels.removed.replace("{title}", title));
}

unsafe fn clear_manual(window: HWND) {
    let clear = state(window).is_some_and(|state| queue_menu(&state.rows, 0).clear);
    if !clear {
        return;
    }
    if !state(window)
        .is_some_and(|state| state.actions.playback.edit_queue(&QueueEdit::ClearManual))
    {
        report_changed(window);
        return;
    }
    if let Some(state) = state_mut(window) {
        state.rows.retain(|row| {
            !matches!(
                row,
                SpotifyQueueRow::Entry {
                    section: apricot_spotify::QueueSection::Manual,
                    ..
                }
            )
        });
        fill_list(window, initial_selection(window));
    }
    announce(window, |labels| labels.cleared.clone());
}

unsafe fn announce(window: HWND, message: impl FnOnce(&SpotifyQueueDialogLabels) -> String) {
    if let Some(state) = state(window) {
        state.announcer.announce(&message(&state.labels), true);
    }
}

/// Applications key, Shift+F10 and the right button: only what the selected
/// row supports.
unsafe fn show_context_menu(window: HWND) {
    let Some(state) = state(window) else {
        return;
    };
    let menu_items = queue_menu(&state.rows, selected_index(state).unwrap_or(usize::MAX));
    let entries = [
        (ID_PLAY, &state.labels.play, menu_items.play),
        (ID_MOVE_UP, &state.labels.move_up, menu_items.move_up),
        (ID_MOVE_DOWN, &state.labels.move_down, menu_items.move_down),
        (ID_REMOVE, &state.labels.remove, menu_items.remove),
        (ID_CLEAR, &state.labels.clear, menu_items.clear),
    ];
    if !entries.iter().any(|(_, _, shown)| *shown) {
        return;
    }
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    for (id, label, shown) in entries {
        if shown {
            let label = wide(label);
            let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(label.as_ptr()));
        }
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
            ID_MOVE_UP => move_selected(window, true),
            ID_MOVE_DOWN => move_selected(window, false),
            ID_REMOVE => remove_selected(window),
            ID_CLEAR => clear_manual(window),
            _ => {}
        }
    }
    let _ = DestroyMenu(menu);
}

unsafe fn fill_list(window: HWND, selection: usize) {
    let Some(state) = state(window) else {
        return;
    };
    SendMessageW(state.list, LB_RESETCONTENT, None, None);
    for row in &state.rows {
        add_list_string(state.list, row.label());
    }
    let selection = selection.min(state.rows.len().saturating_sub(1));
    SendMessageW(state.list, LB_SETCURSEL, Some(WPARAM(selection)), None);
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

unsafe fn state(window: HWND) -> Option<&'static SpotifyQueueDialogState> {
    let pointer =
        GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *const SpotifyQueueDialogState;
    pointer.as_ref()
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut SpotifyQueueDialogState> {
    let pointer =
        GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut SpotifyQueueDialogState;
    pointer.as_mut()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
