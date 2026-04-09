use arboard::Clipboard;
use async_channel;
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

use isolang::Language;
use lenzu::capture;
use lenzu::client;
use lenzu::config;
use lenzu::ocr;
use lenzu::utils;

const HISTORY_PATH: &str = "/dev/shm/lenzu/ocr_history.txt";

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
    /// DBNet text detector, shared across capture threads via `Arc`.
    /// `None` when `text_detection_model` is not configured or the `onnx` feature is absent.
    text_detector: Option<std::sync::Arc<dyn ocr::text_detection::TextDetector + Send + Sync>>,
    /// Current HUD vertical position: `true` = top, `false` = bottom.
    /// Auto-toggled when the cursor moves within 30 % of the opposite screen edge.
    hud_at_top: bool,
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

/// Country-flag emoji for common languages; falls back to 🌐.
fn lang_flag(lang: &Language) -> &'static str {
    match lang.to_639_1() {
        Some("ja") => "🇯🇵",
        Some("en") => "🇺🇸",
        Some("zh") => "🇨🇳",
        Some("ko") => "🇰🇷",
        Some("fr") => "🇫🇷",
        Some("de") => "🇩🇪",
        Some("es") => "🇪🇸",
        _ => "🌐",
    }
}

fn ready_status(src: &Language, dest: &Language) -> String {
    format!(
        "{}→{} | Shift+Click | Ctrl+Shift+Click | Shift+H:ヘルプ | ESC",
        lang_flag(src),
        lang_flag(dest),
    )
}

/// Send a position command to lenzu_server so the HUD moves to `"top"` or `"bottom"`.
fn send_hud_position(pos: &str, port: u16) {
    if let Ok(socket) = std::net::UdpSocket::bind("127.0.0.1:0") {
        let addr = format!("127.0.0.1:{port}");
        let msg = serde_json::json!({"type": "position", "pos": pos});
        let _ = socket.send_to(msg.to_string().as_bytes(), addr);
    }
}

