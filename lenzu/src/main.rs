use arboard::Clipboard;
use gdk::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use pango;
use pangocairo;
use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::Write;
use std::rc::Rc;
use std::time::{Duration, Instant};

// Import the local library modules
mod capture;
mod client;
mod utils;

// --- CONFIGURATION ---
const LENS_SIZE: i32 = 400;
const UI_PANEL_HEIGHT: i32 = 130;
const HISTORY_PATH: &str = "/dev/shm/ocr_history.txt";

struct AppState {
    pixels: Option<gdk_pixbuf::Pixbuf>,
    ocr_result: String,
    status: String,
    last_capture: Instant,
    clipboard: Clipboard,
    api_key: String,
    is_loading: bool,
    spinner_angle: f64,
    flash_alpha: f64,
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

    // Disambiguate .screen() call for the compiler
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

    // UPDATED: Channel type now matches Result<client::TranslationResult, String>
    let (tx, rx) = glib::MainContext::channel::<Result<client::TranslationResult, String>>(
        glib::Priority::default(),
    );

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

        if s.flash_alpha > 0.0 {
            cr.set_source_rgba(1.0, 1.0, 1.0, s.flash_alpha);
            cr.rectangle(0.0, 0.0, LENS_SIZE as f64, LENS_SIZE as f64);
            cr.fill().ok();
        }

        cr.set_source_rgb(0.0, 1.0, 0.8);
        cr.set_line_width(2.0);
        cr.rectangle(1.0, 1.0, (LENS_SIZE - 2) as f64, (LENS_SIZE - 2) as f64);
        cr.stroke().ok();

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
    // UPDATED: Receiver logic to parse TranslationResult
    rx.attach(None, move |api_result| {
        let mut s = state_rx.borrow_mut();
        s.is_loading = false;
        match api_result {
            Ok(res) => {
                // If English exists, use it; otherwise fallback to original OCR
                let display_text = res.english.clone().unwrap_or(res.original.clone());
                s.ocr_result = display_text.clone();
                s.status = "SUCCESS".to_string();

                // Copy the original OCR to clipboard
                let _ = s.clipboard.set_text(res.original);

                if let Ok(mut f) = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(HISTORY_PATH)
                {
                    let _ = writeln!(
                        f,
                        "[{}] {}",
                        chrono::Local::now().format("%H:%M:%S"),
                        display_text
                    );
                }
            }
            Err(e) => s.status = format!("API Error: {}", e),
        }
        window_rx.queue_draw();
        glib::ControlFlow::Continue
    });

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
            s.flash_alpha -= 0.1;
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
                s.flash_alpha = 1.0;
                window_main.queue_draw();

                window_main.hide();
                while gtk::events_pending() {
                    gtk::main_iteration();
                }
                std::thread::sleep(Duration::from_millis(400));

                if let Ok(raw) = capture::capture_x11(
                    win_x.max(0),
                    win_y.max(0),
                    LENS_SIZE as u32,
                    LENS_SIZE as u32,
                ) {
                    let rgb = utils::raw_to_rgb(&raw);
                    utils::save_debug_image(&rgb, LENS_SIZE as u32, LENS_SIZE as u32);
                    let b64 = utils::encode_to_base64(&rgb, LENS_SIZE as u32, LENS_SIZE as u32);

                    let mut pb_data = raw.clone();
                    utils::swap_bytes_for_pixbuf(&mut pb_data);

                    s.pixels = Some(gdk_pixbuf::Pixbuf::from_mut_slice(
                        pb_data,
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
                        let ocr_client = client::OcrClient::new(api_key);
                        let result = ocr_client.call_api(&b64).map_err(|e| e.to_string());
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
