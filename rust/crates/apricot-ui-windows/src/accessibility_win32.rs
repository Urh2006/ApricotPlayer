//! Small MSAA dynamic-annotation helpers for native Win32 controls.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::cell::RefCell;

use windows::{
    Win32::{
        Foundation::HWND,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::{
            Accessibility::{
                CLSID_AccPropServices, IAccPropServices, NotifyWinEvent, PROPID_ACC_DESCRIPTION,
                PROPID_ACC_NAME, PROPID_ACC_VALUE,
            },
            WindowsAndMessaging::{
                CHILDID_SELF, EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_VALUECHANGE, OBJID_CLIENT,
                SetWindowTextW,
            },
        },
    },
    core::PCWSTR,
};

pub fn set_control_name(window: HWND, value: &str) {
    // SAFETY: The annotation targets a live HWND owned by the calling UI
    // thread. COM initialization is balanced by the local guard, and the
    // UTF-16 buffer remains alive through both synchronous calls.
    unsafe { set_control_name_win32(window, value) };
}

unsafe fn set_control_name_win32(window: HWND, value: &str) {
    let text = wide(value);
    let _ = SetWindowTextW(window, PCWSTR(text.as_ptr()));
    annotate_control_name(window, value);
}

/// Names an edit control without overwriting its user-visible value.
pub fn annotate_control_name(window: HWND, value: &str) {
    // SAFETY: Same UI-thread HWND and synchronous annotation contract as above.
    unsafe { annotate_control_name_win32(window, value) };
}

unsafe fn annotate_control_name_win32(window: HWND, value: &str) {
    let value = wide(value);
    ACCESSIBILITY_SERVICES.with_borrow_mut(|slot| {
        if matches!(slot, AccessibilityServicesState::Uninitialized) {
            *slot = unsafe { AccessibilityServices::initialize() }.map_or(
                AccessibilityServicesState::Unavailable,
                AccessibilityServicesState::Ready,
            );
        }
        let AccessibilityServicesState::Ready(services) = slot else {
            return;
        };
        if services
            .properties
            .SetHwndPropStr(
                window,
                OBJID_CLIENT.0.cast_unsigned(),
                CHILDID_SELF,
                PROPID_ACC_NAME,
                PCWSTR(value.as_ptr()),
            )
            .is_ok()
        {
            NotifyWinEvent(
                EVENT_OBJECT_NAMECHANGE,
                window,
                OBJID_CLIENT.0,
                CHILDID_SELF.cast_signed(),
            );
        }
    });
}

/// Python `set_equalizer_slider_accessibility` with `SliderAccessible`: the
/// slider is named by its label, reports `value` (for example "3.0 dB") and
/// describes itself as "label: value". A value change is announced only when
/// `notify` is set, as Python suppresses notifications for programmatic
/// updates.
pub fn annotate_slider(window: HWND, name: &str, value: &str, notify: bool) {
    // SAFETY: Same UI-thread HWND and synchronous annotation contract as above.
    unsafe { annotate_slider_win32(window, name, value, notify) };
}

