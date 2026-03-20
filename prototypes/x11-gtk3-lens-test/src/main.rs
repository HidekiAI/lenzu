use gdk::prelude::*;
use gtk::prelude::*;
use half::f16;
use image::{ImageBuffer, Rgb};
use ort::session::Session;
use std::cell::RefCell;
use std::fs;
use std::rc::Rc;
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
    conf: f32,
}

struct AppState {
    pixels: Option<gdk_pixbuf::Pixbuf>,
    info_text: String,
    model: Session,
    last_capture: Instant,
    detection: Option<Detection>,
}

const LENS_SIZE: i32 = 256;

const LABELS: &[&str] = &[
    "person",
    "bicycle",
    "car",
    "motorcycle",
    "airplane",
    "bus",
    "train",
    "truck",
    "boat",
    "traffic light",
    "fire hydrant",
    "stop sign",
    "parking meter",
    "bench",
    "bird",
    "cat",
    "dog",
    "horse",
    "sheep",
    "cow",
    "elephant",
    "bear",
    "zebra",
    "giraffe",
    "backpack",
    "umbrella",
    "handbag",
    "tie",
    "suitcase",
    "frisbee",
    "skis",
    "snowboard",
    "sports ball",
    "kite",
    "baseball bat",
    "baseball glove",
    "skateboard",
    "surfboard",
    "tennis racket",
    "bottle",
    "wine glass",
    "cup",
    "fork",
    "knife",
    "spoon",
    "bowl",
    "banana",
    "apple",
    "sandwich",
    "orange",
    "broccoli",
    "carrot",
    "hot dog",
    "pizza",
    "donut",
    "cake",
    "chair",
    "couch",
    "potted plant",
    "bed",
    "dining table",
    "toilet",
    "tv",
    "laptop",
    "mouse",
    "remote",
    "keyboard",
    "cell phone",
    "microwave",
    "oven",
    "toaster",
    "sink",
    "refrigerator",
    "book",
    "clock",
    "vase",
    "scissors",
    "teddy bear",
    "hair drier",
    "toothbrush",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    gtk::init().expect("Failed to initialize GTK.");

    let model_path = fs::read_dir(".")?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.extension().and_then(|s| s.to_str()) == Some("onnx")
                && fs::metadata(p).map(|m| m.len()).unwrap_or(0) > 1_000_000
        })
        .expect("No valid .onnx file found (>1MB)!");

    println!("--- LENS STARTUP ---");
    let model = Session::builder()?.commit_from_file(&model_path)?;

    let state = Rc::new(RefCell::new(AppState {
        pixels: None,
        info_text: "READY: Shift+LeftClick".to_string(),
        model,
        last_capture: Instant::now() - Duration::from_secs(1),
        detection: None,
    }));

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(LENS_SIZE, LENS_SIZE + 60);
    window.set_decorated(false);
    window.set_keep_above(true);
    window.set_app_paintable(true);

    let state_draw = state.clone();
    window.connect_draw(move |_, cr| {
        let s = match state_draw.try_borrow() {
            Ok(s) => s,
            Err(_) => return glib::Propagation::Proceed,
        };
        let size = LENS_SIZE as f64;
        cr.set_source_rgb(0.0, 0.0, 0.0);
        cr.paint().ok();

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
        glib::Propagation::Proceed
    });

    let window_poll = window.clone();
    let state_poll = state.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        let display = gdk::Display::default().unwrap();
        let seat = display.default_seat().unwrap();
        let device = seat.pointer().unwrap();
        let screen = gdk::Screen::default().unwrap();
        let root_win = screen.root_window().unwrap();
        let (_, x, y, modifier) = root_win.device_position(&device);
        window_poll.move_(x - (LENS_SIZE / 2), y - ((LENS_SIZE + 60) / 2));

        if modifier.contains(gdk::ModifierType::SHIFT_MASK)
            && modifier.contains(gdk::ModifierType::BUTTON1_MASK)
        {
            if let Ok(mut s) = state_poll.try_borrow_mut() {
                if s.last_capture.elapsed() > Duration::from_millis(1000) {
                    s.last_capture = Instant::now();
                    window_poll.hide();
                    while gtk::events_pending() {
                        gtk::main_iteration();
                    }
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

                        s.detection = run_inference(&mut s.model, &raw);
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
                    }
                    window_poll.show();
                }
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
    Ok(())
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
            conf: max_conf,
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
