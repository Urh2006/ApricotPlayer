//! Accessible, scrollable native forms shared by optional player tools.
#![allow(unsafe_code, unsafe_op_in_unsafe_fn)]

use apricot_shell::{FeatureForm, FormFieldKind, FormValue, FormValues};
use std::{ffi::c_void, mem::size_of};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Controls::SetScrollInfo,
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, IsWindowEnabled, SetFocus},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, CBS_DROPDOWNLIST, CW_USEDEFAULT,
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL,
                ES_PASSWORD, ES_READONLY, GetClientRect, GetMessageW, GetParent, GetWindowLongPtrW,
                GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsDialogMessageW, IsWindow,
                LoadCursorW, MB_ICONWARNING, MB_OK, MSG, MessageBoxW, MoveWindow, PostQuitMessage,
                RegisterClassW, SB_VERT, SCROLLINFO, SIF_PAGE, SIF_POS, SIF_RANGE, SW_SHOW,
                SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW, ShowWindow,
                TranslateMessage, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
                WM_NCDESTROY, WM_SETFOCUS, WM_SETFONT, WM_SIZE, WM_VSCROLL, WNDCLASSW, WS_CAPTION,
                WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_SYSMENU, WS_TABSTOP,
                WS_THICKFRAME, WS_VISIBLE, WS_VSCROLL,
            },
        },
    },
    core::{PCWSTR, w},
};

const FIRST_FIELD: usize = 200;
const ROW: i32 = 40;
const CLASS: PCWSTR = w!("ApricotFeatureForm");

struct Bound {
    label: Option<HWND>,
    control: HWND,
    /// The Browse button of a file field.
    browse: Option<HWND>,
}

/// Browse buttons use these ids plus the field index.
const FIRST_BROWSE: usize = 600;
#[derive(Clone, Copy)]
struct Preview {
    data: *mut c_void,
    invoke: unsafe fn(*mut c_void, &FormValues),
}
struct State {
    form: FeatureForm,
    fields: Vec<Bound>,
    submit: HWND,
    cancel: HWND,
    offset: i32,
    result: Option<FormValues>,
    preview: Option<Preview>,
}

/// Shows a native form, preserving the owner's message processing while it is open.
pub(crate) fn show(owner: HWND, form: &FeatureForm) -> Option<FormValues> {
    // SAFETY: window state and controls live on the UI thread until the modal loop exits.
    unsafe { show_inner(owner, form, None).ok().flatten() }
}

pub(crate) fn show_preview(
    owner: HWND,
    form: &FeatureForm,
    preview: &mut dyn FnMut(&FormValues),
) -> Option<FormValues> {
    // The erased callback points to this stack slot only during the synchronous modal loop.
    // No callback is retained by any window after show_inner destroys the dialog.
    let mut callback = preview;
    let preview = Preview {
        data: std::ptr::from_mut(&mut callback).cast(),
        invoke: invoke_preview,
    };
    unsafe { show_inner(owner, form, Some(preview)).ok().flatten() }
}

unsafe fn invoke_preview(data: *mut c_void, values: &FormValues) {
    let callback = &mut *data.cast::<&mut dyn FnMut(&FormValues)>();
    callback(values);
}

