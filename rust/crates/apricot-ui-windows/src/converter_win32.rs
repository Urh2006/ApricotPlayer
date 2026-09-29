//! Python `show_converter_dialog` (file and folder converter) and the folder
//! conversion progress dialog (Python `show_conversion_progress_dialog`).
//!
//! The converter dialog only collects the choices and runs Python's checks;
//! the caller starts the conversion with the returned [`ConverterStart`].

#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use std::{
    ffi::c_void,
    mem::size_of,
    path::{Path, PathBuf},
    time::Instant,
};

use apricot_app::converter::{
    self, converted_folder_path, default_output_path, dialog_format_values, format_label,
    input_kind, path_from_text, replaced_output_path, shows_audio_video_options,
    with_default_extension,
};
use apricot_core::TranslationCatalog;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::{PBM_SETPOS, PBM_SETRANGE32},
            Input::KeyboardAndMouse::{
                EnableWindow, GetActiveWindow, GetFocus, SetFocus, VK_ESCAPE,
            },
            WindowsAndMessaging::{
                CB_ADDSTRING, CB_GETCURSEL, CB_GETDROPPEDSTATE, CB_RESETCONTENT, CB_SETCURSEL,
                CBS_DROPDOWNLIST, CW_USEDEFAULT, CreateDialogIndirectParamW, CreateWindowExW,
                DLGTEMPLATE, DS_CENTER, DS_MODALFRAME, DefWindowProcW, DestroyWindow,
                DispatchMessageW, ES_AUTOHSCROLL, GWLP_USERDATA, GetClientRect, GetMessageW,
                GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, HMENU,
                IDC_ARROW, IsChild, IsDialogMessageW, IsWindow, IsWindowVisible, KillTimer,
                LoadCursorW, MB_ICONWARNING, MB_OK, MSG, MessageBoxW, MoveWindow, PostQuitMessage,
                RegisterClassW, SW_HIDE, SW_SHOW, SWP_NOMOVE, SWP_NOZORDER, SendMessageW,
                SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW,
                ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE,
                WM_CLOSE, WM_COMMAND, WM_KEYDOWN, WM_NCDESTROY, WM_SETFONT, WM_TIMER, WNDCLASSW,
                WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_SYSMENU,
                WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, Result, w},
};

const CLASS_NAME: PCWSTR = w!("ApricotPlayer2BetaConverterWindow");
const PATH_LABEL: usize = 2401;
const PATH: usize = 2402;
const BROWSE: usize = 2403;
const TARGET: usize = 2404;
const TARGET_LABEL: usize = 2405;
const OPTIONS_LABEL: usize = 2406;
const ADD_IMAGE: usize = 2407;
const DARK: usize = 2408;
const IMAGE_LABEL: usize = 2409;
const IMAGE: usize = 2410;
const CHOOSE_IMAGE: usize = 2411;
const OUTPUT_LABEL: usize = 2412;
const CREATE_NEW: usize = 2413;
const REPLACE: usize = 2414;
const CONVERT: usize = 2415;
const BACK: usize = 2416;
const CONTROL_COUNT: usize = 16;
/// `IsDialogMessageW` turns Enter and Escape into `IDOK` and `IDCANCEL`.
const IDOK_COMMAND: usize = 1;
const IDCANCEL_COMMAND: usize = 2;
const BN_CLICKED: usize = 0;
const CBN_SELCHANGE: usize = 1;
const EN_CHANGE: usize = 0x0300;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: usize = 1;
const BS_AUTOCHECKBOX: u32 = 0x0003;

/// What Python hands to `start_file_conversion` or `start_folder_conversion`.
#[derive(Clone, Debug, PartialEq)]
pub enum ConverterStart {
    File {
        source: PathBuf,
        output: PathBuf,
        target: String,
        image: Option<PathBuf>,
        replace_original: bool,
    },
    Folder {
        source: PathBuf,
        output_folder: PathBuf,
        target: String,
        image: Option<PathBuf>,
        replace_originals: bool,
    },
}

pub struct ConverterDialogOptions {
    pub catalog: TranslationCatalog,
    pub folder_mode: bool,
    /// Python `announce_player`.
    pub announce: Box<dyn Fn(&str)>,
}

