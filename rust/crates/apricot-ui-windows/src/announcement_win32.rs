//! Single-path Windows screen-reader announcement adapter.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{mem::transmute, path::PathBuf};

use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE, HWND},
        System::{
            Com::{
                CLSIDFromProgID, COINIT_APARTMENTTHREADED, CoInitializeEx, DISPATCH_METHOD,
                DISPPARAMS, IDispatch,
            },
            LibraryLoader::{GetProcAddress, LoadLibraryW},
            Ole::GetActiveObject,
            Variant::VARIANT,
        },
        UI::{
            Accessibility::NotifyWinEvent,
            WindowsAndMessaging::{
                EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_VALUECHANGE, EVENT_SYSTEM_ALERT, GA_ROOT,
                GetAncestor, OBJID_ALERT, OBJID_CLIENT, SetWindowTextW,
            },
        },
    },
    core::{GUID, IUnknown, Interface, PCSTR, PCWSTR, w},
};

type TextFunction = unsafe extern "system" fn(*const u16) -> i32;
type CancelFunction = unsafe extern "system" fn() -> i32;

struct NvdaClient {
    module: HMODULE,
    speak: TextFunction,
    braille: Option<TextFunction>,
    cancel: Option<CancelFunction>,
}

impl NvdaClient {
    unsafe fn load() -> Option<Self> {
        for candidate in nvda_client_candidates() {
            if !candidate.is_absolute() || !candidate.is_file() {
                continue;
            }
            let path = wide(candidate.to_string_lossy().as_ref());
            let Ok(module) = LoadLibraryW(PCWSTR(path.as_ptr())) else {
                continue;
            };
            let Some(speak) = text_function(module, b"nvdaController_speakText\0") else {
                let _ = FreeLibrary(module);
                continue;
            };
            return Some(Self {
                module,
                speak,
                braille: text_function(module, b"nvdaController_brailleMessage\0"),
                cancel: cancel_function(module, b"nvdaController_cancelSpeech\0"),
            });
        }
        None
    }

    unsafe fn announce(&self, text: &[u16], interrupt: bool) -> bool {
        if interrupt && let Some(cancel) = self.cancel {
            let _ = cancel();
        }
        let spoken = (self.speak)(text.as_ptr()) == 0;
        let brailled = self
            .braille
            .is_some_and(|braille| braille(text.as_ptr()) == 0);
        spoken || brailled
    }
}

impl Drop for NvdaClient {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.module);
        }
    }
}

/// JAWS automation server, the path Python `_jaws_speak_ctypes` intends.
/// Python calls `ole32.CoGetActiveObject`, which ole32 does not export, so
/// its JAWS call never succeeds; here the running object comes from
/// `oleaut32.GetActiveObject` (Urh's decision after E6).
struct JawsClient {
    clsid: GUID,
}

const LOCALE_USER_DEFAULT: u32 = 0x0400;

impl JawsClient {
    /// Resolves the `ProgID` once. It is registered whenever JAWS is
    /// installed, running or not; without JAWS no COM call is made later.
    unsafe fn load() -> Option<Self> {
        CLSIDFromProgID(w!("FreedomSci.JawsApi"))
            .ok()
            .map(|clsid| Self { clsid })
    }

    /// Asks the running JAWS for a fresh reference on every call, so a JAWS
    /// restart during the session cannot leave a stale pointer behind.
    unsafe fn announce(&self, text: &str, interrupt: bool) -> bool {
        // S_FALSE or RPC_E_CHANGED_MODE both leave COM usable on this thread.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mut unknown: Option<IUnknown> = None;
        if GetActiveObject(&raw const self.clsid, None, &raw mut unknown).is_err() {
            return false;
        }
        let Some(dispatch) = unknown.and_then(|object| object.cast::<IDispatch>().ok()) else {
            return false;
        };
        let name = w!("SayString");
        let mut dispid = -1;
        if dispatch
            .GetIDsOfNames(
                &GUID::zeroed(),
                &raw const name,
                1,
                LOCALE_USER_DEFAULT,
                &raw mut dispid,
            )
            .is_err()
        {
            return false;
        }
        let mut arguments = jaws_say_string_arguments(text, interrupt);
        let parameters = DISPPARAMS {
            rgvarg: arguments.as_mut_ptr(),
            rgdispidNamedArgs: std::ptr::null_mut(),
            cArgs: 2,
            cNamedArgs: 0,
        };
        dispatch
            .Invoke(
                dispid,
                &GUID::zeroed(),
                LOCALE_USER_DEFAULT,
                DISPATCH_METHOD,
                &raw const parameters,
                None,
                None,
                None,
            )
            .is_ok()
    }
}

