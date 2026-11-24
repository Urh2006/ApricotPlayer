//! Letter navigation with several letters in every list box, drop-down list
//! and popup menu of the UI thread: typing "slov" quickly selects the first
//! row that starts with "slov", a pause of a second starts a new search and
//! the same letter again moves on to the next row on that letter. The search
//! is [`apricot_shell::TypeAhead`], which the macOS lists and drop-down lists
//! use too.
//!
//! Native list boxes, combo boxes and menus only search by the first letter,
//! so two thread hooks see the typed characters before the control does:
//! `WH_GETMESSAGE` for the message loops of the windows and dialogs, and
//! `WH_MSGFILTER` for the modal loop of a popup menu. The new row is selected
//! the way the control selects it for its own first-letter search, so NVDA
//! hears the same events (measured in the isolated kit, 24. 11. 2026):
//! - list box: `LB_SETCURSEL` gives the item's focus and selection events,
//!   then `LBN_SELCHANGE` goes to the parent;
//! - drop-down list: `CB_SETCURSEL`, then the selection event of its list
//!   (the open list raises it itself, with the focus) and the combo box's
//!   value change, then `CBN_SELCHANGE` goes to the parent;
//! - menu: one letter (and the same letter again) stays with the menu, which
//!   moves to the next item on that letter or runs the only one; from the
//!   second different letter on, `MN_SELECTITEM` moves the highlight, which
//!   gives the item's focus event.
//!
//! A character the search does not take (a control character, or a space
//! that does not continue a search) and every other control keep their own
//! keyboard handling.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{cell::RefCell, time::Instant};

use apricot_shell::TypeAhead;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::Threading::GetCurrentThreadId,
        UI::{
            Accessibility::NotifyWinEvent,
            Controls::{COMBOBOXINFO, GetComboBoxInfo},
            Input::KeyboardAndMouse::{
                VK_APPS, VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_INSERT,
                VK_LEFT, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_TAB, VK_UP,
            },
            WindowsAndMessaging::{
                CB_GETCOUNT, CB_GETCURSEL, CB_GETDROPPEDSTATE, CB_GETLBTEXT, CB_GETLBTEXTLEN,
                CB_SETCURSEL, CBN_SELCHANGE, CBS_DROPDOWNLIST, CHILDID_SELF, CallNextHookEx,
                EVENT_OBJECT_SELECTION, EVENT_OBJECT_VALUECHANGE, EnumThreadWindows,
                GUI_INMENUMODE, GUITHREADINFO, GWL_STYLE, GetClassNameW, GetDlgCtrlID,
                GetGUIThreadInfo, GetMenuItemCount, GetMenuState, GetMenuStringW, GetParent,
                GetSubMenu, GetWindowLongPtrW, HC_ACTION, HMENU, IsWindowVisible, LB_GETCOUNT,
                LB_GETCURSEL, LB_GETTEXT, LB_GETTEXTLEN, LB_SETCURSEL, LBN_SELCHANGE,
                LBS_EXTENDEDSEL, LBS_MULTIPLESEL, LBS_NODATA, LBS_NOSEL, LBS_NOTIFY, MF_BYPOSITION,
                MF_HILITE, MF_SEPARATOR, MSG, MSGF_MENU, OBJID_CLIENT, PM_REMOVE, SendMessageW,
                SetWindowsHookExW, WH_GETMESSAGE, WH_MSGFILTER, WM_CHAR, WM_COMMAND, WM_KEYDOWN,
                WM_NULL,
            },
        },
    },
    core::BOOL,
};

/// `MN_GETHMENU` is documented; `MN_SELECTITEM` is not, but the menu window
/// has handled it the same way since Windows 95.
const MN_GETHMENU: u32 = 0x01E1;
const MN_SELECTITEM: u32 = 0x01E5;

thread_local! {
    static SEARCH: RefCell<Search> = RefCell::default();
}

#[derive(Default)]
struct Search {
    typing: TypeAhead,
    /// The list, drop-down list or menu the letters were typed in.
    target: isize,
}

/// Installs the hooks on the calling (UI) thread for the rest of the run.
pub fn install() {
    // SAFETY: thread hooks of the calling thread with procedures of this
    // module; they stay installed until the thread ends.
    unsafe {
        let thread = GetCurrentThreadId();
        let _ = SetWindowsHookExW(WH_GETMESSAGE, Some(get_message_hook), None, thread);
        let _ = SetWindowsHookExW(WH_MSGFILTER, Some(message_filter_hook), None, thread);
    }
}

