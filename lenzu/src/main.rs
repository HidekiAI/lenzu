extern crate winapi;
mod ocr_tesseract;
mod ocr_traits;
mod ocr_winmedia;

use kakasi::convert;
use kakasi::IsJapanese;

use image::{DynamicImage, GenericImageView, ImageBuffer};
use ocr_tesseract::OcrTesseract;
use ocr_traits::OcrTrait;
use std::collections::HashMap;
use std::{
    ffi::{CStr, CString},
    ptr,
};
use winapi::{
    shared::{minwindef::BYTE, windef::RECT},
    um::{
        wingdi::{
            AlphaBlend, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
            GetDIBits, SelectObject, SetDIBits, StretchBlt, TextOutW, BITMAPINFO, BI_RGB,
            DIB_RGB_COLORS, SRCCOPY,
        },
        winnt::{LPSTR, LPWSTR},
        winuser::{
            BeginPaint, CreateWindowExW, DefWindowProcW, DispatchMessageW, EndPaint, GetCursorPos,
            GetDC, GetMessageW, GetMonitorInfoW, GetWindowRect, InvalidateRect, MonitorFromPoint,
            PostQuitMessage, RegisterClassW, ReleaseDC, ShowWindow, TranslateMessage,
            CW_USEDEFAULT, MONITORINFO, MONITOR_DEFAULTTONEAREST, MSG, PAINTSTRUCT, SW_HIDE,
            SW_SHOW, VK_ESCAPE, VK_SPACE, WM_KEYDOWN, WS_OVERLAPPEDWINDOW,
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

#[derive(Debug, Clone, Copy)]
struct CursorData {
    x: i32, // cursor positions may be negative based on monitor positon relative to primary monitor (i.e. monitors left of primary monitor have negative X coordinates)
    y: i32,
    // info about current monitor the cursor at (x,y) is located, probably only useful for capturing the WHOLE screen
    monitor_x: i32, // upper left corner of the monitor relative to the PRIMARY monitor
    monitor_y: i32,
    monitor_width: u32, // rcMonitor.right - rcMonitor.left (even if both are negative, it should come out as positive) - i.e. (0 - -1024 = 1024), (-1024 - -2048 = 1024), etc
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
            monitor_x: 0,
            monitor_y: 0,
            monitor_width: 1024,
            monitor_height: 768,
            window_x: 0,
            window_y: 0,
            window_width: 1,
            window_height: 1,
        }
    }

    fn update(&mut self, application_window_handle: winapi::shared::windef::HWND) {
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

        // Get dimension of the monitor the cursor is currently on (see MonitorFromPoint()) via GetMonitorInfoW()
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
        self.monitor_x = monitor_info.rcWork.left; // can be negative, based on being placed LEFT of the PRIMARY monitor
        self.monitor_y = monitor_info.rcWork.top;
        self.monitor_width =
            std::cmp::max(monitor_info.rcWork.right - monitor_info.rcWork.left, 1024) as u32;
        self.monitor_height =
            std::cmp::max(monitor_info.rcWork.bottom - monitor_info.rcWork.top, 768) as u32;

        // get current dimension of the windown on the monitor via via GetWindowRect() (GetWindowInfo() can do the same, but it provides more info that we care...)
        let mut window_rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        // get the dimension of the application window
        unsafe {
            GetWindowRect(application_window_handle, &mut window_rect);
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
    let window_name = "Lenzu-OCR";

    let mut ocr = OcrTesseract::new();
    let ocr_langugages = ocr.init();

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
                    let supported_languages = ocr_langugages.join("+");
                    capture_and_ocr(hwnd, &mut ocr, cursor, supported_languages.clone().as_str());
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

// NOTE: Make sure to call ShowWindow(hwnd, SW_IDE) prior to calling this method and ShowWindow(hwnd, SW_SHOW) after image is captured
// this is so that we do not get the image-echo effect (like a mirror reflecting a mirror) when we capture the screen
// will need to experiment, but it seems we do not need to invalidate since ShowWindow() will implicitly refresh window
fn from_screen_to_image(cursor_pos: CursorData) -> DynamicImage {
    // first, get DC of the entire desktop (hence we do not need HWND passed here) via calling GetDC(NULL) - NULL means the entire desktop
    let source_desktop_dc = unsafe { GetDC(ptr::null_mut()) };

    // Create a compatible device context and bitmap
    let destination_memory_dc = unsafe { CreateCompatibleDC(source_desktop_dc) };
    let destination_bitmap = unsafe {
        CreateCompatibleBitmap(
            source_desktop_dc,
            cursor_pos.window_width as i32,
            cursor_pos.window_height as i32,
        )
    };

    // select the bitmap into the memory device context
    let previous_screen_for_restore_dc = unsafe {
        SelectObject(
            destination_memory_dc,
            destination_bitmap as *mut winapi::ctypes::c_void,
        )
    };
    let mut image: DynamicImage;
    unsafe {
        // BitBlt from the screen DC to the memory DC
        BitBlt(
            destination_memory_dc, // destination device context
            0,                     // destination x
            0,                     // destination y
            cursor_pos.window_width as i32,
            cursor_pos.window_height as i32,
            source_desktop_dc,          // source device context
            cursor_pos.window_x as i32, // source x - note that coordinate can be negative value (e.g. cursor is on the left side of the PRIMARY monitor)
            cursor_pos.window_y as i32, // source y
            SRCCOPY,
        );

        // Clean up: Select the OLD bitmap back into the memory DC
        SelectObject(destination_memory_dc, previous_screen_for_restore_dc);

        // At this point, destination_bitmap contains the captured image
        // Create a BITMAPINFO structure to receive the bitmap data
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFO>() as u32;
        info.bmiHeader.biWidth = cursor_pos.window_width as i32;
        info.bmiHeader.biHeight = -(cursor_pos.window_height as i32); // top-down bitmap
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32; // each pixel is a 32-bit RGB color
        info.bmiHeader.biCompression = BI_RGB;

        // Allocate a buffer to receive the bitmap data
        let mut data: Vec<BYTE> =
            vec![0; (cursor_pos.window_width * cursor_pos.window_height * 4) as usize];

        // Get the bitmap data
        GetDIBits(
            destination_memory_dc,
            destination_bitmap,
            0,
            cursor_pos.window_height,
            data.as_mut_ptr() as *mut _,
            &mut info,
            DIB_RGB_COLORS,
        );

        // Convert the data to a DynamicImage
        image = ImageBuffer::from_fn(cursor_pos.window_width, cursor_pos.window_height, |x, y| {
            let i = ((y * cursor_pos.window_width + x) * 4) as usize;
            image::Rgba([data[i + 2], data[i + 1], data[i], 255])
        })
        .into(); // At this point, image is a DynamicImage containing the bitmap image

        DeleteDC(destination_memory_dc);
        ReleaseDC(ptr::null_mut(), source_desktop_dc);
        DeleteObject(destination_bitmap as *mut winapi::ctypes::c_void);
    };
    image
}

fn from_image_to_window(
    application_window_handle: *mut winapi::shared::windef::HWND__,
    image: DynamicImage,
) {
    unsafe {
        // Convert the DynamicImage to raw pixel data
        let (width, height) = image.dimensions();
        let mut data: Vec<BYTE> = Vec::with_capacity((width * height * 4) as usize);
        for (_, _, pixel) in image.pixels() {
            let image::Rgba([r, g, b, _]) = pixel;
            data.extend_from_slice(&[b, g, r, 255]);
        }

        // Get the device context for the window
        let hdc = GetDC(application_window_handle);

        // Create a compatible device context and bitmap
        let hdc_mem = CreateCompatibleDC(hdc);
        let hbitmap = CreateCompatibleBitmap(hdc, width as i32, height as i32);

        // Select the bitmap into the memory device context
        let hbitmap_old = SelectObject(hdc_mem, hbitmap as *mut _);

        // Set the bitmap data
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFO>() as u32;
        info.bmiHeader.biWidth = width as i32;
        info.bmiHeader.biHeight = -(height as i32); // top-down bitmap
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32; // each pixel is a 32-bit RGB color
        info.bmiHeader.biCompression = BI_RGB;
        SetDIBits(
            hdc_mem,
            hbitmap,
            0,
            height as u32,
            data.as_ptr() as *const _,
            &info,
            DIB_RGB_COLORS,
        );

        // BitBlt from the memory DC to the window DC
        BitBlt(
            hdc,
            0,
            0,
            width as i32,
            height as i32,
            hdc_mem,
            0,
            0,
            SRCCOPY,
        );

        // Clean up: Select the old bitmap back into the memory DC
        SelectObject(hdc_mem, hbitmap_old);
    }
}

fn capture_and_ocr(
    hwnd: *mut winapi::shared::windef::HWND__,
    ocr: &mut OcrTesseract,
    cursor_pos: CursorData,
    supported_lang: &str, // '+' separated list of supported languages(i.e. "jpn+jpn_ver+osd"), note that longer this list, longer it takes to OCR (ie. 10sec/lang so if there are 4 in this list, it can take 40 seconds!)
) {
    // first, hide application window
    unsafe { ShowWindow(hwnd, SW_HIDE) };

    // now capture the screen
    let screenshot = from_screen_to_image(cursor_pos);

    // show the application window again
    unsafe { ShowWindow(hwnd, SW_SHOW) };

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
    let gray_scale_image = screenshot.grayscale(); // Convert the image to grayscale
    let ocr_result = ocr.evaluate(&gray_scale_image);

    // now run kakasi to convert the kanji to hiragana
    // Translate Japanese text to hiragana
    let start_kakasi = std::time::Instant::now();
    let translate = match ocr_result {
        Ok(text) => {
            let res = kakasi::convert(text);
            res.hiragana
        }
        Err(e) => format!("Error: {:?}", e).into(),
    };
    println!(
        "Kakasi Result ({} mSec): '{:?}'",
        start_kakasi.elapsed().as_millis(),
        translate
    );

    //// Blend the topmost layer onto the primary image
    //let blend_func = winapi::um::wingdi::BLENDFUNCTION {
    //    BlendOp: winapi::um::wingdi::AC_SRC_OVER,
    //    BlendFlags: 0,
    //    SourceConstantAlpha: 128, // Adjust alpha value (0-255) for transparency
    //    AlphaFormat: winapi::um::wingdi::AC_SRC_ALPHA,
    //};
    //unsafe {
    //    AlphaBlend(
    //        destination_dc,
    //        0,
    //        0,
    //        cursor_pos.window_width as i32,
    //        cursor_pos.window_height as i32,
    //        mem_dc_topmost,
    //        0,
    //        0,
    //        cursor_pos.window_width as i32,
    //        cursor_pos.window_height as i32,
    //        blend_func,
    //    );
    //}

    // now render what we've captured
    // TODO: scale/stretchBlt()
    from_image_to_window(hwnd, screenshot);
}

fn capture_and_magnify(hwnd: *mut winapi::shared::windef::HWND__, cursor_pos: CursorData) {
    // first, hide application window
    //unsafe { ShowWindow(hwnd, SW_HIDE) };

    // now capture the screen
    let screenshot = from_screen_to_image(cursor_pos);

    // show the application window again
    //unsafe { ShowWindow(hwnd, SW_SHOW) };

    // now render what we've captured
    // TODO: scale/stretchBlt()
    from_image_to_window(hwnd, screenshot);

    //let destination_bitmap = unsafe {
    //    CreateCompatibleBitmap(
    //        source_dc,
    //        cursor_pos.window_width as i32,
    //        cursor_pos.window_height as i32,
    //    )
    //};

    //let destination_buffer = unsafe {
    //    SelectObject(
    //        destination_dc,
    //        destination_bitmap as *mut winapi::ctypes::c_void,
    //    )
    //};
    //unsafe {
    //    BitBlt(
    //        destination_dc, // destination device context
    //        0,              // destination x
    //        0,              // destination y
    //        cursor_pos.window_width as i32,
    //        cursor_pos.window_height as i32,
    //        source_dc,                  // source device context
    //        cursor_pos.window_x as i32, // source x
    //        cursor_pos.window_y as i32, // source y
    //        SRCCOPY,
    //    )
    //};

    //let mut repaint_area = PAINTSTRUCT {
    //    hdc: ptr::null_mut(),
    //    fErase: 0,
    //    rcPaint: RECT {
    //        left: 0,
    //        top: 0,
    //        right: 0,
    //        bottom: 0,
    //    },
    //    fRestore: 0,
    //    fIncUpdate: 0,
    //    rgbReserved: [0; 32],
    //};

    //let final_destination_dc = unsafe { BeginPaint(hwnd, &mut repaint_area) };

    //// scale/mangify
    //unsafe {
    //    StretchBlt(
    //        final_destination_dc,
    //        0,
    //        0,
    //        (cursor_pos.window_width * MAGNIFY_SCALE_FACTOR) as i32,
    //        (cursor_pos.window_height * MAGNIFY_SCALE_FACTOR) as i32,
    //        destination_dc,
    //        0,
    //        0,
    //        cursor_pos.window_width as i32,
    //        cursor_pos.window_height as i32,
    //        SRCCOPY,
    //    );

    //    EndPaint(hwnd, &repaint_area);
    //    InvalidateRect(hwnd, ptr::null_mut(), 0); // mark for refresh/update
    //    SelectObject(destination_dc, destination_buffer);

    //    DeleteDC(destination_dc);
    //    ReleaseDC(ptr::null_mut(), source_dc);
    //    DeleteObject(destination_bitmap as *mut winapi::ctypes::c_void);
    //}
}
