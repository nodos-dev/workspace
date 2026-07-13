//! Native single-choice list dialog. No GUI library is linked into nosman:
//! each platform's GUI facility is loaded at runtime (user32 dialog templates
//! on Windows, dlopen'd GTK 3 on Linux, dlopen'd AppKit on macOS), so nosman
//! keeps working on headless machines where the dialog is simply unavailable.

/// Show a modal single-choice list dialog. Returns the index of the chosen
/// item, or None if the user cancelled or no GUI is available on this machine.
pub fn select_from_list(title: &str, prompt: &str, items: &[String]) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    imp::select_from_list(title, prompt, items)
}

#[cfg(windows)]
mod imp {
    use std::ptr::null_mut;
    use winapi::shared::minwindef::{LPARAM, UINT, WPARAM};
    use winapi::shared::windef::HWND;
    use winapi::um::libloaderapi::GetModuleHandleW;
    use winapi::um::winuser::{
        DialogBoxIndirectParamW, EndDialog, GetDlgItem, SendMessageW, BS_DEFPUSHBUTTON,
        DLGTEMPLATE, DS_CENTER, DS_MODALFRAME, DS_SETFONT, IDCANCEL, IDOK, LBN_DBLCLK,
        LBS_NOINTEGRALHEIGHT, LBS_NOTIFY, LB_ADDSTRING, LB_GETCURSEL, LB_SETCURSEL, WM_COMMAND,
        WM_INITDIALOG, WS_BORDER, WS_CAPTION, WS_CHILD, WS_POPUP, WS_SYSMENU, WS_TABSTOP,
        WS_VISIBLE, WS_VSCROLL,
    };

    const ID_LIST: i32 = 1000;
    // Predefined dialog item class ordinals.
    const CLASS_BUTTON: u16 = 0x0080;
    const CLASS_STATIC: u16 = 0x0082;
    const CLASS_LISTBOX: u16 = 0x0083;

    fn push_u32(buf: &mut Vec<u16>, value: u32) {
        buf.push((value & 0xFFFF) as u16);
        buf.push((value >> 16) as u16);
    }

    fn push_sz(buf: &mut Vec<u16>, text: &str) {
        buf.extend(text.encode_utf16());
        buf.push(0);
    }

    fn push_item(buf: &mut Vec<u16>, style: u32, x: i16, y: i16, cx: i16, cy: i16, id: u16, class: u16, text: &str) {
        if buf.len() % 2 != 0 {
            buf.push(0); // items must be DWORD-aligned within the template
        }
        push_u32(buf, style);
        push_u32(buf, 0); // extended style
        buf.extend([x as u16, y as u16, cx as u16, cy as u16, id]);
        buf.extend([0xFFFF, class]);
        push_sz(buf, text);
        buf.push(0); // no creation data
    }

