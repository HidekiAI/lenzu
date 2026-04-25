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
use lenzu::furigana;
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
            Furigana => r.furigana.clone()
                .or_else(|| r.romaji.clone())
                .unwrap_or_else(|| r.original.clone()),
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
    /// manga-ocr-rs local OCR handle, shared across threads.
    /// When both detection and OCR confidence >= 71%, results are returned
    /// immediately without hitting any LLM service.
    local_ocr: Option<std::sync::Arc<manga_ocr_rs::MangaOcr>>,
    /// Current HUD vertical position: `true` = top, `false` = bottom.
    /// Auto-toggled when the cursor moves within 30 % of the opposite screen edge.
    hud_at_top: bool,
    /// Cloned handle to the tokio runtime owned by `main`. Used by the OCR
    /// worker to spawn async tasks. Clone freely — handles are cheap and
    /// the runtime is dropped when `main` returns.
    tokio_handle: tokio::runtime::Handle,
    /// JoinHandle of the in-flight OCR task, if any. Taking and calling
    /// abort() on this closes the underlying reqwest TCP socket, so the
    /// remote backend stops billing/computing.
    in_flight: Option<tokio::task::JoinHandle<()>>,
    /// Monotonically increasing generation id. Bumped every time a new
    /// capture starts (or the in-flight task is cancelled). The worker
    /// stamps every preview/result with the gen it was spawned under;
    /// the receiver drops mismatches so no stale frame paints the HUD.
    /// Step 6 wires the stamp + filter; step 5 only maintains the counter.
    current_generation: u64,
}

impl AppState {
    /// Cancel any in-flight OCR task and advance the generation counter.
    /// Called from every shift-modified interaction: shift+click,
    /// ctrl+shift+click, shift+tab, shift+h, and shift+esc.
    fn cancel_inflight(&mut self, reason: &'static str) {
        if let Some(h) = self.in_flight.take() {
            eprintln!("[OCR] cancel: {reason}");
            h.abort();
        }
        self.current_generation = self.current_generation.wrapping_add(1);
    }
}

/// Path to `lenzu_server` directory (dev tree only).
/// Resolved at runtime from the binary location (target/debug/lenzu →
/// ../../lenzu_server). Returns None when the binary is installed system-wide
/// (e.g. /usr/bin/lenzu) — in that case the packaged `lenzu-hud` binary on
/// PATH is used instead.
fn lenzu_server_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| {
            // binary: <repo>/target/debug/lenzu  →  parent×2 = <repo>/target  →  parent×3 = <repo>
            p.parent()?.parent()?.parent().map(|r| r.join("lenzu_server"))
        })
        .filter(|p| p.is_dir())
        .or_else(|| {
            let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../lenzu_server");
            manifest.is_dir().then_some(manifest)
        })
}