unsafe fn show_inner(
    owner: HWND,
    form: &FeatureForm,
    preview: Option<Preview>,
) -> windows::core::Result<Option<FormValues>> {
    let previous_focus = GetFocus();
    let instance = HINSTANCE(GetModuleHandleW(None)?.0);
    let class = WNDCLASSW {
        cbWndExtra: i32::try_from(size_of::<isize>()).unwrap_or(8),
        hCursor: LoadCursorW(None, IDC_ARROW)?,
        hInstance: instance,
        lpszClassName: CLASS,
        lpfnWndProc: Some(window_proc),
        ..Default::default()
    };
    // Reopening the form deliberately reuses the registered class.
    let _ = RegisterClassW(&raw const class);
    let title = wide(&form.title);
    let field_height = i32::try_from(form.fields.len())
        .unwrap_or(12)
        .saturating_mul(ROW);
    let height = (field_height + 125).clamp(200, 720);
    let window = CreateWindowExW(
        WS_EX_DLGMODALFRAME,
        CLASS,
        PCWSTR(title.as_ptr()),
        WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_VSCROLL,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        760,
        height,
        Some(owner),
        None,
        Some(instance),
        None,
    )?;
    let controls = create_controls(window, instance, form);
    let (fields, submit, cancel) = match controls {
        Ok(controls) => controls,
        Err(error) => {
            let _ = DestroyWindow(window);
            return Err(error);
        }
    };
    let mut state = Box::new(State {
        form: form.clone(),
        fields,
        submit,
        cancel,
        offset: 0,
        result: None,
        preview,
    });
    SetWindowLongPtrW(
        window,
        WINDOW_LONG_PTR_INDEX(0),
        std::ptr::from_mut(state.as_mut()) as isize,
    );
    let owner_enabled = IsWindowEnabled(owner).as_bool();
    let _ = EnableWindow(owner, false);
    layout(window, &mut state);
    let _ = ShowWindow(window, SW_SHOW);
    let _ = SetForegroundWindow(window);
    let first = state
        .fields
        .get(form.initial_focus)
        .or(state.fields.first())
        .map_or(submit, |field| field.control);
    let _ = SetFocus(Some(first));
    let mut message = MSG::default();
    while IsWindow(Some(window)).as_bool() {
        let received = GetMessageW(&raw mut message, None, 0, 0).0;
        if received <= 0 {
            if received == 0 {
                PostQuitMessage(i32::try_from(message.wParam.0).unwrap_or_default());
            }
            let _ = DestroyWindow(window);
            break;
        }
        if crate::accessibility_win32::tab_into_list(window, &message) {
            continue;
        }
        if !IsDialogMessageW(window, &raw const message).as_bool() {
            let _ = TranslateMessage(&raw const message);
            DispatchMessageW(&raw const message);
        }
    }
    if owner_enabled && IsWindow(Some(owner)).as_bool() {
        let _ = EnableWindow(owner, true);
        let _ = SetForegroundWindow(owner);
        let _ = SetFocus(Some(if IsWindow(Some(previous_focus)).as_bool() {
            previous_focus
        } else {
            owner
        }));
    }
    Ok(state.result.take())
}

unsafe fn child(
    parent: HWND,
    instance: HINSTANCE,
    class: PCWSTR,
    title: &str,
    style: WINDOW_STYLE,
    id: usize,
) -> windows::core::Result<HWND> {
    let title = wide(title);
    let control = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        class,
        PCWSTR(title.as_ptr()),
        WS_CHILD | WS_VISIBLE | style,
        0,
        0,
        0,
        0,
        Some(parent),
        Some(HMENU(id as *mut c_void)),
        Some(instance),
        None,
    )?;
    SendMessageW(
        control,
        WM_SETFONT,
        Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize)),
        Some(LPARAM(1)),
    );
    Ok(control)
}

#[allow(clippy::too_many_lines)] // One branch per field kind.
unsafe fn create_controls(
    parent: HWND,
    instance: HINSTANCE,
    form: &FeatureForm,
) -> windows::core::Result<(Vec<Bound>, HWND, HWND)> {
    let mut fields = Vec::new();
    for (index, field) in form.fields.iter().enumerate() {
        let label = if matches!(field.kind, FormFieldKind::Checkbox { .. }) {
            None
        } else {
            Some(child(
                parent,
                instance,
                w!("STATIC"),
                &field.label,
                WINDOW_STYLE::default(),
                0,
            )?)
        };
        let id = FIRST_FIELD + index;
        let control = match &field.kind {
            FormFieldKind::File { value, .. } => child(
                parent,
                instance,
                w!("EDIT"),
                value,
                WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                id,
            )?,
            FormFieldKind::Text { value, secret } => child(
                parent,
                instance,
                w!("EDIT"),
                value,
                WS_TABSTOP
                    | WINDOW_STYLE(
                        ES_AUTOHSCROLL as u32 | if *secret { ES_PASSWORD as u32 } else { 0 },
                    ),
                id,
            )?,
            FormFieldKind::Number { value, .. } => child(
                parent,
                instance,
                w!("EDIT"),
                &value.to_string(),
                WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                id,
            )?,
            FormFieldKind::ReadOnly { value } => child(
                parent,
                instance,
                w!("EDIT"),
                value,
                WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32 | ES_READONLY as u32),
                id,
            )?,
            FormFieldKind::Checkbox { checked } => {
                let control = child(
                    parent,
                    instance,
                    w!("BUTTON"),
                    &field.label,
                    WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                    id,
                )?;
                SendMessageW(control, 0x00F1, Some(WPARAM(usize::from(*checked))), None);
                control
            }
            FormFieldKind::Choice { options, selected } => {
                let control = child(
                    parent,
                    instance,
                    w!("COMBOBOX"),
                    "",
                    WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
                    id,
                )?;
                for option in options {
                    let option = wide(option);
                    SendMessageW(
                        control,
                        0x0143,
                        None,
                        Some(LPARAM(option.as_ptr() as isize)),
                    );
                }
                SendMessageW(control, 0x014E, Some(WPARAM(*selected)), None);
                control
            }
        };
        if !SetWindowSubclass(control, Some(field_proc), 1, index).as_bool() {
            return Err(windows::core::Error::from_thread());
        }
        // The Browse button follows its field in the Tab order.
        let browse = match &field.kind {
            FormFieldKind::File { browse, .. } => Some(child(
                parent,
                instance,
                w!("BUTTON"),
                browse,
                WS_TABSTOP,
                FIRST_BROWSE + index,
            )?),
            _ => None,
        };
        fields.push(Bound {
            label,
            control,
            browse,
        });
    }
    let submit = child(
        parent,
        instance,
        w!("BUTTON"),
        &form.submit,
        WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
        1,
    )?;
    let cancel = child(parent, instance, w!("BUTTON"), &form.cancel, WS_TABSTOP, 2)?;
    Ok((fields, submit, cancel))
}

