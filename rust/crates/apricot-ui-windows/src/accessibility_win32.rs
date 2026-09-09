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
                CLSID_AccPropServices, IAccPropServices, NotifyWinEvent, PROPID_ACC_NAME,
            },
            WindowsAndMessaging::{
                CHILDID_SELF, EVENT_OBJECT_NAMECHANGE, OBJID_CLIENT, SetWindowTextW,
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
    let value = wide(value);
    let _ = SetWindowTextW(window, PCWSTR(value.as_ptr()));
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