struct DialogState {
    controls: [HWND; CONTROL_COUNT],
    options: ConverterDialogOptions,
    targets: Vec<&'static str>,
    result: Option<ConverterStart>,
}

impl DialogState {
    fn control(&self, id: usize) -> HWND {
        self.controls[id - PATH_LABEL]
    }

    fn text(&self, key: &str) -> String {
        self.options.catalog.text(key).to_owned()
    }

    /// Python `selected_target`.
    unsafe fn selected_target(&self) -> &'static str {
        let selection = SendMessageW(self.control(TARGET), CB_GETCURSEL, None, None).0;
        usize::try_from(selection)
            .ok()
            .and_then(|index| self.targets.get(index).copied())
            .or_else(|| self.targets.first().copied())
            .unwrap_or("mp3")
    }

    unsafe fn checked(&self, id: usize) -> bool {
        SendMessageW(self.control(id), BM_GETCHECK, None, None).0 == BST_CHECKED.cast_signed()
    }

    unsafe fn set_checked(&self, id: usize, checked: bool) {
        SendMessageW(
            self.control(id),
            BM_SETCHECK,
            Some(WPARAM(if checked { BST_CHECKED } else { 0 })),
            None,
        );
    }

    unsafe fn shown(&self, id: usize) -> bool {
        IsWindowVisible(self.control(id)).as_bool()
    }
}

/// Opens the modal converter and returns the conversion to start, or `None`
/// when the user went back.
///
/// # Errors
/// Returns a Windows error if the dialog cannot be created.
pub fn show(owner: HWND, options: ConverterDialogOptions) -> Result<Option<ConverterStart>> {
    // SAFETY: The state stays owned by this call for the whole nested loop.
    unsafe { show_win32(owner, options) }
}

unsafe fn register(instance: HINSTANCE) -> Result<()> {
    let class = WNDCLASSW {
        cbWndExtra: i32::try_from(size_of::<isize>()).expect("pointer size fits i32"),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hInstance: instance,
        lpszClassName: CLASS_NAME,
        lpfnWndProc: Some(window_proc),
        ..Default::default()
    };
    // Registering the same process-local class again is harmless.
    let _ = RegisterClassW(&raw const class);
    Ok(())
}

