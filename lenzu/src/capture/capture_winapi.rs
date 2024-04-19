use super::capture_traits::*;
use gtk4::{gdk::Surface, prelude::*};
use image::{ImageBuffer, RgbaImage};
use winapi::{
    shared::{
        minwindef::BYTE,
        windef::{HWND, RECT},
    },
    um::{
        processthreadsapi::GetCurrentProcess,
        wingdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
            SelectObject, SetDIBits, BITMAPINFO, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
        },
        winuser::{
            DispatchMessageW, EnumWindows, GetCursorPos, GetDC, GetMessageW, GetMonitorInfoW,
            GetWindowLongW, GetWindowRect, GetWindowTextW, InvalidateRect, MonitorFromPoint,
            PostQuitMessage, ReleaseDC, ShowWindow, TranslateMessage, GWL_EXSTYLE, MONITORINFO,
            MONITOR_DEFAULTTONEAREST, MSG, SW_SHOW, VK_ESCAPE, VK_SPACE, WM_KEYDOWN,
        },
    },
};

struct CaptureWinApi {
    cursor_data: CursorData,

    hwnd: winapi::shared::windef::HWND,
}

impl CaptureTrait for CaptureWinApi {
    fn new() -> Self
    where
        Self: Sized,
    {
        CaptureWinApi {
            cursor_data: CursorData::new(),
            hwnd: std::ptr::null_mut(), // use set_hwnd
        }
    }

    fn init(&self) -> bool {
        todo!()
    }

    fn capture(
        &self,
        //ocr: &mut std::boxed::Box<dyn OcrTrait>,
        cursor_pos: CursorData,
        //ocr_font: &mut OCRImage,
        //interpreter: &mut std::boxed::Box<dyn InterpreterTrait>,
    ) -> Result<image::DynamicImage, anyhow::Error> {
        // first, set transparancy of the window to 99% (i.e. almost invisible) using SetLayeredWindowAttributes()
        self.hide_window();

        // now capture the screen
        let screenshot = from_screen_to_image(cursor_pos);

        // show the application window again
        self.show_window();

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
        let ocr_start_time = std::time::Instant::now();
        let ocr_result = ocr.evaluate(&gray_scale_image);
        let ocr_time = ocr_start_time.elapsed().as_millis();

        // now run kakasi to convert the kanji to hiragana
        // Translate Japanese text to hiragana
        let start_interpreter = std::time::Instant::now();
        let possible_result_tupled = match ocr_result {
            Ok(recognized_result) => {
                println!("OCR Result: '{:?}' {} mSec", recognized_result, ocr_time);

                for line in recognized_result.lines.clone() {
                    if line.contains(" ") {
                        panic!("evaluate_async(): Detected space in line: {:?}", line);
                    }
                    // dump each char as bytes
                    for c in line.chars() {
                        print!("{:?} ", c as u8);
                    }
                    println!("");
                }
                let possible_translate_result = interpreter.convert(&recognized_result.lines);
                match possible_translate_result {
                    Ok(translate_result) => {
                        println!(
                            "Interpreter Result: '{:?}' {} mSec",
                            translate_result,
                            start_interpreter.elapsed().as_millis()
                        );
                        Some((recognized_result, translate_result))
                    }
                    Err(e) => {
                        println!(
                            "Error: {:?} - {} mSec",
                            e,
                            start_interpreter.elapsed().as_millis()
                        );
                        Some((recognized_result, InterpreterTraitResult::new()))
                    }
                }
            }
            Err(e) => {
                println!("Error: {:?} - {} mSec", e, ocr_time);
                None
            }
        };
        match possible_result_tupled {
            Some((recognized_result, translate_result)) => {
                println!(
                    "########################## Interpreter Result ({} mSec):\n'{}'\n'{}'\n",
                    start_interpreter.elapsed().as_millis(),
                    recognized_result,
                    translate_result,
                );

                // And then, layer this PNG onto the original image (blend  png_buffer onto gray_scale_image)
                // image width and height is based on max of the two
                // now create a PNG with alpha channel and draw the text onto the image
                let mut recognized_image = screenshot;
                if !translate_result.text.is_empty() {
                    ocr_font.set_image(recognized_image);
                    recognized_image = ocr_font.overlay_text(translate_result.text.as_str(), 0, 0);
                }

                if cfg!(debug_assertions) {
                    // save the image for debugging purposes
                    println!("Saving debug image: recognized_image.png");
                    recognized_image.save("recognized_image.png").unwrap();
                }

                // render translated text onto the window
                self.from_image_to_window(recognized_image);
            }
            None => {
                println!(
                    "Interpreter Result ({} mSec): '{:?}'",
                    start_interpreter.elapsed().as_millis(),
                    possible_result_tupled
                );
                // render what we've captured originally instead
                self.from_image_to_window(screenshot);
            }
        }
    }

