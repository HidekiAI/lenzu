extern crate winapi;

use image::DynamicImage;
use rusty_tesseract::{tesseract, Args};
use std::ffi::{CStr, CString};
use std::ptr;
use winapi::{
    shared::windef::RECT,
    um::{
        wingdi::{
            AlphaBlend, BitBlt, CreateCompatibleBitmap,
            CreateCompatibleDC as CreateCompatibleDCConst, DeleteDC, DeleteObject,
            SelectObject as SelectObjectConst, StretchBlt, TextOutW, SRCCOPY,
        },
        winnt::{LPSTR, LPWSTR},
        winuser::{
            BeginPaint as BeginPaintConst, CreateWindowExW, DefWindowProcW, DispatchMessageW,
            EndPaint, GetCursorPos, GetDC, GetMessageW, GetMonitorInfoW, GetWindowInfo,
            InvalidateRect, MonitorFromPoint, PostQuitMessage, RegisterClassW, ReleaseDC,
            ShowWindow, TranslateMessage, CW_USEDEFAULT, MONITORINFO, MONITOR_DEFAULTTONEAREST,
            MSG, PAINTSTRUCT as PAINTSTRUCTConst, VK_ESCAPE, VK_SPACE, WM_KEYDOWN,
            WS_OVERLAPPEDWINDOW,
        },
    },
};

const MAGNIFY_SCALE_FACTOR: u32 = 2;
const TOGGLE_WINDOW_MOVE_KEY: std::ffi::c_int = VK_SPACE;
enum ToggleState {
    Free,
    MoveWindow,
    Capture,
    Captured, // past-tense
}

static mut TOGGLE_STATE: ToggleState = ToggleState::Free;

fn main() {
    let class_name = "Lenzu";
    let window_name = "Tesseract";

    let h_instance = ptr::null_mut();
    let class_name_cstr = CString::new(class_name).expect("CString creation failed");
    let window_name_cstr = CString::new(window_name).expect("CString creation failed");

    let wc = winapi::um::winuser::WNDCLASSW {
        style: 0,
        lpfnWndProc: Some(DefWindowProcW),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: h_instance,
        hIcon: ptr::null_mut(),
        hCursor: ptr::null_mut(),
        hbrBackground: ptr::null_mut(),
        lpszMenuName: ptr::null_mut(),
        lpszClassName: class_name_cstr.as_ptr() as *const u16,
    };

    if unsafe { RegisterClassW(&wc) } == 0 {
        return;
    }

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name_cstr.as_ptr() as *const u16,
            window_name_cstr.as_ptr() as *const u16,
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            ptr::null_mut(),
            ptr::null_mut(),
            h_instance,
            ptr::null_mut(),
        )
    };

    if hwnd.is_null() {
        return;
    }

    unsafe {
        ShowWindow(hwnd, winapi::um::winuser::SW_SHOWDEFAULT);
    }

    let mut msg = MSG {
        hwnd: ptr::null_mut(),
        message: 0,
        wParam: 0,
        lParam: 0,
        time: 0,
        pt: winapi::shared::windef::POINT { x: 0, y: 0 },
    };

    loop {
        if unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } == 0 {
            break;
        }

        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if msg.message == WM_KEYDOWN {
            match msg.wParam as std::ffi::c_int {
                VK_ESCAPE => {
                    // Check for the ESCAPE key press and exit the application
                    unsafe { PostQuitMessage(0) };
                }
                TOGGLE_WINDOW_MOVE_KEY => {
                    // unsure why I need to use unsafe here, but compiler complains if I don't
                    unsafe {
                        TOGGLE_STATE = match TOGGLE_STATE {
                            ToggleState::Free => ToggleState::MoveWindow,
                            ToggleState::MoveWindow => ToggleState::Capture, // note that interally, Capture will trnasform to Captured
                            ToggleState::Captured => ToggleState::Free,
                            ToggleState::Capture => {
                                // should never be in this state
                                assert!(false, "unexpected toggle_state");
                                ToggleState::Captured // just return to NEXT expteded state in RELEASE mode...
                            }
                        }
                    }
                }
                _ => (),
            }
        }

        let mut cursor_pos = winapi::shared::windef::POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut cursor_pos) } == 0 {
            // Handle the error appropriately if necessary.
        }
        unsafe {
            match TOGGLE_STATE {
                ToggleState::Free => capture_and_magnify(hwnd, cursor_pos),
                ToggleState::MoveWindow => {
                    // move the window to the cursor position (a sticky window)
                    winapi::um::winuser::SetWindowPos(
                        hwnd,
                        ptr::null_mut(),
                        cursor_pos.x,
                        cursor_pos.y,
                        0,
                        0,
                        winapi::um::winuser::SWP_NOSIZE | winapi::um::winuser::SWP_NOZORDER,
                    );
                    //// invalidate the window so it can redraw the window onto the Desktop/monitor
                    //InvalidateRect(hwnd, ptr::null_mut(), 0);
                    capture_and_magnify(hwnd, cursor_pos); // show contents UNDERNEATH the window (will InvalidateRect() so that it'll also redraw the actual window onto the )
                }
                ToggleState::Capture => {
                    // capture the screen and magnify it
                    //capture_and_ocr(hwnd, cursor_pos);
                    capture_and_ocr(hwnd, cursor_pos);
                    // once it's blitted to that window, stay still..
                    TOGGLE_STATE = ToggleState::Captured;
                }
                ToggleState::Captured => {
                    // don't render/update/Invalidate the window, just stay still/frozen until the user toggles the window again
                    ()
                }
            }
        }
    } // loop
}