unsafe fn show_win32(
    owner: HWND,
    options: ConverterDialogOptions,
) -> Result<Option<ConverterStart>> {
    let instance = HINSTANCE(GetModuleHandleW(None)?.0);
    register(instance)?;
    let title = wide(options.catalog.text(if options.folder_mode {
        "folder_converter"
    } else {
        "file_converter"
    }));
    let window = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        CLASS_NAME,
        PCWSTR(title.as_ptr()),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        640,
        360,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let mut state = Box::new(DialogState {
        controls: [HWND::default(); CONTROL_COUNT],
        options,
        targets: Vec::new(),
        result: None,
    });
    if let Err(error) = create_controls(window, instance, &mut state) {
        let _ = DestroyWindow(window);
        return Err(error);
    }
    // Python: "Create a new file" starts checked.
    state.set_checked(CREATE_NEW, true);
    SetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0), (&raw mut *state) as isize);
    update_formats(window, &mut state);
    let previous = GetFocus();
    let _ = EnableWindow(owner, false);
    let _ = ShowWindow(window, SW_SHOW);
    // wx.Dialog focuses its first control, the path field.
    let _ = SetFocus(Some(state.control(PATH)));
    let mut message = MSG::default();
    let mut error = None;
    while IsWindow(Some(window)).as_bool() {
        let status = GetMessageW(&raw mut message, None, 0, 0);
        if status.0 <= 0 {
            if status.0 < 0 {
                error = Some(windows::core::Error::from_thread());
            } else {
                PostQuitMessage(i32::try_from(message.wParam.0).unwrap_or_default());
            }
            let _ = DestroyWindow(window);
            break;
        }
        let ours = message.hwnd == window || IsChild(window, message.hwnd).as_bool();
        // Escape closes an open format list first, as in the wx choice.
        if ours
            && message.message == WM_KEYDOWN
            && message.wParam.0 == usize::from(VK_ESCAPE.0)
            && SendMessageW(state.control(TARGET), CB_GETDROPPEDSTATE, None, None).0 != 0
        {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
            continue;
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    let _ = EnableWindow(owner, true);
    let _ = SetForegroundWindow(owner);
    if IsWindow(Some(previous)).as_bool() {
        let _ = SetFocus(Some(previous));
    }
    match error {
        Some(error) => Err(error),
        None => Ok(state.result.take()),
    }
}

/// Controls in Python's creation order, which is also the Tab order.
// The control table mirrors Python's dialog in one place.
#[allow(clippy::too_many_lines)]
unsafe fn create_controls(
    window: HWND,
    instance: HINSTANCE,
    state: &mut DialogState,
) -> Result<()> {
    let folder = state.options.folder_mode;
    let path_key = if folder {
        "folder_to_convert"
    } else {
        "file_to_convert"
    };
    let specs: [(usize, PCWSTR, String, u32, bool); CONTROL_COUNT] = [
        (PATH_LABEL, w!("STATIC"), state.text(path_key), 0, false),
        (PATH, w!("EDIT"), String::new(), ES_AUTOHSCROLL as u32, true),
        (
            BROWSE,
            w!("BUTTON"),
            state.text(if folder {
                "browse_folder"
            } else {
                "browse_file"
            }),
            0,
            true,
        ),
        (
            TARGET,
            w!("COMBOBOX"),
            String::new(),
            (CBS_DROPDOWNLIST as u32) | WS_VSCROLL.0,
            true,
        ),
        (
            TARGET_LABEL,
            w!("STATIC"),
            state.text("convert_to"),
            0,
            false,
        ),
        (
            OPTIONS_LABEL,
            w!("STATIC"),
            state.text("converter_audio_to_video_options"),
            0,
            false,
        ),
        (
            ADD_IMAGE,
            w!("BUTTON"),
            state.text("add_image"),
            BS_AUTOCHECKBOX,
            true,
        ),
        (
            DARK,
            w!("BUTTON"),
            state.text("dark_background"),
            BS_AUTOCHECKBOX,
            true,
        ),
        (
            IMAGE_LABEL,
            w!("STATIC"),
            state.text("image_path"),
            0,
            false,
        ),
        (
            IMAGE,
            w!("EDIT"),
            String::new(),
            ES_AUTOHSCROLL as u32,
            true,
        ),
        (
            CHOOSE_IMAGE,
            w!("BUTTON"),
            state.text("choose_image"),
            0,
            true,
        ),
        (
            OUTPUT_LABEL,
            w!("STATIC"),
            state.text("output_format"),
            0,
            false,
        ),
        (
            CREATE_NEW,
            w!("BUTTON"),
            state.text(if folder {
                "converter_create_new_folder"
            } else {
                "converter_create_new_file"
            }),
            BS_AUTOCHECKBOX,
            true,
        ),
        (
            REPLACE,
            w!("BUTTON"),
            state.text(if folder {
                "converter_replace_originals"
            } else {
                "converter_replace_original_file"
            }),
            BS_AUTOCHECKBOX,
            true,
        ),
        (CONVERT, w!("BUTTON"), state.text("convert"), 0, true),
        (BACK, w!("BUTTON"), state.text("back"), 0, true),
    ];
    let font = GetStockObject(DEFAULT_GUI_FONT);
    for (index, (id, class, label, style, focusable)) in specs.into_iter().enumerate() {
        let text = wide(&label);
        let mut window_style = WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style);
        if focusable {
            window_style |= WS_TABSTOP;
        }
        let control = CreateWindowExW(
            if matches!(id, PATH | IMAGE) {
                WS_EX_CLIENTEDGE
            } else {
                WINDOW_EX_STYLE::default()
            },
            class,
            PCWSTR(text.as_ptr()),
            window_style,
            0,
            0,
            100,
            if id == TARGET { 300 } else { 24 },
            Some(window),
            Some(HMENU(id as *mut c_void)),
            Some(instance),
            None,
        )?;
        SendMessageW(
            control,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
        state.controls[index] = control;
    }
    // Python `SetName` on the path, format and image fields.
    let names = [
        (PATH, state.text(path_key)),
        (TARGET, state.text("convert_to")),
        (IMAGE, state.text("image_path")),
    ];
    for (id, name) in names {
        crate::accessibility_win32::annotate_control_name(state.control(id), &name);
    }
    Ok(())
}

/// Python `update_formats`: refills the formats and keeps the chosen one.
unsafe fn update_formats(window: HWND, state: &mut DialogState) {
    let path = window_text(state.control(PATH));
    let values = dialog_format_values(state.options.folder_mode, &path);
    if values != state.targets {
        let current = (!state.targets.is_empty()).then(|| state.selected_target());
        let combo = state.control(TARGET);
        SendMessageW(combo, CB_RESETCONTENT, None, None);
        for value in &values {
            let label = wide(&format_label(value));
            SendMessageW(
                combo,
                CB_ADDSTRING,
                None,
                Some(LPARAM(label.as_ptr() as isize)),
            );
        }
        let selection = current
            .and_then(|current| values.iter().position(|value| *value == current))
            .unwrap_or(0);
        SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(selection)), None);
        state.targets = values;
    }
    update_audio_video_controls(window, state);
}

