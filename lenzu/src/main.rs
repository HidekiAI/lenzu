mod capture;
mod interpreter;
mod ocr;
mod cursor_data;

use crate::{glib::clone, interpreter::interpreter_traits::InterpreterTraitResult};
use cursor_data::CursorData;
use gtk4::{
    ffi::{gtk_list_store_append, GtkButton, GtkWidget},
    gdk_pixbuf::Pixbuf,
    gio, glib,
    prelude::*,
    subclass::widget,
    ApplicationWindow, Button, HeaderBar, Image, Orientation, Picture, Widget,
};
// NOTE: Are we using imageproc::ImageBuffer or image::ImageBuffer?
use image::{imageops::overlay, DynamicImage, Rgba, *};
use imageproc::image::{self, GenericImageView, ImageBuffer};

use ocr::{image_handling::OCRImage, ocr_tesseract::OcrTesseract, ocr_traits::OcrTrait, ocr_winmedia::OcrWinMedia};
use interpreter::{interpreter_ja_mecab::InterpreterJaMecab, interpreter_traits::InterpreterTrait};

use std::boxed::Box;
use std::sync::OnceLock;
use std::{ffi::CString, ptr};
use tokio::runtime::Runtime;
use winapi::{
    shared::minwindef::BYTE,
    um::{
        wingdi::{
            BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
            SelectObject, SetDIBits, BITMAPINFO, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
        },
        winuser::{
            DispatchMessageW, GetDC, GetMessageW, GetWindowLongW, InvalidateRect, PostQuitMessage,
            ReleaseDC, ShowWindow, TranslateMessage, GWL_EXSTYLE, MSG, SW_SHOW, VK_ESCAPE,
            VK_SPACE, WM_KEYDOWN,
        },
    },
};

const GTK_APP_ID: &str = "com.github.hidekiai.lenzu";
const GTK_APP_PATH: &str = "/com/github/hidekiai/lenzu/";
const TOGGLE_WINDOW_MOVE_KEY: std::ffi::c_int = VK_SPACE;
const DEFAULT_WINDOW_WIDTH: i32 = 1024;
const DEFAULT_WINDOW_HEIGHT: i32 = 768;

enum ToggleState {
    Free,
    MoveWindow,
    Capture,
    Captured, // past-tense
}

static mut TOGGLE_STATE: ToggleState = ToggleState::Free;

fn create_ocr(args: &Vec<String>) -> std::boxed::Box<dyn OcrTrait> {
    let force_windows_ocr = cfg!(target_os = "windows");
    if force_windows_ocr {
        // even if tesseract is installed, if on Windows, use the most reliable OCR available instead if no arguments are passed
        if args.len() > 1 && args[1] != "--use-winmedia-ocr" {
            return std::boxed::Box::new(OcrTesseract::new()); //  if the first arg is not --use-winmedia-ocr, then use Tesseract
        }
        // just use default windows OCR
        return std::boxed::Box::new(OcrWinMedia::new());
    }
    // Not on Windows, so use Tesseract OCR
    std::boxed::Box::new(OcrTesseract::new()) // default to Tesseract (because even if it unreliable, at least it is cross-platform and can be used on Linux)
}

fn create_interpreter(_args: &Vec<String>) -> std::boxed::Box<dyn InterpreterTrait> {
    std::boxed::Box::new(InterpreterJaMecab::new())
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
            cursor_pos.window_width() as i32,
            cursor_pos.window_height() as i32,
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
            cursor_pos.window_width() as i32,
            cursor_pos.window_height() as i32,
            source_desktop_dc,     // source device context
            cursor_pos.window_x(), // source x - note that coordinate can be negative value (e.g. cursor is on the left side of the PRIMARY monitor)
            cursor_pos.window_y(), // source y
            SRCCOPY,
        );

        // Clean up: Select the OLD bitmap back into the memory DC
        SelectObject(destination_memory_dc, previous_screen_for_restore_dc);

        // At this point, destination_bitmap contains the captured image
        // Create a BITMAPINFO structure to receive the bitmap data
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFO>() as u32;
        info.bmiHeader.biWidth = cursor_pos.window_width() as i32;
        info.bmiHeader.biHeight = -(cursor_pos.window_height() as i32); // top-down bitmap
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32; // each pixel is a 32-bit RGB color
        info.bmiHeader.biCompression = BI_RGB;

        // Allocate a buffer to receive the bitmap data
        let mut data: Vec<BYTE> =
            vec![0; (cursor_pos.window_width() * cursor_pos.window_height() * 4) as usize];

        // Get the bitmap data
        GetDIBits(
            destination_memory_dc,
            destination_bitmap,
            0,
            cursor_pos.window_height(),
            data.as_mut_ptr() as *mut _,
            &mut info,
            DIB_RGB_COLORS,
        );

        // Convert the data to a DynamicImage
        image = ImageBuffer::from_fn(
            cursor_pos.window_width(),
            cursor_pos.window_height(),
            |x, y| {
                let i = ((y * cursor_pos.window_width() + x) * 4) as usize;
                image::Rgba([data[i + 2], data[i + 1], data[i], 255])
            },
        )
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
        // just in case, show window
        ShowWindow(application_window_handle, SW_SHOW);

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

        //EndPaint(hwnd, &repaint_area);
        InvalidateRect(application_window_handle, ptr::null_mut(), 0); // mark for refresh/update

        // Clean up: Select the old bitmap back into the memory DC
        SelectObject(hdc_mem, hbitmap_old);
    }
}

