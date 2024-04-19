use std::ptr;

use super::capture_traits::*;
use gdk4_win32::Win32Surface;
use gtk4::{
    ffi::gtk_native_get_surface,
    gdk::{Backend, Surface},
    gio,
    prelude::*,
    Widget,
};
//use gtk4::gio::list_model::ListModelMutatedDuringIter;
use image::{DynamicImage, GenericImageView, ImageBuffer, RgbaImage};
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
            EnumWindows, GetCursorPos, GetDC, GetMonitorInfoW, GetWindowLongW, GetWindowRect, GetWindowTextW, InvalidateRect, MonitorFromPoint, PostQuitMessage, ReleaseDC, ShowWindow, GWL_EXSTYLE, MONITORINFO, MONITOR_DEFAULTTONEAREST, SW_SHOW
        },
    },
};

pub struct CaptureWinApi {
    pub cursor_data: CursorData,
    pub hwnd: winapi::shared::windef::HWND,
    pub handle: HandleTypes,
}

impl CaptureTrait for CaptureWinApi {
    fn new() -> Self
    where
        Self: Sized,
    {
        CaptureWinApi {
            cursor_data: CursorData::new(),
            hwnd: std::ptr::null_mut(), // use set_hwnd
            handle: HandleTypes::Win32Handle(gdk4_win32::HWND::default()),
        }
    }

    fn init(&mut self, app_window_gtk: &gtk4::ApplicationWindow) -> bool {
        self.from_gtk4(app_window_gtk);
        true
    }

    fn capture(
        &mut self,
        possible_rect: Option<CaptureRect>,
    ) -> Result<image::DynamicImage, anyhow::Error> {
        match possible_rect {
            Some(rect) => self.cursor_data.window = rect,
            None => (),
        }

        // first, set transparancy of the window to 99% (i.e. almost invisible) using SetLayeredWindowAttributes()
        self.hide_window();

        // now capture the screen
        let screenshot = self.capture_from_screen_to_image(self.cursor_data);
        self.render(screenshot);

        // show the application window again
        self.show_window();

        todo!("Not implemented yet")
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
        self.cursor_data.monitor.x = monitor_info.rcWork.left; // can be negative, based on being placed LEFT of the PRIMARY monitor
        self.cursor_data.monitor.y = monitor_info.rcWork.top;
        self.cursor_data.monitor.width =
            std::cmp::max(monitor_info.rcWork.right - monitor_info.rcWork.left, 1024) as u32;
        self.cursor_data.monitor.height =
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
            GetWindowRect(self.hwnd, &mut window_rect);
        }
        self.cursor_data.window.width = (window_rect.right - window_rect.left) as u32;
        self.cursor_data.window.height = (window_rect.bottom - window_rect.top) as u32;
        // window position (upper left corner) is recalculated based off of cursor (x,y) and upper-left is offset by center of window to be where the mouse cursor will be
        // the tricky part of this is that the windows position coordinate can be negative (same as cursor position) so it's not possible to test for min()/max() for
        // edge of the monitor, and so we'll not do snap to monitor and allow windows to get beyond the edges of the monitors
        self.cursor_data.window.x =
            self.cursor_data.x - (self.cursor_data.window.width as i32 / 2) as i32;
        self.cursor_data.window.y =
            self.cursor_data.y - (self.cursor_data.window.height as i32 / 2) as i32;
    }

    fn render(&mut self, image: DynamicImage) {
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
}

impl CaptureWinApi {
    pub fn set_hwnd(&mut self, hwnd: winapi::shared::windef::HWND) {
        self.hwnd = hwnd;
        self.handle = HandleTypes::Win32Handle(gdk4_win32::HWND(hwnd as isize));
    }