/// Python `update_audio_video_controls`.
unsafe fn update_audio_video_controls(window: HWND, state: &DialogState) {
    let path = window_text(state.control(PATH));
    let show_options =
        shows_audio_video_options(state.options.folder_mode, &path, state.selected_target());
    if show_options && !state.checked(ADD_IMAGE) && !state.checked(DARK) {
        state.set_checked(DARK, true);
    }
    let show_image = show_options && state.checked(ADD_IMAGE);
    for (id, shown) in [
        (OPTIONS_LABEL, show_options),
        (ADD_IMAGE, show_options),
        (DARK, show_options),
        (IMAGE_LABEL, show_image),
        (IMAGE, show_image),
        (CHOOSE_IMAGE, show_image),
    ] {
        let _ = ShowWindow(state.control(id), if shown { SW_SHOW } else { SW_HIDE });
    }
    layout(window, state);
}

/// Python's two-column form, with hidden rows removed as `dialog.Fit()` does.
unsafe fn layout(window: HWND, state: &DialogState) {
    let margin = 12;
    let gap = 6;
    let row = 26;
    let label_width = 200;
    let width = 640;
    let field_x = margin + label_width + gap;
    let field_width = width - field_x - margin - 16;
    let button_width = 130;
    let mut y = margin;
    let place = |id: usize, x: i32, y: i32, w: i32, h: i32| {
        let _ = MoveWindow(state.control(id), x, y, w, h, true);
    };
    place(PATH_LABEL, margin, y + 4, label_width, row);
    place(PATH, field_x, y, field_width - button_width - gap, row);
    place(
        BROWSE,
        field_x + field_width - button_width,
        y,
        button_width,
        row,
    );
    y += row + gap;
    place(TARGET_LABEL, margin, y + 4, label_width, row);
    place(TARGET, field_x, y, field_width, row * 10);
    y += row + gap;
    if state.shown(ADD_IMAGE) {
        place(OPTIONS_LABEL, margin, y + 4, label_width, row);
        place(ADD_IMAGE, field_x, y, field_width, row);
        y += row + gap;
        place(DARK, field_x, y, field_width, row);
        y += row + gap;
    }
    if state.shown(IMAGE) {
        place(IMAGE_LABEL, margin, y + 4, label_width, row);
        place(IMAGE, field_x, y, field_width - button_width - gap, row);
        place(
            CHOOSE_IMAGE,
            field_x + field_width - button_width,
            y,
            button_width,
            row,
        );
        y += row + gap;
    }
    place(OUTPUT_LABEL, margin, y + 4, label_width, row);
    place(CREATE_NEW, field_x, y, field_width, row);
    y += row + 3;
    place(REPLACE, field_x, y, field_width, row);
    y += row + margin;
    let buttons_x = width - margin - 16 - button_width * 2 - gap;
    place(CONVERT, buttons_x, y, button_width, row + 4);
    place(
        BACK,
        buttons_x + button_width + gap,
        y,
        button_width,
        row + 4,
    );
    y += row + 4 + margin;
    // Resize the window around the client area, as `dialog.Fit()`.
    let mut client = RECT::default();
    let mut frame = RECT::default();
    if GetClientRect(window, &raw mut client).is_ok()
        && GetWindowRect(window, &raw mut frame).is_ok()
    {
        let extra = (frame.bottom - frame.top) - (client.bottom - client.top);
        let _ = SetWindowPos(
            window,
            None,
            0,
            0,
            frame.right - frame.left,
            y + extra,
            SWP_NOMOVE | SWP_NOZORDER,
        );
    }
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND => {
            let id = wparam.0 & 0xffff;
            let notification = (wparam.0 >> 16) & 0xffff;
            if let Some(state) = state_mut(window) {
                command(window, state, id, notification);
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

unsafe fn command(window: HWND, state: &mut DialogState, id: usize, notification: usize) {
    match (id, notification) {
        (IDCANCEL_COMMAND, _) | (BACK, BN_CLICKED) => {
            let _ = DestroyWindow(window);
        }
        (IDOK_COMMAND, _) => {
            // Enter presses the focused button; elsewhere wx does nothing.
            let focus = GetFocus();
            if let Some(button) = [BROWSE, CHOOSE_IMAGE, CONVERT, BACK]
                .into_iter()
                .find(|button| state.control(*button) == focus)
            {
                command(window, state, button, BN_CLICKED);
            }
        }
        (PATH, EN_CHANGE) => update_formats(window, state),
        (TARGET, CBN_SELCHANGE) => update_audio_video_controls(window, state),
        (BROWSE, BN_CLICKED) => browse_path(window, state),
        (CHOOSE_IMAGE, BN_CLICKED) => browse_image(window, state),
        (ADD_IMAGE, BN_CLICKED) => {
            // Python `on_add_image`.
            if state.checked(ADD_IMAGE) {
                state.set_checked(DARK, false);
            } else if !state.checked(DARK) {
                state.set_checked(DARK, true);
            }
            update_audio_video_controls(window, state);
        }
        (DARK, BN_CLICKED) => {
            // Python `on_dark`.
            if state.checked(DARK) {
                state.set_checked(ADD_IMAGE, false);
            } else if !state.checked(ADD_IMAGE) {
                state.set_checked(ADD_IMAGE, true);
            }
            update_audio_video_controls(window, state);
        }
        (CREATE_NEW, BN_CLICKED) => {
            // Python `on_create_new`.
            if state.checked(CREATE_NEW) {
                state.set_checked(REPLACE, false);
            } else if !state.checked(REPLACE) {
                state.set_checked(CREATE_NEW, true);
            }
        }
        (REPLACE, BN_CLICKED) => {
            // Python `on_replace`.
            if state.checked(REPLACE) {
                state.set_checked(CREATE_NEW, false);
            } else if !state.checked(CREATE_NEW) {
                state.set_checked(REPLACE, true);
            }
        }
        (CONVERT, BN_CLICKED) => convert(window, state),
        _ => {}
    }
}

/// Python `browse_path`.
unsafe fn browse_path(window: HWND, state: &mut DialogState) {
    let focus = GetFocus();
    let chosen = if state.options.folder_mode {
        crate::folder_dialog_win32::choose_media_folder(window, &state.text("browse_folder"))
    } else {
        let filters = converter::input_filters(&state.options.catalog);
        crate::file_dialog_win32::choose_file_with_filters(
            window,
            &state.text("browse_file"),
            &filters,
        )
        .ok()
        .flatten()
    };
    restore_focus(focus);
    if let Some(path) = chosen {
        // EN_CHANGE runs `update_formats`.
        set_window_text(state.control(PATH), &path.to_string_lossy());
    }
    update_formats(window, state);
}

/// Python `browse_image`.
unsafe fn browse_image(window: HWND, state: &DialogState) {
    let focus = GetFocus();
    let filters = converter::image_filters(&state.options.catalog);
    let chosen = crate::file_dialog_win32::choose_file_with_filters(
        window,
        &state.text("select_image_file"),
        &filters,
    );
    restore_focus(focus);
    if let Ok(Some(path)) = chosen {
        set_window_text(state.control(IMAGE), &path.to_string_lossy());
    }
}

/// Python `convert`.
unsafe fn convert(window: HWND, state: &mut DialogState) {
    let raw = window_text(state.control(PATH));
    let source = path_from_text(&raw);
    if raw.trim().trim_matches('"').is_empty() || !source.exists() {
        warn(window, &state.text("no_selection"));
        return;
    }
    let target = state.selected_target();
    let use_image = state.shown(ADD_IMAGE) && state.checked(ADD_IMAGE);
    let image = use_image.then(|| path_from_text(&window_text(state.control(IMAGE))));
    if image.as_ref().is_some_and(|image| !image.exists()) {
        warn(window, &state.text("select_image_file"));
        return;
    }
    let replace = state.checked(REPLACE);
    let start = if state.options.folder_mode {
        let output_folder = if replace {
            source.clone()
        } else {
            let focus = GetFocus();
            let chosen = crate::folder_dialog_win32::choose_download_folder(
                window,
                &state.text("choose_output_folder"),
                &source,
            );
            restore_focus(focus);
            let Some(chosen) = chosen else {
                (state.options.announce)(&state.text("conversion_cancelled"));
                return;
            };
            converted_folder_path(&chosen, &source)
        };
        ConverterStart::Folder {
            source,
            output_folder,
            target: target.to_owned(),
            image,
            replace_originals: replace,
        }
    } else {
        if input_kind(&source).is_none() {
            warn(window, &state.text("unsupported_input_format"));
            return;
        }
        let output = if replace {
            replaced_output_path(&source, target)
        } else {
            let Some(output) = choose_output_file(window, state, &source, target) else {
                (state.options.announce)(&state.text("conversion_cancelled"));
                return;
            };
            with_default_extension(output, target)
        };
        ConverterStart::File {
            source,
            output,
            target: target.to_owned(),
            image,
            replace_original: replace,
        }
    };
    state.result = Some(start);
    let _ = DestroyWindow(window);
}

unsafe fn choose_output_file(
    window: HWND,
    state: &DialogState,
    source: &Path,
    target: &str,
) -> Option<PathBuf> {
    let default_output = default_output_path(source, target);
    let focus = GetFocus();
    let chosen = crate::file_dialog_win32::save_file_with_filters(
        window,
        &state.text("choose_output_file"),
        &converter::target_filters(&state.options.catalog, target),
        default_output.parent().unwrap_or_else(|| Path::new("")),
        &default_output
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        converter::output_extension(target),
    );
    restore_focus(focus);
    chosen.ok().flatten()
}

/// Python `self.message(text, wx.ICON_WARNING)` over the converter.
unsafe fn warn(window: HWND, text: &str) {
    let focus = GetFocus();
    let text = wide(text);
    let _ = MessageBoxW(
        Some(window),
        PCWSTR(text.as_ptr()),
        w!("ApricotPlayer 2 Beta"),
        MB_OK | MB_ICONWARNING,
    );
    restore_focus(focus);
}

unsafe fn restore_focus(focus: HWND) {
    if !focus.is_invalid() && IsWindow(Some(focus)).as_bool() {
        let _ = SetFocus(Some(focus));
    }
}

unsafe fn state_mut(window: HWND) -> Option<&'static mut DialogState> {
    let pointer = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut DialogState;
    pointer.as_mut()
}

unsafe fn window_text(control: HWND) -> String {
    let length = GetWindowTextLengthW(control);
    let mut value = vec![0_u16; usize::try_from(length).unwrap_or_default() + 1];
    let copied = GetWindowTextW(control, &mut value);
    String::from_utf16_lossy(&value[..usize::try_from(copied).unwrap_or_default()])
}

unsafe fn set_window_text(control: HWND, text: &str) {
    let text = wide(text);
    let _ = SetWindowTextW(control, PCWSTR(text.as_ptr()));
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

const PROGRESS_MESSAGE: i32 = 2451;
const PROGRESS_BAR: i32 = 2452;
const PROGRESS_TIMES: i32 = 2453;
const PROGRESS_TIMER: usize = 1;

struct ProgressState {
    owner: HWND,
    previous_focus: HWND,
    message: HWND,
    bar: HWND,
    times: HWND,
    total: usize,
    converted: usize,
    started: Instant,
}

/// Python's modeless `wx.ProgressDialog` for folder conversion: a real dialog
/// window, so screen readers read its title and text when it appears.
#[derive(Clone, Copy)]
pub struct ConversionProgressWindow {
    window: HWND,
}

impl ConversionProgressWindow {
    /// Python `show_conversion_progress_dialog`.
    ///
    /// # Errors
    /// Returns a Windows error if the dialog cannot be created.
    pub unsafe fn create(owner: HWND, title: &str, message: &str, total: usize) -> Result<Self> {
        let instance = HINSTANCE(GetModuleHandleW(None)?.0);
        let template = dialog_template(title);
        let window = CreateDialogIndirectParamW(
            Some(instance),
            template.as_ptr().cast::<DLGTEMPLATE>(),
            Some(owner),
            Some(progress_proc),
            LPARAM(0),
        )?;
        let font = GetStockObject(DEFAULT_GUI_FONT);
        let control = |class: PCWSTR, text: &str, id: i32| -> Result<HWND> {
            let text = wide(text);
            let control = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                PCWSTR(text.as_ptr()),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                10,
                10,
                Some(window),
                Some(HMENU(id as isize as *mut c_void)),
                Some(instance),
                None,
            )?;
            SendMessageW(
                control,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
            Ok(control)
        };
        let created = (|| {
            Ok::<_, windows::core::Error>((
                control(w!("STATIC"), message, PROGRESS_MESSAGE)?,
                control(w!("msctls_progress32"), "", PROGRESS_BAR)?,
                control(w!("STATIC"), "", PROGRESS_TIMES)?,
            ))
        })();
        let (message, bar, times) = match created {
            Ok(controls) => controls,
            Err(error) => {
                let _ = DestroyWindow(window);
                return Err(error);
            }
        };
        let maximum = total.max(1);
        SendMessageW(
            bar,
            PBM_SETRANGE32,
            Some(WPARAM(0)),
            Some(LPARAM(isize::try_from(maximum).unwrap_or(isize::MAX))),
        );
        let _ = MoveWindow(message, 12, 12, 436, 52, true);
        let _ = MoveWindow(bar, 12, 72, 436, 20, true);
        let _ = MoveWindow(times, 12, 100, 436, 52, true);
        let state = Box::new(ProgressState {
            owner,
            previous_focus: GetFocus(),
            message,
            bar,
            times,
            total: maximum,
            converted: 0,
            started: Instant::now(),
        });
        SetWindowLongPtrW(window, GWLP_USERDATA, Box::into_raw(state) as isize);
        update_times(window);
        let _ = SetTimer(Some(window), PROGRESS_TIMER, 1000, None);
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        Ok(Self { window })
    }

    pub unsafe fn is_open(self) -> bool {
        IsWindow(Some(self.window)).as_bool()
    }

    /// Python `update_conversion_progress_dialog`.
    pub unsafe fn update(self, converted: usize, message: &str) {
        let Some(state) = progress_state(self.window) else {
            return;
        };
        state.converted = converted.min(state.total);
        set_window_text(state.message, message);
        SendMessageW(state.bar, PBM_SETPOS, Some(WPARAM(state.converted)), None);
        update_times(self.window);
    }

    pub unsafe fn handles_dialog_message(self, message: &MSG) -> bool {
        self.is_open() && IsDialogMessageW(self.window, message).as_bool()
    }

    /// Python `close_conversion_progress_dialog`. Focus goes back to where it
    /// was when the dialog still had it.
    pub unsafe fn destroy(self) {
        if !self.is_open() {
            return;
        }
        let active = GetActiveWindow() == self.window;
        let (owner, previous) = progress_state(self.window)
            .map_or((HWND::default(), HWND::default()), |state| {
                (state.owner, state.previous_focus)
            });
        let _ = DestroyWindow(self.window);
        if active && IsWindow(Some(owner)).as_bool() {
            let _ = SetForegroundWindow(owner);
            restore_focus(previous);
        }
    }
}

/// An empty dialog template: the window class is the system dialog class.
fn dialog_template(title: &str) -> Vec<u32> {
    let style = (WS_POPUP | WS_CAPTION | WS_SYSMENU).0
        | u32::try_from(DS_MODALFRAME | DS_CENTER).unwrap_or_default();
    let mut words: Vec<u16> = Vec::new();
    let header = DLGTEMPLATE {
        style,
        dwExtendedStyle: 0,
        cdit: 0,
        x: 0,
        y: 0,
        cx: 250,
        cy: 110,
    };
    // SAFETY: DLGTEMPLATE is a packed plain-data struct of 18 bytes.
    let bytes = unsafe {
        std::slice::from_raw_parts((&raw const header).cast::<u8>(), size_of::<DLGTEMPLATE>())
    };
    words.extend(
        bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair)),
    );
    // No menu, the default dialog class, then the title.
    words.push(0);
    words.push(0);
    words.extend(title.encode_utf16());
    words.push(0);
    if words.len() % 2 == 1 {
        words.push(0);
    }
    words
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u32::from(pair[0]) | (u32::from(pair[1]) << 16))
        .collect()
}

