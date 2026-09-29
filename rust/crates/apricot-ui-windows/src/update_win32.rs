//! Python `show_update_prompt`: "Update available" with the version, the
//! read-only "What's new?" text, "Would you like to update now?", and the
//! Update now and Skip this version buttons.

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::ffi::c_void;

use apricot_core::TranslationCatalog;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::EM_SETSEL,
            Input::KeyboardAndMouse::SetFocus,
            WindowsAndMessaging::{
                BS_DEFPUSHBUTTON, BS_PUSHBUTTON, CreateWindowExW, DLGTEMPLATE,
                DialogBoxIndirectParamW, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, ES_WANTRETURN,
                EndDialog, GetClientRect, GetDlgItem, HMENU, MoveWindow, SendMessageW,
                WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_INITDIALOG, WM_SETFONT, WM_SIZE,
                WS_CHILD, WS_EX_CLIENTEDGE, WS_TABSTOP, WS_THICKFRAME, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, w},
};

const VERSION_LABEL: i32 = 2501;
const WHATS_NEW_LABEL: i32 = 2502;
const DETAILS: i32 = 2503;
const QUESTION: i32 = 2504;
/// wx `ID_YES` and `ID_NO` as the Windows `IDYES` and `IDNO`.
const UPDATE_NOW: i32 = 6;
const SKIP: i32 = 7;
const IDCANCEL_COMMAND: usize = 2;
const DM_SETDEFID: u32 = 0x0401;

struct PromptTexts {
    version: String,
    whats_new: String,
    changelog: String,
    question: String,
    update_now: String,
    skip: String,
}

/// Python `show_update_prompt`. `true` means Update now; Skip this version,
/// Escape and closing the window mean skip.
pub unsafe fn show_update_prompt(
    owner: HWND,
    catalog: &TranslationCatalog,
    version: &str,
    changelog: &str,
) -> bool {
    let Ok(module) = GetModuleHandleW(None) else {
        return false;
    };
    let texts = PromptTexts {
        version: catalog
            .text("update_version_heading")
            .replace("{version}", version),
        whats_new: catalog.text("whats_new").to_owned(),
        changelog: changelog.replace("\r\n", "\n").replace('\n', "\r\n"),
        question: catalog.text("update_now").to_owned(),
        update_now: catalog.text("update_now_button").to_owned(),
        skip: catalog.text("skip_version_button").to_owned(),
    };
    // wx `SetMinSize((640, 420))` with a resizable border.
    let template = crate::converter_win32::sized_dialog_template(
        catalog.text("update_available_title"),
        430,
        280,
        WS_THICKFRAME.0,
    );
    let result = DialogBoxIndirectParamW(
        Some(HINSTANCE(module.0)),
        template.as_ptr().cast::<DLGTEMPLATE>(),
        Some(owner),
        Some(prompt_proc),
        LPARAM(std::ptr::from_ref(&texts) as isize),
    );
    result == isize::try_from(UPDATE_NOW).unwrap_or_default()
}

unsafe extern "system" fn prompt_proc(
    dialog: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    match message {
        WM_INITDIALOG => {
            let texts = &*(lparam.0 as *const PromptTexts);
            create_controls(dialog, texts);
            layout(dialog);
            // wx.CallAfter(safe_set_focus, details): the caret starts at the
            // top of the text.
            if let Ok(details) = GetDlgItem(Some(dialog), DETAILS) {
                let _ = SetFocus(Some(details));
                SendMessageW(details, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(0)));
            }
            0
        }
        WM_SIZE => {
            layout(dialog);
            1
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            if id == usize::try_from(UPDATE_NOW).unwrap_or_default() {
                let _ = EndDialog(dialog, isize::try_from(UPDATE_NOW).unwrap_or_default());
            } else if id == usize::try_from(SKIP).unwrap_or_default() || id == IDCANCEL_COMMAND {
                // wx `SetEscapeId(wx.ID_NO)`: Escape and closing skip.
                let _ = EndDialog(dialog, isize::try_from(SKIP).unwrap_or_default());
            }
            1
        }
        _ => 0,
    }
}

unsafe fn create_controls(dialog: HWND, texts: &PromptTexts) {
    let Ok(module) = GetModuleHandleW(None) else {
        return;
    };
    let instance = HINSTANCE(module.0);
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let control = |class: PCWSTR, text: &str, id: i32, style: WINDOW_STYLE, extended| {
        let text: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let created = CreateWindowExW(
            extended,
            class,
            PCWSTR(text.as_ptr()),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            10,
            10,
            Some(dialog),
            Some(HMENU(id as isize as *mut c_void)),
            Some(instance),
            None,
        );
        if let Ok(created) = created {
            SendMessageW(
                created,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }
        created.ok()
    };
    let none = WINDOW_STYLE::default();
    let plain = WINDOW_EX_STYLE::default();
    control(w!("STATIC"), &texts.version, VERSION_LABEL, none, plain);
    control(w!("STATIC"), &texts.whats_new, WHATS_NEW_LABEL, none, plain);
    // A read-only multiline wx.TextCtrl keeps Enter to itself.
    let details_style = WS_TABSTOP
        | WS_VSCROLL
        | WINDOW_STYLE(
            u32::try_from(ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL | ES_WANTRETURN)
                .unwrap_or_default(),
        );
    if let Some(details) = control(
        w!("EDIT"),
        &texts.changelog,
        DETAILS,
        details_style,
        WS_EX_CLIENTEDGE,
    ) {
        crate::accessibility_win32::annotate_control_name(details, &texts.whats_new);
    }
    control(w!("STATIC"), &texts.question, QUESTION, none, plain);
    control(
        w!("BUTTON"),
        &texts.update_now,
        UPDATE_NOW,
        WS_TABSTOP | WINDOW_STYLE(u32::try_from(BS_DEFPUSHBUTTON).unwrap_or_default()),
        plain,
    );
    control(
        w!("BUTTON"),
        &texts.skip,
        SKIP,
        WS_TABSTOP | WINDOW_STYLE(u32::try_from(BS_PUSHBUTTON).unwrap_or_default()),
        plain,
    );
    // wx `update_button.SetDefault()`.
    SendMessageW(
        dialog,
        DM_SETDEFID,
        Some(WPARAM(usize::try_from(UPDATE_NOW).unwrap_or_default())),
        None,
    );
}

unsafe fn layout(dialog: HWND) {
    let mut client = RECT::default();
    if GetClientRect(dialog, &raw mut client).is_err() {
        return;
    }
    let width = (client.right - 20).max(100);
    let place = |id: i32, x: i32, y: i32, w: i32, h: i32| {
        if let Ok(control) = GetDlgItem(Some(dialog), id) {
            let _ = MoveWindow(control, x, y, w, h, true);
        }
    };
    let buttons_top = client.bottom - 40;
    let question_top = buttons_top - 30;
    place(VERSION_LABEL, 10, 10, width, 20);
    place(WHATS_NEW_LABEL, 10, 38, width, 20);
    place(DETAILS, 10, 60, width, (question_top - 70).max(40));
    place(QUESTION, 10, question_top, width, 20);
    place(UPDATE_NOW, client.right - 250, buttons_top, 115, 28);
    place(SKIP, client.right - 125, buttons_top, 115, 28);
}
