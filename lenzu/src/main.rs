use arboard::Clipboard;
use gdk::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use pango;
use pangocairo;
use std::cell::RefCell;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::rc::Rc;
extern crate libc;
use std::time::{Duration, Instant};

use lenzu::capture;
use lenzu::client;
use lenzu::config;
use lenzu::utils;

const HISTORY_PATH: &str = "/dev/shm/ocr_history.txt";

fn format_for_overlay(
    results: &[client::TranslationResult],
    mode: &config::OverlayRenderMode,
) -> String {
    use config::OverlayRenderMode::*;
    results
        .iter()
        .map(|r| match mode {
            Original => r.original.clone(),
            English => r.english.clone().unwrap_or_else(|| r.original.clone()),
            Furigana => r.furigana.clone().unwrap_or_else(|| r.original.clone()),
            Romaji => r.romaji.clone().unwrap_or_else(|| r.original.clone()),
            All => {
                let mut parts = vec![r.original.clone()];
                if let Some(v) = &r.english {
                    parts.push(v.clone());
                }
                if let Some(v) = &r.furigana {
                    parts.push(v.clone());
                }
                if let Some(v) = &r.romaji {
                    parts.push(v.clone());
                }
                parts.join("  |  ")
            }
            Debug => {
                let mut parts = vec![r.original.clone()];
                if let Some(v) = &r.english {
                    parts.push(format!("en: {}", v));
                }
                if let Some(v) = &r.furigana {
                    parts.push(format!("furigana: {}", v));
                }
                if let Some(v) = &r.romaji {
                    parts.push(format!("romaji: {}", v));
                }
                if let Some(v) = &r.top_xy {
                    parts.push(format!("top: {}", v));
                }
                if let Some(v) = &r.bot_xy {
                    parts.push(format!("bot: {}", v));
                }
                if let Some(v) = &r.debug_info {
                    parts.push(format!("debug: {}", v));
                }
                parts.join("\n")
            }
        })
        .collect::<Vec<_>>()
        .join("\n---\n")
}

fn send_to_overlay(text: &str, port: u16) {
    if let Ok(socket) = std::net::UdpSocket::bind("127.0.0.1:0") {
        let addr = format!("127.0.0.1:{}", port);
        let message = serde_json::json!({
            "type": "message",
            "text": text
        });
        eprintln!(
            "[UDP] About to send message to port {}: {:?}",
            port, message
        );
        eprintln!("[HUD] Sending overlay text: {}", text);
        let _ = socket.send_to(message.to_string().as_bytes(), addr);
        eprintln!("[UDP] Sent message to port {}: {}", port, text);
    }
}