fn capture_and_ocr(
    hwnd: *mut winapi::shared::windef::HWND__,
    ocr: &mut std::boxed::Box<dyn OcrTrait>,
    cursor_pos: CursorData,
    ocr_font: &mut OCRImage,
    _supported_lang: &str, // '+' separated list of supported languages(i.e. "jpn+jpn_ver+osd"), note that longer this list, longer it takes to OCR (ie. 10sec/lang so if there are 4 in this list, it can take 40 seconds!)
    interpreter: &mut std::boxed::Box<dyn InterpreterTrait>,
) {
    // first, set transparancy of the window to 99% (i.e. almost invisible) using SetLayeredWindowAttributes()
    hide_window(hwnd);

    // now capture the screen
    let screenshot = from_screen_to_image(cursor_pos);

    // show the application window again
    show_window(hwnd);

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
            from_image_to_window(hwnd, recognized_image);
        }
        None => {
            println!(
                "Interpreter Result ({} mSec): '{:?}'",
                start_interpreter.elapsed().as_millis(),
                possible_result_tupled
            );
            // render what we've captured originally instead
            from_image_to_window(hwnd, screenshot);
        }
    }
}

// In order to now get the mirror-effect, we have to hide the window, capture the screen, show the window, then render the captured screen
// unfortunately, "hide" isn't based on ShowWindow(SW_HIDE) because that effect is similar/same as when the window is minimized, and
// you completely loose control of the window (i.e. you cannot move window, nor will hitting the ESCAPE key work because the window is NOT in focus!)
// Hence, when we "hide" the window, it actually is more like setting the transparancy of the window to 99% (i.e. almost invisible)
fn hide_window(hwnd: *mut winapi::shared::windef::HWND__) {
    unsafe {
        let current_flags = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_flags: i32 = (winapi::um::winuser::WS_EX_LAYERED as i32 | current_flags)
            .try_into()
            .unwrap();
        // have to turn ON the Layered bit first...
        winapi::um::winuser::SetWindowLongW(hwnd, winapi::um::winuser::GWL_EXSTYLE, new_flags);
        // now set it to 1%
        winapi::um::winuser::SetLayeredWindowAttributes(
            hwnd,
            0,
            (((1u32 * 255u32) / 100u32) & 0xFF) as u8, // 100% transparancy
            winapi::um::winuser::LWA_ALPHA,
        );
    }
}
fn show_window(hwnd: *mut winapi::shared::windef::HWND__) {
    unsafe {
        let current_flags = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_flags: i32 = (winapi::um::winuser::WS_EX_LAYERED as i32 & !current_flags)
            .try_into()
            .unwrap();
        // have to turn OFF the layered bit
        winapi::um::winuser::SetWindowLongW(hwnd, winapi::um::winuser::GWL_EXSTYLE, new_flags);
        // now set it to 100%
        winapi::um::winuser::SetLayeredWindowAttributes(
            hwnd,
            0,
            (((100u32 * 255u32) / 100u32) & 0xFF) as u8, // 100% transparancy
            winapi::um::winuser::LWA_ALPHA,
        );
    }
}

fn capture_and_scale(hwnd: *mut winapi::shared::windef::HWND__, cursor_pos: CursorData) {
    // first, set transparancy of the window to 99% (i.e. almost invisible) using SetLayeredWindowAttributes()
    hide_window(hwnd);

    // now capture the screen
    let screenshot = from_screen_to_image(cursor_pos);

    // show the application window again
    show_window(hwnd);

    // now render what we've captured
    from_image_to_window(hwnd, screenshot);
}

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("Setting up tokio runtime needs to succeed."))
}