    // On a splucations based on:
    // * get_first_child(): https://docs.gtk.org/gtk4/method.Widget.get_first_child.html
    // * get_next_sibling(): https://docs.gtk.org/gtk4/method.Widget.get_next_sibling.html
    // You first get the first child of the parent, then iterate through the siblings
    // and return list of Win32Surfaces
    fn find_surfaces(&self, app_window_gtk: &gtk4::ApplicationWindow) -> Vec<Win32Surface> {
        let mut surfaces: Vec<Win32Surface> = Vec::new();
        // see if ApplicationWindow itself has a surface (doubtful but just in case)
        if app_window_gtk.surface().is_some() {
            let win32_surface_result = app_window_gtk.surface().unwrap().downcast::<Win32Surface>();
            if win32_surface_result.is_ok() {
                println!("Found Win32Surface in the application window");
                surfaces.push(win32_surface_result.unwrap());
            }
        }

        let first_child = app_window_gtk.first_child();
        if first_child.is_none() {
            println!("No children found in the application window");
            panic!("GDK4 Surface will NOT be found if you call init() before the window is shown/realized/presented!");
            return surfaces;
        }
        let mut child = first_child.unwrap();
        loop {
            let possible_native = child.native();
            if possible_native.is_none() {
                // see if next sibling has a native
                let possible_widget = child.next_sibling();
                if possible_widget.is_none() {
                    break;
                }
                child = possible_widget.unwrap();
                continue;
            }
            let possible_surface = possible_native.unwrap().surface();
            if possible_surface.is_none() {
                // see if next sibling has a surface
                let possible_widget = child.next_sibling();
                if possible_widget.is_none() {
                    break;
                }
                child = possible_widget.unwrap();
                continue;
            }
            let win32_surface_result = possible_surface.unwrap().downcast::<Win32Surface>();
            if win32_surface_result.is_ok() {
                println!("Found Win32Surface in a child widget");
                surfaces.push(win32_surface_result.unwrap());
            }
            let possible_widget = child.next_sibling();
            if possible_widget.is_none() {
                break;
            }
            child = possible_widget.unwrap();
        } // loop
        surfaces
    }

    // use this method so so that the application can remain agnostic to the platform
    fn from_gtk4(&mut self, app_window_gtk: &gtk4::ApplicationWindow) {
        // using gtk4 gdk_win32_window_get_handle to get the HWND seems to be the practice used by OpenGL users
        // now that all the renderable widgets are appended to parent_box, we can inspect display and surface
        let display: gtk4::gdk::Display = app_window_gtk.clone().upcast::<Widget>().display();
        let backend: Backend = display.backend();

        // using GtkNative, we can get the surface via gtk_native_get_surface(), but not all widgets have a native
        // so we'll take the first native we can find that has a surface
        let surfaces = self.find_surfaces(app_window_gtk);
        if surfaces.is_empty() {
            panic!("No Win32Surface found in the application window and its children");
        }

        // Try to find the first Win32Surface that has a valid HWND
        let win_surfaces = surfaces
            .iter()
            .flat_map(|current_surface| {
                match current_surface.handle() != gdk4_win32::HWND::default() {
                    true => Some(current_surface),
                    false => None,
                }
            })
            .collect::<Vec<&Win32Surface>>();
        let win_surface = win_surfaces[0];

        // get GDK display monitor dimensions as well as the window dimensions
        let monitor = match display.monitor_at_surface(win_surface) {
            Some(monitor) => monitor,
            None => panic!("Monitor is not available"),
        };
        if self.hwnd.is_null() {
            // NOTE: Handles are mainly for the purpose of Windows and/or X11 specific desktop libraries
            //       that needs the surface for rendering (ie. without handles, you cannot screen-capture)
            let my_handle: HandleTypes = if cfg!(target_os = "windows") {
                // Windows
                if backend.is_win32() {
                    let gdk_win32_hwnd: gdk4_win32::HWND = win_surface.handle(); // am I the only who thinks this was a bit too complicated to get the HWND?
                    let win32_handle = gdk_win32_hwnd.0 as winapi::shared::windef::HWND;
                    // for verification, let's locate the "Title" of the HWND window
                    let mut title = [0u16; 1024];
                    let title_len = unsafe {
                        winapi::um::winuser::GetWindowTextW(
                            win32_handle,
                            title.as_mut_ptr(),
                            title.len() as i32,
                        )
                    };
                    // have to make sure to null-terminate AND trim all the nulls
                    let title = String::from_utf16(&title[..title_len as usize])
                        .unwrap()
                        .trim_matches(char::from(0))
                        .to_string();
                    println!("HWND Title: {}", title);
                    self.hwnd = win32_handle;
                    HandleTypes::Win32Handle(gdk_win32_hwnd)
                } else {
                    panic!("Windows backend is not win32");
                }
            } else {
                // Linux
                todo!("Linux not supported yet");
            };
            self.handle = my_handle;

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
        } // if hwnd is NULL

        let monitor_rect = monitor.geometry();
        self.cursor_data.monitor.x = monitor_rect.x();
        self.cursor_data.monitor.y = monitor_rect.y();
        self.cursor_data.monitor.width = monitor_rect.width() as u32;
        self.cursor_data.monitor.height = monitor_rect.height() as u32;
        self.cursor_data.window.width = app_window_gtk.width() as u32;
        self.cursor_data.window.height = app_window_gtk.height() as u32;
        self.cursor_data.window.x = 0; //app_window_gtk.x();
        self.cursor_data.window.y = 0; //app_window_gtk.y();
    }

