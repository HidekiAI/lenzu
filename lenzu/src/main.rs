use arboard::Clipboard;
use base64::{engine::general_purpose, Engine as _};
use gdk::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use pango;
use pangocairo;
use serde_json::json;
use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::Write;
use std::rc::Rc;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

// --- CONFIGURATION ---
const LENS_SIZE: i32 = 400;
const UI_PANEL_HEIGHT: i32 = 130;
const HISTORY_PATH: &str = "/dev/shm/ocr_history.txt";
const DEBUG_IMAGE_PATH: &str = "/dev/shm/debug_lens.png";

struct AppState {
    pixels: Option<gdk_pixbuf::Pixbuf>,
    ocr_result: String,
    status: String,
    last_capture: Instant,
    clipboard: Clipboard,
    api_key: String,
    is_loading: bool,
    spinner_angle: f64,
    flash_alpha: f64, // New: for the camera flash effect
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .expect("ERROR: OPENROUTER_API_KEY environment variable not set!");

    gtk::init().expect("Failed to initialize GTK.");

    let state = Rc::new(RefCell::new(AppState {
        pixels: None,
        ocr_result: String::new(),
        status: "READY: Shift+Click | ESC to Quit".to_string(),
        last_capture: Instant::now() - Duration::from_secs(2),
        clipboard: Clipboard::new().expect("Failed to init clipboard"),
        api_key,
        is_loading: false,
        spinner_angle: 0.0,
        flash_alpha: 0.0,
    }));

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(LENS_SIZE, LENS_SIZE + UI_PANEL_HEIGHT);
    window.set_decorated(false);
    window.set_keep_above(true);
    window.set_app_paintable(true);

    if let Some(screen) = gtk::prelude::WidgetExt::screen(&window) {
        if let Some(visual) = screen.rgba_visual() {
            window.set_visual(Some(&visual));
        }
    }

    window.connect_key_press_event(|_, event| {
        if event.keyval() == gdk::keys::constants::Escape {
            gtk::main_quit();
        }
        glib::Propagation::Proceed
    });

    let (tx, rx) = glib::MainContext::channel(glib::Priority::default());

    let state_draw = state.clone();
    window.connect_draw(move |win, cr| {
        let s = state_draw.borrow();

        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.set_operator(cairo::Operator::Source);
        cr.paint().ok();
        cr.set_operator(cairo::Operator::Over);

        if let Some(ref pb) = s.pixels {
            cr.set_source_pixbuf(pb, 0.0, 0.0);
            cr.paint().ok();
        }

        // --- CAMERA FLASH EFFECT ---
        if s.flash_alpha > 0.0 {
            cr.set_source_rgba(1.0, 1.0, 1.0, s.flash_alpha);
            cr.rectangle(0.0, 0.0, LENS_SIZE as f64, LENS_SIZE as f64);
            cr.fill().ok();
        }

        // Cyan Border
        cr.set_source_rgb(0.0, 1.0, 0.8);
        cr.set_line_width(2.0);
        cr.rectangle(1.0, 1.0, (LENS_SIZE - 2) as f64, (LENS_SIZE - 2) as f64);
        cr.stroke().ok();

        // UI Panel
        cr.set_source_rgba(0.01, 0.01, 0.05, 0.85);
        cr.rectangle(
            0.0,
            LENS_SIZE as f64,
            LENS_SIZE as f64,
            UI_PANEL_HEIGHT as f64,
        );
        cr.fill().ok();

        let context = win.pango_context();
        let layout = pango::Layout::new(&context);

        cr.set_source_rgb(0.0, 1.0, 0.8);
        layout.set_text(&s.status);
        cr.move_to(12.0, (LENS_SIZE + 10) as f64);
        pangocairo::show_layout(cr, &layout);

        if s.is_loading {
            cr.save().ok();
            cr.translate((LENS_SIZE - 30) as f64, (LENS_SIZE + 20) as f64);
            cr.rotate(s.spinner_angle);
            cr.set_line_width(3.0);
            cr.set_source_rgb(0.0, 1.0, 0.8);
            cr.arc(0.0, 0.0, 8.0, 0.0, 1.5 * std::f64::consts::PI);
            cr.stroke().ok();
            cr.restore().ok();
        }

        cr.set_source_rgb(1.0, 1.0, 1.0);
        let font_desc = pango::FontDescription::from_string("Sans Bold 13");
        layout.set_font_description(Some(&font_desc));
        layout.set_text(&s.ocr_result);
        layout.set_width(pango::units_from_double((LENS_SIZE - 24) as f64));
        layout.set_ellipsize(pango::EllipsizeMode::End);
        cr.move_to(12.0, (LENS_SIZE + 40) as f64);
        pangocairo::show_layout(cr, &layout);

        glib::Propagation::Proceed
    });

