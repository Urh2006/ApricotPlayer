//! The Radio, IPTV and Recording lists inside the main window, so the
//! background player follows them in Tab order as on every other screen and
//! no separate window is announced (Urh, 15. 11. 2026). The list waits for a
//! choice in its own message loop, as the separate window did, so the
//! sources controller keeps one flow for Windows and the Mac.

use apricot_shell::sources::{SourceBrowser, SourceSelection};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, POINT, WPARAM},
        Graphics::Gdi::{ClientToScreen, DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{
                GetFocus, GetKeyState, SetFocus, VK_APPS, VK_ESCAPE, VK_F10, VK_RETURN, VK_SHIFT,
                VK_SPACE, VK_TAB,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, DestroyMenu, GW_CHILD, GW_HWNDNEXT, GetMessageW,
                GetWindow, IsChild, IsWindow, IsWindowVisible, LB_ADDSTRING, LB_GETCURSEL,
                LB_RESETCONTENT, LB_SETCURSEL, LBN_DBLCLK, LBS_NOTIFY, MF_STRING, MSG, MoveWindow,
                PostQuitMessage, SW_HIDE, SW_SHOW, SendMessageW, ShowWindow, TPM_LEFTALIGN,
                TPM_RETURNCMD, TrackPopupMenu, WINDOW_EX_STYLE, WINDOW_STYLE, WM_KEYDOWN, WM_KEYUP,
                WM_SETFONT, WS_CHILD, WS_EX_CLIENTEDGE, WS_TABSTOP, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, w},
};

use super::{WindowState, create_control, state, state_mut, wide};

const PROMPT: usize = 3700;
const LIST: usize = 3701;
const OPEN: usize = 3702;
const ACTIONS: usize = 3703;
const BACK: usize = 3704;

/// The list and its buttons, made once and shown while a source list is
/// open.
pub(super) struct SourcesPanel {
    prompt: HWND,
    list: HWND,
    open: HWND,
    actions: HWND,
    back: HWND,
    model: Option<SourceBrowser>,
    /// The choice that ends the wait, once `done`; `None` is Back.
    result: Option<SourceSelection>,
    done: bool,
    active: bool,
    /// Where the focus returns when the list closes.
    previous: HWND,
}

/// A list control of the panel.
unsafe fn create_list(window: HWND) -> windows::core::Result<HWND> {
    let instance = HINSTANCE(GetModuleHandleW(None)?.0);
    let list = create_control(
        window,
        instance,
        w!("LISTBOX"),
        w!(""),
        WS_CHILD | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(LBS_NOTIFY as u32),
        WS_EX_CLIENTEDGE,
        LIST,
    )?;
    let font = Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize));
    SendMessageW(list, WM_SETFONT, font, Some(LPARAM(1)));
    Ok(list)
}

impl SourcesPanel {
    unsafe fn create(window: HWND) -> windows::core::Result<Self> {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let font = Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize));
        let make = |class: PCWSTR, style: WINDOW_STYLE, extended: WINDOW_EX_STYLE, id| {
            let control = create_control(
                window,
                instance,
                class,
                w!(""),
                WS_CHILD | style,
                extended,
                id,
            )?;
            SendMessageW(control, WM_SETFONT, font, Some(LPARAM(1)));
            Ok::<_, windows::core::Error>(control)
        };
        Ok(Self {
            prompt: make(
                w!("STATIC"),
                WINDOW_STYLE::default(),
                WINDOW_EX_STYLE::default(),
                PROMPT,
            )?,
            list: create_list(window)?,
            open: make(w!("BUTTON"), WS_TABSTOP, WINDOW_EX_STYLE::default(), OPEN)?,
            actions: make(
                w!("BUTTON"),
                WS_TABSTOP,
                WINDOW_EX_STYLE::default(),
                ACTIONS,
            )?,
            back: make(w!("BUTTON"), WS_TABSTOP, WINDOW_EX_STYLE::default(), BACK)?,
            model: None,
            result: None,
            done: false,
            active: false,
            previous: HWND::default(),
        })
    }

    fn controls(&self) -> [HWND; 5] {
        [self.prompt, self.list, self.open, self.actions, self.back]
    }
}

