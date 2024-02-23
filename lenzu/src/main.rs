extern crate winapi;

use std::ffi::CString;
use std::ptr;
use winapi::um::wingdi::{BitBlt, CreateCompatibleBitmap, DeleteDC, DeleteObject, StretchBlt};
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, GetWindowInfo, PostQuitMessage, RegisterClassW, ReleaseDC,
    ShowWindow, VK_SPACE,
};

use winapi::shared::windef::RECT;
use winapi::um::winuser::DispatchMessageW as DispatchMessageWConst;
use winapi::um::winuser::EndPaint as EndPaintConst;
use winapi::um::winuser::GetCursorPos;
use winapi::um::winuser::GetDC as GetDCConst;
use winapi::um::winuser::GetMessageW as GetMessageConst;
use winapi::um::winuser::GetMonitorInfoW;
use winapi::um::winuser::InvalidateRect as InvalidateRectConst;
use winapi::um::winuser::MonitorFromPoint;
use winapi::um::winuser::TranslateMessage as TranslateMessageConst;
use winapi::um::winuser::CW_USEDEFAULT;
use winapi::um::winuser::MONITORINFO;
use winapi::um::winuser::MONITOR_DEFAULTTONEAREST;
use winapi::um::winuser::MSG;
use winapi::um::winuser::PAINTSTRUCT as PAINTSTRUCTConst;
use winapi::um::winuser::VK_ESCAPE;
use winapi::um::winuser::WM_KEYDOWN;
use winapi::um::winuser::WS_OVERLAPPEDWINDOW;
use winapi::um::{
    wingdi::{
        CreateCompatibleDC as CreateCompatibleDCConst, SelectObject as SelectObjectConst, SRCCOPY,
    },
    winuser::BeginPaint as BeginPaintConst,
};

const MAGNIFY_SCALE_FACTOR: u32 = 2;
const TOGGLE_WINDOW_MOVE_KEY: std::ffi::c_int = winapi::um::winuser::VK_SPACE;
enum ToggleState {
    Free,
    MoveWindow,
    Capture,
}

static mut toggle_state: ToggleState = ToggleState::Free;

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
        if unsafe { GetMessageConst(&mut msg, ptr::null_mut(), 0, 0) } == 0 {
            break;
        }

        unsafe {
            TranslateMessageConst(&msg);
            DispatchMessageWConst(&msg);
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
                        toggle_state = match toggle_state {
                            ToggleState::Free => ToggleState::MoveWindow,
                            ToggleState::MoveWindow => ToggleState::Capture,
                            ToggleState::Capture => ToggleState::Free,
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
            match toggle_state {
                ToggleState::Free => capture_and_magnify(hwnd, cursor_pos),
                ToggleState::MoveWindow => {
                    // move the window to the cursor position (a sticky window)
                    unsafe {
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
                        //InvalidateRectConst(hwnd, ptr::null_mut(), 0);
                        capture_and_magnify(hwnd, cursor_pos); // show contents UNDERNEATH the window (will InvalidateRect() so that it'll also redraw the actual window onto the )
                    }
                }
                ToggleState::Capture => {
                    // capture the screen and magnify it
                    //capture_and_ocr(hwnd, cursor_pos);
                    capture_and_magnify(hwnd, cursor_pos);
                }
            }
        }
    } // loop
}

fn capture_and_ocr(
    hwnd: *mut winapi::shared::windef::HWND__,
    cursor_pos: winapi::shared::windef::POINT,
) {
    todo!()
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
    let mut window_rect = RECT {
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
        window_rect = window_info.rcWindow;
    }
    let width = std::cmp::max(2, window_rect.right - window_rect.left);
    let height = std::cmp::max(2, window_rect.bottom - window_rect.top);
    // capture rectangle is based off of consierating to edge of the monitor relative to the cursor position
    let capture_rect = RECT {
        left: std::cmp::max(monitor_info.rcMonitor.left, cursor_pos.x - (width / 2)), // make sure left is not less than monitor left
        top: std::cmp::max(monitor_info.rcMonitor.top, cursor_pos.y - (height / 2)), // make sure top is not less than monitor top
        right: std::cmp::min(monitor_info.rcMonitor.right, cursor_pos.x + (width / 2)), // make sure right is not greater than monitor right
        bottom: std::cmp::min(monitor_info.rcMonitor.bottom, cursor_pos.y + (height / 2)), // make sure bottom is not greater than monitor bottom
    };

    let h_screen = unsafe { GetDCConst(ptr::null_mut()) };
    let h_dc = unsafe { CreateCompatibleDCConst(h_screen) };

    let h_bitmap = unsafe {
        CreateCompatibleBitmap(
            h_screen,
            capture_rect.right - capture_rect.left,
            capture_rect.bottom - capture_rect.top,
        )
    };

    let old_obj = unsafe { SelectObjectConst(h_dc, h_bitmap as *mut winapi::ctypes::c_void) };
    unsafe {
        BitBlt(
            h_dc,
            0,
            0,
            capture_rect.right - capture_rect.left,
            capture_rect.bottom - capture_rect.top,
            h_screen,
            capture_rect.left,
            capture_rect.top,
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

    let hdc = unsafe { BeginPaintConst(hwnd, &mut ps) };

    // scale/mangify
    unsafe {
        StretchBlt(
            hdc,
            0,
            0,
            (capture_rect.right - capture_rect.left) * MAGNIFY_SCALE_FACTOR as i32,
            (capture_rect.bottom - capture_rect.top) * MAGNIFY_SCALE_FACTOR as i32,
            h_dc,
            0,
            0,
            capture_rect.right - capture_rect.left,
            capture_rect.bottom - capture_rect.top,
            SRCCOPY,
        );

        EndPaintConst(hwnd, &ps);
        InvalidateRectConst(hwnd, ptr::null_mut(), 0);
        SelectObjectConst(h_dc, old_obj);
        DeleteDC(h_dc);
        ReleaseDC(ptr::null_mut(), h_screen);
        DeleteObject(h_bitmap as *mut winapi::ctypes::c_void);
    }
}
