extern crate winapi;

use std::ffi::CString;
use std::ptr;
use winapi::um::wingdi::{DeleteDC, DeleteObject, StretchBlt, CreateCompatibleBitmap, BitBlt};
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, PostQuitMessage, RegisterClassW, ReleaseDC, ShowWindow,
};

use winapi::um::{wingdi::{CreateCompatibleDC as CreateCompatibleDCConst, SelectObject as SelectObjectConst, SRCCOPY}, winuser::BeginPaint as BeginPaintConst};
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
use winapi::shared::windef::RECT;

fn main() {
    let class_name = "Sample Window Class";
    let window_name = "Test Window";

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

        if msg.message == WM_KEYDOWN && msg.wParam as u32 == VK_ESCAPE as u32 {
            // Check for the ESCAPE key press and exit the application
            unsafe {
                PostQuitMessage(0);
            }
        }

        let mut cursor_pos = winapi::shared::windef::POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut cursor_pos) } == 0 {
            // Handle the error appropriately if necessary.
        }

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

        let capture_rect = RECT {
            left: std::cmp::max(monitor_info.rcMonitor.left, cursor_pos.x - 256),
            top: std::cmp::max(monitor_info.rcMonitor.top, cursor_pos.y - 256),
            right: std::cmp::min(monitor_info.rcMonitor.right, cursor_pos.x + 256),
            bottom: std::cmp::min(monitor_info.rcMonitor.bottom, cursor_pos.y + 256),
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

        unsafe {
            StretchBlt(
                hdc,
                0,
                0,
                (capture_rect.right - capture_rect.left) * 2,
                (capture_rect.bottom - capture_rect.top) * 2,
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
}
