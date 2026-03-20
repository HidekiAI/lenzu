use cairo::{RectangleInt, Region};
use gdk::prelude::*;
use gtk::prelude::*;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

#[derive(Clone)]
struct Detection {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    label: String,
    is_clicked: bool,
}

struct AppState {
    pixels: Option<gdk_pixbuf::Pixbuf>,
    zoom: f64,
    detections: Vec<Detection>,
    is_frozen: bool,
}

fn main() {
    gtk::init().expect("Failed to initialize GTK.");

    let state = Rc::new(RefCell::new(AppState {
        pixels: None,
        zoom: 2.0,
        detections: vec![Detection {
            x: 0.45,
            y: 0.45,
            w: 0.1,
            h: 0.1,
            label: "COPY ME".to_string(),
            is_clicked: false,
        }],
        is_frozen: false,
    }));

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(1024, 1024);
    window.set_decorated(false);
    window.set_app_paintable(true);
    window.set_keep_above(true);

    if let Some(screen) = GtkWindowExt::screen(&window) {
        if let Some(visual) = screen.rgba_visual() {
            window.set_visual(Some(&visual));
        }
    }

    // --- CLICK HANDLING ---
    let state_click = state.clone();
    window.add_events(gdk::EventMask::BUTTON_PRESS_MASK);
    window.connect_button_press_event(move |win, event| {
        let mut s = state_click.borrow_mut();
        if s.is_frozen {
            let (click_x, click_y) = event.position();
            let size = 1024.0;
            let zoom_push = (size * (s.zoom - 1.0)) / 2.0;
            let norm_x = (click_x + zoom_push) / (size * s.zoom);
            let norm_y = (click_y + zoom_push) / (size * s.zoom);

            for det in s.detections.iter_mut() {
                if norm_x >= det.x
                    && norm_x <= (det.x + det.w)
                    && norm_y >= det.y
                    && norm_y <= (det.y + det.h)
                {
                    let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
                    clipboard.set_text(&det.label);
                    det.is_clicked = true;
                }
            }
            win.queue_draw();
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });

    // --- DRAWING ---
    let state_draw = state.clone();
    window.connect_draw(move |_, cr| {
        let s = state_draw.borrow();
        let size = 1024.0;
        let center = size / 2.0;
        let radius = center - 20.0;

        if let Some(ref pb) = s.pixels {
            cr.arc(center, center, radius, 0.0, 2.0 * std::f64::consts::PI);
            cr.clip();

            cr.save().expect("Save failed");
            cr.scale(s.zoom, s.zoom);
            let offset = (size / 2.0) * (1.0 - 1.0 / s.zoom);
            cr.set_source_pixbuf(pb, offset, offset);
            cr.paint().expect("Paint failed");
            cr.restore().expect("Restore failed");

            for det in &s.detections {
                cr.set_source_rgba(if det.is_clicked { 1.0 } else { 0.0 }, 1.0, 0.0, 0.8);
                let zoom_push = (size * (s.zoom - 1.0)) / 2.0;
                let bx = (det.x * size * s.zoom) - zoom_push;
                let by = (det.y * size * s.zoom) - zoom_push;
                cr.set_line_width(3.0);
                cr.rectangle(bx, by, det.w * size * s.zoom, det.h * size * s.zoom);
                cr.stroke().ok();
            }

            cr.reset_clip();
            cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
            cr.set_line_width(6.0);
            cr.arc(center, center, radius, 0.0, 2.0 * std::f64::consts::PI);
            cr.stroke().ok();
        }
        glib::Propagation::Proceed
    });

    // --- KEYBOARD ---
    let state_key = state.clone();
    let window_key = window.clone();
    window.connect_key_press_event(move |_, event| {
        let mut s = state_key.borrow_mut();
        match event.keyval() {
            gdk::keys::constants::space => {
                s.is_frozen = !s.is_frozen;
                set_click_through(&window_key, !s.is_frozen);
            }
            gdk::keys::constants::Escape => gtk::main_quit(),
            _ => {}
        }
        window_key.queue_draw();
        glib::Propagation::Stop
    });

    // --- CAPTURE LOOP ---
    let window_loop = window.clone();
    let state_loop = state.clone();
    glib::timeout_add_local(Duration::from_millis(33), move || {
        let mut s = state_loop.borrow_mut();
        if !s.is_frozen {
            let display = gdk::Display::default().expect("No display");
            let seat = display.default_seat().expect("No seat");
            if let Some(device) = seat.pointer() {
                let (_, x, y) = device.position();
                window_loop.move_(x - 512, y - 512);
                window_loop.hide();
                display.flush();
                if let Ok(mut raw_data) = capture_x11(x - 512, y - 512, 1024, 1024) {
                    window_loop.show();
                    for chunk in raw_data.chunks_exact_mut(4) {
                        chunk.swap(0, 2);
                    }
                    let pixbuf = gdk_pixbuf::Pixbuf::from_mut_slice(
                        raw_data,
                        gdk_pixbuf::Colorspace::Rgb,
                        true,
                        8,
                        1024,
                        1024,
                        1024 * 4,
                    );
                    s.pixels = Some(pixbuf);
                    window_loop.queue_draw();
                } else {
                    window_loop.show();
                }
            }
        }
        glib::ControlFlow::Continue
    });

    window.show_all();
    set_click_through(&window, true);
    gtk::main();
}

fn set_click_through(window: &gtk::Window, enable: bool) {
    if let Some(gdk_win) = window.window() {
        if enable {
            let region = Region::create();
            gdk_win.input_shape_combine_region(&region, 0, 0);
        } else {
            // FIX: Using RectangleInt::new constructor
            let rect = RectangleInt::new(0, 0, 1024, 1024);
            let region = Region::create_rectangle(&rect);
            gdk_win.input_shape_combine_region(&region, 0, 0);
        }
    }
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
