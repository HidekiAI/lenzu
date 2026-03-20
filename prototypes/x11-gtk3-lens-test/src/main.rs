use gdk::prelude::*;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

struct AppState {
    pixels: Option<gdk_pixbuf::Pixbuf>,
    info_text: String,
    crosshair_color: String,
    last_capture: Instant,
}

const LENS_SIZE: i32 = 256;

fn main() {
    gtk::init().expect("Failed to initialize GTK.");

    let state = Rc::new(RefCell::new(AppState {
        pixels: None,
        info_text: "Hold L+R Click to Capture".to_string(),
        crosshair_color: "#FFFFFF".to_string(),
        last_capture: Instant::now() - Duration::from_secs(1),
    }));

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(LENS_SIZE, LENS_SIZE + 60);
    window.set_decorated(false);
    window.set_keep_above(true);
    window.set_app_paintable(true);
    window.set_skip_taskbar_hint(true);

    // --- DRAWING ---
    let state_draw = state.clone();
    window.connect_draw(move |_, cr| {
        let s = state_draw.borrow();
        let size = LENS_SIZE as f64;
        let center = size / 2.0;

        cr.set_source_rgb(0.0, 0.0, 0.0);
        cr.paint().ok();

        if let Some(ref pb) = s.pixels {
            cr.set_source_pixbuf(pb, 0.0, 0.0);
            cr.paint().ok();
        }

        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_line_width(1.0);
        cr.arc(center, center, center - 2.0, 0.0, 6.28);
        cr.stroke().ok();

        cr.set_source_rgb(1.0, 0.0, 0.0);
        cr.move_to(center, center - 15.0);
        cr.line_to(center, center + 15.0);
        cr.move_to(center - 15.0, center);
        cr.line_to(center + 15.0, center);
        cr.stroke().ok();

        cr.set_source_rgb(0.1, 0.1, 0.1);
        cr.rectangle(0.0, size, size, 60.0);
        cr.fill().ok();

        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_font_size(14.0);
        cr.move_to(10.0, size + 25.0);
        cr.show_text(&s.info_text).ok();
        cr.move_to(10.0, size + 45.0);
        cr.show_text(&format!("HEX: {}", s.crosshair_color)).ok();

        glib::Propagation::Proceed
    });

    // --- MAIN POLLING LOOP ---
    let window_poll = window.clone();
    let state_poll = state.clone();

    glib::timeout_add_local(Duration::from_millis(16), move || {
        let display = gdk::Display::default().unwrap();
        let seat = display.default_seat().unwrap();
        let device = seat.pointer().unwrap();

        // 1. Get the Root Window for global coordinates
        let screen = gdk::Screen::default().unwrap();
        let root_win = screen.root_window().unwrap();

        // 2. Query global position and modifier state (Buttons)
        // device_position returns (Option<Window>, x, y, ModifierType)
        let (_, x, y, modifier) = root_win.device_position(&device);

        // 3. Move lens
        window_poll.move_(x - (LENS_SIZE / 2), y - (LENS_SIZE / 2));

        // 4. TRIGGER: Left (BUTTON1) + Right (BUTTON3)
        let is_left = modifier.contains(gdk::ModifierType::BUTTON1_MASK);
        let is_right = modifier.contains(gdk::ModifierType::BUTTON3_MASK);

        if is_left && is_right {
            let mut s = state_poll.borrow_mut();
            if s.last_capture.elapsed() > Duration::from_millis(800) {
                s.last_capture = Instant::now();

                window_poll.hide();
                display.flush();
                display.sync();
                std::thread::sleep(Duration::from_millis(150));

                if let Ok(mut raw) = capture_x11_raw(
                    x - (LENS_SIZE / 2),
                    y - (LENS_SIZE / 2),
                    LENS_SIZE as u32,
                    LENS_SIZE as u32,
                ) {
                    for chunk in raw.chunks_exact_mut(4) {
                        chunk.swap(0, 2);
                    }

                    let center_idx = ((LENS_SIZE / 2) * LENS_SIZE + (LENS_SIZE / 2)) as usize * 4;
                    if center_idx + 3 < raw.len() {
                        let r = raw[center_idx];
                        let g = raw[center_idx + 1];
                        let b = raw[center_idx + 2];
                        s.crosshair_color = format!("#{:02X}{:02X}{:02X}", r, g, b);
                        s.info_text = format!("Captured: RGB({}, {}, {})", r, g, b);
                    }

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

    if let Some(gdk_win) = window.window() {
        let region = cairo::Region::create();
        gdk_win.input_shape_combine_region(&region, 0, 0);
    }

    gtk::main();
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