    fn update(&mut self) {
        // first, get cursor position so that we can dtermine which monitor we are on
        let mut cursor_pos = winapi::shared::windef::POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut cursor_pos) } == 0 {
            // Handle the error appropriately if necessary.
            println!("Could not get cursor position");
            // post quit
            unsafe { PostQuitMessage(-1) };
        }
        self.cursor_data.x = cursor_pos.x;
        self.cursor_data.y = cursor_pos.y;

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
        self.cursor_data.monitor_x = monitor_info.rcWork.left; // can be negative, based on being placed LEFT of the PRIMARY monitor
        self.cursor_data.monitor_y = monitor_info.rcWork.top;
        self.cursor_data.monitor_width =
            std::cmp::max(monitor_info.rcWork.right - monitor_info.rcWork.left, 1024) as u32;
        self.cursor_data.monitor_height =
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
            GetWindowRect(self.self.hwnd, &mut window_rect);
        }
        self.cursor_data.window_width = (window_rect.right - window_rect.left) as u32;
        self.cursor_data.window_height = (window_rect.bottom - window_rect.top) as u32;
        // window position (upper left corner) is recalculated based off of cursor (x,y) and upper-left is offset by center of window to be where the mouse cursor will be
        // the tricky part of this is that the windows position coordinate can be negative (same as cursor position) so it's not possible to test for min()/max() for
        // edge of the monitor, and so we'll not do snap to monitor and allow windows to get beyond the edges of the monitors
        self.cursor_data.window_x =
            self.cursor_data.x - (self.cursor_data.window_width as i32 / 2) as i32;
        self.cursor_data.window_y =
            self.cursor_data.y - (self.cursor_data.window_height as i32 / 2) as i32;
    }
}