unsafe fn set_text(control: HWND, text: &str) {
    let text = wide(text);
    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowTextW(control, PCWSTR(text.as_ptr()));
}

unsafe fn fill(list: HWND, rows: &[String], selected: usize) {
    SendMessageW(list, LB_RESETCONTENT, None, None);
    for row in rows {
        let row = wide(row);
        SendMessageW(
            list,
            LB_ADDSTRING,
            None,
            Some(LPARAM(row.as_ptr() as isize)),
        );
    }
    if !rows.is_empty() {
        SendMessageW(
            list,
            LB_SETCURSEL,
            Some(WPARAM(selected.min(rows.len() - 1))),
            None,
        );
    }
}

/// Shows `model` in the main window and waits for a choice. The list stays
/// for the next call; `None` (Back or Escape) closes it.
pub(super) unsafe fn browse(window: HWND, model: &SourceBrowser) -> Option<SourceSelection> {
    let state = state_mut(window)?;
    if state.sources_panel.is_none() {
        state.sources_panel = Some(SourcesPanel::create(window).ok()?);
    }
    let focus = GetFocus();
    let panel = state.sources_panel.as_mut()?;
    let same = panel.active
        && panel
            .model
            .as_ref()
            .is_some_and(|shown| shown.title == model.title && shown.list_name == model.list_name);
    if same {
        // Only what changed changes, and the focus stays.
        let shown = panel
            .model
            .as_ref()
            .map(|shown| shown.rows.clone())
            .unwrap_or_default();
        if shown == model.rows {
            let selected =
                usize::try_from(SendMessageW(panel.list, LB_GETCURSEL, None, None).0).ok();
            if !model.rows.is_empty() && selected != Some(model.selected.min(model.rows.len() - 1))
            {
                SendMessageW(
                    panel.list,
                    LB_SETCURSEL,
                    Some(WPARAM(model.selected.min(model.rows.len() - 1))),
                    None,
                );
            }
        } else {
            fill(panel.list, &model.rows, model.selected);
        }
    } else {
        if panel.active {
            // Another list in the open panel, such as the stations after the
            // radio menu: a new control takes the focus, so the screen
            // reader reads the new list once, by its name.
            if let Ok(list) = create_list(window) {
                let old = std::mem::replace(&mut panel.list, list);
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(old);
            }
        } else {
            panel.previous = focus;
        }
        set_text(panel.prompt, &model.prompt);
        set_text(
            panel.open,
            model.actions.first().map_or("Open", |action| &action.label),
        );
        set_text(panel.actions, &model.actions_button);
        set_text(panel.back, &model.back_button);
        fill(panel.list, &model.rows, model.selected);
        crate::accessibility_win32::set_control_name(panel.list, &model.list_name);
    }
    let first = !panel.active || !same;
    panel.model = Some(model.clone());
    panel.active = true;
    panel.result = None;
    panel.done = false;
    let list = panel.list;
    super::layout_controls_state(window, state);
    if first {
        // A new list: the focus moves to it once, so it is read once.
        let _ = SetFocus(Some(list));
    }
    wait(window)
}

/// Hides the list and gives the focus back to where it was.
pub(super) unsafe fn close(window: HWND) {
    let Some(state) = state_mut(window) else {
        return;
    };
    let Some(panel) = state.sources_panel.as_mut() else {
        return;
    };
    if !panel.active {
        return;
    }
    panel.active = false;
    panel.model = None;
    for control in panel.controls() {
        let _ = ShowWindow(control, SW_HIDE);
    }
    let previous = panel.previous;
    super::layout_controls_state(window, state);
    if IsWindow(Some(previous)).as_bool() && IsWindowVisible(previous).as_bool() {
        let _ = SetFocus(Some(previous));
    } else {
        let _ = SetFocus(Some(super::active_primary_control(state)));
    }
}

/// While the list is open: its controls in Tab order, the background player
/// after them as everywhere else.
pub(super) fn tab_controls(state: &WindowState) -> Option<Vec<HWND>> {
    let panel = state.sources_panel.as_ref().filter(|panel| panel.active)?;
    Some(vec![panel.list, panel.open, panel.actions, panel.back])
}

