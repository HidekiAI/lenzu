use arboard::Clipboard;
use base64::{engine::general_purpose, Engine as _};
use gdk_pixbuf::prelude::*;
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use pango;
use pangocairo::functions::show_layout;
use serde_json::json;
use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::Write;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

const LENS_SIZE: i32 = 400;
const UI_PANEL_HEIGHT: i32 = 130;
const HISTORY_PATH: &str = "/dev/shm/ocr_history.txt";
const DEBUG_IMAGE_PATH: &str = "/dev/shm/debug_lens.png";
const APP_ID: &str = "tld.mydomain.lenzu.prototype.jp_ocr_app";

static X11_CONN: OnceLock<RustConnection> = OnceLock::new();
static WINDOW_XID: OnceLock<u32> = OnceLock::new();
static ROOT_WINDOW: OnceLock<u32> = OnceLock::new();

fn init_x11() -> bool {
    if X11_CONN.get().is_some() {
        return true;
    }
    let (conn, screen_num) = match RustConnection::connect(None) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let root = conn.setup().roots[screen_num].root;
    let _ = ROOT_WINDOW.set(root);
    let _ = X11_CONN.set(conn);
    true
}

fn x11_conn() -> Option<&'static RustConnection> {
    init_x11();
    X11_CONN.get()
}

fn root_window() -> Option<u32> {
    init_x11();
    ROOT_WINDOW.get().copied()
}

fn pointer_position() -> Option<(i32, i32, u16)> {
    let conn = x11_conn()?;
    let root = root_window()?;
    let reply = conn.query_pointer(root).ok()?.reply().ok()?;
    Some((reply.root_x as i32, reply.root_y as i32, reply.mask.bits()))
}

fn set_window_state(window: &gtk4::ApplicationWindow) {
    let conn = match x11_conn() {
        Some(c) => c,
        None => return,
    };
    let root = match root_window() {
        Some(r) => r,
        None => return,
    };
    let xid = match get_xid(window) {
        Some(x) => x,
        None => return,
    };
    let _ = WINDOW_XID.set(xid);

    let net_wm_state = match conn.intern_atom(false, b"_NET_WM_STATE").ok().and_then(|c| c.reply().ok()) {
        Some(r) => r.atom,
        None => return,
    };
    let above = match conn.intern_atom(false, b"_NET_WM_STATE_ABOVE").ok().and_then(|c| c.reply().ok()) {
        Some(r) => r.atom,
        None => return,
    };
    let data: ClientMessageData = [1u32, above, 0, 0, 0].into();
    let msg = ClientMessageEvent::new(32, xid, net_wm_state, data);
    let _ = conn.send_event(false, root, EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT, msg);
    let aux = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
    let _ = conn.configure_window(xid, &aux);
}

fn get_xid(window: &gtk4::ApplicationWindow) -> Option<u32> {
    let surface = window.surface()?;
    let x11_surface = surface.downcast::<gdk4_x11::X11Surface>().ok()?;
    Some(x11_surface.xid() as u32)
}

fn move_window(x: i32, y: i32) {
    let conn = match x11_conn() {
        Some(c) => c,
        None => return,
    };
    let xid = match WINDOW_XID.get() {
        Some(x) => *x,
        None => return,
    };
    let aux = ConfigureWindowAux::new().x(x).y(y);
    let _ = conn.configure_window(xid, &aux);
}

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

