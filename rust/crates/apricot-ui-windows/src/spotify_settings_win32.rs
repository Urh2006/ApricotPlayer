//! Spotify settings dialog (`docs/SPOTIFY_PLAN.md` 9.2): streaming quality,
//! volume normalisation, autoplay and the library order, then OK and
//! Cancel. Each control has
//! its label as its accessible name; Escape cancels.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{ffi::c_void, mem::size_of};

use apricot_spotify::settings::{Autoplay, LibraryOrder, Quality, SpotifySettings};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Input::KeyboardAndMouse::{EnableWindow, SetFocus, VK_ESCAPE},
            WindowsAndMessaging::{
                BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, CBS_DROPDOWNLIST, CW_USEDEFAULT,
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect,
                GetMessageW, GetWindowLongPtrW, HMENU, IDC_ARROW, IDYES, IsChild, IsDialogMessageW,
                IsWindow, LoadCursorW, MB_DEFBUTTON2, MB_ICONWARNING, MB_YESNOCANCEL, MSG,
                MessageBoxW, MoveWindow, PostQuitMessage, RegisterClassW, SW_SHOW, SendMessageW,
                SetForegroundWindow, SetWindowLongPtrW, ShowWindow, TranslateMessage,
                WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
                WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_SIZE, WNDCLASSW, WS_CAPTION, WS_CHILD,
                WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS: PCWSTR = w!("ApricotPlayer2BetaSpotifySettingsWindow");
const ID_QUALITY: usize = 2201;
const ID_NORMALISATION: usize = 2202;
const ID_AUTOPLAY: usize = 2203;
const ID_OK: usize = 2204;
const ID_CANCEL: usize = 2205;
const ID_ORDER: usize = 2206;
const ID_HISTORY: usize = 2207;
const ID_KEY_CACHE: usize = 2208;
const IDOK_COMMAND: usize = 1;
const IDCANCEL_COMMAND: usize = 2;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const CB_GETDROPPEDSTATE: u32 = 0x0157;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;

pub struct SettingsTexts<'a> {
    pub title: &'a str,
    pub quality: &'a str,
    /// Labels in `Quality::ALL` order.
    pub qualities: [&'a str; 3],
    pub normalisation: &'a str,
    pub listening_history: &'a str,
    pub experimental_warning: &'a str,
    pub key_cache: &'a str,
    pub autoplay: &'a str,
    /// Labels in `Autoplay::ALL` order.
    pub autoplays: [&'a str; 3],
    pub order: &'a str,
    /// Labels in `LibraryOrder::ALL` order.
    pub orders: [&'a str; 5],
    pub ok: &'a str,
    pub cancel: &'a str,
}

struct DialogState {
    controls: Vec<HWND>,
    quality: HWND,
    normalisation: HWND,
    listening_history: HWND,
    history_consented: bool,
    key_cache: HWND,
    warning_title: String,
    warning: String,
    autoplay: HWND,
    order: HWND,
    result: Option<SpotifySettings>,
}

pub fn register() -> Result<()> {
    // SAFETY: The class is registered once before the main message loop.
    unsafe {
        let module = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size fits in i32"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hInstance: HINSTANCE(module.0),
            lpszClassName: CLASS,
            lpfnWndProc: Some(window_proc),
            ..Default::default()
        };
        if RegisterClassW(&raw const class) == 0 {
            return Err(windows::core::Error::from_thread());
        }
    }
    Ok(())
}

/// Shows the dialog with `current`; the chosen settings after OK.
///
/// # Errors
///
/// Returns a Win32 error when the window or a control cannot be created.
pub fn show(
    owner: HWND,
    texts: &SettingsTexts<'_>,
    current: SpotifySettings,
) -> Result<Option<SpotifySettings>> {
    // SAFETY: The nested modal loop owns its state and disables its owner
    // until the state allocation has been recovered.
    unsafe { show_win32(owner, texts, current) }
}

#[allow(clippy::too_many_lines)]
unsafe fn show_win32(
    owner: HWND,
    texts: &SettingsTexts<'_>,
    current: SpotifySettings,
) -> Result<Option<SpotifySettings>> {
    let module = GetModuleHandleW(None)?;
    let instance = HINSTANCE(module.0);
    let title = wide(texts.title);
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CLASS,
        PCWSTR(title.as_ptr()),
        WS_CAPTION | WS_SYSMENU,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        460,
        345,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let create = |class: PCWSTR, text: &str, style: WINDOW_STYLE, id: usize| {
        let text = wide(text);
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            100,
            30,
            Some(window),
            Some(HMENU(id as *mut c_void)),
            Some(instance),
            None,
        )
    };
    let combo = WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32);
    let built = (|| -> Result<DialogState> {
        let quality_label = create(w!("STATIC"), texts.quality, WINDOW_STYLE(0), 0)?;
        let quality = create(w!("COMBOBOX"), "", combo, ID_QUALITY)?;
        let normalisation = create(
            w!("BUTTON"),
            texts.normalisation,
            WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
            ID_NORMALISATION,
        )?;
        let autoplay_label = create(w!("STATIC"), texts.autoplay, WINDOW_STYLE(0), 0)?;
        let autoplay = create(w!("COMBOBOX"), "", combo, ID_AUTOPLAY)?;
        let order_label = create(w!("STATIC"), texts.order, WINDOW_STYLE(0), 0)?;
        let order = create(w!("COMBOBOX"), "", combo, ID_ORDER)?;
        let listening_history = create(
            w!("BUTTON"),
            texts.listening_history,
            WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
            ID_HISTORY,
        )?;
        let key_cache = create(
            w!("BUTTON"),
            texts.key_cache,
            WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
            ID_KEY_CACHE,
        )?;
        let ok = create(
            w!("BUTTON"),
            texts.ok,
            WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            ID_OK,
        )?;
        let cancel = create(w!("BUTTON"), texts.cancel, WS_TABSTOP, ID_CANCEL)?;
        Ok(DialogState {
            controls: vec![
                quality_label,
                quality,
                normalisation,
                autoplay_label,
                autoplay,
                order_label,
                order,
                listening_history,
                key_cache,
                ok,
                cancel,
            ],
            quality,
            normalisation,
            autoplay,
            order,
            listening_history,
            history_consented: current.listening_history,
            key_cache,
            warning_title: texts.title.to_owned(),
            warning: texts.experimental_warning.to_owned(),
            result: None,
        })
    })();
    let state = match built {
        Ok(state) => state,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for control in &state.controls {
        SendMessageW(
            *control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
    }
    let fill = |control: HWND, labels: &[&str], selected: usize| {
        for label in labels {
            let label = wide(label);
            SendMessageW(
                control,
                CB_ADDSTRING,
                None,
                Some(LPARAM(label.as_ptr() as isize)),
            );
        }
        SendMessageW(control, CB_SETCURSEL, Some(WPARAM(selected)), None);
    };
    fill(
        state.quality,
        &texts.qualities,
        Quality::ALL
            .iter()
            .position(|quality| *quality == current.quality)
            .unwrap_or(2),
    );
    fill(
        state.autoplay,
        &texts.autoplays,
        Autoplay::ALL
            .iter()
            .position(|autoplay| *autoplay == current.autoplay)
            .unwrap_or(0),
    );
    fill(
        state.order,
        &texts.orders,
        LibraryOrder::ALL
            .iter()
            .position(|order| *order == current.library_order)
            .unwrap_or(0),
    );
    SendMessageW(
        state.normalisation,
        BM_SETCHECK,
        Some(WPARAM(usize::from(current.normalisation))),
        None,
    );
    crate::accessibility_win32::annotate_control_name(state.quality, texts.quality);
    crate::accessibility_win32::annotate_control_name(state.autoplay, texts.autoplay);
    crate::accessibility_win32::annotate_control_name(state.order, texts.order);
    SendMessageW(
        state.listening_history,
        BM_SETCHECK,
        Some(WPARAM(usize::from(current.listening_history))),
        None,
    );
    SendMessageW(
        state.key_cache,
        BM_SETCHECK,
        Some(WPARAM(usize::from(current.key_cache))),
        None,
    );
    let initial_focus = state.quality;
    let pointer = Box::into_raw(Box::new(state));
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), pointer as isize);
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
            PostQuitMessage(i32::try_from(message.wParam.0).unwrap_or_default());
            break;
        }
        let inside = message.hwnd == window || IsChild(window, message.hwnd).as_bool();
        let dropped = [(*pointer).quality, (*pointer).autoplay, (*pointer).order]
            .iter()
            .any(|combo| SendMessageW(*combo, CB_GETDROPPEDSTATE, None, None).0 != 0);
        if inside
            && !dropped
            && message.message == WM_KEYDOWN
            && message.wParam.0 == usize::from(VK_ESCAPE.0)
        {
            let _ = DestroyWindow(window);
            continue;
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    let state = Box::from_raw(pointer);
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
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
            if command == ID_OK || command == IDOK_COMMAND {
                accept(window);
            } else if command == ID_HISTORY {
                confirm_history_control(window);
            } else if command == ID_CANCEL || command == IDCANCEL_COMMAND {
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

unsafe fn accept(window: HWND) {
    if !confirm_history_control(window) {
        return;
    }
    let Some(state) = state(window) else {
        return;
    };
    let selected =
        |combo: HWND| usize::try_from(SendMessageW(combo, CB_GETCURSEL, None, None).0).unwrap_or(0);
    state.result = Some(SpotifySettings {
        quality: Quality::ALL
            .get(selected(state.quality))
            .copied()
            .unwrap_or(Quality::VeryHigh),
        normalisation: SendMessageW(state.normalisation, BM_GETCHECK, None, None).0 == 1,
        listening_history: SendMessageW(state.listening_history, BM_GETCHECK, None, None).0 == 1,
        autoplay: Autoplay::ALL
            .get(selected(state.autoplay))
            .copied()
            .unwrap_or(Autoplay::Account),
        library_order: LibraryOrder::ALL
            .get(selected(state.order))
            .copied()
            .unwrap_or_default(),
        key_cache: SendMessageW(state.key_cache, BM_GETCHECK, None, None).0 == 1,
    });
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
    let width = (bounds.right - bounds.left).max(360);
    let height = (bounds.bottom - bounds.top).max(246);
    let margin = 10;
    let label_width = 140;
    let row = 32;
    let field_x = margin + label_width + 6;
    let field_width = width - field_x - margin;
    let [
        quality_label,
        quality,
        normalisation,
        autoplay_label,
        autoplay,
        order_label,
        order,
        listening_history,
        key_cache,
        ok,
        cancel,
    ] = state.controls[..]
    else {
        return;
    };
    let _ = MoveWindow(quality_label, margin, margin + 4, label_width, 26, true);
    let _ = MoveWindow(quality, field_x, margin, field_width, 200, true);
    let _ = MoveWindow(
        normalisation,
        margin,
        margin + row,
        width - margin * 2,
        26,
        true,
    );
    let _ = MoveWindow(
        autoplay_label,
        margin,
        margin + row * 2 + 4,
        label_width,
        26,
        true,
    );
    let _ = MoveWindow(autoplay, field_x, margin + row * 2, field_width, 200, true);
    let _ = MoveWindow(
        order_label,
        margin,
        margin + row * 3 + 4,
        label_width,
        26,
        true,
    );
    let _ = MoveWindow(order, field_x, margin + row * 3, field_width, 200, true);
    let _ = MoveWindow(
        listening_history,
        margin,
        margin + row * 4,
        width - margin * 2,
        28,
        true,
    );
    let _ = MoveWindow(
        key_cache,
        margin,
        margin + row * 5,
        width - margin * 2,
        28,
        true,
    );
    let button_y = height - 30 - margin;
    let _ = MoveWindow(ok, width - margin - 206, button_y, 100, 30, true);
    let _ = MoveWindow(cancel, width - margin - 100, button_y, 100, 30, true);
}

pub(crate) unsafe fn confirm_listening_history(window: HWND, title: &str, warning: &str) -> bool {
    let title = wide(title);
    let warning = wide(warning);
    MessageBoxW(
        Some(window),
        PCWSTR(warning.as_ptr()),
        PCWSTR(title.as_ptr()),
        MB_YESNOCANCEL | MB_ICONWARNING | MB_DEFBUTTON2,
    ) == IDYES
}

unsafe fn confirm_history_control(window: HWND) -> bool {
    let Some(state) = state(window) else {
        return false;
    };
    let checked = SendMessageW(state.listening_history, BM_GETCHECK, None, None).0 == 1;
    if !checked {
        state.history_consented = false;
        return true;
    }
    if state.history_consented {
        return true;
    }
    if confirm_listening_history(window, &state.warning_title, &state.warning) {
        state.history_consented = true;
        return true;
    }
    SendMessageW(state.listening_history, BM_SETCHECK, Some(WPARAM(0)), None);
    let _ = SetFocus(Some(state.listening_history));
    false
}

unsafe fn state(window: HWND) -> Option<&'static mut DialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut DialogState;
    pointer.as_mut()
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
    use windows::Win32::UI::WindowsAndMessaging::{
        BS_DEFPUSHBUTTON, FindWindowW, GWL_STYLE, GetDlgItem, GetWindowThreadProcessId, IDCANCEL,
        IDNO, KillTimer, PostMessageW, SetTimer,
    };

    static ACTION: AtomicI32 = AtomicI32::new(0);
    static DEFAULT_NO: AtomicBool = AtomicBool::new(false);
    static TICKS: AtomicU32 = AtomicU32::new(0);
    static TIMED_OUT: AtomicBool = AtomicBool::new(false);
    static DRIVER: Mutex<()> = Mutex::new(());
    const TITLE: PCWSTR = w!("Apricot isolated consent fixture");

    unsafe extern "system" fn reply(_: HWND, _: u32, _: usize, _: u32) {
        let Ok(dialog) = FindWindowW(w!("#32770"), TITLE) else {
            return;
        };
        let mut process_id = 0;
        GetWindowThreadProcessId(dialog, Some(&raw mut process_id));
        if process_id != std::process::id() {
            return;
        }
        if TICKS.fetch_add(1, Ordering::SeqCst) > 100 {
            TIMED_OUT.store(true, Ordering::SeqCst);
            let _ = PostMessageW(
                Some(dialog),
                WM_COMMAND,
                WPARAM(usize::try_from(IDCANCEL.0).unwrap()),
                LPARAM(0),
            );
            return;
        }
        if let Ok(no) = GetDlgItem(Some(dialog), IDNO.0) {
            DEFAULT_NO.store(
                GetWindowLongPtrW(no, GWL_STYLE) & 0xf
                    == isize::try_from(BS_DEFPUSHBUTTON).unwrap(),
                Ordering::SeqCst,
            );
        }
        let action = ACTION.swap(0, Ordering::SeqCst);
        if action == IDCANCEL.0 {
            let _ = PostMessageW(
                Some(dialog),
                WM_KEYDOWN,
                WPARAM(usize::from(VK_ESCAPE.0)),
                LPARAM(0),
            );
        } else if action != 0 {
            let _ = PostMessageW(
                Some(dialog),
                WM_COMMAND,
                WPARAM(usize::try_from(action).unwrap()),
                LPARAM(0),
            );
        }
    }

    #[test]
    #[ignore = "Native modal fixture; no accounts or reporting. Run on an available desktop."]
    fn native_consent_defaults_no_and_rejects_escape() {
        let _driver = DRIVER.lock().unwrap();
        // SAFETY: This fixture addresses only its own unique-caption dialog,
        // verifies its process ID, and never creates a Spotify runtime.
        unsafe {
            for (action, expected) in [(IDNO.0, false), (IDCANCEL.0, false), (IDYES.0, true)] {
                ACTION.store(action, Ordering::SeqCst);
                DEFAULT_NO.store(false, Ordering::SeqCst);
                TICKS.store(0, Ordering::SeqCst);
                TIMED_OUT.store(false, Ordering::SeqCst);
                let timer = SetTimer(None, 0, 10, Some(reply));
                assert_ne!(timer, 0);
                let accepted = confirm_listening_history(
                    HWND::default(),
                    "Apricot isolated consent fixture",
                    "Fixture warning only. No account or settings are changed.",
                );
                let _ = KillTimer(None, timer);
                assert_eq!(accepted, expected);
                assert!(
                    !TIMED_OUT.load(Ordering::SeqCst),
                    "native response must complete without forced dismissal"
                );
                assert!(
                    DEFAULT_NO.load(Ordering::SeqCst),
                    "No must be the default button"
                );
            }
        }
    }

    #[test]
    #[ignore = "Isolated native checkbox and consent fixture; no accounts or reporting."]
    fn native_history_checkbox_rejects_and_reconfirms_after_disable() {
        let _driver = DRIVER.lock().unwrap();
        // SAFETY: All handles and modal input belong to this fixture's thread.
        unsafe {
            register().unwrap();
            let module = GetModuleHandleW(None).unwrap();
            let instance = HINSTANCE(module.0);
            let owner = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                CLASS,
                w!("Isolated checkbox fixture"),
                WS_CAPTION | WS_SYSMENU,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                360,
                180,
                None,
                None,
                Some(instance),
                None,
            )
            .unwrap();
            let checkbox = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                w!("Fixture option"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                10,
                10,
                280,
                30,
                Some(owner),
                Some(HMENU(ID_HISTORY as *mut _)),
                Some(instance),
                None,
            )
            .unwrap();
            let mut data = Box::new(DialogState {
                controls: vec![checkbox],
                quality: HWND::default(),
                normalisation: HWND::default(),
                autoplay: HWND::default(),
                order: HWND::default(),
                listening_history: checkbox,
                history_consented: false,
                key_cache: HWND::default(),
                warning_title: "Apricot isolated consent fixture".into(),
                warning: "Fixture warning only. No account or settings are changed.".into(),
                result: None,
            });
            SetWindowLongPtrW(
                owner,
                WINDOW_LONG_PTR_INDEX(0),
                std::ptr::from_mut(&mut *data) as isize,
            );
            let _ = ShowWindow(owner, SW_SHOW);
            for action in [IDNO.0, IDCANCEL.0, IDYES.0] {
                SendMessageW(checkbox, BM_SETCHECK, Some(WPARAM(1)), None);
                ACTION.store(action, Ordering::SeqCst);
                TICKS.store(0, Ordering::SeqCst);
                TIMED_OUT.store(false, Ordering::SeqCst);
                let timer = SetTimer(None, 0, 10, Some(reply));
                assert_ne!(timer, 0);
                assert_eq!(confirm_history_control(owner), action == IDYES.0);
                let _ = KillTimer(None, timer);
                assert!(!TIMED_OUT.load(Ordering::SeqCst));
                assert_eq!(data.history_consented, action == IDYES.0);
                assert_eq!(
                    SendMessageW(checkbox, BM_GETCHECK, None, None).0 == 1,
                    action == IDYES.0
                );
                if action != IDYES.0 {
                    assert_eq!(
                        windows::Win32::UI::Input::KeyboardAndMouse::GetFocus(),
                        checkbox
                    );
                }
            }
            SendMessageW(checkbox, BM_SETCHECK, Some(WPARAM(0)), None);
            assert!(confirm_history_control(owner));
            assert!(
                !data.history_consented,
                "turning off must discard previous consent"
            );
            SendMessageW(checkbox, BM_SETCHECK, Some(WPARAM(1)), None);
            ACTION.store(IDNO.0, Ordering::SeqCst);
            TICKS.store(0, Ordering::SeqCst);
            let timer = SetTimer(None, 0, 10, Some(reply));
            assert_ne!(timer, 0);
            assert!(
                !confirm_history_control(owner),
                "re-enabling must ask again"
            );
            let _ = KillTimer(None, timer);
            let _ = DestroyWindow(owner);
        }
    }
}