/// `SayString(text, flush)` arguments. `IDispatch` takes them in reverse
/// declaration order, so the flush flag comes first.
fn jaws_say_string_arguments(text: &str, flush: bool) -> [VARIANT; 2] {
    [VARIANT::from(flush), VARIANT::from(text)]
}

pub struct WindowsAnnouncer {
    status_control: HWND,
    nvda: Option<NvdaClient>,
    jaws: Option<JawsClient>,
}

impl WindowsAnnouncer {
    pub unsafe fn new(status_control: HWND) -> Self {
        Self {
            status_control,
            nvda: NvdaClient::load(),
            jaws: JawsClient::load(),
        }
    }

    pub unsafe fn announce(&self, text: &str, interrupt: bool) {
        if text.trim().is_empty() {
            return;
        }
        let wide_text = wide(text);
        if self
            .nvda
            .as_ref()
            .is_some_and(|client| client.announce(&wide_text, interrupt))
        {
            return;
        }
        // Python tries JAWS only when NVDA did not take the text, so a system
        // running both screen readers does not hear it twice.
        if self
            .jaws
            .as_ref()
            .is_some_and(|client| client.announce(text, interrupt))
        {
            return;
        }
        let text = wide_text;
        if self.status_control.is_invalid() {
            return;
        }
        // Python `raise_accessibility_alert` for Narrator and
        // other MSAA screen readers. The alert is raised only because neither
        // NVDA nor JAWS took the text; they would otherwise read it twice.
        let _ = SetWindowTextW(self.status_control, PCWSTR(text.as_ptr()));
        NotifyWinEvent(
            EVENT_OBJECT_NAMECHANGE,
            self.status_control,
            OBJID_CLIENT.0,
            0,
        );
        let root = GetAncestor(self.status_control, GA_ROOT);
        if !root.is_invalid() {
            NotifyWinEvent(EVENT_SYSTEM_ALERT, root, OBJID_ALERT.0, 0);
        }
        NotifyWinEvent(
            EVENT_OBJECT_VALUECHANGE,
            self.status_control,
            OBJID_CLIENT.0,
            0,
        );
    }
}

unsafe fn text_function(module: HMODULE, name: &'static [u8]) -> Option<TextFunction> {
    let address = GetProcAddress(module, PCSTR(name.as_ptr()))?;
    Some(transmute::<
        unsafe extern "system" fn() -> isize,
        TextFunction,
    >(address))
}

unsafe fn cancel_function(module: HMODULE, name: &'static [u8]) -> Option<CancelFunction> {
    let address = GetProcAddress(module, PCSTR(name.as_ptr()))?;
    Some(transmute::<
        unsafe extern "system" fn() -> isize,
        CancelFunction,
    >(address))
}

fn nvda_client_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        candidates.push(directory.join("nvda").join("nvdaControllerClient64.dll"));
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../vendor/nvda/nvdaControllerClient64.dll"),
    );
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(
            PathBuf::from(program_files)
                .join("NVDA")
                .join("nvdaControllerClient64.dll"),
        );
    }
    candidates
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{jaws_say_string_arguments, nvda_client_candidates};
    use windows::Win32::System::Variant::{VT_BOOL, VT_BSTR};

    #[test]
    fn jaws_say_string_receives_flush_first_and_text_last() {
        let arguments = jaws_say_string_arguments("Settings saved.", true);
        assert_eq!(arguments[0].vt(), VT_BOOL);
        assert!(bool::try_from(&arguments[0]).unwrap());
        assert_eq!(arguments[1].vt(), VT_BSTR);
        let text = unsafe { &arguments[1].Anonymous.Anonymous.Anonymous.bstrVal };
        assert_eq!(text.to_string(), "Settings saved.");
        let queued = jaws_say_string_arguments("x", false);
        assert!(!bool::try_from(&queued[0]).unwrap());
    }

    #[test]
    fn jaws_client_is_absent_or_resolved_without_panicking() {
        // Only a registered ProgID yields a client; nothing is spoken here.
        let _ = unsafe { super::JawsClient::load() };
    }

    #[test]
    fn nvda_libraries_are_loaded_only_from_absolute_candidates() {
        let candidates = nvda_client_candidates();
        assert!(!candidates.is_empty());
        assert!(candidates.iter().all(|path| path.is_absolute()));
        assert!(candidates.iter().all(|path| {
            path.file_name()
                .is_some_and(|name| name == "nvdaControllerClient64.dll")
        }));
    }
}
