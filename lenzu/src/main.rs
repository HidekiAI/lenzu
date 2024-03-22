extern crate winapi;
mod cursor_data;
mod interpreter_ja;
mod interpreter_traits;
mod ocr_tesseract;
mod ocr_traits;
mod ocr_winmedia;
mod orcr_gcloud;
//use crate::interpreter_traits::InterpreterTrait;
use crate::interpreter_traits::{InterpreterTrait, InterpreterTraitResult};
use crate::ocr_traits::OcrTrait;
use imageproc::drawing::text_size;
// NOTE: if not declared with 'use', won't be able to use Box<dyn crate::ocr_traits::OcrTrait>
use rusttype::{point, Font, PositionedGlyph, Scale, ScaledGlyph};

use cursor_data::CursorData;
// NOTE: We want to use imageproc::image rather than image crate because we want to use imageproc::drawing::draw_text_mut()
use imageproc::{
    drawing::draw_text_mut,
    image::{
        self, imageops::overlay, load_from_memory, ColorType, DynamicImage, GenericImageView,
        GrayAlphaImage, ImageBuffer, Rgba,
    },
};

use ab_glyph::FontRef;
use rusty_tesseract::image::Luma;
use std::io::Read;
use std::{cmp::max, ffi::CString, path::Path, ptr, thread::current};
use winapi::{
    shared::minwindef::BYTE,
    um::{
        gl,
        wingdi::{
            AlphaBlend, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
            GetDIBits, SelectObject, SetDIBits, BITMAPINFO, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
        },
        winuser::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, GetDC, GetMessageW, GetWindowLongW,
            PostQuitMessage, RegisterClassW, ReleaseDC, ShowWindow, TranslateMessage,
            CW_USEDEFAULT, GWL_EXSTYLE, MSG, SW_HIDE, SW_SHOW, VK_ESCAPE, VK_SPACE, WM_KEYDOWN,
            WS_OVERLAPPEDWINDOW,
        },
    },
};

const MAGNIFY_SCALE_FACTOR: u32 = 2;
const TOGGLE_WINDOW_MOVE_KEY: std::ffi::c_int = VK_SPACE;
const DEFAULT_WINDOW_WIDTH: i32 = 1024;
const DEFAULT_WINDOW_HEIGHT: i32 = 768;

const DEFAULT_FONT_SIZE: f32 = 14.0;

enum ToggleState {
    Free,
    MoveWindow,
    Capture,
    Captured, // past-tense
}

static mut TOGGLE_STATE: ToggleState = ToggleState::Free;

fn create_ocr(args: &Vec<String>) -> Box<dyn crate::ocr_traits::OcrTrait> {
    let force_windows_ocr = cfg!(target_os = "windows");
    if force_windows_ocr {
        // even if tesseract is installed, if on Windows, use the most reliable OCR available instead if no arguments are passed
        if args.len() > 1 && args[1] != "--use-winmedia-ocr" {
            return Box::new(ocr_tesseract::OcrTesseract::new()); //  if the first arg is not --use-winmedia-ocr, then use Tesseract
        }
        // just use default windows OCR
        return Box::new(ocr_winmedia::OcrWinMedia::new());
    }
    // Not on Windows, so use Tesseract OCR
    Box::new(ocr_tesseract::OcrTesseract::new()) // default to Tesseract (because even if it unreliable, at least it is cross-platform and can be used on Linux)
}

fn create_interpreter(_args: &Vec<String>) -> Box<dyn crate::interpreter_traits::InterpreterTrait> {
    Box::new(interpreter_ja::InterpreterJa::new())
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

        //    EndPaint(hwnd, &repaint_area);
        //    InvalidateRect(hwnd, ptr::null_mut(), 0); // mark for refresh/update
        //    SelectObject(destination_dc, destination_buffer);
        // Clean up: Select the old bitmap back into the memory DC
        SelectObject(hdc_mem, hbitmap_old);
    }
}