/// Scrolling moves the fields; it never hides them. A hidden window cannot
/// keep the keyboard focus, and the dialog manager skips hidden controls when
/// Tab looks for the next field.
unsafe fn layout(window: HWND, state: &mut State) {
    let mut area = RECT::default();
    let _ = GetClientRect(window, &raw mut area);
    let content_height = i32::try_from(state.fields.len())
        .unwrap_or(0)
        .saturating_mul(ROW);
    let visible = (area.bottom - 70).max(1);
    state.offset = state.offset.clamp(0, (content_height - visible).max(0));
    let info = SCROLLINFO {
        cbSize: u32::try_from(size_of::<SCROLLINFO>()).expect("scroll info size fits"),
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
        nMin: 0,
        nMax: content_height,
        nPage: u32::try_from(visible).unwrap_or(1),
        nPos: state.offset,
        ..Default::default()
    };
    SetScrollInfo(window, SB_VERT, &raw const info, true);
    let column = (area.right / 3).clamp(160, 280);
    for (index, field) in state.fields.iter().enumerate() {
        let y = 12 + i32::try_from(index).unwrap_or(0) * ROW - state.offset;
        if let Some(label) = field.label {
            let _ = MoveWindow(label, 12, y + 4, column - 20, 26, true);
            let h = if matches!(state.form.fields[index].kind, FormFieldKind::Choice { .. }) {
                300
            } else {
                28
            };
            let width = (area.right - column - 16).max(100);
            if let Some(browse) = field.browse {
                let _ = MoveWindow(field.control, column, y, (width - 110).max(80), h, true);
                let _ = MoveWindow(browse, column + (width - 100).max(90), y, 100, 28, true);
            } else {
                let _ = MoveWindow(field.control, column, y, width, h, true);
            }
        } else {
            let _ = MoveWindow(field.control, 12, y, (area.right - 28).max(100), 28, true);
        }
    }
    let _ = MoveWindow(state.submit, 12, area.bottom - 44, 130, 30, true);
    let _ = MoveWindow(state.cancel, 154, area.bottom - 44, 130, 30, true);
}

/// Fills a file field from the file dialog; focus returns to the field, so
/// the screen reader reads the chosen path.
unsafe fn browse_for_file(window: HWND, state: &mut State, index: usize) {
    let (Some(field), Some(bound)) = (state.form.fields.get(index), state.fields.get(index)) else {
        return;
    };
    let FormFieldKind::File { extensions, .. } = &field.kind else {
        return;
    };
    let patterns = extensions
        .iter()
        .map(|extension| format!("*.{}", extension.trim_start_matches('.')))
        .collect::<Vec<_>>()
        .join(";");
    let filters = [(field.label.clone(), patterns)];
    if let Ok(Some(path)) =
        crate::file_dialog_win32::choose_file_with_filters(window, &field.label, &filters)
    {
        let text = wide(&path.to_string_lossy());
        let _ = SetWindowTextW(bound.control, PCWSTR(text.as_ptr()));
    }
    let _ = SetFocus(Some(bound.control));
}

/// Scrolls just enough to show the field at `index`.
unsafe fn reveal(window: HWND, state: &mut State, index: usize) {
    let mut area = RECT::default();
    let _ = GetClientRect(window, &raw mut area);
    let top = i32::try_from(index).unwrap_or(0) * ROW;
    let visible = (area.bottom - 70).max(ROW);
    if top < state.offset {
        state.offset = top;
    } else if top + ROW > state.offset + visible {
        state.offset = top + ROW - visible;
    } else {
        return;
    }
    layout(window, state);
}

