use gdk_pixbuf::prelude::*;
use gtk4::gdk;
use gtk4::gdk::prelude::*;
use gtk4::glib;
use gtk4::prelude::*;
use half::f16;
use image::{ImageBuffer, Rgb};
use ort::session::Session;
use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

struct Detection {
    label: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

struct AppState {
    pixels: Option<gdk_pixbuf::Pixbuf>,
    info_text: String,
    model: Option<Session>,
    last_capture: Instant,
    detection: Option<Detection>,
}

const LENS_SIZE: i32 = 256;
const APP_ID: &str = "tld.mydomain.lenzu.prototype.x11_gtk_lens_test";

static POINTER_CONN: OnceLock<RustConnection> = OnceLock::new();
static WINDOW_XID: OnceLock<u32> = OnceLock::new();

fn init_x11() -> bool {
    if POINTER_CONN.get().is_some() {
        return true;
    }
    if let Ok((conn, _)) = RustConnection::connect(None) {
        let _ = POINTER_CONN.set(conn);
        true
    } else {
        false
    }
}

fn pointer_position() -> Option<(i32, i32, u16)> {
    let conn = POINTER_CONN.get()?;
    let root = conn.setup().roots[0].root;
    let reply = conn.query_pointer(root).ok()?.reply().ok()?;
    Some((reply.root_x as i32, reply.root_y as i32, reply.mask.bits()))
}

#[rustfmt::skip]
const LABELS: &[&str] = &[
    "person", "bicycle", "car", "motorcycle", "airplane", "bus", "train", "truck", "boat", "traffic light",
    "fire hydrant", "stop sign", "parking meter", "bench", "bird", "cat", "dog", "horse", "sheep", "cow",
    "elephant", "bear", "zebra", "giraffe", "backpack", "umbrella", "handbag", "tie", "suitcase", "frisbee",
    "skis", "snowboard", "sports ball", "kite", "baseball bat", "baseball glove", "skateboard", "surfboard",
    "tennis racket", "bottle", "wine glass", "cup", "fork", "knife", "spoon", "bowl", "banana", "apple",
    "sandwich", "orange", "broccoli", "carrot", "hot dog", "pizza", "donut", "cake", "chair", "couch",
    "potted plant", "bed", "dining table", "toilet", "tv", "laptop", "mouse", "remote", "keyboard", "cell phone",
    "microwave", "oven", "toaster", "sink", "refrigerator", "book", "clock", "vase", "scissors", "teddy bear",
    "hair drier", "toothbrush",
];

fn build_ui(application: &gtk4::Application) {
    init_x11();

    let model = fs::read_dir(".")
        .ok()
        .and_then(|entries| {
            entries.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
                p.extension().and_then(|s| s.to_str()) == Some("onnx")
                    && fs::metadata(p).map(|m| m.len()).unwrap_or(0) > 1_000_000
            })
        })
        .and_then(|p| {
            println!("--- LENS STARTUP (model: {}) ---", p.display());
            Session::builder().ok()?.commit_from_file(&p).ok()
        });

    let state = Rc::new(RefCell::new(AppState {
        pixels: None,
        info_text: "READY: Shift+LeftClick".to_string(),
        model,
        last_capture: Instant::now() - Duration::from_secs(1),
        detection: None,
    }));

    let window = gtk4::ApplicationWindow::new(application);
    window.set_default_size(LENS_SIZE, LENS_SIZE + 60);
    window.set_decorated(false);
    window.set_visible(false);

    let css = gtk4::CssProvider::new();
    css.load_from_string("window { background: transparent; }");
    let display = gtk4::prelude::RootExt::display(&window);
    gtk4::style_context_add_provider_for_display(
        &display,
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let key_controller = gtk4::EventControllerKey::new();
    let app = application.clone();
    key_controller.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gdk::Key::Escape {
            app.quit();
        }
        glib::Propagation::Stop
    });
    window.add_controller(key_controller);

    let area = gtk4::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);

    let state_draw = state.clone();
    area.set_draw_func(move |_da, cr, _w, _h| {
        let s = match state_draw.try_borrow() {
            Ok(s) => s,
            Err(_) => return,
        };
        let size = LENS_SIZE as f64;
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        cr.set_operator(cairo::Operator::Source);
        cr.paint().ok();
        cr.set_operator(cairo::Operator::Over);

        if let Some(ref pb) = s.pixels {
            cr.set_source_pixbuf(pb, 0.0, 0.0);
            cr.paint().ok();
        }

        if let Some(ref det) = s.detection {
            cr.set_source_rgb(1.0, 0.0, 0.0);
            cr.set_line_width(2.0);
            cr.rectangle(det.x, det.y, det.w, det.h);
            cr.stroke().ok();
            cr.move_to(det.x, det.y - 5.0);
            cr.set_font_size(14.0);
            cr.show_text(&det.label).ok();
        }

        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.set_line_width(1.0);
        cr.arc(size / 2.0, size / 2.0, size / 2.0 - 2.0, 0.0, 6.28);
        cr.stroke().ok();

        cr.set_source_rgb(0.1, 0.1, 0.1);
        cr.rectangle(0.0, size, size, 60.0);
        cr.fill().ok();
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.move_to(10.0, size + 35.0);
        cr.show_text(&s.info_text).ok();
    });

    window.set_child(Some(&area));

    let window_poll = window.clone();
    let state_poll = state.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        if let Some((x, y, mask)) = pointer_position() {
            let win_x = x - (LENS_SIZE / 2);
            let win_y = y - ((LENS_SIZE + 60) / 2);
            if let (Some(conn), Some(&xid)) = (POINTER_CONN.get(), WINDOW_XID.get()) {
                let aux = ConfigureWindowAux::new().x(win_x).y(win_y);
                let _ = conn.configure_window(xid, &aux);
            }

            let shift = x11rb::protocol::xproto::KeyButMask::SHIFT;
            let button1 = x11rb::protocol::xproto::KeyButMask::BUTTON1;
            if (mask & shift.bits()) != 0 && (mask & button1.bits()) != 0 {
                let should_capture = state_poll
                    .try_borrow()
                    .map(|s| s.last_capture.elapsed() > Duration::from_millis(1000))
                    .unwrap_or(false);
                if should_capture {
                    {
                        let mut s = state_poll.borrow_mut();
                        s.last_capture = Instant::now();
                    }

                    window_poll.set_visible(false);
                    while glib::MainContext::default().iteration(false) {}
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
                        save_debug_image(&raw);

                        let mut s = state_poll.borrow_mut();
                        if let Some(ref mut m) = s.model {
                            s.detection = run_inference(m, &raw);
                        }
                        s.info_text = s
                            .detection
                            .as_ref()
                            .map(|d| d.label.clone())
                            .unwrap_or("No detection.".into());

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

                        window_poll.set_visible(true);
                    } else {
                        window_poll.set_visible(true);
                    }
                }
            }
        }
        glib::ControlFlow::Continue
    });

    window.present();
    set_window_state(&window);
    input_shape_clickthrough(&window);
}