/// Spawn the Electron HUD. UDP port is passed via `LENZU_OVERLAY_UDP_PORT` so it
/// stays in sync with `overlay_udp_port` in `lenzu_config.json`.
///
/// Lookup order:
///   1. `lenzu-hud` on PATH — packaged install (electron-builder .deb).
///   2. Dev-tree fallback: `<repo>/lenzu_server/node_modules/.bin/electron dist/main.js`.
///
/// The HUD child is placed in its own process group so we can kill the entire
/// Electron tree on exit (Electron forks a renderer + zygote).
fn spawn_server(port: u16) -> Option<std::process::Child> {
    let mut cmd = if let Some(dir) = lenzu_server_dir() {
        // Dev tree: launch Electron directly so the child PID is the Electron process.
        let mut c = std::process::Command::new("node_modules/.bin/electron");
        c.args(["dist/main.js"]).current_dir(&dir);
        c
    } else {
        // Packaged: lenzu-hud is electron-builder's launcher binary on PATH.
        std::process::Command::new("lenzu-hud")
    };

    cmd.env("LENZU_OVERLAY_UDP_PORT", port.to_string())
        .env("GTK_CSD", "0")
        .process_group(0)
        .spawn()
        .map_err(|e| eprintln!("lenzu: could not spawn HUD (lenzu-hud or dev tree): {e}"))
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

/// Resolve the path to one of the NOTICES files.  Tries dev tree first
/// (`<repo>/lenzu/<filename>`), then the packaged location
/// (`/usr/share/doc/lenzu/<filename>`).
fn notices_path(filename: &str) -> Option<std::path::PathBuf> {
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join(filename);
    if dev.is_file() {
        return Some(dev);
    }
    let packaged = Path::new("/usr/share/doc/lenzu").join(filename);
    packaged.is_file().then_some(packaged)
}

/// Scrollable dialog showing third-party license attributions.
/// Loads the curated `NOTICES.md` and (if present) the auto-generated
/// `NOTICES.crates.md` from `cargo-about`, concatenating with a separator.
/// Falls back to a short "see /usr/share/doc/lenzu/" message if both are missing.
fn show_about_dialog(parent: &gtk::Window) {
    let read = |name: &str| -> Option<String> {
        notices_path(name).and_then(|p| std::fs::read_to_string(p).ok())
    };
    let body = match (read("NOTICES.md"), read("NOTICES.crates.md")) {
        (Some(curated), Some(crates)) => format!("{curated}\n\n---\n\n{crates}"),
        (Some(curated), None) => curated,
        (None, Some(crates)) => crates,
        (None, None) => "Third-party notices not found.  See:\n\
             /usr/share/doc/lenzu/NOTICES.md\n\
             https://github.com/hidekiai/lenzu"
            .to_string(),
    };

    let dialog = gtk::Dialog::with_buttons(
        Some("Lenzu — About / Third-Party Notices"),
        Some(parent),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        &[("Close", gtk::ResponseType::Close)],
    );
    dialog.set_default_size(640, 480);

    let scrolled = gtk::ScrolledWindow::new(gtk::Adjustment::NONE, gtk::Adjustment::NONE);
    scrolled.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);

    let text_view = gtk::TextView::new();
    text_view.set_editable(false);
    text_view.set_cursor_visible(false);
    text_view.set_wrap_mode(gtk::WrapMode::Word);
    text_view.set_left_margin(12);
    text_view.set_right_margin(12);
    text_view.set_top_margin(8);
    text_view.set_bottom_margin(8);
    text_view.buffer().expect("text view buffer").set_text(&body);

    scrolled.add(&text_view);
    dialog.content_area().pack_start(&scrolled, true, true, 0);
    dialog.show_all();
    dialog.run();
    dialog.close();
}

/// Modal dialog listing all keyboard shortcuts, displayed in Japanese.
/// Buttons: "About" (opens the third-party notices dialog) and "Close"
/// (Esc-style exit).  Clicking About dismisses Help and shows Notices —
/// re-opening Help is one Shift+H away.  Linear flow avoids GTK's nested-
/// event-loop hazards from re-running the same dialog within a loop.
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
  → 実行中のOCRをキャンセル