fn capture_and_ocr(
    hwnd: *mut winapi::shared::windef::HWND__,
    cursor_pos: winapi::shared::windef::POINT,
) {
    let h_monitor = unsafe { MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rcWork: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dwFlags: 0,
    };

    unsafe {
        GetMonitorInfoW(h_monitor, &mut monitor_info);
    }

    // get current dimension of the windown on the monitor via GetWindowInfo
    let mut _window_rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut window_info = winapi::um::winuser::WINDOWINFO {
        cbSize: std::mem::size_of::<winapi::um::winuser::WINDOWINFO>() as u32,
        rcWindow: RECT {
            // note: we'll  copy this to window_rect after the call to GetWindoInfo() below
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rcClient: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dwStyle: 0,
        dwExStyle: 0,
        dwWindowStatus: 0,
        cxWindowBorders: 0,
        cyWindowBorders: 0,
        atomWindowType: 0,
        wCreatorVersion: 0,
    };
    unsafe {
        GetWindowInfo(hwnd, &mut window_info);
        _window_rect = window_info.rcWindow;
    }
    let win_width = std::cmp::max(2, _window_rect.right - _window_rect.left);
    let win_height = std::cmp::max(2, _window_rect.bottom - _window_rect.top);
    // capture rectangle is based off of consierating to edge of the monitor relative to the cursor position
    let capture_rect = RECT {
        left: std::cmp::max(monitor_info.rcMonitor.left, cursor_pos.x - (win_width / 2)), // make sure left is not less than monitor left
        top: std::cmp::max(monitor_info.rcMonitor.top, cursor_pos.y - (win_height / 2)), // make sure top is not less than monitor top
        right: std::cmp::min(monitor_info.rcMonitor.right, cursor_pos.x + (win_width / 2)), // make sure right is not greater than monitor right
        bottom: std::cmp::min(
            monitor_info.rcMonitor.bottom,
            cursor_pos.y + (win_height / 2),
        ), // make sure bottom is not greater than monitor bottom
    };
    let width = ((capture_rect.right - capture_rect.left) + 1) as u32;
    let height = ((capture_rect.bottom - capture_rect.top) + 1) as u32;

    let source_dc = unsafe { GetDC(ptr::null_mut()) };
    let destination_dc = unsafe { CreateCompatibleDCConst(source_dc) };

    // Create a compatible bitmap for the primary image
    let destination_bitmap =
        unsafe { CreateCompatibleBitmap(destination_dc, width as i32, height as i32) };

    let old_obj = unsafe {
        SelectObjectConst(
            destination_dc,
            destination_bitmap as *mut winapi::ctypes::c_void,
        )
    };
    unsafe {
        BitBlt(
            destination_dc,
            0, // destination x
            0, // destination y
            width as i32,
            height as i32,
            source_dc,
            capture_rect.left, // source x
            capture_rect.top,  // source y
            SRCCOPY,
        )
    };

    let mut ps = PAINTSTRUCTConst {
        hdc: ptr::null_mut(),
        fErase: 0,
        rcPaint: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        fRestore: 0,
        fIncUpdate: 0,
        rgbReserved: [0; 32],
    };

    // Create a compatible bitmap for the topmost layer (text overlay)
    let bmp_topmost =
        unsafe { CreateCompatibleBitmap(destination_dc, width as i32, height as i32) };
    let mem_dc_topmost = unsafe { CreateCompatibleDCConst(destination_dc) };
    let _old_obj2 =
        unsafe { SelectObjectConst(mem_dc_topmost, bmp_topmost as *mut winapi::ctypes::c_void) };

    // the image we just captured, we'll need to now pass it down to OCR and get the text back
    // We will (for now) assume it is either "jpn" or "jpn_vert" and we'll just pass it down
    // to kakasi and convert all kanji to hiragana
    // 1. convert to grayscale
    // 2. pass it down to OCR
    // 3. get the text back
    // 4. draw the text onto the mem_dc_topmost
    // 5. blend the topmost layer onto the primary image
    // 6. scale/magnify
    // 7. draw the magnified image onto the window
    // 8. repeat
    // 9. profit
    // convert DC to RGBA - probably can get away with 24-bit but for better byte alignment, will stay at 32-bit
    let rgba_image = DynamicImage::new_rgba8(width, height); // pre-allocate buffer
                                                             // Convert the image to grayscale
    let gray_scale_image = rgba_image.grayscale();
    let ocr_args: rusty_tesseract::Args = Args {
        lang: "jpn_vert+jpn+eng".into(),
        ..Default::default()
    };
    let ocr_image = rusty_tesseract::Image::from_dynamic_image(&gray_scale_image);
    let ocr_result = match ocr_image {
        Ok(img) => rusty_tesseract::image_to_string(&img, &ocr_args),
        Err(e) => {
            println!("Error: {:?}", e);
            return;
        }
    };

    // Draw text onto mem_dc_topmost (use TextOutW() or other text-drawing functions)
    // Create a sample CString (replace this with your actual text)
    let text = match ocr_result {
        Ok(s) => CString::new(s).expect("CString creation failed"),
        Err(e) => CString::new(format!("Error: {:?}", e)).expect("CString creation failed"),
    };

    // Convert CString to LPCTSTR (raw pointer to a null-terminated wide string)
    let lpstr: LPSTR = text.as_ptr() as LPSTR;

    // Create a std::wstring from the LPCTSTR
    let wstr = unsafe { CStr::from_ptr(lpstr).to_string_lossy().into_owned() };
    let lpwstr: LPWSTR = wstr.as_ptr() as LPWSTR;

    let pos_x = 0;
    let pos_y = 0;

    // Call TextOutW to display the text
    unsafe {
        TextOutW(
            mem_dc_topmost,
            pos_x,
            pos_y,
            lpwstr,
            text.as_bytes().len() as i32,
        );
    }

    // Blend the topmost layer onto the primary image
    let blend_func = winapi::um::wingdi::BLENDFUNCTION {
        BlendOp: winapi::um::wingdi::AC_SRC_OVER,
        BlendFlags: 0,
        SourceConstantAlpha: 128, // Adjust alpha value (0-255) for transparency
        AlphaFormat: winapi::um::wingdi::AC_SRC_ALPHA,
    };
    unsafe {
        AlphaBlend(
            destination_dc,
            0,
            0,
            width as i32,
            height as i32,
            mem_dc_topmost,
            0,
            0,
            width as i32,
            height as i32,
            blend_func,
        );
    }

    let final_destination_dc = unsafe { BeginPaintConst(hwnd, &mut ps) };

    // scale/mangify
    unsafe {
        StretchBlt(
            final_destination_dc,
            0,
            0,
            (width * MAGNIFY_SCALE_FACTOR) as i32,
            (height * MAGNIFY_SCALE_FACTOR) as i32,
            destination_dc,
            0,
            0,
            width as i32,
            height as i32,
            SRCCOPY,
        );

        EndPaint(hwnd, &ps);
        InvalidateRect(hwnd, ptr::null_mut(), 0);
        SelectObjectConst(destination_dc, old_obj);
        DeleteDC(destination_dc);
        ReleaseDC(ptr::null_mut(), source_dc);
        DeleteObject(destination_bitmap as *mut winapi::ctypes::c_void);
    }
}

fn capture_and_magnify(
    hwnd: *mut winapi::shared::windef::HWND__,
    cursor_pos: winapi::shared::windef::POINT,
) {
    let h_monitor = unsafe { MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rcWork: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dwFlags: 0,
    };

    unsafe {
        GetMonitorInfoW(h_monitor, &mut monitor_info);
    }

    // get current dimension of the windown on the monitor via GetWindowInfo
    let mut _window_rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut window_info = winapi::um::winuser::WINDOWINFO {
        cbSize: std::mem::size_of::<winapi::um::winuser::WINDOWINFO>() as u32,
        rcWindow: RECT {
            // note: we'll  copy this to window_rect after the call to GetWindoInfo() below
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        rcClient: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dwStyle: 0,
        dwExStyle: 0,
        dwWindowStatus: 0,
        cxWindowBorders: 0,
        cyWindowBorders: 0,
        atomWindowType: 0,
        wCreatorVersion: 0,
    };
    unsafe {
        GetWindowInfo(hwnd, &mut window_info);
        _window_rect = window_info.rcWindow;
    }
    let win_width = std::cmp::max(2, _window_rect.right - _window_rect.left);
    let win_height = std::cmp::max(2, _window_rect.bottom - _window_rect.top);
    // capture rectangle is based off of consierating to edge of the monitor relative to the cursor position
    let capture_rect = RECT {
        left: std::cmp::max(monitor_info.rcMonitor.left, cursor_pos.x - (win_width / 2)), // make sure left is not less than monitor left
        top: std::cmp::max(monitor_info.rcMonitor.top, cursor_pos.y - (win_height / 2)), // make sure top is not less than monitor top
        right: std::cmp::min(monitor_info.rcMonitor.right, cursor_pos.x + (win_width / 2)), // make sure right is not greater than monitor right
        bottom: std::cmp::min(
            monitor_info.rcMonitor.bottom,
            cursor_pos.y + (win_height / 2),
        ), // make sure bottom is not greater than monitor bottom
    };
    let width = ((capture_rect.right - capture_rect.left) + 1) as u32;
    let height = ((capture_rect.bottom - capture_rect.top) + 1) as u32;

    let source_dc = unsafe { GetDC(ptr::null_mut()) };
    let destination_dc = unsafe { CreateCompatibleDCConst(source_dc) };

    let destination_bitmap =
        unsafe { CreateCompatibleBitmap(source_dc, width as i32, height as i32) };

    let old_obj = unsafe {
        SelectObjectConst(
            destination_dc,
            destination_bitmap as *mut winapi::ctypes::c_void,
        )
    };
    unsafe {
        BitBlt(
            destination_dc, // destination device context
            0,              // destination x
            0,              // destination y
            width as i32,
            height as i32,
            source_dc,         // source device context
            capture_rect.left, // source x
            capture_rect.top,  // source y
            SRCCOPY,
        )
    };

    let mut ps = PAINTSTRUCTConst {
        hdc: ptr::null_mut(),
        fErase: 0,
        rcPaint: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        fRestore: 0,
        fIncUpdate: 0,
        rgbReserved: [0; 32],
    };

    let final_destination_dc = unsafe { BeginPaintConst(hwnd, &mut ps) };

    // scale/mangify
    unsafe {
        StretchBlt(
            final_destination_dc,
            0,
            0,
            (width * MAGNIFY_SCALE_FACTOR) as i32,
            (height * MAGNIFY_SCALE_FACTOR) as i32,
            destination_dc,
            0,
            0,
            width as i32,
            height as i32,
            SRCCOPY,
        );

        EndPaint(hwnd, &ps);
        InvalidateRect(hwnd, ptr::null_mut(), 0);
        SelectObjectConst(destination_dc, old_obj);
        DeleteDC(destination_dc);
        ReleaseDC(ptr::null_mut(), source_dc);
        DeleteObject(destination_bitmap as *mut winapi::ctypes::c_void);
    }
}