fn set_window_state(window: &gtk4::ApplicationWindow) {
    init_x11();
    if let (Some(conn), Some(surface)) = (POINTER_CONN.get(), window.surface()) {
        if let Ok(x11_surface) = surface.downcast::<gdk4_x11::X11Surface>() {
            let xid = x11_surface.xid() as u32;
            let _ = WINDOW_XID.set(xid);

            let root = conn.setup().roots[0].root;
            let net_wm_state = conn
                .intern_atom(false, b"_NET_WM_STATE")
                .ok()
                .and_then(|c| c.reply().ok());
            let above = conn
                .intern_atom(false, b"_NET_WM_STATE_ABOVE")
                .ok()
                .and_then(|c| c.reply().ok());
            if let (Some(net), Some(abv)) = (net_wm_state, above) {
                let data: ClientMessageData = [1u32, abv.atom, 0, 0, 0].into();
                let msg = ClientMessageEvent::new(32, xid, net.atom, data);
                let _ = conn.send_event(
                    false,
                    root,
                    EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
                    msg,
                );
                let aux = ConfigureWindowAux::new().stack_mode(StackMode::ABOVE);
                let _ = conn.configure_window(xid, &aux);
            }
        }
    }
}

fn input_shape_clickthrough(window: &gtk4::ApplicationWindow) {
    if let Some(surface) = window.surface() {
        let region = cairo::Region::create();
        surface.set_input_region(Some(&region));
    }
}

fn main() -> glib::ExitCode {
    let application = gtk4::Application::builder().application_id(APP_ID).build();
    application.connect_activate(build_ui);
    application.run()
}