/// Modal dialog listing all keyboard shortcuts, displayed in Japanese.
fn show_help_dialog(parent: &gtk::Window) {
    let help = "\
ショートカット一覧
━━━━━━━━━━━━━━━━━━━━━━━━━━
Shift＋クリック
  → レンズ内のテキストをOCR・翻訳

Ctrl＋Shift＋クリック
  → 全画面スキャン
    （カーソル最近傍のテキストを翻訳）

Shift＋Tab
  → 翻訳方向を切り替え
    （🇯🇵→🇺🇸  ⟷  🇺🇸→🇯🇵）

Shift＋H
  → このヘルプを表示

ESC
  → 終了";

    let dialog = gtk::MessageDialog::new(
        Some(parent),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        gtk::MessageType::Info,
        gtk::ButtonsType::Close,
        help,
    );
    dialog.set_title("Lenzu ヘルプ");
    dialog.run();
    dialog.hide();
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure /dev/shm/lenzu/ exists for all runtime output files.
    let _ = std::fs::create_dir_all("/dev/shm/lenzu");

    // OPENROUTER_API_KEY is optional — ollama (local) is the primary backend.
    // When the key is absent, the fallback path is disabled; Ctrl+Shift+Click remote
    // override will show an error in the HUD instead of making a remote call.
    let api_key = std::env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| {
        eprintln!("INFO: OPENROUTER_API_KEY not set — OpenRouter fallback disabled.");
        String::new()
    });

    let cfg = config::AppConfig::load();
    eprintln!(
        "[Config] llm={} | model={} | text_detection_model={} | threshold={} dilation={} pad={}x{}",
        cfg.llm_api_endpoint,
        cfg.llm_default_model,
        cfg.text_detection_model.as_deref().unwrap_or("(none)"),
        cfg.text_detection_threshold,
        cfg.text_detection_dilation,
        cfg.text_detection_pad_x,
        cfg.text_detection_pad_y,
    );

    // Build the text detector once at startup; shared across capture threads via Arc.
    let text_detector: Option<std::sync::Arc<dyn ocr::text_detection::TextDetector + Send + Sync>> =
        match ocr::text_detection::build_text_detector(
            cfg.text_detection_model.as_deref(),
            cfg.text_detection_threshold,
            cfg.text_detection_dilation,
            cfg.text_detection_pad_x,
            cfg.text_detection_pad_y,
        ) {
            Ok(Some(arc)) => {
                eprintln!("[OCR] text detection enabled ({})", cfg.text_detection_model.as_deref().unwrap_or(""));
                Some(arc)
            }
            Ok(None) => None,
            Err(e) => {
                eprintln!("[OCR] failed to load text detector: {e} — falling back to full-image OCR");
                None
            }
        };

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
        status: ready_status(&cfg.translate_src, &cfg.translate_dest),
        last_capture: Instant::now() - Duration::from_secs(2),
        clipboard: Clipboard::new().expect("Failed to init clipboard"),
        api_key,
        is_loading: false,
        spinner_angle: 0.0,
        flash_alpha: 0.0,
        server_process,
        text_detector,
        hud_at_top: false,
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

    let cfg_key = cfg.clone();
    let state_key = state.clone();
    let window_key = window.clone();
    window.connect_key_press_event(move |_, event| {
        let kv = event.keyval();
        let mods = event.state();

        // ESC → quit
        if kv == gdk::keys::constants::Escape {
            kill_server(&mut state_key.borrow_mut().server_process, &cfg_key);
            gtk::main_quit();
            return glib::Propagation::Proceed;
        }

        // Shift+H → Japanese help dialog
        if (kv == gdk::keys::constants::h || kv == gdk::keys::constants::H)
            && mods.contains(gdk::ModifierType::SHIFT_MASK)
        {
            show_help_dialog(&window_key);
            return glib::Propagation::Proceed;
        }

        // Shift+Tab → toggle translation direction (swap src ↔ dest)
        if kv == gdk::keys::constants::ISO_Left_Tab
            || (kv == gdk::keys::constants::Tab
                && mods.contains(gdk::ModifierType::SHIFT_MASK))
        {
            let mut s = state_key.borrow_mut();
            // Language is Copy — read both then assign back to avoid split-borrow error
            let (new_src, new_dest) = (s.config.translate_dest, s.config.translate_src);
            s.config.translate_src = new_src;
            s.config.translate_dest = new_dest;
            s.status = ready_status(&s.config.translate_src, &s.config.translate_dest);
            window_key.queue_draw();
            return glib::Propagation::Proceed;
        }

        glib::Propagation::Proceed
    });

    let cfg_del = cfg.clone();
    let state_del = state.clone();
    window.connect_delete_event(move |_, _| {
        kill_server(&mut state_del.borrow_mut().server_process, &cfg_del);
        glib::Propagation::Proceed // allow window close → GTK loop ends naturally
    });

    let (tx, rx) = async_channel::bounded::<Result<(Vec<client::TranslationResult>, client::OcrMeta), String>>(1);

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
            cr.new_sub_path();
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
    glib::MainContext::default().spawn_local(async move {
        while let Ok(api_result) = rx.recv().await {
        let mut s = state_rx.borrow_mut();
        s.is_loading = false;
        match api_result {
            Ok((results, meta)) => {
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

                let trimmed_english = combined_english.trim();
                if !trimmed_english.is_empty() {
                    if let Ok(mut f) = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(HISTORY_PATH)
                    {
                        let combined_original = results
                            .iter()
                            .map(|r| r.original.clone())
                            .collect::<Vec<_>>()
                            .join(" / ");
                        let _ = writeln!(
                            f,
                            "[{}] ({}, {:.1}s) {} → {}",
                            chrono::Local::now().format("%H:%M:%S"),
                            meta.backend,
                            meta.elapsed_ms as f64 / 1000.0,
                            combined_original.trim(),
                            trimmed_english
                        );
                    } else {
                        eprintln!("[history] failed to open {}", HISTORY_PATH);
                    }
                }
            }
            Err(e) => {
                eprintln!("[OCR] API/parse error: {}", e);
                s.status = format!("API Error: {}", e);
            }
        }
            window_rx.queue_draw();
        }
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
        // Safe GDK accessors — any None means the display isn't ready yet; skip tick.
        let display = match gdk::Display::default() {
            Some(d) => d,
            None => return glib::ControlFlow::Continue,
        };
        let seat = match display.default_seat() {
            Some(s) => s,
            None => return glib::ControlFlow::Continue,
        };
        let device = match seat.pointer() {
            Some(d) => d,
            None => return glib::ControlFlow::Continue,
        };
        let screen = match gdk::Screen::default() {
            Some(s) => s,
            None => return glib::ControlFlow::Continue,
        };
        let root_win = match screen.root_window() {
            Some(w) => w,
            None => return glib::ControlFlow::Continue,
        };
        let (_, x, y, modifier) = root_win.device_position(&device);

        let s_conf = state_main.borrow().config.clone();
        let win_x = x - (s_conf.lens_size / 2);
        let win_y = y - (s_conf.lens_size / 2);

        let is_shift_click = modifier.contains(gdk::ModifierType::SHIFT_MASK)
            && modifier.contains(gdk::ModifierType::BUTTON1_MASK);
        let force_remote = is_shift_click && modifier.contains(gdk::ModifierType::CONTROL_MASK);

        // Show the lens only while Shift is held (preview), while OCR is running,
        // or for 5 s after the last capture (so the user can read the result).
        // Otherwise hide it so the window doesn't follow the cursor everywhere.
        let shift_held = modifier.contains(gdk::ModifierType::SHIFT_MASK);
        let show_lens = {
            let s = state_main.borrow();
            shift_held || s.is_loading || s.last_capture.elapsed() < Duration::from_secs(s.config.result_display_secs)
        };
        if show_lens {
            window_main.move_(win_x, win_y);
            if !window_main.is_visible() {
                window_main.show();
            }
        } else if window_main.is_visible() {
            window_main.hide();
        }

        // ── HUD auto-reposition ───────────────────────────────────────────────
        // Cursor in bottom 30 % → HUD to top; cursor in top 30 % → HUD to bottom.
        // 30–70 % is a dead zone to prevent oscillation.
        {
            let sh = root_win.height();
            let frac = if sh > 0 { y as f32 / sh as f32 } else { 0.5 };
            let (cur_top, port) = {
                let s = state_main.borrow();
                (s.hud_at_top, s.config.overlay_udp_port)
            };
            if frac > 0.70 && !cur_top {
                send_hud_position("top", port);
                state_main.borrow_mut().hud_at_top = true;
            } else if frac < 0.30 && cur_top {
                send_hud_position("bottom", port);
                state_main.borrow_mut().hud_at_top = false;
            }
        }

        if is_shift_click {
            // Check debounce + loading flag, then release the borrow immediately.
            // IMPORTANT: do NOT hold borrow_mut() across gtk::main_iteration() —
            // the animation timer also calls borrow_mut() and will panic (BorrowMutError).
            let should_capture = {
                let s = state_main.borrow();
                s.last_capture.elapsed() > Duration::from_secs(1) && !s.is_loading
            };

            if should_capture {
                // Arm state, extract config values, then DROP borrow before event loop.
                let (fallback_api_key, primary_endpoint, primary_model,
                     free_remote_endpoint, free_remote_model,
                     fallback_endpoint, fallback_model, prompt,
                     per_region_prompt, text_detector) = {
                    let mut s = state_main.borrow_mut();
                    s.last_capture = Instant::now();
                    s.status = if force_remote {
                        "CAPTURING (remote)...".to_string()
                    } else {
                        "CAPTURING...".to_string()
                    };
                    s.is_loading = true;
                    s.flash_alpha = 1.0;
                    let vals = (
                        s.api_key.clone(),
                        s.config.llm_api_endpoint.clone(),
                        s.config.llm_default_model.clone(),
                        s.config.free_remote_endpoint.clone(),
                        s.config.free_remote_model.clone(),
                        s.config.fallback_llm_api_endpoint.clone(),
                        s.config.fallback_llm_model.clone(),
                        s.config.resolved_prompt(),
                        s.config.resolved_per_region_prompt(),
                        s.text_detector.clone(),
                    );
                    vals
                    // borrow_mut dropped here — safe for other callbacks to borrow
                };

                window_main.queue_draw();
                window_main.hide();
                while gtk::events_pending() {
                    gtk::main_iteration();
                }
                std::thread::sleep(Duration::from_millis(400));

                // Feature 2: Ctrl+Shift+Click with DBNet → capture full desktop so DBNet
                // can scan the entire screen for the closest text region to the cursor.
                // Without a detector (no `onnx` feature or no model configured) the normal
                // lens-sized capture is used and sent to the remote backend unchanged.
                let is_fullscreen_scan = force_remote && text_detector.is_some();
                let (cap_x, cap_y, cap_w, cap_h) = if is_fullscreen_scan {
                    let (sw, sh) = capture::screen_size().unwrap_or((1920, 1080));
                    (0i32, 0i32, sw, sh)
                } else {
                    (win_x.max(0), win_y.max(0),
                     s_conf.lens_size as u32, s_conf.lens_size as u32)
                };
                // Cursor position inside the captured image (same as screen coords when
                // cap origin is (0,0); used by closest_box_to_point in Feature 2).
                let cursor_cap_x = (x - cap_x) as u32;
                let cursor_cap_y = (y - cap_y) as u32;

                match capture::capture_x11(cap_x, cap_y, cap_w, cap_h) {
                    Ok(raw) => {
                        let dyn_image = utils::raw_to_dynamic_image(&raw, cap_w, cap_h);

                        // Only update the lens pixbuf for the lens-sized capture; the
                        // full-desktop image is too large to display in the small window.
                        if !is_fullscreen_scan {
                            let mut pb_data = raw.clone();
                            utils::swap_bytes_for_pixbuf(&mut pb_data);
                            let mut s = state_main.borrow_mut();
                            s.pixels = Some(gdk_pixbuf::Pixbuf::from_mut_slice(
                                pb_data,
                                gdk_pixbuf::Colorspace::Rgb,
                                true,
                                8,
                                s_conf.lens_size,
                                s_conf.lens_size,
                                s_conf.lens_size * 4,
                            ));
                        }

                        window_main.show();
                        let tx_clone = tx.clone();
                        std::thread::spawn(move || {
                            // Catch any unexpected panic so is_loading is always reset.
                            // AssertUnwindSafe: the Arc<dyn TextDetector> contains Mutex
                            // interior mutability; we don't rely on its state being
                            // consistent after a panic — each click creates a fresh dual client.
                            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                // Greyscale once; DBNet only needs luminance and this avoids
                                // the crate doing its own (possibly inconsistent) conversion.
                                let gray_image = dyn_image.grayscale();

                                // ── Feature 2: Ctrl+Shift+Click ──────────────────────────────
                                // Full desktop was captured; run DBNet and pick the text region
                                // nearest the cursor, then send that tight crop to remote OCR.
                                if force_remote {
                                    if let Some(ref det) = text_detector {
                                        let boxes = det.detect(&gray_image);
                                        utils::save_fullscreen_debug(&dyn_image, &boxes);
                                        if !boxes.is_empty() {
                                            let idx = ocr::text_detection::closest_box_to_point(
                                                &boxes, cursor_cap_x, cursor_cap_y,
                                            );
                                            let cropper = ocr::text_cropper::TextCropper::new(
                                                s_conf.text_detection_crop_padding, 256,
                                            );
                                            if let Some(crop) = cropper
                                                .crop(&dyn_image, &[boxes[idx].clone()])
                                                .into_iter()
                                                .next()
                                            {
                                                eprintln!(
                                                    "[DBNet] fullscreen: {} boxes, closest idx={} \
                                                     box=({},{})→({},{}) cursor=({},{})",
                                                    boxes.len(), idx,
                                                    boxes[idx].x1, boxes[idx].y1,
                                                    boxes[idx].x2, boxes[idx].y2,
                                                    cursor_cap_x, cursor_cap_y,
                                                );
                                                let dual = client::DualOcrClient::new(
                                                    primary_endpoint,
                                                    primary_model,
                                                    s_conf.local_fallback_models.clone(),
                                                    free_remote_endpoint,
                                                    free_remote_model,
                                                    fallback_endpoint,
                                                    fallback_model,
                                                    fallback_api_key,
                                                    s_conf.fallback_max_dimension,
                                                    s_conf.primary_num_ctx,
                                                    s_conf.local_timeout_secs,
                                                    s_conf.remote_timeout_secs,
                                                    s_conf.paid_remote_timeout_secs,
                                                    prompt,
                                                );
                                                return dual
                                                    .call_api_force_fallback(&crop.image)
                                                    .map_err(|e| e.to_string());
                                            }
                                        }
                                        return Err(
                                            "No text detected near cursor".to_string()
                                        );
                                    }
                                    // No detector — fall through to Phase 1 with lens image
                                }

                                // ── Feature 1: Shift+Click (per-region DBNet) ────────────────
                                // DBNet detects all text regions in the lens capture; each crop
                                // is sent as a separate OCR request with the per-region prompt.
                                // Falls back to full-image OCR if detection finds nothing or all
                                // per-region calls fail.
                                //
                                // OPT-1: keep the detected boxes so the Phase 1 fallback can
                                // crop to the union bbox instead of sending the full lens image.
                                let mut detected_boxes: Vec<ocr::text_detection::TextBoundingBox> = Vec::new();
                                if !force_remote {
                                    if let Some(ref det) = text_detector {
                                        let boxes = det.detect(&gray_image);
                                        if !boxes.is_empty() {
                                            detected_boxes = boxes.clone();
                                            let cropper = ocr::text_cropper::TextCropper::new(
                                                s_conf.text_detection_crop_padding,
                                                256,
                                            );
                                            let crops = cropper.crop(&dyn_image, &boxes);
                                            if !crops.is_empty() {
                                                let dual_region = client::DualOcrClient::new(
                                                    primary_endpoint.clone(),
                                                    primary_model.clone(),
                                                    s_conf.local_fallback_models.clone(),
                                                    free_remote_endpoint.clone(),
                                                    free_remote_model.clone(),
                                                    fallback_endpoint.clone(),
                                                    fallback_model.clone(),
                                                    fallback_api_key.clone(),
                                                    s_conf.fallback_max_dimension,
                                                    s_conf.primary_num_ctx,
                                                    s_conf.local_timeout_secs,
                                                    s_conf.remote_timeout_secs,
                                                    s_conf.paid_remote_timeout_secs,
                                                    per_region_prompt,
                                                );
                                                let mut all_results = Vec::new();
                                                let mut last_meta = None;
                                                for crop in crops {
                                                    match dual_region.call_api(&crop.image) {
                                                        Ok((mut results, meta)) => {
                                                            // top_xy/bot_xy come from the DBNet
                                                            // box (lens coords), not from the LLM.
                                                            for r in &mut results {
                                                                r.top_xy = Some(format!(
                                                                    "{},{}",
                                                                    crop.source_box.x1,
                                                                    crop.source_box.y1
                                                                ));
                                                                r.bot_xy = Some(format!(
                                                                    "{},{}",
                                                                    crop.source_box.x2,
                                                                    crop.source_box.y2
                                                                ));
                                                            }
                                                            all_results.extend(results);
                                                            last_meta = Some(meta);
                                                        }
                                                        Err(e) => {
                                                            eprintln!(
                                                                "[OCR] per-region call failed: {e}"
                                                            );
                                                        }
                                                    }
                                                }
                                                if !all_results.is_empty() {
                                                    return Ok((all_results, last_meta.unwrap()));
                                                }
                                                // All per-region calls failed — fall through to
                                                // full-image OCR below.
                                            }
                                        }
                                    }
                                }

                                // ── Phase 1 fallback ─────────────────────────────────────────
                                // No detector, no boxes detected, all per-region calls failed,
                                // or force_remote without a detector (lens image → remote).
                                let dual = client::DualOcrClient::new(
                                    primary_endpoint,
                                    primary_model,
                                    s_conf.local_fallback_models.clone(),
                                    free_remote_endpoint,
                                    free_remote_model,
                                    fallback_endpoint,
                                    fallback_model,
                                    fallback_api_key,
                                    s_conf.fallback_max_dimension,
                                    s_conf.primary_num_ctx,
                                    s_conf.local_timeout_secs,
                                    s_conf.remote_timeout_secs,
                                    s_conf.paid_remote_timeout_secs,
                                    prompt,
                                );
                                // OPT-1: if DBNet found boxes but all per-region calls failed,
                                // crop to the union bbox rather than sending the full lens image.
                                // When no boxes were detected (or force_remote), falls back to
                                // the full dyn_image unchanged.
                                let union_crop = if !force_remote {
                                    ocr::text_detection::compute_union_bbox(&detected_boxes)
                                        .and_then(|union| {
                                            let cropper = ocr::text_cropper::TextCropper::new(
                                                s_conf.text_detection_crop_padding, 0,
                                            );
                                            cropper.crop(&dyn_image, &[union])
                                                .into_iter().next().map(|c| c.image)
                                        })
                                } else {
                                    None
                                };
                                let fallback_img = union_crop.as_ref().unwrap_or(&dyn_image);
                                if force_remote {
                                    dual.call_api_force_fallback(fallback_img)
                                } else {
                                    dual.call_api(fallback_img)
                                }.map_err(|e| e.to_string())
                            }))
                            .unwrap_or_else(|_| Err("OCR thread panicked".to_string()));
                            let _ = tx_clone.send_blocking(result);
                        });
                    }
                    Err(_) => {
                        let mut s = state_main.borrow_mut();
                        s.is_loading = false;
                        s.status = "Capture Failed".to_string();
                        window_main.show();
                    }
                }
            }
        }
        glib::ControlFlow::Continue
    });

    window.show_all();

    // Make the entire window click-through so mouse events (clicks, scroll wheel)
    // pass through to whatever is underneath.  Lenzu detects Shift+Click by polling
    // the root window — it never needed to *receive* mouse events directly.
    // Note: ESC still works after alt+tabbing to the Lenzu window (or Ctrl+C in terminal).
    if let Some(gdk_win) = gtk::prelude::WidgetExt::window(&window) {
        // An empty cairo::Region means no area accepts pointer input → fully click-through.
        let empty = cairo::Region::create();
        gdk_win.input_shape_combine_region(&empty, 0, 0);
    }

    gtk::main();
    Ok(())
}