unsafe fn read_values(state: &State) -> FormValues {
    state
        .form
        .fields
        .iter()
        .zip(&state.fields)
        .filter_map(|(field, bound)| {
            let value = match field.kind {
                FormFieldKind::ReadOnly { .. } => return None,
                FormFieldKind::Checkbox { .. } => {
                    FormValue::Checkbox(SendMessageW(bound.control, 0x00F0, None, None).0 == 1)
                }
                FormFieldKind::Choice { .. } => FormValue::Choice(
                    usize::try_from(SendMessageW(bound.control, 0x0147, None, None).0)
                        .unwrap_or(usize::MAX),
                ),
                _ => FormValue::Text(window_text(bound.control)),
            };
            Some((field.id.clone(), value))
        })
        .collect()
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(window, WINDOW_LONG_PTR_INDEX(0)) as *mut State;
    if let Some(state) = ptr.as_mut() {
        match message {
            WM_CLOSE => {
                let _ = DestroyWindow(window);
                return LRESULT(0);
            }
            WM_COMMAND if w.0 & 0xffff == 2 => {
                let _ = DestroyWindow(window);
                return LRESULT(0);
            }
            WM_COMMAND if w.0 & 0xffff == 1 => {
                match state.form.validate(&read_values(state)) {
                    Ok(values) => {
                        state.result = Some(values);
                        let _ = DestroyWindow(window);
                    }
                    Err(label) => {
                        let message = wide(&state.form.error_message(&label));
                        let title = wide(&state.form.title);
                        MessageBoxW(
                            Some(window),
                            PCWSTR(message.as_ptr()),
                            PCWSTR(title.as_ptr()),
                            MB_OK | MB_ICONWARNING,
                        );
                        if let Some(field) = state
                            .form
                            .field_index(&label)
                            .and_then(|index| state.fields.get(index))
                        {
                            let _ = SetFocus(Some(field.control));
                        }
                    }
                }
                return LRESULT(0);
            }
            WM_SIZE => {
                layout(window, state);
                return LRESULT(0);
            }
            WM_COMMAND
                if (FIRST_BROWSE..FIRST_BROWSE + state.fields.len()).contains(&(w.0 & 0xffff)) =>
            {
                browse_for_file(window, state, (w.0 & 0xffff) - FIRST_BROWSE);
                return LRESULT(0);
            }
            WM_COMMAND if w.0 & 0xffff >= FIRST_FIELD => {
                let notification = (w.0 >> 16) & 0xffff;
                if matches!(notification, 0 | 1 | 0x0300)
                    && let (Some(preview), Ok(values)) =
                        (state.preview, state.form.validate(&read_values(state)))
                {
                    (preview.invoke)(preview.data, &values);
                }
            }
            WM_VSCROLL => {
                let code = w.0 & 0xffff;
                state.offset += match code {
                    0 => -ROW,
                    1 => ROW,
                    2 => -200,
                    3 => 200,
                    _ => 0,
                };
                if code == 5 || code == 4 {
                    state.offset = i32::try_from(w.0 >> 16).unwrap_or(0);
                }
                layout(window, state);
                return LRESULT(0);
            }
            _ => {}
        }
    }
    DefWindowProcW(window, message, w, l)
}

unsafe extern "system" fn field_proc(
    window: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    index: usize,
) -> LRESULT {
    if message == WM_SETFOCUS
        && let Ok(parent) = GetParent(window)
    {
        let ptr = GetWindowLongPtrW(parent, WINDOW_LONG_PTR_INDEX(0)) as *mut State;
        if let Some(state) = ptr.as_mut() {
            reveal(parent, state, index);
        }
    }
    if message == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(window, Some(field_proc), id);
    }
    DefSubclassProc(window, message, w, l)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
unsafe fn window_text(window: HWND) -> String {
    let length = usize::try_from(GetWindowTextLengthW(window)).unwrap_or(0);
    let mut buffer = vec![0; length + 1];
    let written = usize::try_from(GetWindowTextW(window, &mut buffer)).unwrap_or(0);
    String::from_utf16_lossy(&buffer[..written])
}

#[cfg(test)]
mod tests {
    use super::*;
    use apricot_shell::FormField;
    use std::{thread, time::Duration};
    use windows::Win32::{
        Graphics::Gdi::ClientToScreen,
        UI::WindowsAndMessaging::{
            FindWindowW, GetDlgItem, GetNextDlgTabItem, GetWindowRect, IsWindowVisible,
            PostMessageW, SetWindowTextW, WM_NEXTDLGCTL,
        },
    };

    fn field_id(offset: usize) -> i32 {
        i32::try_from(FIRST_FIELD + offset).unwrap()
    }