impl CaptureWinApi {
    fn set_hwnd(&mut self, hwnd: winapi::shared::windef::HWND) {
        self.hwnd = hwnd;
    }
    // use this method so so that the application can remain agnostic to the platform
    fn from_gtk4(&mut self, gtk_app_window: &gtk4::ApplicationWindow) {
        if self.hwnd.is_null() {
            // using gtk4 gdk_win32_window_get_handle to get the HWND seems to be the practice used by OpenGL users
            let window_handle = unsafe {
                //for X11:
                //gdk_x11_drawable_get_xid(gtk_widget_get_window(gtk_app_window))
                // for Win32:
                GDK_SURFACE_HWND(gtk_widget_get_surface(gtk_app_window))
            };

            // but unsure if that option exist now?  We'll do it the traditinal WinAPI way...
            // first, get the current PcoressId; in which we can tehn call EnumWindows() to get the HWND
            let current_process = unsafe { GetCurrentProcess() };

            // see: https://learn.microsoft.com/en-us/previous-versions/windows/desktop/legacy/ms633498(v=vs.85)
            // EnumWindowsProc callback function
            extern "system" fn enum_windows_proc(
                hwnd: winapi::shared::windef::HWND,
                lparam: isize,
            ) -> i32 {
                let mut found_hwnd: winapi::shared::windef::HWND = std::ptr::null_mut();
                let mut window_text: [u16; 256] = [0; 256];
                unsafe {
                    GetWindowTextW(hwnd, window_text.as_mut_ptr(), window_text.len() as i32);
                    if window_text[0] != 0 {
                        // if the window has a title, then we'll use that as the window to capture
                        found_hwnd = hwnd;
                    }
                    if found_hwnd.is_null() {
                        // if we didn't find a window with a title, then we'll just use the first window we find
                        found_hwnd = hwnd;
                    }
                    *(lparam as *mut winapi::shared::windef::HWND) = found_hwnd;

                    0 // continue enumeration
                }
            }

            let mut hwnd: winapi::shared::windef::HWND = std::ptr::null_mut();
            let have_hwnd = unsafe {
                EnumWindows(
                    Some(enum_windows_proc),
                    &mut hwnd as *mut winapi::shared::windef::HWND as isize,
                )
            };
            self.set_hwnd(hwnd);
        }
        let gdk_display = gtk_window.get_display();
        let gdk_monitor = gdk_display.get_monitor_at_window(&gdk_window).unwrap();
        let monitor_geometry = gdk_monitor.get_geometry();
        let monitor_rect = gdk_monitor.get_workarea();
        let window_rect = gdk_window.get_frame_extents();
        self.cursor_data.monitor_x = monitor_rect.x;
        self.cursor_data.monitor_y = monitor_rect.y;
        self.cursor_data.monitor_width = monitor_rect.width;
        self.cursor_data.monitor_height = monitor_rect.height;
        self.cursor_data.window_width = window_rect.width;
        self.cursor_data.window_height = window_rect.height;
        self.cursor_data.window_x = window_rect.x;
        self.cursor_data.window_y = window_rect.y;
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
        let image: DynamicImage;
        unsafe {
            // BitBlt from the screen DC to the memory DC
            BitBlt(
                destination_memory_dc, // destination device context
                0,                     // destination x
                0,                     // destination y
                cursor_pos.window_width as i32,
                cursor_pos.window_height as i32,
                source_desktop_dc,   // source device context
                cursor_pos.window_x, // source x - note that coordinate can be negative value (e.g. cursor is on the left side of the PRIMARY monitor)
                cursor_pos.window_y, // source y
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
            image =
                ImageBuffer::from_fn(cursor_pos.window_width, cursor_pos.window_height, |x, y| {
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
    fn from_image_to_window(&self, image: DynamicImage) {
        unsafe {
            // just in case, show window
            ShowWindow(self.hwnd, SW_SHOW);

            // Convert the DynamicImage to raw pixel data
            let (width, height) = image.dimensions();
            let mut data: Vec<BYTE> = Vec::with_capacity((width * height * 4) as usize);
            for (_, _, pixel) in image.pixels() {
                let image::Rgba([r, g, b, _]) = pixel;
                data.extend_from_slice(&[b, g, r, 255]);
            }

            // Get the device context for the window
            let hdc = GetDC(self.hwnd);

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

            //EndPaint(self.hwnd, &repaint_area);
            InvalidateRect(self.hwnd, std::ptr::null_mut(), 0); // mark for refresh/update

            // Clean up: Select the old bitmap back into the memory DC
            SelectObject(hdc_mem, hbitmap_old);
        }
    }

    fn capture_and_scale(&self, cursor_pos: CursorData) {
        // first, set transparancy of the window to 99% (i.e. almost invisible) using SetLayeredWindowAttributes()
        self.hide_window();

        // now capture the screen
        let screenshot = self.from_screen_to_image(cursor_pos);

        // show the application window again
        self.show_window();

        // now render what we've captured
        self.from_image_to_window(screenshot);
    }

    // In order to now get the mirror-effect, we have to hide the window, capture the screen, show the window, then render the captured screen
    // unfortunately, "hide" isn't based on ShowWindow(SW_HIDE) because that effect is similar/same as when the window is minimized, and
    // you completely loose control of the window (i.e. you cannot move window, nor will hitting the ESCAPE key work because the window is NOT in focus!)
    // Hence, when we "hide" the window, it actually is more like setting the transparancy of the window to 99% (i.e. almost invisible)
    fn hide_window(&self) {
        unsafe {
            let current_flags = GetWindowLongW(self.hwnd, GWL_EXSTYLE);
            let new_flags: i32 = (winapi::um::winuser::WS_EX_LAYERED as i32 | current_flags)
                .try_into()
                .unwrap();
            // have to turn ON the Layered bit first...
            winapi::um::winuser::SetWindowLongW(
                self.hwnd,
                winapi::um::winuser::GWL_EXSTYLE,
                new_flags,
            );
            // now set it to 1%
            winapi::um::winuser::SetLayeredWindowAttributes(
                self.hwnd,
                0,
                (((1u32 * 255u32) / 100u32) & 0xFF) as u8, // 100% transparancy
                winapi::um::winuser::LWA_ALPHA,
            );
        }
    }
    fn show_window(&self) {
        unsafe {
            let current_flags = GetWindowLongW(self.hwnd, GWL_EXSTYLE);
            let new_flags: i32 = (winapi::um::winuser::WS_EX_LAYERED as i32 & !current_flags)
                .try_into()
                .unwrap();
            // have to turn OFF the layered bit
            winapi::um::winuser::SetWindowLongW(
                self.hwnd,
                winapi::um::winuser::GWL_EXSTYLE,
                new_flags,
            );
            // now set it to 100%
            winapi::um::winuser::SetLayeredWindowAttributes(
                self.hwnd,
                0,
                (((100u32 * 255u32) / 100u32) & 0xFF) as u8, // 100% transparancy
                winapi::um::winuser::LWA_ALPHA,
            );
        }
    }

    //let main_builder = WindowBuilder::new().with_title(class_name);
    //let window: Window = main_builder.build(&winit_event_loop).unwrap();
    //let (self.hwnd, _possible_h_instance) = match window.window_handle().unwrap().as_raw() {
    //    RawWindowHandle::Win32(handle) => {
    //        let self.hwnd_isize: isize = handle.self.hwnd.get();
    //        let possible_hinstance = match handle.hinstance {
    //            Some(hinstance) => {
    //                let hinstance_isize: isize = hinstance.get();
    //                Some(hinstance_isize as winapi::shared::minwindef::HINSTANCE)
    //            }
    //            None => {
    //                // if hinstance is None, then we'll just use the current process handle
    //                None
    //            }
    //        };
    //        (
    //            self.hwnd_isize as winapi::shared::windef::self.hwnd,
    //            possible_hinstance,
    //        )
    //    }
    //    _ => panic!("not running on Windows"),
    //};
    //let sub_builder = WindowBuilder::new().with_title(window_name);
    //let sub_window = sub_builder.build(&winit_event_loop).unwrap();
    //let (_sub_self.hwnd, _possible_sub_hinstance) = match sub_window.window_handle().unwrap().as_raw() {
    //    RawWindowHandle::Win32(handle) => {
    //        let self.hwnd_isize: isize = handle.self.hwnd.get();
    //        let possible_hinstance = match handle.hinstance {
    //            Some(hinstance) => {
    //                let hinstance_isize: isize = hinstance.get();
    //                Some(hinstance_isize as winapi::shared::minwindef::HINSTANCE)
    //            }
    //            None => {
    //                // if hinstance is None, then we'll just use the current process handle
    //                None
    //            }
    //        };
    //        (
    //            self.hwnd_isize as winapi::shared::windef::self.hwnd,
    //            possible_hinstance,
    //        )
    //    }
    //    _ => panic!("not running on Windows"),
    //};
    //// start off with the window hidden in case garbage is displayed
    //window.set_visible(false);
    //sub_window.set_visible(false);

    //if self.hwnd.is_null() {
    //    // Instead of panic!(), we'll just close it cleanly with PostQuitMessage() and log to explain the cause/reasons
    //    println!("Failed to create window.");
    //    unsafe { PostQuitMessage(0) }; // Even if self.hwnd was not created, can we post a quit message?
    //    return;
    //}

    //window.set_visible(true);
    //unsafe {
    //    ShowWindow(self.hwnd, winapi::um::winuser::SW_SHOWDEFAULT);
    //}

    //let mut cursor = CursorData::new();
    //let mut msg = MSG {
    //    self.hwnd: ptr::null_mut(),
    //    message: 0,
    //    wParam: 0,
    //    lParam: 0,
    //    time: 0,
    //    pt: winapi::shared::windef::POINT { x: 0, y: 0 },
    //};

    //// Handle events for both windows
    //// (Implement event handlers as needed)

    //// now show the window
    //window.set_visible(true);
    ////sub_window.set_visible(true);

    //let winit_run_result = //winit_event_loop.run(move |event, _, control_flow| {
    //    winit_event_loop.run(move |winit_event, winit_event_target| {
    //        match winit_event_target.control_flow() {
    //            ControlFlow::Poll => (),
    //            ControlFlow::Wait => (),
    //            ControlFlow::WaitUntil(_instant) => (),
    //        }

    //        match winit_event {
    //            Event::NewEvents(_start_cause) => (),
    //            Event::WindowEvent {
    //                event: window_event,
    //                window_id: _window_id,
    //            } => {
    //                match window_event {
    //                    WindowEvent::CloseRequested => {
    //                        // Close the application when the main window is closed

    //                        // if the main window is closed, then close the sub_window as well
    //                        unsafe { PostQuitMessage(0) };

    //                        // finally, break out of the loop
    //                        winit_event_target.exit();  // exits winit::run() loop, same as loop{break}
    //                    }
    //                    WindowEvent::KeyboardInput {event: key_event, ..} => {
    //                        println!("Key pressed: {:?}", key_event);
    //                        match key_event {
    //                            // Escape key:
    //                            //  KeyEvent { physical_key: Code(Escape), logical_key: Named(Escape), text: None, location: Standard, state: Released, repeat: false, platform_specific: KeyEventExtra { text_with_all_modifers: None, key_without_modifiers: Named(Escape) } }
    //                            KeyEvent {
    //                                state: ElementState::Released,
    //                                physical_key: PhysicalKey::Code(KeyCode::Escape),
    //                                ..
    //                            } => {
    //                                println!("The escape key was pressed; stopping");
    //                                winit_event_target.exit();
    //                            }
    //                            _  /* KeyEvent */=> ()
    //                        }
    //                        match key_event.physical_key{
    //                            Code(KeyCode::Escape) => {
    //                                // Check for the ESCAPE key press and exit the application
    //                                unsafe { PostQuitMessage(0) };
    //                            }
    //                            Code(KeyCode::Space) => {
    //                                // unsure why I need to use unsafe here, but compiler complains if I don't
    //                                unsafe {
    //                                    TOGGLE_STATE = match TOGGLE_STATE {
    //                                        ToggleState::Free => ToggleState::MoveWindow,
    //                                        ToggleState::MoveWindow => ToggleState::Capture, // note that interally, Capture will trnasform to Captured
    //                                        ToggleState::Captured => ToggleState::Free,
    //                                        ToggleState::Capture => {
    //                                            // should never be in this state
    //                                            assert!(false, "unexpected toggle_state");
    //                                            ToggleState::Captured // just return to NEXT expteded state in RELEASE mode...
    //                                        }
    //                                    }
    //                                }
    //                            }
    //                            _ /* Code(KeyCode::*) */ => {
    //                                // do nothing
    //                            }
    //                        }
    //                    }
    //                    WindowEvent::RedrawRequested => {
    //                        // Redraw the application.
    //                        //
    //                        // It's preferable for applications that do not render continuously to render in
    //                        // this event rather than in AboutToWait, since rendering in here allows
    //                        // the program to gracefully handle redraws requested by the OS.
    //                    },
    //                    _ /* WindowEvent::* */ => (),
    //                }

    //            },
    //            Event::DeviceEvent { device_id: _device_id, event: _device_event } => {
    //            }
    //            Event::UserEvent(_user_event) => {
    //                // UserEvent is a custom event that can be sent to the event loop from other threads.
    //            }
    //            Event::Suspended => {
    //                // The application has been suspended.
    //            }
    //            Event::Resumed => {
    //                // The application has been resumed.
    //            }
    //            Event::AboutToWait => {
    //                // Application update code.

    //                // Queue a RedrawRequested event.
    //                //
    //                // You only need to call this if you've determined that you need to redraw in
    //                // applications which do not always need to. Applications that redraw continuously
    //                // can render here instead.
    //                window.request_redraw();
    //            }
    //            Event::LoopExiting => {
    //                // The event loop is about to exit.
    //            }
    //            Event::MemoryWarning => {
    //                // The system is running low on available memory.
    //            }
    //        };  // match winit_event

    //        match winit_event_target {
    //            _ => (),
    //        }

    //        if unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) } == 0 {
    //            //break;
    //            winit_event_target.exit();  // exits winit::run() loop, same as loop{break}
    //        }
    //        unsafe {
    //            TranslateMessage(&msg);
    //            DispatchMessageW(&msg);
    //        }
    //        cursor.update(self.hwnd);

    //        // either left-click or keydown to toggle states
    //        if msg.message == WM_KEYDOWN {
    //            match msg.wParam as std::ffi::c_int {
    //                VK_ESCAPE => {
    //                    // Check for the ESCAPE key press and exit the application
    //                    unsafe { PostQuitMessage(0) };
    //                }
    //                TOGGLE_WINDOW_MOVE_KEY => {
    //                    // unsure why I need to use unsafe here, but compiler complains if I don't
    //                    unsafe {
    //                        TOGGLE_STATE = match TOGGLE_STATE {
    //                            ToggleState::Free => ToggleState::MoveWindow,
    //                            ToggleState::MoveWindow => ToggleState::Capture, // note that interally, Capture will trnasform to Captured
    //                            ToggleState::Captured => ToggleState::Free,
    //                            ToggleState::Capture => {
    //                                // should never be in this state
    //                                assert!(false, "unexpected toggle_state");
    //                                ToggleState::Captured // just return to NEXT expteded state in RELEASE mode...
    //                            }
    //                        }
    //                    }
    //                }
    //                _ => (),
    //            }
    //        } else if msg.message == winapi::um::winuser::WM_LBUTTONUP {
    //            // on left button click RELEASE (as in, it was pressed and now released)
    //            // unsure why I need to use unsafe here, but compiler complains if I don't
    //            unsafe {
    //                TOGGLE_STATE = match TOGGLE_STATE {
    //                    ToggleState::Free => ToggleState::MoveWindow,
    //                    ToggleState::MoveWindow => ToggleState::Capture, // note that interally, Capture will trnasform to Captured
    //                    ToggleState::Captured => ToggleState::Free,
    //                    ToggleState::Capture => {
    //                        // should never be in this state
    //                        assert!(false, "unexpected toggle_state");
    //                        ToggleState::Captured // just return to NEXT expteded state in RELEASE mode...
    //                    }
    //                }
    //            }
    //        }

    //        unsafe {
    //            match TOGGLE_STATE {
    //                ToggleState::Free => capture_and_scale(self.hwnd, cursor),
    //                ToggleState::MoveWindow => {
    //                    // move the window to the cursor position (a sticky window)
    //                    winapi::um::winuser::SetWindowPos(
    //                        self.hwnd,
    //                        ptr::null_mut(),
    //                        cursor.window_x(),
    //                        cursor.window_y(),
    //                        0, // width will be ignored because will use SWP_NOSIZE to retain current size
    //                        0, // height ignored
    //                        winapi::um::winuser::SWP_NOSIZE | winapi::um::winuser::SWP_NOZORDER,
    //                    );
    //                    //// invalidate the window so it can redraw the window onto the Desktop/monitor
    //                    //InvalidateRect(self.hwnd, ptr::null_mut(), 0);
    //                    capture_and_scale(self.hwnd, cursor); // show contents UNDERNEATH the window (will InvalidateRect() so that it'll also redraw the actual window onto the )
    //                }
    //                ToggleState::Capture => {
    //                    // capture the screen and magnify it
    //                    let supported_languages = ocr_langugages.join("+");
    //                    capture_and_ocr(
    //                        self.hwnd,
    //                        &mut ocr,
    //                        cursor,
    //                        &mut ocr_font,
    //                        supported_languages.clone().as_str(),
    //                        &mut interpreter,
    //                    );
    //                    // once it's blitted to that window, stay still..
    //                    TOGGLE_STATE = ToggleState::Captured;
    //                }
    //                ToggleState::Captured => {
    //                    // don't render/update/Invalidate the window, just stay still/frozen until the user toggles the window again
    //                    ()
    //                }
    //            }
    //        }   // unsafe
    //        //} // loop
    //}); // winit_event_loop.run()
    //match winit_run_result {
    //    Ok(_) => println!("{} - Winit event loop exited cleanly", class_name),
    //    Err(e) => {
    //        println!("Error: {:?}", e);
    //    }
    //}
}