fn build_ui(application: &gtk4::Application) {
    init_x11();

    let api_key = std::env::var("OPENROUTER_API_KEY")
        .expect("ERROR: OPENROUTER_API_KEY environment variable not set!");

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

    let window = gtk4::ApplicationWindow::new(application);
    window.set_default_size(LENS_SIZE, LENS_SIZE + UI_PANEL_HEIGHT);
    window.set_decorated(false);
    window.set_visible(false);

    let css = gtk4::CssProvider::new();
    css.load_from_string("window { background: transparent; }");
    let display = gtk4::prelude::RootExt::display(&window);
    gtk4::style_context_add_provider_for_display(&display, &css, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);

    let key_controller = gtk4::EventControllerKey::new();
    let app = application.clone();
    key_controller.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gdk::Key::Escape {
            app.quit();
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    let area = gtk4::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);

    let state_draw = state.clone();
    area.set_draw_func(move |da, cr, w, h| {
        let s = match state_draw.try_borrow() {
            Ok(s) => s,
            Err(_) => return,
        };
        let _ = (w, h);

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
        cr.rectangle(0.0, LENS_SIZE as f64, LENS_SIZE as f64, UI_PANEL_HEIGHT as f64);
        cr.fill().ok();

        let context = da.pango_context();
        let layout = pango::Layout::new(&context);

        cr.set_source_rgb(0.0, 1.0, 0.8);
        layout.set_text(&s.status);
        cr.move_to(12.0, (LENS_SIZE + 10) as f64);
        show_layout(cr, &layout);

        if s.is_loading {
            cr.save().ok();
            cr.translate((LENS_SIZE - 30) as f64, (LENS_SIZE + 20) as f64);
            cr.rotate(s.spinner_angle);
            cr.set_line_width(3.0);
            cr.set_source_rgb(0.0, 1.0, 0.8);
            cr.new_sub_path();
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
        show_layout(cr, &layout);
    });

    window.set_child(Some(&area));

    let state_rx = state.clone();
    let window_rx = window.clone();
    let (tx, rx): (async_channel::Sender<Result<String, String>>, async_channel::Receiver<Result<String, String>>) = async_channel::bounded(1);
    glib::spawn_future_local(async move {
        while let Ok(api_result) = rx.recv().await {
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
        }
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

    let window_poll = window.clone();
    let state_poll = state.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        if let Some((x, y, mask)) = pointer_position() {
            let win_x = x - (LENS_SIZE / 2);
            let win_y = y - (LENS_SIZE / 2);

            move_window(win_x, win_y);

            let shift = x11rb::protocol::xproto::KeyButMask::SHIFT;
            let button1 = x11rb::protocol::xproto::KeyButMask::BUTTON1;
            if (mask & shift.bits()) != 0 && (mask & button1.bits()) != 0 {
                let should_capture = {
                    let s = state_poll.borrow();
                    s.last_capture.elapsed() > Duration::from_secs(1) && !s.is_loading
                };
                if should_capture {
                    {
                        let mut s = state_poll.borrow_mut();
                        s.last_capture = Instant::now();
                        s.status = "CAPTURING...".to_string();
                        s.is_loading = true;
                        s.flash_alpha = 1.0;
                        window_poll.queue_draw();
                    }

                    window_poll.set_visible(false);
                    while glib::MainContext::default().iteration(false) {}
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
                        let mut s = state_poll.borrow_mut();
                        s.pixels = Some(gdk_pixbuf::Pixbuf::from_mut_slice(
                            rgb,
                            gdk_pixbuf::Colorspace::Rgb,
                            true,
                            8,
                            LENS_SIZE,
                            LENS_SIZE,
                            LENS_SIZE * 4,
                        ));
                        window_poll.set_visible(true);
                        window_poll.queue_draw();

                        let api_key = s.api_key.clone();
                        let tx_clone = tx.clone();
                        std::thread::spawn(move || {
                            let result = call_api(&api_key, &b64).map_err(|e| e.to_string());
                            let _ = tx_clone.send_blocking(result);
                        });
                    } else {
                        let mut s = state_poll.borrow_mut();
                        s.is_loading = false;
                        s.status = "Capture Failed".to_string();
                        window_poll.set_visible(true);
                    }
                }
            }
        }
        glib::ControlFlow::Continue
    });

    window.present();
    set_window_state(&window);
}

fn main() -> glib::ExitCode {
    let application = gtk4::Application::builder().application_id(APP_ID).build();
    application.connect_activate(build_ui);
    application.run()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::panic::catch_unwind;
    use std::rc::Rc;

    #[test]
    fn test_refcell_borrow_across_blocking_panics() {
        let state = Rc::new(RefCell::new(0i32));
        let result = catch_unwind(std::panic::AssertUnwindSafe(move || {
            let mut s = state.borrow_mut();
            *s = 1;
            let _other = state.borrow();
        }));
        assert!(result.is_err(), "borrow() while borrow_mut() is active must panic");
    }

    #[test]
    fn test_refcell_scope_guard_prevents_panic() {
        let state = Rc::new(RefCell::new(0i32));
        let result = catch_unwind(std::panic::AssertUnwindSafe(move || {
            {
                let mut s = state.borrow_mut();
                *s = 1;
            }
            let s = state.borrow();
            assert_eq!(*s, 1);
        }));
        assert!(result.is_ok(), "dropping guard before re-borrow must not panic");
    }

    #[test]
    fn test_refcell_try_borrow_graceful() {
        let state = Rc::new(RefCell::new(42i32));
        let val = state.try_borrow().map(|s| *s).unwrap_or(-1);
        assert_eq!(val, 42);
    }

    #[test]
    fn test_pixbuf_from_vec() {
        let w = 10;
        let h = 10;
        let data = vec![128u8; (w * h * 4) as usize];
        let pb = gdk_pixbuf::Pixbuf::from_mut_slice(
            data, gdk_pixbuf::Colorspace::Rgb, true, 8, w, h, w * 4,
        );
        assert_eq!(pb.width(), w);
        assert_eq!(pb.height(), h);
        assert!(pb.has_alpha());
    }
}

fn capture_x11(x: i32, y: i32, w: u32, h: u32) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let (conn, screen_num) = RustConnection::connect(None)?;
    let root = conn.setup().roots[screen_num].root;
    let reply = conn
        .get_image(ImageFormat::Z_PIXMAP, root, x as i16, y as i16, w as u16, h as u16, 0xffffffff)?
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
    let res = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", key))
        .json(&json!({
            "model": "google/gemini-2.0-flash-001",
            "messages": [{"role": "user", "content": [
                {"type": "text", "text": "OCR the Japanese text. Output transcription only, line by line."},
                {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{}", b64)}}
            ]}]
        }))
        .send()?;
    let body: serde_json::Value = res.json()?;
    Ok(body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string())
}