    /// Runs `action` against the form, then always closes it, so a failed
    /// assertion can never leave a modal window waiting on the desktop.
    fn drive(title: &str, action: impl FnOnce(HWND) + Send + 'static) -> thread::JoinHandle<()> {
        let title = title.to_owned();
        thread::spawn(move || unsafe {
            for _ in 0..250 {
                let title = wide(&title);
                if let Ok(window) = FindWindowW(CLASS, PCWSTR(title.as_ptr()))
                    && IsWindowVisible(window).as_bool()
                {
                    let outcome =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| action(window)));
                    if IsWindow(Some(window)).as_bool() {
                        let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    if let Err(panic) = outcome {
                        std::panic::resume_unwind(panic);
                    }
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
            panic!("native feature form did not appear");
        })
    }

    #[test]
    #[ignore = "opens an isolated native dialog"]
    fn native_feature_form_commits_typed_controls() {
        let form = FeatureForm::new(
            "Apricot isolated typed form",
            vec![
                FormField::number("length", "Duration", 30.0, 1.0, 120.0),
                FormField::choice("mode", "Mode", vec!["First".into(), "Second".into()], 0),
                FormField::checkbox("remember", "Remember", false),
            ],
        );
        let driver = drive(&form.title, |window| unsafe {
            let edit = GetDlgItem(Some(window), field_id(0)).unwrap();
            let value = wide("2.5");
            SetWindowTextW(edit, PCWSTR(value.as_ptr())).unwrap();
            SendMessageW(
                GetDlgItem(Some(window), field_id(1)).unwrap(),
                0x014E,
                Some(WPARAM(1)),
                None,
            );
            SendMessageW(
                GetDlgItem(Some(window), field_id(2)).unwrap(),
                0x00F1,
                Some(WPARAM(1)),
                None,
            );
            SendMessageW(window, WM_COMMAND, Some(WPARAM(1)), None);
        });
        let result = show(HWND::default(), &form).expect("saved form");
        driver.join().expect("native driver");
        assert_eq!(result["length"].number(), Some(2.5));
        assert_eq!(result["mode"].choice(), Some(1));
        assert_eq!(result["remember"].checked(), Some(true));
    }

    #[test]
    #[ignore = "opens an isolated native dialog"]
    fn native_feature_form_preview_is_live_and_cancel_has_no_result() {
        let form = FeatureForm::new(
            "Apricot isolated preview form",
            vec![FormField::number("gain", "Gain", 0.0, -24.0, 24.0)],
        );
        let driver = drive(&form.title, |window| unsafe {
            let value = wide("6");
            SetWindowTextW(
                GetDlgItem(Some(window), field_id(0)).unwrap(),
                PCWSTR(value.as_ptr()),
            )
            .unwrap();
            SendMessageW(window, WM_COMMAND, Some(WPARAM(2)), None);
        });
        let mut heard = Vec::new();
        let result = show_preview(HWND::default(), &form, &mut |values| {
            heard.push(values["gain"].number().unwrap());
        });
        driver.join().expect("native driver");
        assert!(result.is_none());
        assert!(heard.contains(&6.0));
    }

    #[test]
    #[ignore = "opens an isolated native dialog"]
    fn native_feature_form_tab_reaches_rows_beyond_the_viewport() {
        let form = FeatureForm::new(
            "Apricot isolated scroll form",
            (0..24)
                .map(|index| {
                    FormField::number(
                        &format!("band{index}"),
                        format!("Band {index}"),
                        0.0,
                        -24.0,
                        24.0,
                    )
                })
                .collect(),
        );
        let driver = drive(&form.title, |window| unsafe {
            let last = GetDlgItem(Some(window), field_id(23)).unwrap();
            let mut field = GetDlgItem(Some(window), field_id(0)).unwrap();
            for _ in 0..23 {
                field = GetNextDlgTabItem(window, Some(field), false).unwrap();
            }
            assert_eq!(field, last, "Tab reaches the last field");
            SendMessageW(
                window,
                WM_NEXTDLGCTL,
                Some(WPARAM(last.0 as usize)),
                Some(LPARAM(1)),
            );
            let mut area = RECT::default();
            let mut client = RECT::default();
            let _ = GetWindowRect(last, &raw mut area);
            let _ = GetClientRect(window, &raw mut client);
            let mut corner = windows::Win32::Foundation::POINT {
                x: 0,
                y: client.bottom,
            };
            let _ = ClientToScreen(window, &raw mut corner);
            assert!(
                area.bottom <= corner.y,
                "focused field is scrolled into view"
            );
            SendMessageW(window, WM_COMMAND, Some(WPARAM(2)), None);
        });
        assert!(show(HWND::default(), &form).is_none());
        driver.join().expect("native driver");
    }
}