    unsafe extern "system" fn dlg_proc(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> isize {
        match msg {
            WM_INITDIALOG => {
                let labels = &*(lparam as *const Vec<Vec<u16>>);
                let list = GetDlgItem(hwnd, ID_LIST);
                for label in labels {
                    SendMessageW(list, LB_ADDSTRING, 0, label.as_ptr() as LPARAM);
                }
                SendMessageW(list, LB_SETCURSEL, 0, 0);
                1
            }
            WM_COMMAND => {
                let id = (wparam & 0xFFFF) as i32;
                let code = (wparam >> 16) as i32;
                if id == IDOK || (id == ID_LIST && code == LBN_DBLCLK as i32) {
                    let selected = SendMessageW(GetDlgItem(hwnd, ID_LIST), LB_GETCURSEL, 0, 0);
                    // EndDialog's result must distinguish "picked index 0" from
                    // "cancelled", so selections are returned 1-based.
                    EndDialog(hwnd, if selected < 0 { 0 } else { selected + 1 });
                    1
                } else if id == IDCANCEL {
                    EndDialog(hwnd, 0);
                    1
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    pub fn select_from_list(title: &str, prompt: &str, items: &[String]) -> Option<usize> {
        // Sizes are in dialog units.
        let list_height = (items.len() as i16 * 10 + 4).clamp(30, 120);
        let buttons_y = 20 + list_height + 7;
        let mut template: Vec<u16> = Vec::new();
        push_u32(&mut template, DS_SETFONT as u32 | DS_MODALFRAME as u32 | DS_CENTER as u32 | WS_POPUP | WS_CAPTION | WS_SYSMENU);
        push_u32(&mut template, 0); // extended style
        template.push(4); // item count
        template.extend([0u16, 0, 240, (buttons_y + 14 + 7) as u16]); // x, y, cx, cy
        template.extend([0u16, 0]); // no menu, default dialog window class
        push_sz(&mut template, title);
        template.push(9); // font point size
        push_sz(&mut template, "Segoe UI");
        push_item(&mut template, WS_CHILD | WS_VISIBLE, 7, 7, 226, 9, 0xFFFF, CLASS_STATIC, prompt);
        push_item(&mut template, WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | WS_VSCROLL | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT,
                  7, 20, 226, list_height, ID_LIST as u16, CLASS_LISTBOX, "");
        push_item(&mut template, WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_DEFPUSHBUTTON, 132, buttons_y, 50, 14, IDOK as u16, CLASS_BUTTON, "Launch");
        push_item(&mut template, WS_CHILD | WS_VISIBLE | WS_TABSTOP, 186, buttons_y, 50, 14, IDCANCEL as u16, CLASS_BUTTON, "Cancel");
        // The template must be DWORD-aligned in memory; Vec<u16> only guarantees
        // 2-byte alignment, so move it into a u32 buffer.
        let mut aligned = vec![0u32; (template.len() + 1) / 2];
        let labels: Vec<Vec<u16>> = items.iter()
            .map(|s| s.encode_utf16().chain(std::iter::once(0)).collect())
            .collect();
        let result = unsafe {
            std::ptr::copy_nonoverlapping(template.as_ptr(), aligned.as_mut_ptr() as *mut u16, template.len());
            DialogBoxIndirectParamW(
                GetModuleHandleW(null_mut()),
                aligned.as_ptr() as *const DLGTEMPLATE,
                null_mut(),
                Some(dlg_proc),
                &labels as *const _ as LPARAM,
            )
        };
        if result > 0 {
            Some(result as usize - 1)
        } else {
            None
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_uint, c_void};
    use std::ptr::{null, null_mut};
    use libloading::{Library, Symbol};

    const GTK_DIALOG_MODAL: c_int = 1;
    const GTK_RESPONSE_OK: c_int = -5;
    const GTK_RESPONSE_CANCEL: c_int = -6;
    const GTK_ALIGN_START: c_int = 1;

    pub fn select_from_list(title: &str, prompt: &str, items: &[String]) -> Option<usize> {
        let title = CString::new(title).ok()?;
        let prompt = CString::new(prompt).ok()?;
        let launch = CString::new("Launch").ok()?;
        let cancel = CString::new("Cancel").ok()?;
        let item_texts: Vec<CString> = items.iter()
            .filter_map(|s| CString::new(s.as_str()).ok())
            .collect();
        if item_texts.len() != items.len() {
            return None;
        }
        unsafe {
            // GTK is a runtime dependency: if it is not installed (headless
            // machine) the dlopen fails, and if there is no display to connect
            // to gtk_init_check reports failure. Both fall through to None.
            let gtk = Library::new("libgtk-3.so.0").or_else(|_| Library::new("libgtk-3.so")).ok()?;
            let gtk_init_check: Symbol<unsafe extern "C" fn(*mut c_int, *mut c_void) -> c_int> =
                gtk.get(b"gtk_init_check\0").ok()?;
            let gtk_dialog_new_with_buttons: Symbol<unsafe extern "C" fn(*const c_char, *mut c_void, c_int, *const c_char, ...) -> *mut c_void> =
                gtk.get(b"gtk_dialog_new_with_buttons\0").ok()?;
            let gtk_dialog_set_default_response: Symbol<unsafe extern "C" fn(*mut c_void, c_int)> =
                gtk.get(b"gtk_dialog_set_default_response\0").ok()?;
            let gtk_dialog_get_content_area: Symbol<unsafe extern "C" fn(*mut c_void) -> *mut c_void> =
                gtk.get(b"gtk_dialog_get_content_area\0").ok()?;
            let gtk_container_set_border_width: Symbol<unsafe extern "C" fn(*mut c_void, c_uint)> =
                gtk.get(b"gtk_container_set_border_width\0").ok()?;
            let gtk_container_add: Symbol<unsafe extern "C" fn(*mut c_void, *mut c_void)> =
                gtk.get(b"gtk_container_add\0").ok()?;
            let gtk_label_new: Symbol<unsafe extern "C" fn(*const c_char) -> *mut c_void> =
                gtk.get(b"gtk_label_new\0").ok()?;
            let gtk_widget_set_halign: Symbol<unsafe extern "C" fn(*mut c_void, c_int)> =
                gtk.get(b"gtk_widget_set_halign\0").ok()?;
            let gtk_list_box_new: Symbol<unsafe extern "C" fn() -> *mut c_void> =
                gtk.get(b"gtk_list_box_new\0").ok()?;
            let gtk_list_box_insert: Symbol<unsafe extern "C" fn(*mut c_void, *mut c_void, c_int)> =
                gtk.get(b"gtk_list_box_insert\0").ok()?;
            let gtk_list_box_get_row_at_index: Symbol<unsafe extern "C" fn(*mut c_void, c_int) -> *mut c_void> =
                gtk.get(b"gtk_list_box_get_row_at_index\0").ok()?;
            let gtk_list_box_select_row: Symbol<unsafe extern "C" fn(*mut c_void, *mut c_void)> =
                gtk.get(b"gtk_list_box_select_row\0").ok()?;
            let gtk_list_box_get_selected_row: Symbol<unsafe extern "C" fn(*mut c_void) -> *mut c_void> =
                gtk.get(b"gtk_list_box_get_selected_row\0").ok()?;
            let gtk_list_box_row_get_index: Symbol<unsafe extern "C" fn(*mut c_void) -> c_int> =
                gtk.get(b"gtk_list_box_row_get_index\0").ok()?;
            let gtk_widget_show_all: Symbol<unsafe extern "C" fn(*mut c_void)> =
                gtk.get(b"gtk_widget_show_all\0").ok()?;
            let gtk_dialog_run: Symbol<unsafe extern "C" fn(*mut c_void) -> c_int> =
                gtk.get(b"gtk_dialog_run\0").ok()?;
            let gtk_widget_destroy: Symbol<unsafe extern "C" fn(*mut c_void)> =
                gtk.get(b"gtk_widget_destroy\0").ok()?;
            let gtk_events_pending: Symbol<unsafe extern "C" fn() -> c_int> =
                gtk.get(b"gtk_events_pending\0").ok()?;
            let gtk_main_iteration: Symbol<unsafe extern "C" fn() -> c_int> =
                gtk.get(b"gtk_main_iteration\0").ok()?;

            if gtk_init_check(null_mut(), null_mut()) == 0 {
                return None;
            }
            let dialog = gtk_dialog_new_with_buttons(
                title.as_ptr(), null_mut(), GTK_DIALOG_MODAL,
                cancel.as_ptr(), GTK_RESPONSE_CANCEL,
                launch.as_ptr(), GTK_RESPONSE_OK,
                null::<c_char>(),
            );
            gtk_dialog_set_default_response(dialog, GTK_RESPONSE_OK);
            let content = gtk_dialog_get_content_area(dialog);
            gtk_container_set_border_width(content, 12);
            let prompt_label = gtk_label_new(prompt.as_ptr());
            gtk_widget_set_halign(prompt_label, GTK_ALIGN_START);
            gtk_container_add(content, prompt_label);
            let list = gtk_list_box_new();
            for text in &item_texts {
                let label = gtk_label_new(text.as_ptr());
                gtk_widget_set_halign(label, GTK_ALIGN_START);
                gtk_list_box_insert(list, label, -1);
            }
            gtk_list_box_select_row(list, gtk_list_box_get_row_at_index(list, 0));
            gtk_container_add(content, list);
            gtk_widget_show_all(dialog);
            let response = gtk_dialog_run(dialog);
            let selected = if response == GTK_RESPONSE_OK {
                let row = gtk_list_box_get_selected_row(list);
                if row.is_null() {
                    None
                } else {
                    Some(gtk_list_box_row_get_index(row) as usize)
                }
            } else {
                None
            };
            gtk_widget_destroy(dialog);
            // Let GTK process the destroy so the window disappears immediately.
            while gtk_events_pending() != 0 {
                gtk_main_iteration();
            }
            selected
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_void};
    use libloading::{Library, Symbol};

    #[repr(C)]
    struct CGRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }

    const NS_ALERT_FIRST_BUTTON: isize = 1000;
    const NS_ACTIVATION_POLICY_ACCESSORY: isize = 1;

    pub fn select_from_list(title: &str, prompt: &str, items: &[String]) -> Option<usize> {
        unsafe {
            // The window server is a runtime dependency: in an SSH session there
            // is no GUI session and CGSessionCopyCurrentDictionary returns null.
            let core_graphics = Library::new("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics").ok()?;
            let copy_session: Symbol<unsafe extern "C" fn() -> *mut c_void> =
                core_graphics.get(b"CGSessionCopyCurrentDictionary\0").ok()?;
            let core_foundation = Library::new("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation").ok()?;
            let cf_release: Symbol<unsafe extern "C" fn(*mut c_void)> =
                core_foundation.get(b"CFRelease\0").ok()?;
            let session = copy_session();
            if session.is_null() {
                return None;
            }
            cf_release(session);

            // Loading AppKit registers the NS* classes with the Objective-C
            // runtime; everything below goes through objc_msgSend.
            let _appkit = Library::new("/System/Library/Frameworks/AppKit.framework/AppKit").ok()?;
            let objc = Library::new("/usr/lib/libobjc.A.dylib").ok()?;
            let objc_get_class: Symbol<unsafe extern "C" fn(*const c_char) -> *mut c_void> =
                objc.get(b"objc_getClass\0").ok()?;
            let sel_register_name: Symbol<unsafe extern "C" fn(*const c_char) -> *mut c_void> =
                objc.get(b"sel_registerName\0").ok()?;
            let msg_send = *objc.get::<unsafe extern "C" fn()>(b"objc_msgSend\0").ok()?;

            // objc_msgSend must be cast to the signature of each message it sends.
            let msg0: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(msg_send);
            let msg_id: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void =
                std::mem::transmute(msg_send);
            let msg_utf8: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char) -> *mut c_void =
                std::mem::transmute(msg_send);
            let msg_ret_isize: unsafe extern "C" fn(*mut c_void, *mut c_void) -> isize =
                std::mem::transmute(msg_send);
            let msg_isize: unsafe extern "C" fn(*mut c_void, *mut c_void, isize) -> i8 =
                std::mem::transmute(msg_send);
            let msg_bool: unsafe extern "C" fn(*mut c_void, *mut c_void, i8) -> *mut c_void =
                std::mem::transmute(msg_send);
            let msg_rect_bool: unsafe extern "C" fn(*mut c_void, *mut c_void, CGRect, i8) -> *mut c_void =
                std::mem::transmute(msg_send);

            let class = |name: &str| {
                let name = CString::new(name).unwrap();
                objc_get_class(name.as_ptr())
            };
            let sel = |name: &str| {
                let name = CString::new(name).unwrap();
                sel_register_name(name.as_ptr())
            };
            let ns_string = |text: &str| {
                let text = CString::new(text).unwrap_or_default();
                msg_utf8(class("NSString"), sel("stringWithUTF8String:"), text.as_ptr())
            };

            let pool = msg0(msg0(class("NSAutoreleasePool"), sel("alloc")), sel("init"));
            let app = msg0(class("NSApplication"), sel("sharedApplication"));
            msg_isize(app, sel("setActivationPolicy:"), NS_ACTIVATION_POLICY_ACCESSORY);
            msg_bool(app, sel("activateIgnoringOtherApps:"), 1);

            let alert = msg0(msg0(class("NSAlert"), sel("alloc")), sel("init"));
            msg_id(alert, sel("setMessageText:"), ns_string(prompt));
            msg_id(alert, sel("addButtonWithTitle:"), ns_string("Launch"));
            msg_id(alert, sel("addButtonWithTitle:"), ns_string("Cancel"));
            let popup = msg_rect_bool(
                msg0(class("NSPopUpButton"), sel("alloc")),
                sel("initWithFrame:pullsDown:"),
                CGRect { x: 0.0, y: 0.0, width: 280.0, height: 26.0 },
                0,
            );
            for item in items {
                msg_id(popup, sel("addItemWithTitle:"), ns_string(item));
            }
            msg_id(alert, sel("setAccessoryView:"), popup);
            msg_id(msg0(alert, sel("window")), sel("setTitle:"), ns_string(title));
            let response = msg_ret_isize(alert, sel("runModal"));
            let selected = if response == NS_ALERT_FIRST_BUTTON {
                Some(msg_ret_isize(popup, sel("indexOfSelectedItem")) as usize)
            } else {
                None
            };
            msg0(pool, sel("drain"));
            selected
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod imp {
    pub fn select_from_list(_title: &str, _prompt: &str, _items: &[String]) -> Option<usize> {
        None
    }
}