fn main() -> glib::ExitCode {
    let application = gtk4::Application::builder()
        .application_id(GTK_APP_ID)
        .build();
    application.connect_activate(build_ui);
    application.run()
}

fn build_ui(application: &gtk4::Application) {
    // default to Tesseract OCR, but if  --use-winmedia-ocr is passed, then use Windows.Media.Ocr
    let args: &Vec<String> = &std::env::args().collect();
    let mut ocr = create_ocr(&args);
    let ocr_langugages = ocr.init();
    let mut interpreter = create_interpreter(&args);
    let mut ocr_font = OCRImage::new(None);

    let app_window_gtk: ApplicationWindow = gtk4::ApplicationWindow::builder()
        .application(application)
        .title("Lenzu")
        .default_width(DEFAULT_WINDOW_WIDTH)
        .default_height(DEFAULT_WINDOW_HEIGHT)
        .build();
    let window_scrollable = gtk4::ScrolledWindow::builder().build();
    window_scrollable.set_visible(true);
    window_scrollable.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    app_window_gtk.set_child(Some(&window_scrollable));

    // container to append multiple children
    let parent_box = gtk4::Box::new(Orientation::Vertical, 0);

    // as Picture
    let image_path = "recognized_image.png"; // if it exists, load last used image
    let picture = Picture::for_filename(image_path);
    let pic_paintable_dim = match picture.paintable() {
        Some(paintable) => (paintable.intrinsic_width(), paintable.intrinsic_height()),
        None => (1024, 768),
    };
    println!(
        "Loaded image '{}' with dimensions: {:?}",
        image_path, pic_paintable_dim
    );
    picture.set_halign(gtk4::Align::Center);
    picture.set_size_request(pic_paintable_dim.0, pic_paintable_dim.1);
    picture.set_visible(true);
    parent_box.append(&picture);

    // as Image
    let image = Image::from_file(image_path);
    let img_paintable_dim = match image.paintable() {
        Some(paintable) => (paintable.intrinsic_width(), paintable.intrinsic_height()),
        None => (0, 0),
    };
    println!(
        "Loaded image '{}' with dimensions: {:?}",
        image_path, img_paintable_dim
    );
    image.set_halign(gtk4::Align::Center);
    image.set_size_request(img_paintable_dim.0, img_paintable_dim.1);
    image.set_visible(true);
    //parent_box.append(&image);

    let (sender_quit_signal, receiver_quit_signal) = async_channel::bounded(1);

    // Create a button with label and margins
    let button_quit: Button = Button::builder()
        .label("Quit")
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .width_request(16 * 16)
        .height_request(16)
        .halign(gtk4::Align::End) // anchor to bottom right
        .valign(gtk4::Align::End)
        .build();
    button_quit.connect_clicked(move |_| {
        println!("Signal quitting...");
        runtime().spawn(clone!(@strong sender_quit_signal  =>async move {
            sender_quit_signal .send(true) .await.expect("Signal channel is unopenend");
        }));
        println!("Signal sent to quit...");
    });
    parent_box.append(&button_quit);
    glib::spawn_future_local(clone!(@weak button_quit => async move {
        while let Ok(quit_signaled) = receiver_quit_signal.recv().await {
            if quit_signaled {
                button_quit.set_label("Quitting...");

                // quit applications
                std::process::exit(0); // for now, brute-force quit, in future will elegantly signal for exit...
                //break;
            }
        }
    }));

    // all is attached to parent_box, now attach itself to window
    //app_window_gtk.set_child(Some(&parent_box));
    window_scrollable.set_child(Some(&parent_box));
    app_window_gtk.show();
    app_window_gtk.set_visible(true);
    app_window_gtk.present(); // mark (child scene-graph nodes) for refresh

    //let main_builder = WindowBuilder::new().with_title(class_name);
    //let window: Window = main_builder.build(&winit_event_loop).unwrap();
    //let (hwnd, _possible_h_instance) = match window.window_handle().unwrap().as_raw() {
    //    RawWindowHandle::Win32(handle) => {
    //        let hwnd_isize: isize = handle.hwnd.get();
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
    //            hwnd_isize as winapi::shared::windef::HWND,
    //            possible_hinstance,
    //        )
    //    }
    //    _ => panic!("not running on Windows"),
    //};
    //let sub_builder = WindowBuilder::new().with_title(window_name);
    //let sub_window = sub_builder.build(&winit_event_loop).unwrap();
    //let (_sub_hwnd, _possible_sub_hinstance) = match sub_window.window_handle().unwrap().as_raw() {
    //    RawWindowHandle::Win32(handle) => {
    //        let hwnd_isize: isize = handle.hwnd.get();
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
    //            hwnd_isize as winapi::shared::windef::HWND,
    //            possible_hinstance,
    //        )
    //    }
    //    _ => panic!("not running on Windows"),
    //};
    //// start off with the window hidden in case garbage is displayed
    //window.set_visible(false);
    //sub_window.set_visible(false);

    //if hwnd.is_null() {
    //    // Instead of panic!(), we'll just close it cleanly with PostQuitMessage() and log to explain the cause/reasons
    //    println!("Failed to create window.");
    //    unsafe { PostQuitMessage(0) }; // Even if HWND was not created, can we post a quit message?
    //    return;
    //}

    //window.set_visible(true);
    //unsafe {
    //    ShowWindow(hwnd, winapi::um::winuser::SW_SHOWDEFAULT);
    //}

    //let mut cursor = CursorData::new();
    //let mut msg = MSG {
    //    hwnd: ptr::null_mut(),
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
    //        cursor.update(hwnd);

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
    //                ToggleState::Free => capture_and_scale(hwnd, cursor),
    //                ToggleState::MoveWindow => {
    //                    // move the window to the cursor position (a sticky window)
    //                    winapi::um::winuser::SetWindowPos(
    //                        hwnd,
    //                        ptr::null_mut(),
    //                        cursor.window_x(),
    //                        cursor.window_y(),
    //                        0, // width will be ignored because will use SWP_NOSIZE to retain current size
    //                        0, // height ignored
    //                        winapi::um::winuser::SWP_NOSIZE | winapi::um::winuser::SWP_NOZORDER,
    //                    );
    //                    //// invalidate the window so it can redraw the window onto the Desktop/monitor
    //                    //InvalidateRect(hwnd, ptr::null_mut(), 0);
    //                    capture_and_scale(hwnd, cursor); // show contents UNDERNEATH the window (will InvalidateRect() so that it'll also redraw the actual window onto the )
    //                }
    //                ToggleState::Capture => {
    //                    // capture the screen and magnify it
    //                    let supported_languages = ocr_langugages.join("+");
    //                    capture_and_ocr(
    //                        hwnd,
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

#[cfg(test)]
mod tests {
    use super::*;
    // NOTE: We want to use imageproc::image rather than image crate because we want to use imageproc::drawing::draw_text_mut()
    use imageproc::{
        drawing::draw_text_mut,
        image::{self, GrayAlphaImage},
    };

    #[test]
    fn test_text_over_image() {
        let mut ocr_image = OCRImage::new(None);
        ocr_image.load_image("../assets/ubunchu01_02.png").unwrap();
        let is_valid = ocr_image.is_png();
        println!("Is valid PNG: {}", is_valid);

        // and turn those bytes into a DynamicImage
        println!("Overlaying text onto image...");
        let result_bytes =
            ocr_image.overlay_text("最近人気の\nデスクトップな\nリナックスです!", 0, 0);
        // Now you can use `result_bytes` as needed (e.g., send it over the network, etc.)

        // save it as a file for visual confirmation
        result_bytes.save("test_text_over_image.png").unwrap();
    }

    #[test]
    fn test_draw_text_mut() {
        // Create a new blank image
        let ocr_image = OCRImage::from(GrayAlphaImage::new(1024, 768));
        //let mut img: ImageBuffer<image::LumaA<u8>, Vec<u8>> = GrayAlphaImage::new(1024, 768);
        //let img = ImageBuffer::from(ocr_image.get_image().to_luma8());
        let mut canvas = ocr_image.get_image().to_luma_alpha8();

        // Draw some text onto the image
        draw_text_mut(
            &mut canvas,
            image::LumaA([255, 0x7f]), // font color
            0,                         // font x position
            0,                         // font y position
            24.0,                      // font scale
            &ocr_image.get_font_bold(),
            "最近人気の\nデスクトップな\nリナックスです!", // text to draw
        );

        let img =
            image::GrayAlphaImage::from_raw(canvas.width(), canvas.height(), canvas.into_raw())
                .unwrap();
        img.save("test_draw_text_mut.png").unwrap();
    }
}