    // NOTE: Make sure to call ShowWindow(hwnd, SW_IDE) prior to calling this method and ShowWindow(hwnd, SW_SHOW) after image is captured
    // this is so that we do not get the image-echo effect (like a mirror reflecting a mirror) when we capture the screen
    // will need to experiment, but it seems we do not need to invalidate since ShowWindow() will implicitly refresh window
    pub fn capture_from_screen_to_image(&self, cursor_pos: CursorData) -> DynamicImage {
        // first, get DC of the entire desktop (hence we do not need HWND passed here) via calling GetDC(NULL) - NULL means the entire desktop
        let source_desktop_dc = unsafe { GetDC(ptr::null_mut()) };

        // Create a compatible device context and bitmap
        let destination_memory_dc = unsafe { CreateCompatibleDC(source_desktop_dc) };
        let destination_bitmap = unsafe {
            CreateCompatibleBitmap(
                source_desktop_dc,
                cursor_pos.window.width as i32,
                cursor_pos.window.height as i32,
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
                cursor_pos.window.width as i32,
                cursor_pos.window.height as i32,
                source_desktop_dc,   // source device context
                cursor_pos.window.x, // source x - note that coordinate can be negative value (e.g. cursor is on the left side of the PRIMARY monitor)
                cursor_pos.window.y, // source y
                SRCCOPY,
            );

            // Clean up: Select the OLD bitmap back into the memory DC
            SelectObject(destination_memory_dc, previous_screen_for_restore_dc);

            // At this point, destination_bitmap contains the captured image
            // Create a BITMAPINFO structure to receive the bitmap data
            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFO>() as u32;
            info.bmiHeader.biWidth = cursor_pos.window.width as i32;
            info.bmiHeader.biHeight = -(cursor_pos.window.height as i32); // top-down bitmap
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32; // each pixel is a 32-bit RGB color
            info.bmiHeader.biCompression = BI_RGB;

            // Allocate a buffer to receive the bitmap data
            let mut data: Vec<BYTE> =
                vec![0; (cursor_pos.window.width * cursor_pos.window.height * 4) as usize];

            // Get the bitmap data
            GetDIBits(
                destination_memory_dc,
                destination_bitmap,
                0,
                cursor_pos.window.height,
                data.as_mut_ptr() as *mut _,
                &mut info,
                DIB_RGB_COLORS,
            );

            // Convert the data to a DynamicImage
            image =
                ImageBuffer::from_fn(cursor_pos.window.width, cursor_pos.window.height, |x, y| {
                    let i = ((y * cursor_pos.window.width + x) * 4) as usize;
                    image::Rgba([data[i + 2], data[i + 1], data[i], 255])
                })
                .into(); // At this point, image is a DynamicImage containing the bitmap image

            DeleteDC(destination_memory_dc);
            ReleaseDC(ptr::null_mut(), source_desktop_dc);
            DeleteObject(destination_bitmap as *mut winapi::ctypes::c_void);
        };
        image
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
