//! Single-path Windows screen-reader announcement adapter.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{mem::transmute, path::PathBuf};

use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE, HWND},
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
        UI::{
            Accessibility::NotifyWinEvent,
            WindowsAndMessaging::{EVENT_OBJECT_NAMECHANGE, OBJID_CLIENT, SetWindowTextW},
        },
    },
    core::{PCSTR, PCWSTR},
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

pub struct WindowsAnnouncer {
    status_control: HWND,
    nvda: Option<NvdaClient>,
}

impl WindowsAnnouncer {
    pub unsafe fn new(status_control: HWND) -> Self {
        Self {
            status_control,
            nvda: NvdaClient::load(),
        }
    }

    pub unsafe fn announce(&self, text: &str, interrupt: bool) {
        if text.trim().is_empty() {
            return;
        }
        let text = wide(text);
        if self
            .nvda
            .as_ref()
            .is_some_and(|client| client.announce(&text, interrupt))
        {
            return;
        }
        let _ = SetWindowTextW(self.status_control, PCWSTR(text.as_ptr()));
        NotifyWinEvent(
            EVENT_OBJECT_NAMECHANGE,
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
    use super::nvda_client_candidates;

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