/// Send a shutdown command to the server via UDP
fn send_shutdown_command(port: u16) {
    if let Ok(socket) = std::net::UdpSocket::bind("127.0.0.1:0") {
        let addr = format!("127.0.0.1:{}", port);
        let message = serde_json::json!({
            "type": "shutdown"
        });
        let _ = socket.send_to(message.to_string().as_bytes(), addr);
        // Give the server a moment to process the shutdown command
        let _ = std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

struct AppState {
    config: config::AppConfig,
    pixels: Option<gdk_pixbuf::Pixbuf>,
    ocr_result: String,
    status: String,
    last_capture: Instant,
    clipboard: Clipboard,
    api_key: String,
    is_loading: bool,
    spinner_angle: f64,
    flash_alpha: f64,
    server_process: Option<std::process::Child>,
}

/// Path to `lenzu_server` directory.
/// Resolved at runtime from the binary location (target/debug/lenzu →
/// ../../lenzu_server) so both workspaces work without recompiling.
/// Falls back to the compile-time CARGO_MANIFEST_DIR path if not found.
fn lenzu_server_dir() -> std::path::PathBuf {
    let runtime = std::env::current_exe()
        .ok()
        .and_then(|p| {
            // binary: <repo>/target/debug/lenzu  →  parent×2 = <repo>/target  →  parent×3 = <repo>
            p.parent()?.parent()?.parent().map(|r| r.join("lenzu_server"))
        })
        .filter(|p| p.is_dir());

    runtime.unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../lenzu_server"))
}

/// Spawn the Electron HUD (`lenzu_server`). UDP port is passed via `LENZU_OVERLAY_UDP_PORT`
/// so it stays in sync with `overlay_udp_port` in `lenzu_config.json`.
fn spawn_server(port: u16) -> Option<std::process::Child> {
    let dir = lenzu_server_dir();
    if !dir.is_dir() {
        eprintln!(
            "lenzu: lenzu_server not found at {} (expected Electron overlay package)",
            dir.display()
        );
        return None;
    }
    // Launch Electron directly (not via npm/pnpm) so the child PID is the
    // actual Electron process — required for process-group kill on exit.
    // GTK_CSD=0 suppresses client-side decorations on the overlay window.
    std::process::Command::new("node_modules/.bin/electron")
        .args(["dist/main.js"])
        .current_dir(&dir)
        .env("LENZU_OVERLAY_UDP_PORT", port.to_string())
        .env("GTK_CSD", "0")
        .process_group(0)  // put Electron in its own process group
        .spawn()
        .map_err(|e| {
            eprintln!(
                "lenzu: could not spawn Electron overlay (node_modules/.bin/electron in {}): {e}",
                dir.display()
            )
        })
        .ok()
}

/// Kill the server child process and reap it.
fn kill_server(server: &mut Option<std::process::Child>, config: &config::AppConfig) {
    if let Some(mut child) = server.take() {
        // First try graceful shutdown via UDP
        send_shutdown_command(config.overlay_udp_port);
        // Give the server time to process the shutdown command
        std::thread::sleep(std::time::Duration::from_millis(500));
        // Force-kill the entire process group — Electron spawns child processes
        // that child.kill() (SIGKILL on the PID) won't reach.
        let id = child.id() as i32;
        unsafe {
            libc::kill(-id, libc::SIGKILL);
        }
        // Reap without blocking in case the process is already gone
        let _ = child.try_wait();
        eprintln!("lenzu: Electron overlay terminated.");
    }
}

fn hex_to_rgb(hex: &str) -> (f64, f64, f64) {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return (0.0, 1.0, 0.8);
    } // Fallback
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0) as f64 / 255.0;
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255) as f64 / 255.0;
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(204) as f64 / 255.0;
    (r, g, b)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let api_key = std::env::var("OPENROUTER_API_KEY")
        .expect("ERROR: OPENROUTER_API_KEY environment variable not set!");

    let cfg = config::AppConfig::load();

    gtk::init().expect("Failed to initialize GTK.");

    let server_process = if cfg.overlay_enabled {
        spawn_server(cfg.overlay_udp_port)
    } else {
        None
    };

    let state = Rc::new(RefCell::new(AppState {
        config: cfg.clone(),
        pixels: None,
        ocr_result: String::new(),
        status: "READY: Shift+Click | ESC to Quit".to_string(),
        last_capture: Instant::now() - Duration::from_secs(2),
        clipboard: Clipboard::new().expect("Failed to init clipboard"),
        api_key,
        is_loading: false,
        spinner_angle: 0.0,
        flash_alpha: 0.0,
        server_process,
    }));

    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(cfg.lens_size, cfg.lens_size + cfg.ui_panel_height);
    window.set_decorated(false);
    window.set_keep_above(true);
    window.set_app_paintable(true);

    if let Some(screen) = gtk::prelude::WidgetExt::screen(&window) {
        if let Some(visual) = screen.rgba_visual() {
            window.set_visual(Some(&visual));
        }
    }

    let cfg_esc = cfg.clone();
    let state_esc = state.clone();
    window.connect_key_press_event(move |_, event| {
        if event.keyval() == gdk::keys::constants::Escape {
            kill_server(&mut state_esc.borrow_mut().server_process, &cfg_esc);
            gtk::main_quit();
        }
        glib::Propagation::Proceed
    });

    let cfg_del = cfg.clone();
    let state_del = state.clone();
    window.connect_delete_event(move |_, _| {
        kill_server(&mut state_del.borrow_mut().server_process, &cfg_del);
        glib::Propagation::Proceed // allow window close → GTK loop ends naturally
    });

    let (tx, rx) = glib::MainContext::channel::<Result<Vec<client::TranslationResult>, String>>(
        glib::Priority::default(),
    );

    let state_draw = state.clone();
    window.connect_draw(move |win, cr| {
        let s = state_draw.borrow();
        let (r, g, b) = hex_to_rgb(&s.config.hud_color_hex);

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
            cr.rectangle(
                0.0,
                0.0,
                s.config.lens_size as f64,
                s.config.lens_size as f64,
            );
            cr.fill().ok();
        }

        cr.set_source_rgb(r, g, b);
        cr.set_line_width(2.0);
        cr.rectangle(
            1.0,
            1.0,
            (s.config.lens_size - 2) as f64,
            (s.config.lens_size - 2) as f64,
        );
        cr.stroke().ok();

        cr.set_source_rgba(0.01, 0.01, 0.05, 0.85);
        cr.rectangle(
            0.0,
            s.config.lens_size as f64,
            s.config.lens_size as f64,
            s.config.ui_panel_height as f64,
        );
        cr.fill().ok();

        let context = win.pango_context();
        let layout = pango::Layout::new(&context);

        cr.set_source_rgb(r, g, b);
        layout.set_text(&s.status);
        cr.move_to(12.0, (s.config.lens_size + 10) as f64);
        pangocairo::show_layout(cr, &layout);

        if s.is_loading {
            cr.save().ok();
            cr.translate(
                (s.config.lens_size - 30) as f64,
                (s.config.lens_size + 20) as f64,
            );
            cr.rotate(s.spinner_angle);
            cr.set_line_width(3.0);
            cr.set_source_rgb(r, g, b);
            cr.arc(0.0, 0.0, 8.0, 0.0, 1.5 * std::f64::consts::PI);
            cr.stroke().ok();
            cr.restore().ok();
        }

        cr.set_source_rgb(1.0, 1.0, 1.0);
        let font_str = format!("Sans Bold {}", s.config.font_size);
        let font_desc = pango::FontDescription::from_string(&font_str);
        layout.set_font_description(Some(&font_desc));
        layout.set_text(&s.ocr_result);
        layout.set_width(pango::units_from_double((s.config.lens_size - 24) as f64));
        layout.set_ellipsize(pango::EllipsizeMode::End);
        cr.move_to(12.0, (s.config.lens_size + 40) as f64);
        pangocairo::show_layout(cr, &layout);

        glib::Propagation::Proceed
    });

    let state_rx = state.clone();
    let window_rx = window.clone();
    rx.attach(None, move |api_result| {
        let mut s = state_rx.borrow_mut();
        s.is_loading = false;
        match api_result {
            Ok(results) => {
                let combined_english = results
                    .iter()
                    .map(|r| r.english.clone().unwrap_or_else(|| r.original.clone()))
                    .collect::<Vec<_>>()
                    .join("\n");

                let combined_original = results
                    .iter()
                    .map(|r| r.original.clone())
                    .collect::<Vec<_>>()
                    .join("\n");

                s.ocr_result = combined_english.clone();
                s.status = format!("SUCCESS ({} items)", results.len());
                let _ = s.clipboard.set_text(combined_original);

                if s.config.overlay_enabled {
                    let text = format_for_overlay(&results, &s.config.overlay_render_mode);
                    eprintln!("[HUD] Overlay enabled – prepared text: {}", text);
                    send_to_overlay(&text, s.config.overlay_udp_port);
                }

                let trimmed_text = combined_english.trim();
                if !trimmed_text.is_empty() {
                    if let Ok(mut f) = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(HISTORY_PATH)
                    {
                        let _ = writeln!(
                            f,
                            "[{}] {}",
                            chrono::Local::now().format("%H:%M:%S"),
                            trimmed_text
                        );
                    } else {
                        eprintln!("[history] failed to open {}", HISTORY_PATH);
                    }
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
        if s.is_loading {
            s.spinner_angle += 0.2;
            window_anim.queue_draw();
        }
        if s.flash_alpha > 0.0 {
            s.flash_alpha -= 0.1;
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

        let s_conf = state_main.borrow().config.clone();
        let win_x = x - (s_conf.lens_size / 2);
        let win_y = y - (s_conf.lens_size / 2);
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
                    s_conf.lens_size as u32,
                    s_conf.lens_size as u32,
                ) {
                    let rgb = utils::raw_to_rgb(&raw);
                    utils::save_debug_image(&rgb, s_conf.lens_size as u32, s_conf.lens_size as u32);
                    let b64 = utils::encode_to_base64(
                        &rgb,
                        s_conf.lens_size as u32,
                        s_conf.lens_size as u32,
                    );
                    let mut pb_data = raw.clone();
                    utils::swap_bytes_for_pixbuf(&mut pb_data);
                    s.pixels = Some(gdk_pixbuf::Pixbuf::from_mut_slice(
                        pb_data,
                        gdk_pixbuf::Colorspace::Rgb,
                        true,
                        8,
                        s_conf.lens_size,
                        s_conf.lens_size,
                        s_conf.lens_size * 4,
                    ));
                    window_main.show();
                    let api_key = s.api_key.clone();
                    let endpoint = s.config.llm_api_endpoint.clone();
                    let model = s.config.llm_default_model.clone();
                    let prompt = s.config.resolved_prompt();
                    let tx_clone = tx.clone();
                    std::thread::spawn(move || {
                        let ocr_client = client::OcrClient::new(api_key, endpoint, model, prompt);
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
