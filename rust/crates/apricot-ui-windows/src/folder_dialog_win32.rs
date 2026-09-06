//! Native Windows folder picker used by local playback.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::path::PathBuf;

use windows::{
    Win32::{
        Foundation::HWND,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        },
        UI::Shell::{
            FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
            IFileOpenDialog, SIGDN_FILESYSPATH,
        },
    },
    core::PCWSTR,
};

pub fn choose_media_folder(owner: HWND, title: &str) -> Option<PathBuf> {
    // SAFETY: The dialog runs synchronously on the UI thread. COM is balanced
    // by the guard and the shell-allocated path is released after conversion.
    unsafe { choose_media_folder_win32(owner, title) }
}

unsafe fn choose_media_folder_win32(owner: HWND, title: &str) -> Option<PathBuf> {
    let _com = ComApartment::initialize()?;
    let dialog: IFileOpenDialog =
        CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
    let options = dialog.GetOptions().ok()?;
    dialog
        .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
        .ok()?;
    let title = wide(title);
    dialog.SetTitle(PCWSTR(title.as_ptr())).ok()?;
    dialog.Show(Some(owner)).ok()?;
    let item = dialog.GetResult().ok()?;
    let path = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
    let value = path.to_string().ok().map(PathBuf::from);
    CoTaskMemFree(Some(path.0.cast()));
    value
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
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
        // SAFETY: This guard is created only after successful initialization on
        // this thread and is dropped on the same synchronous call stack.
        unsafe { CoUninitialize() };
    }
}