    let state_rx = state.clone();
    let window_rx = window.clone();
    rx.attach(None, move |api_result: Result<String, String>| {
        let mut s = state_rx.borrow_mut();
        s.is_loading = false;
        match api_result {
            Ok(text) => {
                s.ocr_result = text.clone();
                s.status = "SUCCESS".to_string();
                let _ = s.clipboard.set_text(text.clone());
                if let Ok(mut f) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(HISTORY_PATH)
                {
                    let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%H:%M:%S"), text);
                }
            }
            Err(e) => s.status = format!("API Error: {}", e),
        }
        window_rx.queue_draw();
        glib::ControlFlow::Continue
    });

    // Animation timer for Spinner AND Flash Fade
    let window_anim = window.clone();
    let state_anim = state.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        let mut s = state_anim.borrow_mut();
        let mut needs_redraw = false;

        if s.is_loading {
            s.spinner_angle += 0.2;
            needs_redraw = true;
        }

        if s.flash_alpha > 0.0 {
            s.flash_alpha -= 0.1; // Fast fade-out
            if s.flash_alpha < 0.0 {
                s.flash_alpha = 0.0;
            }
            needs_redraw = true;
        }

        if needs_redraw {
            window_anim.queue_draw();
        }
        glib::ControlFlow::Continue
    });

    let window_main = window.clone();
    let state_main = state.clone();

    glib::timeout_add_local(Duration::from_millis(16), move || {
        let display = gdk::Display::default().unwrap();
        let seat = display.default_seat().unwrap();
        let device = seat.pointer().unwrap();
        let screen = gdk::Screen::default().unwrap();
        let root_win = screen.root_window().unwrap();
        let (_, x, y, modifier) = root_win.device_position(&device);

        let win_x = x - (LENS_SIZE / 2);
        let win_y = y - (LENS_SIZE / 2);
        window_main.move_(win_x, win_y);

        if modifier.contains(gdk::ModifierType::SHIFT_MASK)
            && modifier.contains(gdk::ModifierType::BUTTON1_MASK)
        {
            let mut s = state_main.borrow_mut();
            if s.last_capture.elapsed() > Duration::from_secs(1) && !s.is_loading {
                s.last_capture = Instant::now();
                s.status = "CAPTURING...".to_string();
                s.is_loading = true;
                s.flash_alpha = 1.0; // TRIGGER FLASH
                window_main.queue_draw();

                window_main.hide();
                while gtk::events_pending() {
                    gtk::main_iteration();
                }
                std::thread::sleep(Duration::from_millis(400));

                if let Ok(raw) = capture_x11(
                    win_x.max(0),
                    win_y.max(0),
                    LENS_SIZE as u32,
                    LENS_SIZE as u32,
                ) {
                    save_debug_image(&raw, LENS_SIZE as u32, LENS_SIZE as u32);
                    let b64 = encode_to_base64(&raw, LENS_SIZE as u32, LENS_SIZE as u32);

                    let mut rgb = raw;
                    for chunk in rgb.chunks_exact_mut(4) {
                        chunk.swap(0, 2);
                    }
                    s.pixels = Some(gdk_pixbuf::Pixbuf::from_mut_slice(
                        rgb,
                        gdk_pixbuf::Colorspace::Rgb,
                        true,
                        8,
                        LENS_SIZE,
                        LENS_SIZE,
                        LENS_SIZE * 4,
                    ));
                    window_main.show();
                    window_main.queue_draw();

                    let api_key = s.api_key.clone();
                    let tx_clone = tx.clone();
                    std::thread::spawn(move || {
                        let result = call_api(&api_key, &b64).map_err(|e| e.to_string());
                        let _ = tx_clone.send(result);
                    });
                } else {
                    s.is_loading = false;
                    s.status = "Capture Failed".to_string();
                    window_main.show();
                }
            }
        }
        glib::ControlFlow::Continue
    });

    window.show_all();
    gtk::main();
    Ok(())
}

fn capture_x11(x: i32, y: i32, w: u32, h: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let root = conn.setup().roots[screen_num].root;
    let reply = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            root,
            x as i16,
            y as i16,
            w as u16,
            h as u16,
            0xffffffff,
        )?
        .reply()?;
    Ok(reply.data)
}

fn save_debug_image(raw: &[u8], w: u32, h: u32) {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for chunk in raw.chunks_exact(4) {
        rgb.push(chunk[2]);
        rgb.push(chunk[1]);
        rgb.push(chunk[0]);
    }
    if let Some(img) = image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(w, h, rgb) {
        let _ = img.save(DEBUG_IMAGE_PATH);
    }
}

fn encode_to_base64(raw: &[u8], w: u32, h: u32) -> String {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for chunk in raw.chunks_exact(4) {
        rgb.push(chunk[2]);
        rgb.push(chunk[1]);
        rgb.push(chunk[0]);
    }
    let img = image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(w, h, rgb).unwrap();
    let mut buffer = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buffer, image::ImageFormat::Png).unwrap();
    general_purpose::STANDARD.encode(buffer.into_inner())
}

fn call_api(key: &str, b64: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::new();
    let res = client.post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", key))
        .json(&json!({
            "model": "google/gemini-2.0-flash-001",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "OCR the Japanese text. Output transcription only, line by line."},
                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", b64)}}
            ]}]
        })).send()?;
    let body: serde_json::Value = res.json()?;
    Ok(body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string())
}
