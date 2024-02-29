extern crate winapi;

use image::DynamicImage;
use rusty_tesseract::Args;
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::Arc;
use winapi::{
    shared::windef::RECT,
    um::{
        wingdi::{
            AlphaBlend, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
            SelectObject, StretchBlt, TextOutW, SRCCOPY,
        },
        winnt::{LPSTR, LPWSTR},
        winuser::{
            BeginPaint, CreateWindowExW, DefWindowProcW, DispatchMessageW, EndPaint, GetCursorPos,
            GetDC, GetMessageW, GetMonitorInfoW, GetWindowRect, InvalidateRect, MonitorFromPoint,
            PostQuitMessage, RegisterClassW, ReleaseDC, ShowWindow, TranslateMessage,
            CW_USEDEFAULT, MONITORINFO, MONITOR_DEFAULTTONEAREST, MSG, PAINTSTRUCT, VK_ESCAPE,
            VK_SPACE, WM_KEYDOWN, WS_OVERLAPPEDWINDOW,
        },
    },
};
use winit::{
    dpi::LogicalSize,
    event_loop::EventLoop,
    platform::windows::HWND,
    raw_window_handle::{HasWindowHandle, WindowHandle},
    window::WindowBuilder,
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

#[derive(Debug, Clone, Copy)]
struct CursorData {
    x: i32, // cursor positions may be negative based on monitor positon relative to primary monitor (i.e. monitors left of primary monitor have negative X coordinates)
    y: i32,
    // info about current monitor the curso at (x,y) is located
    monitor_width: u32, // dimensions of the current monitor
    monitor_height: u32,
    // current window
    window_x: i32, // position of the window on the monitor that is recalculated based off of cursor (x,y) and upper-left is offset by center of window to be where the mouse cursor will be
    window_y: i32,
    window_width: u32,
    window_height: u32,
}

impl CursorData {
    fn new() -> Self {
        CursorData {
            x: 0,
            y: 0,
            monitor_width: 1024,
            monitor_height: 768,
            window_x: 0,
            window_y: 0,
            window_width: 1,
            window_height: 1,
        }
    }

    fn update(&mut self, hwnd: winapi::shared::windef::HWND) {
        // first, get cursor position so that we can dtermine which monitor we are on
        let mut cursor_pos = winapi::shared::windef::POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut cursor_pos) } == 0 {
            // Handle the error appropriately if necessary.
            println!("Could not get cursor position");
            // post quit
            unsafe { PostQuitMessage(-1) };
        }
        self.x = cursor_pos.x;
        self.y = cursor_pos.y;

        // Get dimension of the monitor the cursor is currently on via GetMonitorInfoW()
        let h_monitor = unsafe { MonitorFromPoint(cursor_pos, MONITOR_DEFAULTTONEAREST) };
        let mut monitor_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT {
                // display area rectangle
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            rcWork: RECT {
                // work area rectangle (rectangle not obscured by taskbar and toolbar)
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
        // note that we use rcWork rectangle, so that we can ignore the taskbar and toolbar
        // work area, unlike monitor area, is usually/should-be positive because it's the area that's not obscured by the taskbar and toolbar
        self.monitor_width = std::cmp::max(monitor_info.rcWork.right, 1024) as u32;
        self.monitor_height = std::cmp::max(monitor_info.rcWork.bottom, 768) as u32;

        // get current dimension of the windown on the monitor via via GetWindowRect() (GetWindowInfo() can do the same, but it provides more info that we care...)
        let mut window_rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        unsafe {
            GetWindowRect(hwnd, &mut window_rect);
        }
        self.window_width = (window_rect.right - window_rect.left) as u32;
        self.window_height = (window_rect.bottom - window_rect.top) as u32;
        // window position (upper left corner) is recalculated based off of cursor (x,y) and upper-left is offset by center of window to be where the mouse cursor will be
        // the tricky part of this is that the windows position coordinate can be negative (same as cursor position) so it's not possible to test for min()/max() for
        // edge of the monitor, and so we'll not do snap to monitor and allow windows to get beyond the edges of the monitors
        self.window_x = self.x - (self.window_width as i32 / 2) as i32;
        self.window_y = self.y - (self.window_height as i32 / 2) as i32;
    }
}

fn main() {
    let class_name = "Lenzu";
    let window_name = "Tesseract";

    //tesseract version
    let tesseract_version = rusty_tesseract::get_tesseract_version().unwrap();
    println!("Tesseract - Version is: {:?}", tesseract_version);

    //available languages
    let tesseract_langs = rusty_tesseract::get_tesseract_langs().unwrap();
    println!(
        "Tesseract - The available languages are: {:?}",
        tesseract_langs
    );

    //available config parameters
    let parameters = rusty_tesseract::get_tesseract_config_parameters().unwrap();
    println!(
        "Tesseract - Config parameter: {}",
        parameters.config_parameters.first().unwrap()
    );

    // initialize a view-window via winit so that it is universal to both Linux and Windows
    //let event_loop = EventLoop::new();
    //let window = WindowBuilder::new()
    //    .with_title(window_name)
    //    .with_inner_size(LogicalSize::new(1024, 768)) // initial size of the window
    //    .with_min_inner_size(LogicalSize::new(1024, 768)) // minimum size of the window
    //    .build(&event_loop.unwrap())
    //    .unwrap();
    //let hw = match window.window_handle().unwrap() {
    //    WindowHandle::Wayland(handle) => handle.wayland_display().unwrap(),
    //    WindowHandle::X(handle) => handle.xlib_display().unwrap(),
    //    WindowHandle::win32(handle) => handle.hwnd().unwrap(),
    //    _ => panic!("Unsupported platform"),
    //};

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

    let hwnd: *mut winapi::shared::windef::HWND__ = unsafe {
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
        // Instead of panic!(), we'll just close it cleanly with PostQuitMessage() and log to explain the cause/reasons
        println!("Failed to create window.");
        unsafe { PostQuitMessage(0) }; // Even if HWND was not created, can we post a quit message?
        return;
    }

    unsafe {
        ShowWindow(hwnd, winapi::um::winuser::SW_SHOWDEFAULT);
    }

    let mut cursor = CursorData::new();
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
        cursor.update(hwnd);

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

        unsafe {
            match TOGGLE_STATE {
                ToggleState::Free => capture_and_magnify(hwnd, cursor),
                ToggleState::MoveWindow => {
                    // move the window to the cursor position (a sticky window)
                    winapi::um::winuser::SetWindowPos(
                        hwnd,
                        ptr::null_mut(),
                        cursor.window_x.clone(),
                        cursor.window_y.clone(),
                        0, // width will be ignored because will use SWP_NOSIZE to retain current size
                        0, // height ignored
                        winapi::um::winuser::SWP_NOSIZE | winapi::um::winuser::SWP_NOZORDER,
                    );
                    //// invalidate the window so it can redraw the window onto the Desktop/monitor
                    //InvalidateRect(hwnd, ptr::null_mut(), 0);
                    capture_and_magnify(hwnd, cursor); // show contents UNDERNEATH the window (will InvalidateRect() so that it'll also redraw the actual window onto the )
                }
                ToggleState::Capture => {
                    // capture the screen and magnify it
                    let supported_languages = tesseract_langs.join("+");
                    capture_and_ocr(hwnd, cursor, supported_languages.clone().as_str());
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
    cursor_pos: CursorData,
    supporte_lang: &str,
) {
    //let (monitor_left, monitor_top, monitor_right, monitor_bottom) = get_current_monitor_rect(cursor_pos);
    let source_dc = unsafe { GetDC(ptr::null_mut()) };
    let destination_dc = unsafe { CreateCompatibleDC(source_dc) };

    // Create a compatible bitmap for the primary image
    let destination_bitmap = unsafe {
        CreateCompatibleBitmap(
            destination_dc,
            cursor_pos.window_width as i32,
            cursor_pos.window_height as i32,
        )
    };

    let destination_buffer = unsafe {
        SelectObject(
            destination_dc,
            destination_bitmap as *mut winapi::ctypes::c_void,
        )
    };
    unsafe {
        BitBlt(
            destination_dc,
            0, // destination x
            0, // destination y
            cursor_pos.window_width as i32,
            cursor_pos.window_height as i32,
            source_dc,
            cursor_pos.window_x as i32, // source x
            cursor_pos.window_y as i32, // source y
            SRCCOPY,
        )
    };

    let mut paint_struct = PAINTSTRUCT {
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
    let bmp_topmost = unsafe {
        CreateCompatibleBitmap(
            destination_dc,
            cursor_pos.window_width as i32,
            cursor_pos.window_height as i32,
        )
    };
    let mem_dc_topmost = unsafe { CreateCompatibleDC(destination_dc) };
    let top_layer_bitmap =
        unsafe { SelectObject(mem_dc_topmost, bmp_topmost as *mut winapi::ctypes::c_void) };

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
    // convert DC to RGBA - probably can get away with 24-bit but for better byte alignment, will stay at 32-bit
    let rgba_image = DynamicImage::new_rgba8(cursor_pos.window_width, cursor_pos.window_height); // pre-allocate buffer
    let _gray_scale_image = rgba_image.grayscale(); // Convert the image to grayscale
    let ocr_args: rusty_tesseract::Args = Args {
        lang: supporte_lang.into(),
        ..Default::default()
    };
    let ocr_image = rusty_tesseract::Image::from_dynamic_image(&rgba_image); // from_dynamic_image(&gray_scale_image);
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
    println!("'{}'", text.to_string_lossy());

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
            cursor_pos.window_width as i32,
            cursor_pos.window_height as i32,
            mem_dc_topmost,
            0,
            0,
            cursor_pos.window_width as i32,
            cursor_pos.window_height as i32,
            blend_func,
        );
    }

    let final_destination_dc = unsafe { BeginPaint(hwnd, &mut paint_struct) };

    // scale/mangify
    unsafe {
        StretchBlt(
            final_destination_dc,
            0,
            0,
            (cursor_pos.window_width * MAGNIFY_SCALE_FACTOR) as i32,
            (cursor_pos.window_height * MAGNIFY_SCALE_FACTOR) as i32,
            destination_dc,
            0,
            0,
            cursor_pos.window_width as i32,
            cursor_pos.window_height as i32,
            SRCCOPY,
        );

        EndPaint(hwnd, &paint_struct);
        InvalidateRect(hwnd, ptr::null_mut(), 0); // mark for refresh/update
        SelectObject(destination_dc, destination_buffer);

        DeleteDC(destination_dc);
        DeleteDC(mem_dc_topmost);
        ReleaseDC(ptr::null_mut(), source_dc);
        DeleteObject(destination_bitmap as *mut winapi::ctypes::c_void);
        DeleteObject(top_layer_bitmap as *mut winapi::ctypes::c_void);
    }
}

fn capture_and_magnify(hwnd: *mut winapi::shared::windef::HWND__, cursor_pos: CursorData) {
    //let (monitor_left, monitor_top, monitor_right, monitor_bottom) = get_current_monitor_rect(cursor_pos);
    let source_dc = unsafe { GetDC(ptr::null_mut()) };
    let destination_dc = unsafe { CreateCompatibleDC(source_dc) };

    let destination_bitmap = unsafe {
        CreateCompatibleBitmap(
            source_dc,
            cursor_pos.window_width as i32,
            cursor_pos.window_height as i32,
        )
    };

    let destination_buffer = unsafe {
        SelectObject(
            destination_dc,
            destination_bitmap as *mut winapi::ctypes::c_void,
        )
    };
    unsafe {
        BitBlt(
            destination_dc, // destination device context
            0,              // destination x
            0,              // destination y
            cursor_pos.window_width as i32,
            cursor_pos.window_height as i32,
            source_dc,                  // source device context
            cursor_pos.window_x as i32, // source x
            cursor_pos.window_y as i32, // source y
            SRCCOPY,
        )
    };

    let mut paint_struct = PAINTSTRUCT {
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

    let final_destination_dc = unsafe { BeginPaint(hwnd, &mut paint_struct) };

    // scale/mangify
    unsafe {
        StretchBlt(
            final_destination_dc,
            0,
            0,
            (cursor_pos.window_width * MAGNIFY_SCALE_FACTOR) as i32,
            (cursor_pos.window_height * MAGNIFY_SCALE_FACTOR) as i32,
            destination_dc,
            0,
            0,
            cursor_pos.window_width as i32,
            cursor_pos.window_height as i32,
            SRCCOPY,
        );

        EndPaint(hwnd, &paint_struct);
        InvalidateRect(hwnd, ptr::null_mut(), 0); // mark for refresh/update
        SelectObject(destination_dc, destination_buffer);

        DeleteDC(destination_dc);
        ReleaseDC(ptr::null_mut(), source_dc);
        DeleteObject(destination_bitmap as *mut winapi::ctypes::c_void);
    }
}