fn save_debug_image(raw_rgb: &[u8]) {
    let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::new(LENS_SIZE as u32, LENS_SIZE as u32);
    for (i, chunk) in raw_rgb.chunks_exact(4).enumerate() {
        let x = (i % LENS_SIZE as usize) as u32;
        let y = (i / LENS_SIZE as usize) as u32;
        img.put_pixel(x, y, Rgb([chunk[0], chunk[1], chunk[2]]));
    }
    let _ = img.save("/dev/shm/lens.capture.png");
}

fn run_inference(session: &mut Session, raw_rgb: &[u8]) -> Option<Detection> {
    let mut input = vec![f16::from_f32(0.0); 3 * 640 * 640];
    for y in 0..640 {
        for x in 0..640 {
            let ix = (x * LENS_SIZE as usize / 640).min(LENS_SIZE as usize - 1);
            let iy = (y * LENS_SIZE as usize / 640).min(LENS_SIZE as usize - 1);
            let p = (iy * LENS_SIZE as usize + ix) * 4;
            input[0 * 640 * 640 + y * 640 + x] = f16::from_f32(raw_rgb[p] as f32 / 255.0);
            input[1 * 640 * 640 + y * 640 + x] = f16::from_f32(raw_rgb[p + 1] as f32 / 255.0);
            input[2 * 640 * 640 + y * 640 + x] = f16::from_f32(raw_rgb[p + 2] as f32 / 255.0);
        }
    }

    let input_value =
        ort::value::Value::from_array(([1, 3, 640, 640], input.into_boxed_slice())).ok()?;
    let outputs = session.run(ort::inputs![input_value]).ok()?;

    let (shape, data_f16) = outputs[0].try_extract_tensor::<f16>().ok()?;
    let data: Vec<f32> = data_f16.iter().map(|&x| x.to_f32()).collect();

    let anchors = shape[2] as usize;
    let mut max_conf = 0.0;
    let mut best_idx = 0;
    let mut best_class = 0;

    for i in 0..anchors {
        for c in 0..80 {
            let conf = data[(c + 4) * anchors + i];
            if conf > max_conf {
                max_conf = conf;
                best_idx = i;
                best_class = c;
            }
        }
    }

    if max_conf > 0.20 {
        let cx = data[0 * anchors + best_idx] / 640.0 * LENS_SIZE as f32;
        let cy = data[1 * anchors + best_idx] / 640.0 * LENS_SIZE as f32;
        let w = data[2 * anchors + best_idx] / 640.0 * LENS_SIZE as f32;
        let h = data[3 * anchors + best_idx] / 640.0 * LENS_SIZE as f32;

        Some(Detection {
            label: LABELS[best_class].to_uppercase().to_string(),
            x: (cx - w / 2.0) as f64,
            y: (cy - h / 2.0) as f64,
            w: w as f64,
            h: h as f64,
        })
    } else {
        None
    }
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
        assert!(
            result.is_err(),
            "borrow() while borrow_mut() is active must panic"
        );
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
        assert!(
            result.is_ok(),
            "dropping guard before re-borrow must not panic"
        );
    }

    #[test]
    fn test_try_borrow_graceful() {
        let state = Rc::new(RefCell::new(42i32));
        let val = state.try_borrow().map(|s| *s).unwrap_or(-1);
        assert_eq!(val, 42);
    }

    #[test]
    fn test_model_loading_graceful_on_empty_dir() {
        // Scan a temp dir with no .onnx files — must return None not panic
        let dir = std::env::temp_dir().join("lens_test_no_model");
        let _ = std::fs::create_dir_all(&dir);
        let found = std::fs::read_dir(&dir).ok().and_then(|entries| {
            entries.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
                p.extension().and_then(|s| s.to_str()) == Some("onnx")
                    && std::fs::metadata(p).map(|m| m.len()).unwrap_or(0) > 1_000_000
            })
        });
        assert!(
            found.is_none(),
            "must return None when no .onnx file exists"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_pixbuf_from_vec() {
        let w = 10;
        let h = 10;
        let data = vec![128u8; (w * h * 4) as usize];
        let pb = gdk_pixbuf::Pixbuf::from_mut_slice(
            data,
            gdk_pixbuf::Colorspace::Rgb,
            true,
            8,
            w,
            h,
            w * 4,
        );
        assert_eq!(pb.width(), w);
        assert_eq!(pb.height(), h);
    }
}