unsafe fn annotate_slider_win32(window: HWND, name: &str, value: &str, notify: bool) {
    let name_text = wide(name);
    let value_text = wide(value);
    let description = wide(&format!("{name}: {value}"));
    let _ = SetWindowTextW(window, PCWSTR(name_text.as_ptr()));
    ACCESSIBILITY_SERVICES.with_borrow_mut(|slot| {
        if matches!(slot, AccessibilityServicesState::Uninitialized) {
            *slot = unsafe { AccessibilityServices::initialize() }.map_or(
                AccessibilityServicesState::Unavailable,
                AccessibilityServicesState::Ready,
            );
        }
        let AccessibilityServicesState::Ready(services) = slot else {
            return;
        };
        let mut annotated = true;
        for (property, text) in [
            (PROPID_ACC_NAME, &name_text),
            (PROPID_ACC_VALUE, &value_text),
            (PROPID_ACC_DESCRIPTION, &description),
        ] {
            annotated &= services
                .properties
                .SetHwndPropStr(
                    window,
                    OBJID_CLIENT.0.cast_unsigned(),
                    CHILDID_SELF,
                    property,
                    PCWSTR(text.as_ptr()),
                )
                .is_ok();
        }
        if annotated && notify {
            NotifyWinEvent(
                EVENT_OBJECT_VALUECHANGE,
                window,
                OBJID_CLIENT.0,
                CHILDID_SELF.cast_signed(),
            );
        }
    });
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

thread_local! {
    static ACCESSIBILITY_SERVICES: RefCell<AccessibilityServicesState> =
        const { RefCell::new(AccessibilityServicesState::Uninitialized) };
}

enum AccessibilityServicesState {
    Uninitialized,
    Unavailable,
    Ready(AccessibilityServices),
}

struct AccessibilityServices {
    properties: IAccPropServices,
    _com: ComApartment,
}

impl AccessibilityServices {
    unsafe fn initialize() -> Option<Self> {
        let com = ComApartment::initialize()?;
        let properties =
            CoCreateInstance(&CLSID_AccPropServices, None, CLSCTX_INPROC_SERVER).ok()?;
        Some(Self {
            properties,
            _com: com,
        })
    }
}

struct ComApartment;

impl ComApartment {
    unsafe fn initialize() -> Option<Self> {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .is_ok()
            .then_some(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: The guard exists only after successful initialization and is
        // dropped synchronously on the same UI thread.
        unsafe { CoUninitialize() };
    }
}

/// Moves the focus onto a list box by way of its window: straight from a
/// button NVDA read the selected item twice (beta 8).
///
/// # Safety
/// `list` is a live window of the calling UI thread.
pub unsafe fn focus_list(list: HWND) {
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::{GetFocus, SetFocus},
        WindowsAndMessaging::GetParent,
    };
    if GetFocus() != list
        && let Ok(parent) = GetParent(list)
    {
        let _ = SetFocus(Some(parent));
    }
    let _ = SetFocus(Some(list));
}

/// The window a modal dialog disabled, kept on the dialog so closing it
/// can enable that window first.
const MODAL_OWNER: windows::core::PCWSTR = windows::core::w!("ApricotModalOwner");

/// Disables `owner` while `dialog` is open, remembering it for
/// [`destroy_modal`].
///
/// # Safety
/// Both handles are live windows of the calling UI thread.
pub unsafe fn disable_owner(dialog: HWND, owner: HWND) {
    use windows::Win32::{Foundation::HANDLE, UI::Input::KeyboardAndMouse::EnableWindow};
    let _ = windows::Win32::UI::WindowsAndMessaging::SetPropW(
        dialog,
        MODAL_OWNER,
        Some(HANDLE(owner.0)),
    );
    let _ = EnableWindow(owner, false);
}

/// Closes a modal dialog. The window it disabled is enabled before the
/// dialog goes away, because Windows hands the activation to another
/// program when the window below is still disabled (beta 8).
///
/// # Safety
/// `window` is a window of the calling UI thread.
pub unsafe fn destroy_modal(window: HWND) -> windows::core::Result<()> {
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::EnableWindow,
        WindowsAndMessaging::{DestroyWindow, RemovePropW},
    };
    if let Ok(owner) = RemovePropW(window, MODAL_OWNER)
        && !owner.is_invalid()
    {
        let _ = EnableWindow(HWND(owner.0), true);
    }
    DestroyWindow(window)
}

/// Tab or Shift+Tab onto a list box in a dialog: the focus goes by way of
/// the dialog, as when a window opens, because straight from a button NVDA
/// read the selected item twice (beta 8). `true` when the key was handled.
pub fn tab_into_list(dialog: HWND, message: &windows::Win32::UI::WindowsAndMessaging::MSG) -> bool {
    use windows::Win32::UI::{
        Input::KeyboardAndMouse::{
            GetFocus, GetKeyState, SetFocus, VK_CONTROL, VK_MENU, VK_SHIFT, VK_TAB,
        },
        WindowsAndMessaging::{GetClassNameW, GetNextDlgTabItem, IsChild, WM_KEYDOWN},
    };
    if message.message != WM_KEYDOWN || message.wParam.0 != usize::from(VK_TAB.0) {
        return false;
    }
    // SAFETY: Called on the dialog's UI thread with its live handles.
    unsafe {
        if GetKeyState(i32::from(VK_CONTROL.0)) < 0 || GetKeyState(i32::from(VK_MENU.0)) < 0 {
            return false;
        }
        let focus = GetFocus();
        if !IsChild(dialog, focus).as_bool() {
            return false;
        }
        let backward = GetKeyState(i32::from(VK_SHIFT.0)) < 0;
        let Ok(next) = GetNextDlgTabItem(dialog, Some(focus), backward) else {
            return false;
        };
        let mut class = [0_u16; 16];
        let length = GetClassNameW(next, &mut class);
        let is_list = usize::try_from(length).is_ok_and(|length| {
            String::from_utf16_lossy(&class[..length]).eq_ignore_ascii_case("ListBox")
        });
        if !is_list || next == focus {
            return false;
        }
        let _ = SetFocus(Some(dialog));
        let _ = SetFocus(Some(next));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{
        Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GetWindowTextW, WINDOW_EX_STYLE, WINDOW_STYLE,
        },
        core::w,
    };

    #[test]
    fn annotating_a_native_edit_preserves_its_text_value() {
        // SAFETY: This hidden standard control is created, read and destroyed on
        // the test thread; no user window, focus or input is touched.
        unsafe {
            let window = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("EDIT"),
                w!("Original lyrics"),
                WINDOW_STYLE::default(),
                0,
                0,
                100,
                30,
                None,
                None,
                None,
                None,
            )
            .expect("native edit control");
            annotate_control_name(window, "Lyrics");
            let mut text = [0_u16; 64];
            let copied = GetWindowTextW(window, &mut text);
            let value = String::from_utf16_lossy(&text[..usize::try_from(copied).unwrap()]);
            let _ = DestroyWindow(window);
            assert_eq!(value, "Original lyrics");
        }
    }
}