unsafe extern "system" fn get_message_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION.cast_signed() && wparam.0 == PM_REMOVE.0 as usize && lparam.0 != 0 {
        // SAFETY: for HC_ACTION, lparam points to the MSG being removed from
        // the queue, which the hook may change.
        let message = &mut *(lparam.0 as *mut MSG);
        if list_message(message) {
            message.message = WM_NULL;
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn message_filter_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == MSGF_MENU.cast_signed() && lparam.0 != 0 {
        // SAFETY: for MSGF_MENU, lparam points to the menu loop's MSG.
        let message = &*(lparam.0 as *const MSG);
        if menu_message(message) {
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// A popup menu runs its own loop; its characters go through
/// [`message_filter_hook`], never to the list under it.
unsafe fn in_menu_mode() -> bool {
    let mut info = GUITHREADINFO {
        cbSize: u32::try_from(size_of::<GUITHREADINFO>()).unwrap_or_default(),
        ..GUITHREADINFO::default()
    };
    GetGUIThreadInfo(GetCurrentThreadId(), &raw mut info).is_ok()
        && info.flags.0 & GUI_INMENUMODE.0 != 0
}

/// Keys that end a letter search, as any other key does in a Windows list.
fn ends_search(key: usize) -> bool {
    [
        VK_TAB, VK_RETURN, VK_ESCAPE, VK_BACK, VK_PRIOR, VK_NEXT, VK_END, VK_HOME, VK_LEFT, VK_UP,
        VK_RIGHT, VK_DOWN, VK_INSERT, VK_DELETE, VK_APPS,
    ]
    .iter()
    .any(|vk| usize::from(vk.0) == key)
}

fn typed_character(wparam: WPARAM) -> Option<char> {
    u32::try_from(wparam.0).ok().and_then(char::from_u32)
}

/// The search step for `character` in the control `target`: `None` when
/// the character is not part of a search, else the row to select (`None`
/// when the selection stays) and whether the search is still one letter.
fn search_step<S: AsRef<str>>(
    target: isize,
    character: char,
    labels: impl FnOnce() -> (Vec<S>, Option<usize>),
) -> Option<(Option<usize>, bool)> {
    let now = Instant::now();
    let takes = SEARCH.with(|search| {
        let mut search = search.borrow_mut();
        if search.target != target {
            search.typing.reset();
            search.target = target;
        }
        let takes = search.typing.takes(character, now);
        if !takes {
            search.typing.reset();
        }
        takes
    });
    if !takes {
        return None;
    }
    // Read outside the borrow: the control answers through its window
    // procedure.
    let (labels, current) = labels();
    SEARCH.with(|search| {
        let mut search = search.borrow_mut();
        let row = search
            .typing
            .type_character(&labels, current, character, now)
            .filter(|row| Some(*row) != current);
        Some((row, search.typing.one_letter()))
    })
}

fn reset_search() {
    SEARCH.with(|search| search.borrow_mut().typing.reset());
}

// ----- list boxes and drop-down lists -------------------------------------

#[derive(Clone, Copy)]
enum ListKind {
    ListBox,
    DropDown,
}

unsafe fn list_kind(window: HWND) -> Option<ListKind> {
    let mut class = [0_u16; 16];
    let length = usize::try_from(GetClassNameW(window, &mut class)).unwrap_or_default();
    let class = String::from_utf16_lossy(&class[..length.min(class.len())]);
    let style = u32::try_from(GetWindowLongPtrW(window, GWL_STYLE) & 0xffff_ffff).unwrap_or(0);
    if class.eq_ignore_ascii_case("ListBox") {
        let own_selection = (LBS_NOSEL | LBS_MULTIPLESEL | LBS_EXTENDEDSEL | LBS_NODATA) as u32;
        return (style & own_selection == 0).then_some(ListKind::ListBox);
    }
    if class.eq_ignore_ascii_case("ComboBox") {
        return (style & 0x3 == CBS_DROPDOWNLIST as u32).then_some(ListKind::DropDown);
    }
    None
}

unsafe fn list_message(message: &MSG) -> bool {
    if message.message == WM_KEYDOWN {
        if ends_search(message.wParam.0) {
            reset_search();
        }
        return false;
    }
    if message.message != WM_CHAR || message.hwnd.is_invalid() {
        return false;
    }
    let Some(character) = typed_character(message.wParam) else {
        return false;
    };
    let Some(kind) = list_kind(message.hwnd) else {
        return false;
    };
    if in_menu_mode() {
        return false;
    }
    let window = message.hwnd;
    let Some((row, _)) = search_step(window.0 as isize, character, || {
        (list_labels(window, kind), list_selection(window, kind))
    }) else {
        return false;
    };
    if let Some(row) = row {
        select_list_row(window, kind, row);
    }
    true
}

unsafe fn list_labels(window: HWND, kind: ListKind) -> Vec<String> {
    let (count, length, text) = match kind {
        ListKind::ListBox => (LB_GETCOUNT, LB_GETTEXTLEN, LB_GETTEXT),
        ListKind::DropDown => (CB_GETCOUNT, CB_GETLBTEXTLEN, CB_GETLBTEXT),
    };
    let count = usize::try_from(SendMessageW(window, count, None, None).0).unwrap_or_default();
    (0..count)
        .map(|index| {
            let size = usize::try_from(SendMessageW(window, length, Some(WPARAM(index)), None).0)
                .unwrap_or_default();
            let mut buffer = vec![0_u16; size + 1];
            let copied = usize::try_from(
                SendMessageW(
                    window,
                    text,
                    Some(WPARAM(index)),
                    Some(LPARAM(buffer.as_mut_ptr() as isize)),
                )
                .0,
            )
            .unwrap_or_default();
            String::from_utf16_lossy(&buffer[..copied.min(size)])
        })
        .collect()
}

unsafe fn list_selection(window: HWND, kind: ListKind) -> Option<usize> {
    let message = match kind {
        ListKind::ListBox => LB_GETCURSEL,
        ListKind::DropDown => CB_GETCURSEL,
    };
    usize::try_from(SendMessageW(window, message, None, None).0).ok()
}

/// Selects `row` and tells the parent, as the control's own letter search
/// does (it sends the change notification after selecting).
unsafe fn select_list_row(window: HWND, kind: ListKind, row: usize) {
    match kind {
        ListKind::ListBox => {
            SendMessageW(window, LB_SETCURSEL, Some(WPARAM(row)), None);
            let style =
                u32::try_from(GetWindowLongPtrW(window, GWL_STYLE) & 0xffff_ffff).unwrap_or(0);
            if style & LBS_NOTIFY as u32 != 0 {
                notify(window, LBN_SELCHANGE);
            }
        }
        ListKind::DropDown => {
            SendMessageW(window, CB_SETCURSEL, Some(WPARAM(row)), None);
            drop_down_events(window, row);
            notify(window, CBN_SELCHANGE);
        }
    }
}

/// `WM_COMMAND` with `notification` from `control` to its parent (for the
/// list of a combo box, the combo box).
unsafe fn notify(control: HWND, notification: u32) {
    let Ok(parent) = GetParent(control) else {
        return;
    };
    let id = usize::try_from(GetDlgCtrlID(control)).unwrap_or_default() & 0xffff;
    let wparam = id | (usize::try_from(notification).unwrap_or_default() << 16);
    SendMessageW(
        parent,
        WM_COMMAND,
        Some(WPARAM(wparam)),
        Some(LPARAM(control.0 as isize)),
    );
}

/// The events the drop-down list's own letter search raises (measured):
/// the selection in its list and the new value, and while the list is open
/// the focus on the row. `CB_SETCURSEL` raises the focus and selection only
/// in the open list and never the value, so the rest follows here.
unsafe fn drop_down_events(window: HWND, row: usize) {
    let open = SendMessageW(window, CB_GETDROPPEDSTATE, None, None).0 != 0;
    let mut info = COMBOBOXINFO {
        cbSize: u32::try_from(size_of::<COMBOBOXINFO>()).unwrap_or_default(),
        ..COMBOBOXINFO::default()
    };
    if !open && GetComboBoxInfo(window, &raw mut info).is_ok() && !info.hwndList.is_invalid() {
        let child = i32::try_from(row + 1).unwrap_or(i32::MAX);
        NotifyWinEvent(EVENT_OBJECT_SELECTION, info.hwndList, OBJID_CLIENT.0, child);
    }
    NotifyWinEvent(
        EVENT_OBJECT_VALUECHANGE,
        window,
        OBJID_CLIENT.0,
        CHILDID_SELF.cast_signed(),
    );
}

// ----- popup menus ---------------------------------------------------------

unsafe fn menu_message(message: &MSG) -> bool {
    if message.message == WM_KEYDOWN {
        if ends_search(message.wParam.0) {
            reset_search();
        }
        return false;
    }
    if message.message != WM_CHAR {
        return false;
    }
    let Some(character) = typed_character(message.wParam) else {
        return false;
    };
    let Some((window, menu)) = active_menu() else {
        return false;
    };
    let Some((row, one_letter)) = search_step(menu.0 as isize, character, || menu_labels(menu))
    else {
        return false;
    };
    if one_letter {
        // The menu's own search: the next item on the letter, or the only
        // one runs at once, as before.
        return false;
    }
    if let Some(row) = row {
        SendMessageW(window, MN_SELECTITEM, Some(WPARAM(row)), None);
    }
    true
}

/// The open popup menu the keyboard works in: the innermost one, whose
/// items open none of the other open menus.
unsafe fn active_menu() -> Option<(HWND, HMENU)> {
    let mut open: Vec<(HWND, HMENU)> = Vec::new();
    let _ = EnumThreadWindows(
        GetCurrentThreadId(),
        Some(collect_menu_window),
        LPARAM(&raw mut open as isize),
    );
    open.iter().copied().find(|(_, menu)| {
        let count = GetMenuItemCount(Some(*menu)).max(0);
        !(0..count).any(|index| {
            let submenu = GetSubMenu(*menu, index);
            !submenu.is_invalid() && open.iter().any(|(_, other)| *other == submenu)
        })
    })
}

unsafe extern "system" fn collect_menu_window(window: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam is the Vec passed by `active_menu` for this enumeration.
    let open = &mut *(lparam.0 as *mut Vec<(HWND, HMENU)>);
    let mut class = [0_u16; 8];
    let length = usize::try_from(GetClassNameW(window, &mut class)).unwrap_or_default();
    if class[..length.min(class.len())] == *"#32768".encode_utf16().collect::<Vec<_>>()
        && IsWindowVisible(window).as_bool()
    {
        let menu = HMENU(SendMessageW(window, MN_GETHMENU, None, None).0 as _);
        if !menu.is_invalid() {
            open.push((window, menu));
        }
    }
    BOOL(1)
}

/// Item texts without their shortcut column and `&` marks, and the
/// highlighted item.
unsafe fn menu_labels(menu: HMENU) -> (Vec<String>, Option<usize>) {
    let count = u32::try_from(GetMenuItemCount(Some(menu))).unwrap_or_default();
    let mut current = None;
    let labels = (0..count)
        .map(|index| {
            let state = GetMenuState(menu, index, MF_BYPOSITION);
            if state & MF_HILITE.0 != 0 {
                current = usize::try_from(index).ok();
            }
            if state & MF_SEPARATOR.0 != 0 {
                return String::new();
            }
            let mut buffer = [0_u16; 512];
            let length = usize::try_from(GetMenuStringW(
                menu,
                index,
                Some(&mut buffer),
                MF_BYPOSITION,
            ))
            .unwrap_or_default();
            menu_label(&String::from_utf16_lossy(
                &buffer[..length.min(buffer.len())],
            ))
        })
        .collect();
    (labels, current)
}

/// "Add to &favorites\tCtrl+F" as the menu shows it: "Add to favorites".
fn menu_label(text: &str) -> String {
    let text = text.split('\t').next().unwrap_or_default();
    let mut label = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '&' {
            if characters.peek() == Some(&'&') {
                label.push('&');
                characters.next();
            }
            continue;
        }
        label.push(character);
    }
    label
}

#[cfg(test)]
mod tests {
    use super::{ends_search, menu_label};

    #[test]
    fn menu_labels_lose_the_shortcut_column_and_mnemonic_marks() {
        assert_eq!(menu_label("Add to favorites\tCtrl+F"), "Add to favorites");
        assert_eq!(menu_label("&Open && play\tEnter"), "Open & play");
        assert_eq!(menu_label("Close"), "Close");
    }

    #[test]
    fn navigation_keys_end_a_search_and_letters_do_not() {
        assert!(ends_search(0x28)); // Down
        assert!(ends_search(0x0D)); // Enter
        assert!(!ends_search(usize::from(b'S')));
    }
}
