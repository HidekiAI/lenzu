#[cfg(target_os = "windows")]
extern crate winapi;

#[cfg(target_os = "linux")]
extern crate x11;

use std::{ffi::CString, ptr};

#[cfg(target_os = "windows")]
use winapi::{
    shared::windef::RECT,
    um::{
        wingdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC as CreateCompatibleDCConst,
            DeleteDC, DeleteObject, SelectObject as SelectObjectConst, StretchBlt, SRCCOPY,
        },
        winuser::{
            BeginPaint as BeginPaintConst, CreateWindowExW, DefWindowProcW, EndPaint as EndPaintConst, GetCursorPos,
            GetDC as GetDCConst, GetMonitorInfoW,
            InvalidateRect as InvalidateRectConst,
            RegisterClassW, ReleaseDC, ShowWindow,
            CW_USEDEFAULT, MONITORINFO, MONITOR_DEFAULTTONEAREST, MSG,
            PAINTSTRUCT as PAINTSTRUCTConst, WS_OVERLAPPEDWINDOW,
        },
    },
};
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::{WindowBuilder},
};

fn main() {
    #[cfg(target_os = "windows")]
    let mut hwnd: winapi::shared::windef::HWND = ptr::null_mut();

    #[cfg(target_os = "windows")]
    {
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

        hwnd = unsafe {
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

        let _msg = MSG {
            hwnd: ptr::null_mut(),
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: winapi::shared::windef::POINT { x: 0, y: 0 },
        };
    }
    #[cfg(target_os = "linux")]
    {}

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new().build(&event_loop).unwrap();
    let mut cursor_pos = winit::dpi::PhysicalPosition::new(0.0, 0.0);
    let _window_id = window.id(); // Declare the window_id variable

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
            } if window_id == window_id => {
                *control_flow = ControlFlow::Exit;
                return;
            }
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        input:
                            winit::event::KeyboardInput {
                                virtual_keycode: Some(winit::event::VirtualKeyCode::Escape),
                                ..
                            },
                        ..
                    },
                window_id,
            } if window_id == window_id => {
                *control_flow = ControlFlow::Exit;
                return;
            }
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                window_id,
            } if window_id == window_id => {
                #[cfg(target_os = "windows")]
                {
                    let mut cursor_pos = winapi::shared::windef::POINT { x: 0, y: 0 };
                    if unsafe { GetCursorPos(&mut cursor_pos) } == 0 {
                        // Handle the error appropriately if necessary.
                    }
                }
                #[cfg(target_os = "linux")]
                {}

                // Update the cursor position
                cursor_pos = position;
                //let available_monitors = event_loop.available_monitors();
                let monitor = window.current_monitor().unwrap();

                // Get the position and size of the monitor
                let monitor_pos = monitor.position();
                let monitor_size = monitor.size();
                #[cfg(target_os = "windows")]
                {
                    let win_cursor_pos: winapi::shared::windef::POINT = {
                        winapi::shared::windef::POINT {
                            x: cursor_pos.x as i32,
                            y: cursor_pos.y as i32,
                        }
                    };
                    let h_monitor = unsafe {
                        winapi::um::winuser::MonitorFromPoint(
                            win_cursor_pos,
                            MONITOR_DEFAULTTONEAREST,
                        )
                    };
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
                        left: std::cmp::max(
                            monitor_info.rcMonitor.left as i32,
                            cursor_pos.x as i32 - 256,
                        ),
                        top: std::cmp::max(monitor_info.rcMonitor.top, cursor_pos.y as i32 - 256),
                        right: std::cmp::min(
                            monitor_info.rcMonitor.right,
                            cursor_pos.x as i32 + 256,
                        ),
                        bottom: std::cmp::min(
                            monitor_info.rcMonitor.bottom,
                            cursor_pos.y as i32 + 256,
                        ),
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
                    // This code will only be compiled when targeting Windows
                    let old_obj =
                        unsafe { SelectObjectConst(h_dc, h_bitmap as *mut winapi::ctypes::c_void) };
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
                #[cfg(target_os = "linux")]
                {}

                // Determine if the cursor is on the monitor
                let cursor_x = cursor_pos.x as i32;
                let cursor_y = cursor_pos.y as i32;
                let monitor_left = monitor_pos.x as i32;
                let monitor_top = monitor_pos.y as i32;
                let monitor_right = monitor_left + monitor_size.width as i32;
                let monitor_bottom = monitor_top + monitor_size.height as i32;
                let cursor_on_monitor = cursor_x >= monitor_left
                    && cursor_x < monitor_right
                    && cursor_y >= monitor_top
                    && cursor_y < monitor_bottom;

                if cursor_on_monitor {
                    println!("Cursor is on monitor {:?}", monitor.name());
                }

                #[cfg(target_os = "windows")]
                {}
                #[cfg(target_os = "linux")]
                {
                    // This code will only be compiled when targeting Linux
                    println!("This code is for Linux!");
                }
                #[cfg(not(any(target_os = "windows", target_os = "linux")))]
                {
                    // This code will only be compiled when not targeting Windows or Linux
                    println!("This code is for other operating systems!");
                }
            }
            _ => {}
        } // match event
    });
}
