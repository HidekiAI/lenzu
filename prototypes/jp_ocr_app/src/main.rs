use gdk::prelude::*;
use gtk::prelude::*;
use ort::session::Session;
use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

struct AppState {
    pixels: Option<gdk_pixbuf::Pixbuf>,
    info_text: String,
    ocr_result: String,
    det_model: Session,
    rec_model: Session,
    last_capture: Instant,
}

const LENS_SIZE: i32 = 256;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    gtk::init().expect("Failed to initialize GTK.");

    // Load converted ONNX models
    let det_model = Session::builder()?.commit_from_file("assets/det_model.onnx")?;
    let rec_model = Session::builder()?.commit_from_file("assets/rec_model.onnx")?;

    let state = Rc::new(RefCell::new(AppState {
        pixels: None,
        info_text: "Target JPN Text & Shift+Click".to_string(),
        ocr_result: String::new(),
        det_model,
        rec_model,
        last_capture: Instant::now() - Duration::from_secs(1),
    }));

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(LENS_SIZE, LENS_SIZE + 100);
    window.set_decorated(false);
    window.set_keep_above(true);
    window.set_app_paintable(true);

    let state_draw = state.clone();
    window.connect_draw(move |_, cr| {
        let s = state_draw.borrow();
        let size = LENS_SIZE as f64;

        cr.set_source_rgb(0.0, 0.0, 0.0);
        cr.paint().ok();

        if let Some(ref pb) = s.pixels {
            cr.set_source_pixbuf(pb, 0.0, 0.0);
            cr.paint().ok();
        }

        // UI Results
        cr.set_source_rgb(0.05, 0.05, 0.1);
        cr.rectangle(0.0, size, size, 100.0);
        cr.fill().ok();

        cr.set_source_rgb(0.0, 1.0, 0.5);
        cr.set_font_size(14.0);
        cr.move_to(10.0, size + 30.0);
        cr.show_text(&format!("OCR: {}", s.ocr_result)).ok();

        glib::Propagation::Proceed
    });

    let window_poll = window.clone();
    let state_poll = state.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        let display = gdk::Display::default().unwrap();
        let seat = display.default_seat().unwrap();
        let device = seat.pointer().unwrap();
        let root_win = gdk::Screen::default().unwrap().root_window().unwrap();
        let (_, x, y, modifier) = root_win.device_position(&device);

        window_poll.move_(x - (LENS_SIZE / 2), y - ((LENS_SIZE + 100) / 2));

        if modifier.contains(gdk::ModifierType::SHIFT_MASK)
            && modifier.contains(gdk::ModifierType::BUTTON1_MASK)
        {
            let mut s = state_poll.borrow_mut();
            if s.last_capture.elapsed() > Duration::from_millis(1500) {
                s.last_capture = Instant::now();
                window_poll.hide();
                display.flush();
                display.sync();
                std::thread::sleep(Duration::from_millis(200));

                if let Ok(mut raw) = capture_x11_raw(
                    x - (LENS_SIZE / 2),
                    y - (LENS_SIZE / 2),
                    LENS_SIZE as u32,
                    LENS_SIZE as u32,
                ) {
                    for chunk in raw.chunks_exact_mut(4) {
                        chunk.swap(0, 2);
                    }

                    // Logic to run Paddle detection + recognition goes here
                    s.ocr_result = "Reading Japanese...".to_string();

                    let pixbuf = gdk_pixbuf::Pixbuf::from_mut_slice(
                        raw,
                        gdk_pixbuf::Colorspace::Rgb,
                        true,
                        8,
                        LENS_SIZE,
                        LENS_SIZE,
                        LENS_SIZE * 4,
                    );
                    s.pixels = Some(pixbuf);
                }
                window_poll.show();
            }
        }
        glib::ControlFlow::Continue
    });

    window.connect_key_press_event(move |_, event| {
        if event.keyval() == gdk::keys::constants::Escape {
            gtk::main_quit();
        }
        glib::Propagation::Stop
    });

    window.show_all();
    gtk::main();
    Ok(())
}

fn capture_x11_raw(x: i32, y: i32, w: u32, h: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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