/// wx shows elapsed, estimated and remaining time under the gauge.
unsafe fn update_times(window: HWND) {
    let Some(state) = progress_state(window) else {
        return;
    };
    let elapsed = state.started.elapsed().as_secs();
    let (estimated, remaining) = if state.converted == 0 {
        ("Unknown".to_owned(), "Unknown".to_owned())
    } else {
        let estimated = elapsed * u64::try_from(state.total).unwrap_or(1)
            / u64::try_from(state.converted).unwrap_or(1);
        (clock(estimated), clock(estimated.saturating_sub(elapsed)))
    };
    let text = format!(
        "Elapsed time: {}\r\nEstimated time: {estimated}\r\nRemaining time: {remaining}",
        clock(elapsed)
    );
    set_window_text(state.times, &text);
}

fn clock(seconds: u64) -> String {
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

unsafe extern "system" fn progress_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    _lparam: LPARAM,
) -> isize {
    // WM_INITDIALOG returns 0 as well: there is nothing to focus, so the
    // dialog itself keeps focus, as wx's native progress dialog without an
    // enabled button.
    match message {
        WM_TIMER if wparam.0 == PROGRESS_TIMER => {
            update_times(window);
            1
        }
        // Python's dialog has no abort button and cannot be closed.
        WM_CLOSE => 1,
        WM_COMMAND if (wparam.0 & 0xffff) == IDCANCEL_COMMAND => 1,
        WM_NCDESTROY => {
            let _ = KillTimer(Some(window), PROGRESS_TIMER);
            let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut ProgressState;
            SetWindowLongPtrW(window, GWLP_USERDATA, 0);
            if !pointer.is_null() {
                drop(Box::from_raw(pointer));
            }
            0
        }
        _ => 0,
    }
}

unsafe fn progress_state(window: HWND) -> Option<&'static mut ProgressState> {
    let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut ProgressState;
    pointer.as_mut()
}

#[cfg(test)]
mod tests {
    use super::{clock, dialog_template};

    #[test]
    fn dialog_template_is_dword_aligned_and_carries_the_title() {
        let template = dialog_template("Converting folder");
        let words = template
            .iter()
            .flat_map(|word| [(*word & 0xffff) as u16, (*word >> 16) as u16])
            .collect::<Vec<_>>();
        // 18 header bytes, then menu and class, then the title.
        assert_eq!(words[9], 0);
        assert_eq!(words[10], 0);
        let title = String::from_utf16_lossy(&words[11..11 + 17]);
        assert_eq!(title, "Converting folder");
        assert_eq!(words[28], 0);
    }

    #[test]
    fn clock_matches_wx_time_format() {
        assert_eq!(clock(5), "0:00:05");
        assert_eq!(clock(3_725), "1:02:05");
    }
}