/// Places the list where the screen's own controls are and hides those, so
/// only the list, the status line and the background player are shown.
/// `true` when the list is open.
pub(super) unsafe fn layout(window: HWND, state: &WindowState, width: i32, height: i32) -> bool {
    let Some(panel) = state.sources_panel.as_ref().filter(|panel| panel.active) else {
        return false;
    };
    let mut keep = panel.controls().to_vec();
    keep.push(state.status);
    keep.push(state.background_player.label());
    keep.extend(state.background_player.button_windows());
    keep.push(state.player_controls.video_host());
    let mut child = GetWindow(window, GW_CHILD).unwrap_or_default();
    while !child.is_invalid() {
        if !keep.contains(&child) && IsWindowVisible(child).as_bool() {
            let _ = ShowWindow(child, SW_HIDE);
        }
        child = GetWindow(child, GW_HWNDNEXT).unwrap_or_default();
    }
    let margin = 12;
    let list_height = (height - 58 - 48 - margin).max(80);
    let _ = MoveWindow(panel.prompt, margin, margin, width - 2 * margin, 40, true);
    let _ = MoveWindow(
        panel.list,
        margin,
        58,
        width - 2 * margin,
        list_height,
        true,
    );
    let buttons_top = 58 + list_height + 8;
    for (index, control) in [panel.open, panel.actions, panel.back]
        .into_iter()
        .enumerate()
    {
        let left = margin + i32::try_from(index).unwrap_or_default() * 170;
        let _ = MoveWindow(control, left, buttons_top, 158, 34, true);
    }
    for control in panel.controls() {
        let _ = ShowWindow(control, SW_SHOW);
    }
    true
}

/// A click on the list's buttons, which reaches the main window.
pub(super) unsafe fn command(window: HWND, id: usize, notification: usize) -> bool {
    if !(PROMPT..=BACK).contains(&id) {
        return false;
    }
    match id {
        OPEN => open(window),
        ACTIONS => menu(window),
        BACK => finish(window, None),
        LIST if notification == LBN_DBLCLK as usize => open(window),
        _ => {}
    }
    true
}

unsafe fn selected_choice(window: HWND, action: &str) -> Option<SourceSelection> {
    let panel = state(window)?.sources_panel.as_ref()?;
    let selected = SendMessageW(panel.list, LB_GETCURSEL, None, None).0;
    Some(SourceSelection {
        index: usize::try_from(selected).unwrap_or_default(),
        action: action.into(),
    })
}

unsafe fn finish(window: HWND, choice: Option<SourceSelection>) {
    if let Some(panel) = state_mut(window).and_then(|state| state.sources_panel.as_mut()) {
        panel.result = choice;
        panel.done = true;
    }
}

unsafe fn open(window: HWND) {
    let action = state(window)
        .and_then(|state| state.sources_panel.as_ref())
        .and_then(|panel| panel.model.as_ref())
        .and_then(|model| model.actions.first())
        .map(|action| action.id.to_owned());
    if let Some(action) = action {
        let choice = selected_choice(window, &action);
        finish(window, choice);
    }
}

unsafe fn menu(window: HWND) {
    let Some(panel) = state(window).and_then(|state| state.sources_panel.as_ref()) else {
        return;
    };
    let Some(model) = panel.model.clone() else {
        return;
    };
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    let row = usize::try_from(SendMessageW(panel.list, LB_GETCURSEL, None, None).0).ok();
    for (index, action) in model.actions.iter().enumerate() {
        let label = action.label_for(row);
        let label = if action.shortcut.is_empty() {
            label.to_owned()
        } else {
            format!("{label}\t{}", action.shortcut)
        };
        let label = wide(&label);
        let _ = AppendMenuW(menu, MF_STRING, index + 1, PCWSTR(label.as_ptr()));
    }
    let mut point = POINT::default();
    let _ = ClientToScreen(panel.list, &raw mut point);
    let chosen = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_LEFTALIGN,
        point.x + 16,
        point.y + 18,
        Some(0),
        window,
        None,
    )
    .0;
    let _ = DestroyMenu(menu);
    if let Some(action) = usize::try_from(chosen)
        .ok()
        .and_then(|chosen| chosen.checked_sub(1))
        .and_then(|index| model.actions.get(index))
    {
        let choice = selected_choice(window, action.id);
        finish(window, choice);
    }
}

