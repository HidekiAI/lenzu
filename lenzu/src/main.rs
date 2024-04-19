mod capture;
mod interpreter;
mod ocr;

use crate::{glib::clone, interpreter::interpreter_traits::InterpreterTraitResult};
use capture::capture_traits::CursorData;
use capture::capture_winapi;
use gdk4_win32::{
    ffi::{gdk_win32_surface_get_impl_hwnd, GdkWin32Surface},
    Win32Surface, HWND,
};
use gtk4::{
    ffi::{gtk_list_store_append, GtkButton, GtkWidget},
    gdk::{self, Backend, Display},
    gdk_pixbuf::Pixbuf,
    gio, glib,
    prelude::*,
    subclass::widget,
    ApplicationWindow, Button, HeaderBar, Image, Orientation, Picture, Widget,
};

// NOTE: Are we using imageproc::ImageBuffer or image::ImageBuffer?
use image::{imageops::overlay, DynamicImage, Rgba, *};
use imageproc::image::{self, GenericImageView, ImageBuffer};

use interpreter::{interpreter_ja_mecab::InterpreterJaMecab, interpreter_traits::InterpreterTrait};
use ocr::{
    image_handling::OCRImage, ocr_tesseract::OcrTesseract, ocr_traits::OcrTrait,
    ocr_winmedia::OcrWinMedia,
};

use std::boxed::Box;
use std::sync::OnceLock;
use std::{ffi::CString, ptr};
use tokio::runtime::Runtime;

const GTK_APP_ID: &str = "com.github.hidekiai.lenzu";
const GTK_APP_PATH: &str = "/com/github/hidekiai/lenzu/";
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

fn capture_and_ocr(
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

enum HandleTypes {
    Win32Handle(gdk4_win32::HWND),
    //Xwin( gdk4_wayland::HANDLE),
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

    // now that all the renderable widgets are appended to parent_box, we can inspect display and surface
    let as_widget = app_window_gtk.clone().upcast::<Widget>();
    let display: gtk4::gdk::Display = as_widget.display();
    let backend: Backend = display.backend();
    let possible_surface = app_window_gtk.surface();
    // NOTE: Handles are mainly for the purpose of Windows and/or X11 specific desktop libraries
    //       that needs the surface for rendering (ie. without handles, you cannot screen-capture)
    let my_handle: HandleTypes = if cfg!(target_os = "windows") {
        // Windows
        if backend.is_win32() {
            let mut win_surface: Win32Surface = match possible_surface {
                Some(surface) => {
                    let win_surface = surface.downcast::<Win32Surface>().unwrap();
                    win_surface
                }
                None => panic!("Surface is not available"),
            };
            let win32_hwnd = win_surface.handle();  // am I the only who thinks this was a bit too complicated to get the HWND?
            HandleTypes::Win32Handle(win32_hwnd)
        } else {
            panic!("Windows backend is not win32");
        }
    } else {
        // Linux
        todo!("Linux not supported yet");
    };

    // all is attached to parent_box, now attach itself to window
    //app_window_gtk.set_child(Some(&parent_box));
    window_scrollable.set_child(Some(&parent_box));
    app_window_gtk.set_visible(true);
    app_window_gtk.present(); // mark (child scene-graph nodes) for refresh
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