Shift＋ESC
  → 終了";

    let about_response = gtk::ResponseType::Other(1);
    let dialog = gtk::MessageDialog::new(
        Some(parent),
        gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
        gtk::MessageType::Info,
        gtk::ButtonsType::None,
        help,
    );
    dialog.set_title("Lenzu ヘルプ");
    dialog.add_button("About", about_response);
    dialog.add_button("Close", gtk::ResponseType::Close);
    let response = dialog.run();
    dialog.close();
    if response == about_response {
        show_about_dialog(parent);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ensure /dev/shm/lenzu/ exists for all runtime output files.
    let _ = std::fs::create_dir_all("/dev/shm/lenzu");

    // Multi-threaded tokio runtime owned for the lifetime of the process.
    // The OCR worker spawns async tasks onto it so a new shift+* input can
    // cancel an in-flight HTTP request. GTK/glib still runs on the main
    // thread with its own executor — the two cooperate via `async-channel`.
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");
    let tokio_handle = tokio_runtime.handle().clone();

    // OPENROUTER_API_KEY is optional — ollama (local) is the primary backend.
    // When the key is absent, the fallback path is disabled; Ctrl+Shift+Click remote
    // override will show an error in the HUD instead of making a remote call.
    let api_key = std::env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| {
        eprintln!("INFO: OPENROUTER_API_KEY not set — OpenRouter fallback disabled.");
        String::new()
    });

    let mut cfg = config::AppConfig::load();

    // CLI override: --furigana_only skips LLM enrichment and romaji
    if std::env::args().any(|a| a == "--furigana_only") {
        cfg.furigana_only = true;
        eprintln!("[Config] --furigana_only: MeCab furigana only, LLM enrichment disabled");
    }

    // CLI override: --nomecab_overwrite disables MeCab furigana overwrite on LLM fallback results
    // (comparison + warning logging still runs regardless)
    if std::env::args().any(|a| a == "--nomecab_overwrite") {
        cfg.mecab_overwrite = false;
        eprintln!("[Config] --nomecab_overwrite: MeCab will compare but NOT overwrite LLM furigana");
    }

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
    eprintln!(
        "[Config] flags: furigana_only={} mecab_overwrite={} enrichment_enabled={} overlay_enabled={}",
        cfg.furigana_only, cfg.mecab_overwrite, cfg.enrichment_enabled, cfg.overlay_enabled,
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

    // Load manga-ocr-rs models once at startup for local-first OCR.
    // Requires the text detector to be enabled — no point doing local OCR
    // without detection to produce bounding boxes.
    let local_ocr: Option<std::sync::Arc<manga_ocr_rs::MangaOcr>> = if text_detector.is_some() {
        match ocr::local_ocr::LocalOcrEngine::new() {
            Ok(engine) => {
                eprintln!("[OCR] local manga-ocr loaded — confidence-gated pipeline active");
                Some(engine.handle())
            }
            Err(e) => {
                eprintln!("[OCR] manga-ocr unavailable ({e}) — local-first pipeline disabled");
                None
            }
        }
    } else {
        None
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
        local_ocr,
        hud_at_top: false,
        tokio_handle: tokio_handle.clone(),
        in_flight: None,
        current_generation: 0,
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

        // Plain ESC → cancel in-flight OCR (no-op if nothing is in flight).
        // Shift+ESC → quit the app.
        if kv == gdk::keys::constants::Escape {
            if mods.contains(gdk::ModifierType::SHIFT_MASK) {
                kill_server(&mut state_key.borrow_mut().server_process, &cfg_key);
                gtk::main_quit();
                return glib::Propagation::Proceed;
            }
            let mut s = state_key.borrow_mut();
            s.cancel_inflight("user-cancel");
            s.is_loading = false;
            s.status = ready_status(&s.config.translate_src, &s.config.translate_dest);
            window_key.queue_draw();
            return glib::Propagation::Proceed;
        }

        // Shift+H → Japanese help dialog
        if (kv == gdk::keys::constants::h || kv == gdk::keys::constants::H)
            && mods.contains(gdk::ModifierType::SHIFT_MASK)
        {
            {
                let mut s = state_key.borrow_mut();
                s.cancel_inflight("help-dialog");
                s.is_loading = false;
            }
            show_help_dialog(&window_key);
            return glib::Propagation::Proceed;
        }

        // Shift+Tab → toggle translation direction (swap src ↔ dest)
        if kv == gdk::keys::constants::ISO_Left_Tab
            || (kv == gdk::keys::constants::Tab
                && mods.contains(gdk::ModifierType::SHIFT_MASK))
        {
            let mut s = state_key.borrow_mut();
            s.cancel_inflight("direction-toggle");
            s.is_loading = false;
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

    // (gen_id, result) — gen_id lets the receiver drop stale frames from
    // a cancelled generation that were already in the buffer before abort()
    // fired. abort() stops future sends, but the channel may hold sends that
    // ran just before the cancel; the gen stamp is how we filter those out.
    let (tx, rx) = async_channel::bounded::<(u64, Result<(Vec<client::TranslationResult>, client::OcrMeta), String>)>(3);

    let state_draw = state.clone();
    window.connect_draw(move |win, cr| {
        let s = state_draw.borrow();
        // HUD color: override based on session paid token spend
        let (r, g, b) = {
            let (sp, sc) = client::session_paid_tokens();
            let total = sp + sc;
            if s.config.token_critical_threshold > 0 && total >= s.config.token_critical_threshold {
                (1.0, 0.27, 0.27) // red (#FF4444)
            } else if s.config.token_warning_threshold > 0 && total >= s.config.token_warning_threshold {
                (1.0, 0.65, 0.0) // orange (#FFA600)
            } else {
                hex_to_rgb(&s.config.hud_color_hex)
            }
        };

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
        while let Ok((msg_gen, api_result)) = rx.recv().await {
        let mut s = state_rx.borrow_mut();
        // Drop results from a cancelled / superseded generation so stale
        // frames never paint the HUD. abort() kills future sends; this
        // handles the window where sends already sat in the channel buffer
        // before abort() fired.
        if msg_gen != s.current_generation {
            continue;
        }
        match api_result {
            Ok((results, meta)) => {
                // Preview results show text immediately but keep the lens modal
                // (is_loading stays true) so the user can't stack new requests.
                if !meta.preview {
                    s.is_loading = false;
                }

                let combined_english = results
                    .iter()
                    .map(|r| r.english.clone().unwrap_or_else(|| r.original.clone()))
                    .collect::<Vec<_>>()
                    .join("\n");

                s.ocr_result = combined_english.clone();
                s.status = if meta.preview {
                    format!("OCR done ({} items) — enriching…", results.len())
                } else {
                    format!("SUCCESS ({} items)", results.len())
                };

                if s.config.overlay_enabled {
                    let text = format_for_overlay(&results, &s.config.overlay_render_mode);
                    eprintln!("[HUD] Overlay enabled – prepared text: {}", text);
                    send_to_overlay(&text, s.config.overlay_udp_port);
                }

                // Only update clipboard and history on final results (not preview)
                if !meta.preview {
                    let combined_original = results
                        .iter()
                        .map(|r| r.original.clone())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let _ = s.clipboard.set_text(combined_original);

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
            }
            Err(e) => {
                s.is_loading = false;
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
            // A new shift-modified click supersedes any in-flight OCR — abort
            // the old request's TCP socket so the remote backend stops billing,
            // and clear is_loading so the new capture isn't blocked by the old
            // task's un-run completion handler.
            {
                let mut s = state_main.borrow_mut();
                if s.in_flight.is_some() {
                    s.cancel_inflight("new-capture");
                    s.is_loading = false;
                }
            }

            // 1s debounce only — the is_loading gate was dropped now that
            // cancel_inflight above supersedes any in-progress OCR. The
            // debounce still prevents accidental double-clicks from firing
            // two captures in rapid succession (which is a different concern
            // from "user deliberately clicked again because they're tired
            // of waiting" — that's handled by cancel_inflight).
            // IMPORTANT: do NOT hold borrow_mut() across gtk::main_iteration() —
            // the animation timer also calls borrow_mut() and will panic.
            let should_capture = {
                let s = state_main.borrow();
                s.last_capture.elapsed() > Duration::from_secs(1)
            };

            if should_capture {
                // Arm state, extract config values, then DROP borrow before event loop.
                let (fallback_api_key, primary_endpoint, primary_model,
                     free_remote_endpoint, free_remote_model,
                     fallback_endpoint, fallback_model, prompt,
                     per_region_prompt, text_detector, local_ocr,
                     enrichment_enabled, enrichment_model, enrichment_prompt,
                     enrichment_timeout_secs) = {
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
                        s.local_ocr.clone(),
                        s.config.enrichment_enabled,
                        s.config.enrichment_model.clone(),
                        s.config.resolved_enrichment_prompt(),
                        s.config.enrichment_timeout_secs,
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
                        let (tokio_handle_thread, gen_id) = {
                            let s = state_main.borrow();
                            (s.tokio_handle.clone(), s.current_generation)
                        };
                        // Spawn on the shared tokio runtime and stash the JoinHandle in
                        // AppState so a later shift-modified interaction can call abort()
                        // on it — which closes the in-flight reqwest TCP socket so the
                        // remote backend stops billing/computing.
                        let handle = tokio_handle_thread.spawn(async move {
                            let mut result: Result<(Vec<client::TranslationResult>, client::OcrMeta), String> = async {
                                // Greyscale once; DBNet only needs luminance and this avoids
                                // the crate doing its own (possibly inconsistent) conversion.
                                // CPU work inside an async task — block_in_place yields the
                                // tokio worker slot so other tasks can progress. Safe here
                                // because the outer runtime is rt-multi-thread.
                                let gray_image = tokio::task::block_in_place(|| dyn_image.grayscale());

                                // ── Feature 2: Ctrl+Shift+Click ──────────────────────────────
                                // Full desktop was captured; run DBNet and pick the text region
                                // nearest the cursor, then send that tight crop to remote OCR.
                                if force_remote {
                                    if let Some(ref det) = text_detector {
                                        let mut boxes = tokio::task::block_in_place(|| det.detect(&gray_image));
                                        utils::save_fullscreen_debug(&dyn_image, &boxes);

                                        if !boxes.is_empty() {
                                            let mut idx = ocr::text_detection::closest_box_to_point(
                                                &boxes, cursor_cap_x, cursor_cap_y,
                                            );

                                            // BUG-4: if the chosen box is an over-merge (spans > 60%
                                            // of screen width OR height), crop to that box and re-run
                                            // DBNet on just that sub-image using scale-table params
                                            // appropriate for the sub-image size.  Sub-box coordinates
                                            // are remapped back to original image space (+ox, +oy) so
                                            // the cursor position stays valid for closest_box_to_point.
                                            let chosen_w = boxes[idx].width();
                                            let chosen_h = boxes[idx].height();
                                            if chosen_w as u64 > cap_w as u64 * 6 / 10
                                                || chosen_h as u64 > cap_h as u64 * 6 / 10
                                            {
                                                let entry = s_conf.detection_params_for(chosen_w, chosen_h);
                                                if let Ok(Some(det_nd)) = ocr::text_detection::build_text_detector(
                                                    s_conf.text_detection_model.as_deref(),
                                                    entry.threshold,
                                                    entry.dilation,
                                                    entry.pad_x,
                                                    entry.pad_y,
                                                ) {
                                                    let ox = boxes[idx].x1;
                                                    let oy = boxes[idx].y1;
                                                    let sub_boxes = tokio::task::block_in_place(|| {
                                                        let sub = dyn_image.crop_imm(ox, oy, chosen_w, chosen_h);
                                                        let sub_gray = sub.grayscale();
                                                        det_nd.detect(&sub_gray)
                                                    });
                                                    // Remap from crop-space → original image space so
                                                    // cursor coords stay valid for closest_box_to_point.
                                                    let remapped: Vec<ocr::text_detection::TextBoundingBox> =
                                                        sub_boxes.into_iter().map(|b| {
                                                            ocr::text_detection::TextBoundingBox {
                                                                x1: b.x1 + ox,
                                                                y1: b.y1 + oy,
                                                                x2: b.x2 + ox,
                                                                y2: b.y2 + oy,
                                                                confidence: b.confidence,
                                                                contours: b.contours,
                                                            }
                                                        }).collect();
                                                    if remapped.len() > boxes.len() {
                                                        eprintln!(
                                                            "[DBNet] BUG-4 retry (sub-crop {}×{}, \
                                                             dilation={} thr={:.2}): {} boxes \
                                                             (was {}, chosen {:.0}%w {:.0}%h of screen)",
                                                            chosen_w, chosen_h,
                                                            entry.dilation, entry.threshold,
                                                            remapped.len(),
                                                            boxes.len(),
                                                            chosen_w as f64 / cap_w as f64 * 100.0,
                                                            chosen_h as f64 / cap_h as f64 * 100.0,
                                                        );
                                                        boxes = remapped;
                                                        idx = ocr::text_detection::closest_box_to_point(
                                                            &boxes, cursor_cap_x, cursor_cap_y,
                                                        );
                                                        utils::save_fullscreen_debug(&dyn_image, &boxes);
                                                    }
                                                }
                                            }
                                            // ── Local-first OCR on chosen box ──────────
                                            // Try manga-ocr-rs before hitting the LLM chain.
                                            if let Some(ref mocr) = local_ocr {
                                                let chosen = &boxes[idx];
                                                if chosen.confidence >= 0.71 {
                                                    let (local_result, _) = tokio::task::block_in_place(|| {
                                                        let engine = ocr::local_ocr::LocalOcrEngine::from_arc(
                                                            std::sync::Arc::clone(mocr),
                                                        );
                                                        engine.try_local_pipeline(
                                                            &dyn_image, &[chosen.clone()],
                                                            s_conf.text_detection_crop_padding, 256,
                                                            Some(s_conf.low_conf_max_chars),
                                                        )
                                                    });
                                                    if let Some(results) = local_result {
                                                        let mut t_results: Vec<client::TranslationResult> = results.iter().map(|r| {
                                                            client::TranslationResult {
                                                                original: r.text.clone(),
                                                                top_xy: Some(format!("{},{}", r.source_box.x1, r.source_box.y1)),
                                                                bot_xy: Some(format!("{},{}", r.source_box.x2, r.source_box.y2)),
                                                                debug_info: Some(format!(
                                                                    "local-ocr det:{:.0}% ocr:{:.1}% {}ms",
                                                                    r.source_box.confidence * 100.0,
                                                                    r.confidence * 100.0, r.ocr_ms,
                                                                )),
                                                                ..Default::default()
                                                            }
                                                        }).collect();
                                                        let total_ocr_ms: u128 = results.iter().map(|r| r.ocr_ms).sum();
                                                        eprintln!("[OCR] fullscreen local-first succeeded — skipping LLM chain");
                                                        // Phase 1: raw text preview
                                                        let _ = tx_clone.send((gen_id, Ok((t_results.clone(), client::OcrMeta {
                                                            backend: "local:manga-ocr".to_string(),
                                                            elapsed_ms: total_ocr_ms,
                                                            preview: true,
                                                        })))).await;

                                                        // Phase 2: furigana (+ romaji unless furigana_only) — MeCab, ~5ms
                                                        let furigana_ok = furigana::annotate(&mut t_results, s_conf.furigana_only);
                                                        let do_enrich = enrichment_enabled && !s_conf.furigana_only;
                                                        if furigana_ok && do_enrich {
                                                            let _ = tx_clone.send((gen_id, Ok((t_results.clone(), client::OcrMeta {
                                                                backend: "local:manga-ocr+furigana".to_string(),
                                                                elapsed_ms: total_ocr_ms,
                                                                preview: true,
                                                            })))).await;
                                                        }

                                                        // Phase 3: LLM enrichment (translation) — skipped in furigana_only mode
                                                        let mut enriched = false;
                                                        if do_enrich {
                                                            let enrich_model = enrichment_model.as_deref().unwrap_or(&primary_model);
                                                            enriched = client::enrich_local_results(
                                                                &mut t_results, &primary_endpoint, enrich_model,
                                                                &enrichment_prompt, enrichment_timeout_secs,
                                                                s_conf.primary_num_ctx,
                                                            ).await;
                                                        }

                                                        // Final send (via closure return → line 1140)
                                                        let backend = match (furigana_ok, enriched) {
                                                            (true, true)  => "local:manga-ocr+furigana+enriched",
                                                            (true, false) => "local:manga-ocr+furigana",
                                                            (false, true) => "local:manga-ocr+enriched",
                                                            (false, false) => "local:manga-ocr",
                                                        };
                                                        return Ok((t_results, client::OcrMeta {
                                                            backend: backend.to_string(),
                                                            elapsed_ms: total_ocr_ms,
                                                            preview: false,
                                                        }));
                                                    }
                                                    eprintln!("[OCR] fullscreen local-first: confidence below gate — falling through to LLM");
                                                }
                                            }

                                            let cropper = ocr::text_cropper::TextCropper::new(
                                                s_conf.text_detection_crop_padding, 256,
                                            ).with_pad_percent(0.10);
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
                                                    s_conf.primary_max_dimension,
                                                    s_conf.primary_num_ctx,
                                                    s_conf.local_timeout_secs,
                                                    s_conf.remote_timeout_secs,
                                                    s_conf.paid_remote_timeout_secs,
                                                    prompt,
                                                );
                                                return dual
                                                    .call_api_force_fallback(&crop.image)
                                                    .await
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
                                        let boxes = tokio::task::block_in_place(|| {
                                            let raw_boxes = det.detect(&gray_image);
                                            // Refine: merge overlapping clusters and
                                            // re-detect within each union region to
                                            // split stacked bubbles / remove bubble-wrap dupes.
                                            ocr::local_ocr::refine_boxes(
                                                &raw_boxes, &dyn_image, det.as_ref(),
                                            )
                                        });
                                        // Save lens debug image with refined bounding boxes.
                                        utils::save_lens_debug(&dyn_image, &boxes);

                                        if !boxes.is_empty() {
                                            detected_boxes = boxes.clone();

                                            // ── Local-first OCR (manga-ocr-rs) ─────────────────
                                            // If all boxes have detection confidence >= 71% AND
                                            // manga-ocr-rs returns OCR confidence >= 71% for each,
                                            // return immediately — no LLM needed.
                                            if let Some(ref mocr) = local_ocr {
                                                // Incremental: send each box's result to
                                                // HUD as it completes so text accumulates
                                                // on screen instead of flashing.
                                                let tx_progress = tx_clone.clone();
                                                let furigana_only_flag = s_conf.furigana_only;
                                                let (local_result, _partials) = tokio::task::block_in_place(|| {
                                                    let engine = ocr::local_ocr::LocalOcrEngine::from_arc(
                                                        std::sync::Arc::clone(mocr),
                                                    );
                                                    engine.try_local_pipeline_incremental(
                                                    &dyn_image,
                                                    &boxes,
                                                    s_conf.text_detection_crop_padding,
                                                    256,
                                                    Some(s_conf.low_conf_max_chars),
                                                    |accumulated| {
                                                        // Show all results as preview — even low-
                                                        // confidence garbage — so the HUD indicates
                                                        // the system is working while the LLM fallback
                                                        // chain produces the final accurate result.
                                                        let mut t_results: Vec<client::TranslationResult> = accumulated.iter().map(|r| {
                                                            client::TranslationResult {
                                                                original: r.text.clone(),
                                                                top_xy: Some(format!("{},{}", r.source_box.x1, r.source_box.y1)),
                                                                bot_xy: Some(format!("{},{}", r.source_box.x2, r.source_box.y2)),
                                                                debug_info: Some(format!(
                                                                    "local-ocr det:{:.0}% ocr:{:.1}% {}ms",
                                                                    r.source_box.confidence * 100.0,
                                                                    r.confidence * 100.0,
                                                                    r.ocr_ms,
                                                                )),
                                                                ..Default::default()
                                                            }
                                                        }).collect();
                                                        furigana::annotate(&mut t_results, furigana_only_flag);
                                                        let total_ms: u128 = accumulated.iter().map(|r| r.ocr_ms).sum();
                                                        let _ = tx_progress.send_blocking((gen_id, Ok((t_results, client::OcrMeta {
                                                            backend: format!("local:manga-ocr+furigana ({}/{})", accumulated.len(), accumulated.len()),
                                                            elapsed_ms: total_ms,
                                                            preview: true,
                                                        }))));
                                                    },
                                                )
                                                });
                                                if let Some(results) = local_result {
                                                    let mut t_results: Vec<client::TranslationResult> = results.iter().map(|r| {
                                                        client::TranslationResult {
                                                            original: r.text.clone(),
                                                            top_xy: Some(format!("{},{}", r.source_box.x1, r.source_box.y1)),
                                                            bot_xy: Some(format!("{},{}", r.source_box.x2, r.source_box.y2)),
                                                            debug_info: Some(format!(
                                                                "local-ocr det:{:.0}% ocr:{:.1}% {}ms",
                                                                r.source_box.confidence * 100.0,
                                                                r.confidence * 100.0,
                                                                r.ocr_ms,
                                                            )),
                                                            ..Default::default()
                                                        }
                                                    }).collect();
                                                    let total_ocr_ms: u128 = results.iter().map(|r| r.ocr_ms).sum();
                                                    eprintln!("[OCR] local-first pipeline succeeded — {} results, skipping LLM chain", t_results.len());

                                                    // Phase 2: furigana (+ romaji unless furigana_only) — MeCab, ~5ms
                                                    let furigana_ok = furigana::annotate(&mut t_results, s_conf.furigana_only);
                                                    let do_enrich = enrichment_enabled && !s_conf.furigana_only;
                                                    if furigana_ok && do_enrich {
                                                        let _ = tx_clone.send((gen_id, Ok((t_results.clone(), client::OcrMeta {
                                                            backend: "local:manga-ocr+furigana".to_string(),
                                                            elapsed_ms: total_ocr_ms,
                                                            preview: true,
                                                        })))).await;
                                                    }

                                                    // Phase 3: LLM enrichment (translation) — skipped in furigana_only mode
                                                    let mut enriched = false;
                                                    if do_enrich {
                                                        let enrich_model = enrichment_model.as_deref().unwrap_or(&primary_model);
                                                        enriched = client::enrich_local_results(
                                                            &mut t_results, &primary_endpoint, enrich_model,
                                                            &enrichment_prompt, enrichment_timeout_secs,
                                                            s_conf.primary_num_ctx,
                                                        ).await;
                                                    }

                                                    // Final send (via closure return)
                                                    let backend = match (furigana_ok, enriched) {
                                                        (true, true)  => "local:manga-ocr+furigana+enriched",
                                                        (true, false) => "local:manga-ocr+furigana",
                                                        (false, true) => "local:manga-ocr+enriched",
                                                        (false, false) => "local:manga-ocr",
                                                    };
                                                    return Ok((t_results, client::OcrMeta {
                                                        backend: backend.to_string(),
                                                        elapsed_ms: total_ocr_ms,
                                                        preview: false,
                                                    }));
                                                }
                                                eprintln!("[OCR] local-first pipeline: confidence below gate — falling through to LLM chain");
                                            }

                                            let cropper = ocr::text_cropper::TextCropper::new(
                                                s_conf.text_detection_crop_padding,
                                                256,
                                            ).with_pad_percent(0.10);
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
                                                    s_conf.primary_max_dimension,
                                                    s_conf.primary_num_ctx,
                                                    s_conf.local_timeout_secs,
                                                    s_conf.remote_timeout_secs,
                                                    s_conf.paid_remote_timeout_secs,
                                                    per_region_prompt,
                                                );
                                                let mut all_results = Vec::new();
                                                let mut last_meta = None;
                                                for crop in crops {
                                                    match dual_region.call_api(&crop.image).await {
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
                                    s_conf.primary_max_dimension,
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
                                            ).with_pad_percent(0.10);
                                            cropper.crop(&dyn_image, &[union])
                                                .into_iter().next().map(|c| c.image)
                                        })
                                } else {
                                    None
                                };
                                let fallback_img = union_crop.as_ref().unwrap_or(&dyn_image);
                                if force_remote {
                                    dual.call_api_force_fallback(fallback_img).await
                                } else {
                                    dual.call_api(fallback_img).await
                                }.map_err(|e| e.to_string())
                            }.await;

                            // MeCab check: always compare LLM furigana against MeCab's
                            // dictionary-based readings (logs timing + MATCH/MISMATCH).
                            // Overwrites LLM furigana with MeCab's unless --nomecab_overwrite.
                            if let Ok((ref mut results, _)) = result {
                                furigana::compare_and_maybe_overwrite(results, s_conf.mecab_overwrite);
                            }

                            let _ = tx_clone.send((gen_id, result)).await;
                        });
                        state_main.borrow_mut().in_flight = Some(handle);
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