fn capture_and_ocr(
    hwnd: *mut winapi::shared::windef::HWND__,
    ocr: &mut Box<dyn crate::ocr_traits::OcrTrait>,
    cursor_pos: CursorData,
    _supported_lang: &str, // '+' separated list of supported languages(i.e. "jpn+jpn_ver+osd"), note that longer this list, longer it takes to OCR (ie. 10sec/lang so if there are 4 in this list, it can take 40 seconds!)
    interpreter: &mut Box<dyn crate::interpreter_traits::InterpreterTrait>,
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
    let ocr_start_time = std::time::Instant::now();
    let ocr_result = ocr.evaluate(&gray_scale_image);
    let ocr_time = ocr_start_time.elapsed().as_millis();

    // now run kakasi to convert the kanji to hiragana
    // Translate Japanese text to hiragana
    let start_interpreter = std::time::Instant::now();
    let possible_result_tupled = match ocr_result {
        Ok(recognized_result) => {
            println!("OCR Result: '{:?}' {} mSec", recognized_result, ocr_time);
            let possible_translate_result = interpreter.convert(recognized_result.text.as_str());
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
                recognized_image =
                    overlay_text(translate_result.text.as_str(), 0, 0, &recognized_image);
            }
            //println!("Saving debug image: recognized_image.png");
            //recognized_image.save("recognized_image.png").unwrap();

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

fn capture_and_magnify(hwnd: *mut winapi::shared::windef::HWND__, cursor_pos: CursorData) {
    // first, set transparancy of the window to 99% (i.e. almost invisible) using SetLayeredWindowAttributes()
    hide_window(hwnd);

    // now capture the screen
    let screenshot = from_screen_to_image(cursor_pos);

    // show the application window again
    show_window(hwnd);

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

//fn my_draw_text_mut(
//    image: &mut image::RgbImage,
//    color: Rgba<u8>,
//    x: i32,
//    y: i32,
//    scale: Scale,
//    font: &Font,
//    text: &str,
//) {
//    let v_metrics = font.v_metrics(scale);
//    let glyphs: Vec<PositionedGlyph> = font
//        .layout(text, scale, point(x as f32, y as f32 + v_metrics.ascent))
//        .collect();
//
//    for glyph in glyphs {
//        if let Some(bb) = glyph.pixel_bounding_box() {
//            glyph.draw(|gx, gy, gv| {
//                let x = x + gx as i32;
//                let y = y + gy as i32;
//                let alpha = (gv * 255.0) as u8;
//                image.put_pixel(
//                    x as u32,
//                    y as u32,
//                    Rgba([color[0], color[1], color[2], alpha]),
//                );
//            });
//        }
//    }
//}

fn overlay_text(
    text: &str,
    textx: i32,
    texty: i32,
    c_background_image: &DynamicImage,
) -> DynamicImage {
    let mut background_image = c_background_image.clone();
    if text.is_empty() {
        println!("Warning: No text to overlay onto image");
        return background_image; // return back the original cloned (for optimization, make sure to pretest text length before calling here, so we won't even need to clone here)
    }

    let scale = Scale::uniform(2.0 * DEFAULT_FONT_SIZE);
    println!("overlay_text() - Scale: {:?}", scale);

    // first, we need to determine how wide the text is, and if it is wider than the image,
    // we need to break the text down to multiple lines
    let image_char_width = std::cmp::max((background_image.width() / scale.x as u32) - 10u32, 1);
    let mut text_width: u32 = text.len() as u32; // number of characters
    let mut text_height: u32 = 1; // number of lines
    let mut text_width_pixels: u32 = text_width * scale.x as u32;
    let mut text_height_pixels: u32 = text_height * scale.y as u32;
    if text_width > image_char_width {
        // if the text is wider than the image, then we need to break it down to multiple lines
        text_width = image_char_width;
        text_height = (text.len() as u32 / text_width) + 1;
        text_width_pixels = text_width * scale.x as u32;
        text_height_pixels = text_height * scale.y as u32;
    }
    // iterate each char and add newline when we reach text_width
    let binding = text
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if (i as u32 + 1) % text_width == 0 {
                format!("{}\n", c)
            } else {
                format!("{}", c)
            }
        })
        .collect::<String>();
    let multi_lined_text: Vec<&str> = binding.split("\n").collect();

    // Note that imageproc::overlay() only works on same byte depth (i.e. 8-bit, 16-bit, 24-bit, 32-bit, etc.),
    let mut color_type: ColorType = background_image.color();

    // see if we can transform the background_image to  Rgba<u8> IF it is just plain gray scale or other format other than 32-bits
    // if this is not possible, we'll always just convert to Luma<u8>  (lowest comman denominator) but I'd like to have it as 32-bits
    // so that I can have the text in RED...
    if color_type != ColorType::Rgba8 {
        // convert to Rgba<u8>
        let (img_width, img_height) = background_image.dimensions().clone();
        let binding = background_image.into_rgba8().clone();
        let cloned_bg = binding.as_raw().as_slice();
        if ocr_traits::is_valid_png(cloned_bg) {
            println!("overlay_text() - Image is PNG");
        } else {
            println!("overlay_text() - Image is NOT PNG");
        }
        let rgba_image: DynamicImage =
            ocr_traits::to_imageproc_dynamic_image(cloned_bg, img_width, img_height);
        let rgba_image_buffer=  // : ImageBuffer<Rgba<u8>, Vec<u8>> =
            rgba_image.as_rgba8().unwrap();
        // now convert it back to DynamicImge
        let ret = DynamicImage::ImageRgba8(rgba_image_buffer.clone());
        background_image = ret;
        color_type = background_image.color();
    }
    println!("overlay_text() - ColorType: {:?}", color_type);

    // instantiate ImageBuffer matching same type as background_image via creating a sample pixel matching color_type
    let p = image::Rgba([0u8, 0, 0, 0]);
    let mut text_image_canvas = ImageBuffer::from_pixel(text_width_pixels, text_height_pixels, p);

    // Load a font (you can replace this with your own font)
    // Create a font (Meiryo or any other suitable Japanese font)
    let font_data: &[u8] = if cfg!(target_os = "windows") {
        include_bytes!("..\\..\\assets\\fonts\\kochi-mincho-subst.ttf")
    } else {
        include_bytes!("../../assets/fonts/kochi-mincho-subst.ttf")
    };
    //let font: Font<'_> = rusttype::Font::try_from_bytes(font_data).expect("Failed to load font");
    let font: ab_glyph::FontArc = ab_glyph::FontArc::try_from_slice(font_data)
        .expect("Failed to load font 'kochi-mincho-subst.ttf'");
    //let scale = Scale {
    //    x: image.width() as f32 * 0.2,
    //    y: font.scale_for_pixel_height(DEFAULT_FONT_SIZE),
    //};
    //let fscale = font.scale_for_pixel_height(DEFAULT_FONT_SIZE);

    // render text on the text_image_canvas
    println!(
        "Width: {}\nHeight: {}\n{:?}\n\n",
        text_width, text_height, multi_lined_text,
    );

    // Draw each line
    let mut line_y: u32 = 0;
    for (index, line) in multi_lined_text.iter().enumerate() {
        println!(
            "Line {}: Y={} - '{}' ({} chars)",
            index,
            line_y,
            line,
            line.len()
        );
        draw_text_mut(
            &mut text_image_canvas,                // canvas surface
            image::Rgba([0xff, 0x40, 0x40, 0xff]), // font color
            0,
            line_y as i32, // Q: Do we need to shift pixels down?
            scale.y,
            &font,
            &line, // text to render (will come out as blank if the UTF8 is not supported by ttf)
        );
        let (ts_width, ts_height) = text_size(scale.y, &font, line); // Adjust y position for the next line
        line_y += ts_height;
    }

    // overlay the two images.  Bottom (background) image is the original image, and the top (foreground) image is the text
    let mut overlayed_image: ImageBuffer<Rgba<u8>, Vec<u8>> = background_image
        .as_rgba8()
        .expect("Unable to convert bg-image as rgba8")
        .clone();
    let overlay_x: i64 = textx as i64 + scale.x as i64;
    let overlay_y: i64 = texty as i64 + scale.y as i64;
    overlay(
        &mut overlayed_image,
        &mut text_image_canvas,
        overlay_x,
        overlay_y,
    );

    // and finally, convert it back to DynamicImage
    let ret = DynamicImage::ImageRgba8(overlayed_image);
    println!(
        "Overlayed text onto image ({}x{}) - {} chars",
        ret.width(),
        ret.height(),
        text.len()
    );
    //if cfg!(debug_assertions) {
    //    // append first 10 characters of the text to the filename ( replace space with no-space)
    //    let first_10 = &text[0..10]
    //        .replace(" ", "")
    //        .replace(":", "_")
    //        .replace("?", "_")
    //        .replace("\\", "_")
    //        .replace("/", "_");
    //    let filename = format!("overlay_text_{}.png", first_10);
    //    println!("Saving: {}", filename.clone());
    //    match text_image_canvas.save(filename.clone()) {
    //        Ok(_) => println!(
    //            "Saved: {} - {} bytes",
    //            filename.clone(),
    //            text_image_canvas.len()
    //        ),
    //        Err(e) => println!("Error: Could not save {} - {}", filename.clone(), e),
    //    }
    //    let filename = format!("overlayed_text_{}.png", first_10);
    //    match ret.clone().save(filename.clone()) {
    //        Ok(_) => println!(
    //            "Saved: {} - {} bytes",
    //            filename.clone(),
    //            ret.clone().as_bytes().len()
    //        ),
    //        Err(e) => println!("Error: Could not save {} - {}", filename.clone(), e),
    //    }
    //}

    ret
}

#[tokio::main]
async fn main() {
    let args = std::env::args().collect::<Vec<String>>();

    let class_name = "Lenzu";
    let window_name = "Lenzu-OCR";

    // default to Tesseract OCR, but if  --use-winmedia-ocr is passed, then use Windows.Media.Ocr
    let mut ocr = create_ocr(&args);
    let ocr_langugages = ocr.init();
    let mut interpreter = create_interpreter(&args);

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
            DEFAULT_WINDOW_WIDTH,  // CW_USEDEFAULT,
            DEFAULT_WINDOW_HEIGHT, // CW_USEDEFAULT,
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

        // either left-click or keydown to toggle states
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
        } else if msg.message == winapi::um::winuser::WM_LBUTTONUP {
            // on left button click RELEASE (as in, it was pressed and now released)
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

        unsafe {
            match TOGGLE_STATE {
                ToggleState::Free => capture_and_magnify(hwnd, cursor),
                ToggleState::MoveWindow => {
                    // move the window to the cursor position (a sticky window)
                    winapi::um::winuser::SetWindowPos(
                        hwnd,
                        ptr::null_mut(),
                        cursor.window_x(),
                        cursor.window_y(),
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
                    capture_and_ocr(
                        hwnd,
                        &mut ocr,
                        cursor,
                        supported_languages.clone().as_str(),
                        &mut interpreter,
                    );
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

#[cfg(test)]
mod tests {
    use super::*;
    // NOTE: We want to use imageproc::image rather than image crate because we want to use imageproc::drawing::draw_text_mut()
    use imageproc::{
        drawing::draw_text_mut,
        image::{
            self, imageops::overlay, load_from_memory, ColorType, DynamicImage, GenericImageView,
            GrayAlphaImage, ImageBuffer, Rgba,
        },
    };
    use rusttype::{point, Font, Scale, ScaledGlyph};

    #[test]
    fn test_text_over_image() {
        let img_width = 1024;
        let img_height = 768;
        let imgbytes = include_bytes!("../../assets/ubunchu01_02.png");
        let is_valid = ocr_traits::is_valid_png(imgbytes);
        println!("Is valid PNG: {}", is_valid);

        // and turn those bytes into a DynamicImage
        println!("Converting bytes to image...");
        let img: DynamicImage = ocr_traits::to_imageproc_dynamic_image(imgbytes, img_width, img_height);

        println!("Overlaying text onto image...");
        let result_bytes = overlay_text("最近人気のデスクトップ", 0, 0, &img);
        // Now you can use `result_bytes` as needed (e.g., send it over the network, etc.)

        // save it as a file for visual confirmation
        result_bytes.save("test_text_over_image.png").unwrap();
    }

    #[test]
    fn test_draw_text_mut() {
        // Create a new blank image
        let mut img = GrayAlphaImage::new(100, 100);
        let font_data: &[u8] = if cfg!(target_os = "windows") {
            include_bytes!("..\\..\\assets\\fonts\\kochi-mincho-subst.ttf")
        } else {
            include_bytes!("../../assets/fonts/kochi-mincho-subst.ttf")
        };

        // Draw some text onto the image
        let font: ab_glyph::FontArc = ab_glyph::FontArc::try_from_slice(font_data).unwrap();
        draw_text_mut(
            &mut img,                  // canvas surface
            image::LumaA([255, 0x7f]), // font color
            0,                         // font x position
            0,                         // font y position
            24.0,                      // font scale
            &font,
            "最近人気のデスクトップなリナックスです!", // text to draw
        );

        // Convert the image back to raw bytes
        let result_bytes = img.into_raw();
        // Now you can use `result_bytes` as needed (e.g., send it over the network, etc.)

        // save it as a file for visual confirmation
        let img = image::GrayAlphaImage::from_raw(100, 100, result_bytes).unwrap();
        img.save("test_draw_text_mut.png").unwrap();
    }
}