/// Keys of the open list; the background player's own keys and Tab go on
/// as on every screen.
unsafe fn key(window: HWND, message: &MSG) -> bool {
    let Some(panel) = state(window).and_then(|state| state.sources_panel.as_ref()) else {
        return false;
    };
    let in_panel = [panel.list, panel.open, panel.actions, panel.back].contains(&message.hwnd);
    let pressed = message.wParam.0;
    if pressed == usize::from(VK_TAB.0) && matches!(message.message, WM_KEYDOWN | WM_KEYUP) {
        return super::handle_view_tab_message(window, message);
    }
    // Escape goes back from the list and from the background player after
    // it, as on every screen.
    if message.message == WM_KEYDOWN && pressed == usize::from(VK_ESCAPE.0) {
        finish(window, None);
        return true;
    }
    if message.message != WM_KEYDOWN || !in_panel {
        return false;
    }
    if pressed == usize::from(VK_APPS.0)
        || (pressed == usize::from(VK_F10.0) && GetKeyState(i32::from(VK_SHIFT.0)) < 0)
    {
        menu(window);
        return true;
    }
    let (open_button, actions_button, back_button, list) =
        (panel.open, panel.actions, panel.back, panel.list);
    if pressed == usize::from(VK_RETURN.0) || pressed == usize::from(VK_SPACE.0) {
        match message.hwnd {
            hwnd if hwnd == actions_button => menu(window),
            hwnd if hwnd == back_button => finish(window, None),
            hwnd if hwnd == open_button => open(window),
            hwnd if hwnd == list && pressed == usize::from(VK_RETURN.0) => open(window),
            _ => return false,
        }
        return true;
    }
    if let Some(chord) = crate::shortcut_win32::chord_from_message(message)
        && let Some(action) = panel
            .model
            .as_ref()
            .and_then(|model| {
                model.actions.iter().find(|action| {
                    !action.shortcut.is_empty() && chord.matches_configured(&action.shortcut)
                })
            })
            .map(|action| action.id.to_owned())
    {
        let choice = selected_choice(window, &action);
        finish(window, choice);
        return true;
    }
    false
}

/// The wait for a choice, with the main window's other controls working.
/// Exit was asked: the open list answers Back, and every list opened
/// after it too, until the controller is back (review 4, A7).
pub(super) unsafe fn abandon(window: HWND) {
    if let Some(panel) = state_mut(window).and_then(|state| state.sources_panel.as_mut())
        && panel.active
    {
        panel.result = None;
        panel.done = true;
    }
}

unsafe fn wait(window: HWND) -> Option<SourceSelection> {
    let mut message = MSG::default();
    loop {
        if state(window).is_some_and(|state| state.exit_after_sources) {
            if let Some(panel) = state_mut(window).and_then(|state| state.sources_panel.as_mut()) {
                panel.done = false;
                panel.result = None;
            }
            return None;
        }
        if let Some(panel) = state_mut(window)
            .and_then(|state| state.sources_panel.as_mut())
            .filter(|panel| panel.done)
        {
            panel.done = false;
            return panel.result.take();
        }
        if !state(window).is_some_and(|state| {
            state
                .sources_panel
                .as_ref()
                .is_some_and(|panel| panel.active)
        }) {
            return None;
        }
        let received = GetMessageW(&raw mut message, None, 0, 0).0;
        if received <= 0 {
            if received == 0 {
                PostQuitMessage(i32::try_from(message.wParam.0).unwrap_or_default());
            }
            return None;
        }
        if (message.hwnd == window || IsChild(window, message.hwnd).as_bool())
            && key(window, &message)
        {
            continue;
        }
        // Everything else goes the main window's way: shortcuts, the
        // background player's keys, Enter on buttons (review 4, B2).
        super::route_main_message(window, &message);
    }
}
