# Lenzu: OCR Screen Lens

Lenzu is a high-performance, real-time screen-capture and OCR utility written in Rust. It provides a "magnifying lens" that follows the mouse cursor, allowing the user to capture and translate text from any window (including browsers and hardware-accelerated apps) using a multi-tier LLM backend chain: local Ollama (primary, free) → free OpenRouter tier → paid remote (Gemini 2.0 Flash).

Lenzu is two processes:
- **`lenzu`** — the GTK3 lens window; does capture, OCR, and manages the overlay lifecycle
- **`lenzu_server`** — a transparent Electron overlay window that renders the translated text on screen

`lenzu` auto-spawns `lenzu_server` on startup and kills it on exit. You only need to run one command.

## 🚀 Accomplishments & Features

### 1. Advanced Screen Capture

- **X11 Root Capture**: Bypasses window-specific limitations by capturing directly from the X11 root window.
- **Hardware Acceleration Bypass**: Successfully captures text from GPU-accelerated browsers (like Yahoo.co.jp news) that traditional screenshot tools often miss.
- **Pixel-Perfect Alignment**: The capture area is mathematically centered on the mouse cursor, ensuring the boundary box perfectly matches the OCR input.

### 2. UI & UX (GTK3 + Cairo + Pango)

- **Dynamic Lens**: A floating window that follows the cursor smoothly (default 400×400px).
- **Transparency & Alpha Support**: Uses RGBA visuals for a modern "frosted glass" UI panel with 85% opacity.
- **Japanese Font Support**: Pango rendering prevents "tofu" (square boxes) for CJK characters.
- **Visual Feedback**: Cyan capture border, white camera-flash effect, animated spinner during API calls.

### 3. Overlay HUD (`lenzu_server`)

- **Transparent Electron window** pinned to the bottom of the screen — results appear as subtitles.
- **Configurable render mode**: show `english`, `furigana`, `romaji`, `original`, `all`, or `debug`.
- **UDP IPC**: client sends text datagrams to server on loopback — fire-and-forget, no blocking.

### 4. Core Logic & Threading

- **Thread-Safe API Calls**: `reqwest` calls run in a background thread; results return via `glib::MainContext::channel`.
- **Automated Clipboard**: OCR results copied to system clipboard on success.
- **Persistent Logging**: Every transcription logged with timestamp to `/dev/shm/ocr_history.txt`.

## 🛠 Technology Dependencies

### Core runtime

| Technology | Role | Notes |
|---|---|---|
| **Rust** (Edition 2021) | Primary language — client binary | All runtime code; no Python |
| **GTK 3** (`gtk-rs` 0.18) | Lens window UI | Fixed at GTK3; GTK4 evaluated and rejected |
| **Cairo** (`cairo-rs`) | 2D drawing, transparency | Flash effect, capture border |
| **Pango** | CJK text layout and rendering | Prevents "tofu" boxes for Japanese |
| **x11rb** | X11 screen capture | Captures GPU-accelerated windows correctly; root-window pixel-read approach inspired by [xfce4-screenshooter](https://gitlab.xfce.org/apps/xfce4-screenshooter) |
| **Electron** (`lenzu_server`) | Transparent HUD overlay | Separate Node.js process |

### Local-first OCR (confidence-gated, no LLM)

| Technology | Role | Confidence | Requires |
|---|---|---|---|
| **jp_detect** >= 0.2.2 (DBNet) | Text detection with per-box confidence | 0-100% detection score | `--features onnx` build |
| **manga-ocr-rs** >= 0.1.1 | Japanese OCR with per-result confidence | 0-100% OCR score | ~140 MB model files (auto-downloaded) |
| [**mecab-furigana-rs**](https://crates.io/crates/mecab-furigana-rs) | Morphological analysis → furigana + romaji | Deterministic (dictionary) | `mecab`, `mecab-ipadic-utf8` or `mecab-naist-jdic` |

Both OCR scores must be >= 71% to pass the confidence gate. When they do, raw text is returned
immediately — no LLM, no Ollama, no network. `mecab-furigana-rs` then annotates the text with
furigana brackets (`最初[さいしょ]`) and romaji in a second instant pass (~5 ms), plus
word segmentation and per-morpheme data. Models loaded once at startup, shared via `Arc`.

**Why MeCab, not kakasi?** MeCab performs context-aware morphological analysis — it understands
word boundaries from neighboring characters, so it correctly segments compound words and
conjugated verbs. kakasi is a simple dictionary lookup that cannot disambiguate readings based
on context. `mecab-furigana-rs` uses MeCab's per-morpheme katakana readings and a pure-Rust
Hepburn converter to produce romaji — no kakasi CLI dependency needed.

### LLM fallback chain (when local OCR confidence is too low)

| Technology | Role | Requires |
|---|---|---|
| **Ollama** | Local LLM runtime | Running daemon at `localhost:11434` |
| **glm-ocr** (via Ollama) | Primary LLM OCR model — fast, specialist | ~2.2 GB VRAM |
| **Gemma 4 E2B** (via Ollama) | Local LLM fallback OCR model | ~1.5 GB quantized |
| **OpenRouter** | Remote API gateway | `OPENROUTER_API_KEY` env var |
| **Google Gemini 2.0 Flash** (via OpenRouter) | Paid remote fallback | OpenRouter key |
| `openrouter/free` | Free remote fallback | Optional key (rate-limited without) |

### Text pre-detection (Phase 4 — `--features onnx`)

| Technology | Role | Notes |
|---|---|---|
| **DBNet** (`stabrise-text_detection_dbnet_ml_v02_model.onnx`) | Locate text bounding boxes before OCR | Reduces LLM token cost ~80–93% per capture |
| **ONNX Runtime** | Run the DBNet ONNX model | Downloaded automatically by `ort` crate at build time — no `apt install` needed |
| **`ort`** (Rust crate, v2.0.0-rc.12) | Rust bindings for ONNX Runtime | |
| **`ndarray`** | N-dimensional input tensor | |
| **`imageproc`** | Binary mask contour extraction | Pure Rust — no OpenCV needed |

### Serialization / networking

| Technology | Role |
|---|---|
| **`serde` + `serde_json`** | Config and API JSON |
| **`reqwest`** (blocking) | HTTP client for all LLM backends |
| **`base64`** | Image encoding for API payloads |
| **`isolang`** | ISO 639-3 language code handling |

### Build / system

| Technology | Role |
|---|---|
| **Cargo** | Build system; manages all Rust deps |
| **pnpm** | Node package manager for `lenzu_server` |
| **`libgtk-3-dev`**, `libcairo2-dev`, `libpango1.0-dev` | System headers (Debian/Ubuntu) |
| **`mecab`**, `mecab-ipadic-utf8`, `mecab-naist-jdic` | MeCab morphological analyzer + UTF-8 dictionaries (furigana/romaji) |
| `fonts-noto-cjk`, `fonts-ipafont-gothic` | CJK font rendering |

> **OpenCV is NOT required.** All image processing (resize, normalize, contour detection) is
> handled by pure-Rust crates. The ONNX Runtime is downloaded automatically at `cargo build`
> time by the `ort` crate — no system-level installation needed.

## ⌨️ Controls

| Shortcut | Action |
|---|---|
| **Mouse Move** | Lens follows cursor |
| **Shift + Left Click** | Capture and OCR. When DBNet detection is enabled: takes an oversample capture, finds text near the cursor, crops to the detected text bounds (shrinks when text is small, expands when text extends past the lens edge), then sends the crop to the OCR backend chain. |
| **Ctrl + Shift + Left Click** | Full-desktop capture mode. Hides the lens window, grabs the entire desktop, runs DBNet to find text nearest the lens position, sends the best crop directly to the remote OCR backend. Use this for text that is too large or too spread out for the lens window. |
| **Esc / Window Close** | Quit (also kills `lenzu_server`) |

### CLI Flags

| Flag | Description |
|---|---|
| `--furigana_only` | MeCab furigana only — skip romaji and LLM enrichment. Overrides `furigana_only` in config. Use via `scripts/run.sh --furigana_only`. |

> **Screen capture approach** inspired by `xfce4-screenshooter`'s method of reading pixels
> directly from the X11 root window, which correctly captures GPU-accelerated and
> hardware-composited windows that traditional screenshot tools miss.
> Source: https://gitlab.xfce.org/apps/xfce4-screenshooter

## OCR Pipeline Flow

Both capture paths share the same confidence-gated decision chain. The pipeline exits
as soon as any tier produces a result; later tiers are only reached on failure.

### Shift+Click (lens capture)

```
Shift+Click
  │
  ├─ hide lens window, sleep 400 ms (avoid capturing self)
  ├─ capture_x11(lens_rect)  ← lens-sized region under cursor
  ├─ grayscale()
  │
  ├─ DBNet detect (jp_detect)
  │   ├─ no boxes found ──────────────────────────────┐
  │   └─ N boxes found                                │
  │       │                                            │
  │       ├─ local OCR (manga-ocr-rs) per box         │
  │       │   ├─ ALL boxes: det >= 71% AND ocr >= 71% │
  │       │   │   │                                    │
  │       │   │   │  ┌── 3-phase progressive HUD ──┐   │
  │       │   │   │  │ Phase 1: raw text (instant)  │  │
  │       │   │   │  │   → HUD shows original text  │  │
  │       │   │   │  │ Phase 2: MeCab (~5 ms)       │  │
  │       │   │   │  │   → 最初[さいしょ] + romaji  │  │
  │       │   │   │  │ Phase 3: LLM (if enabled)    │  │
  │       │   │   │  │   → english translation      │  │
  │       │   │   │  └─────────────────────────────┘   │
  │       │   │   │                                    │
  │       │   │   └─ DONE ← return results ───────────┼──► format_for_overlay()
  │       │   │       backend: "local:manga-ocr        │
  │       │   │         [+furigana][+enriched]"        │        ├─ UDP JSON ──► lenzu_server
  │       │   └─ any box below gate                    │        ├─ clipboard
  │       │       │                                    │        └─ ocr_history.txt
  │       ├─ per-region LLM (per_region_prompt)        │
  │       │   ├─ any region succeeds                   │
  │       │   │   └─ DONE ← return results ────────────┼──► format_for_overlay() ──► ...
  │       │   └─ all per-region calls fail             │
  │       │       │                                    │
  │       ▼       ▼                                    │
  │   ┌───────────────────────────────────────┐        │
  │   │ OPT-1: crop to union bbox of          │◄───────┘
  │   │ detected boxes (or full lens image    │
  │   │ if no boxes were found)               │
  │   └───────────┬───────────────────────────┘
  │               │
  │               ▼
  │   ┌─ LLM fallback chain (full_prompt) ────────────────────────┐
  │   │                                                            │
  │   │  1. Ollama primary (glm-ocr, 3 s timeout)                 │
  │   │     ├─ success ──► DONE                                    │
  │   │     └─ fail/timeout ──► kill stale runner                  │
  │   │                                                            │
  │   │  2. Ollama local fallbacks (gemma4:e2b, ..., 3 s each)    │
  │   │     ├─ any succeeds ──► DONE                               │
  │   │     └─ all fail ──► kill stale runner                      │
  │   │                                                            │
  │   │  3. Free remote (openrouter/free, 15 s)                   │
  │   │     ├─ success ──► DONE                                    │
  │   │     └─ fail/empty                                          │
  │   │                                                            │
  │   │  4. Paid remote (Gemini 2.0 Flash, 60 s)                  │
  │   │     ├─ success ──► DONE                                    │
  │   │     └─ fail ──► return error                               │
  │   │                                                            │
  │   └──── every DONE ──► format_for_overlay() ──► UDP ──► HUD ──┘
  │
  └─ show lens window, display result for result_display_secs
```

### Ctrl+Shift+Click (full-desktop capture)

```
Ctrl+Shift+Click
  │
  ├─ hide lens window, sleep 400 ms
  ├─ capture_x11(0, 0, screen_w, screen_h)  ← entire desktop
  ├─ grayscale()
  │
  ├─ DBNet detect (jp_detect) on full desktop
  │   ├─ no boxes found ──► return "No text detected near cursor"
  │   └─ N boxes found
  │       │
  │       ├─ closest_box_to_point(cursor_x, cursor_y) → chosen box
  │       │
  │       ├─ BUG-4 check: chosen box > 60% of screen?
  │       │   └─ yes: re-run DBNet on sub-crop with tighter params
  │       │          if more boxes found → update chosen box
  │       │
  │       ├─ local OCR (manga-ocr-rs) on chosen box
  │       │   ├─ det >= 71% AND ocr >= 71%
  │       │   │   │
  │       │   │   │  ┌── 3-phase progressive HUD ──┐
  │       │   │   │  │ Phase 1: raw text (instant)  │
  │       │   │   │  │ Phase 2: MeCab (~5 ms)       │
  │       │   │   │  │   → furigana + romaji        │
  │       │   │   │  │ Phase 3: LLM (if enabled)    │
  │       │   │   │  │   → english translation      │
  │       │   │   │  └─────────────────────────────┘
  │       │   │   │
  │       │   │   └─ DONE ← return results ──► format_for_overlay()
  │       │   │       backend: "local:manga-ocr        ├─ UDP ──► lenzu_server
  │       │   │         [+furigana][+enriched]"        ├─ clipboard
  │       │   └─ below gate                            └─ ocr_history.txt
  │       │       │
  │       │       ▼
  │       ├─ crop chosen box → TextCropper
  │       │
  │       ▼
  │   ┌─ Remote fallback chain (call_api_force_fallback) ─────────┐
  │   │                                                            │
  │   │  1. Free remote (openrouter/free, 15 s)                   │
  │   │     ├─ success ──► DONE                                    │
  │   │     └─ fail/empty                                          │
  │   │                                                            │
  │   │  2. Paid remote (Gemini 2.0 Flash, 60 s)                  │
  │   │     ├─ success ──► DONE                                    │
  │   │     └─ fail ──► return error                               │
  │   │                                                            │
  │   └──── every DONE ──► format_for_overlay() ──► UDP ──► HUD ──┘
  │
  └─ show lens window, display result
```

**Key difference**: Ctrl+Shift+Click skips local Ollama entirely (`call_api_force_fallback`)
and goes straight to remote backends after the local OCR gate. This is intentional — the
full-desktop path is for when local models struggle, so there's no point waiting 3 s per
local timeout.

## 🚀 Running

### Prerequisites

```bash
# System dependencies (GTK3 + capture stack + MeCab; no WebKit needed — Electron bundles its own Chromium)
sudo apt install build-essential pkg-config libgtk-3-dev libcairo2-dev libpango1.0-dev \
                 libgdk-pixbuf-2.0-dev libx11-dev libssl-dev \
                 mecab mecab-ipadic-utf8 mecab-naist-jdic \
                 fonts-noto-cjk fonts-ipafont-gothic

# Node.js for lenzu_server — use repo ./lenzu_server/scripts/setup.sh or install Node + run npm install in lenzu_server

# API key
export OPENROUTER_API_KEY=sk-your-key-here
```

### One-shot script (recommended)

A script is provided at the repo root to build and run both processes:

```bash
cd /path/to/lenzu
./scripts/run.sh
```

![Lenzu help screen (Shift+H)](../docs/HELP.png)

Or manually:

```bash
# Install Electron app deps once
cd lenzu_server && npm install && cd ..

# Run client — it spawns `npm run start` in lenzu_server when overlay_enabled is true
cargo run -p lenzu
```

### Development mode (separate terminals)

To hack the HUD without the client spawning a second instance:

```bash
# Terminal 1 — Electron overlay (port must match overlay_udp_port, default 7331)
cd lenzu_server && LENZU_OVERLAY_UDP_PORT=7331 npm start

# Terminal 2 — client with overlay spawn disabled
# Set "overlay_enabled": false in lenzu_config.json
cargo run -p lenzu
```

## ⚙️ Configuration (`lenzu_config.json`)

Optional file in the working directory. All fields have defaults if the file is absent or a field is omitted. This is the client-side config; the Electron HUD reads `lenzu_server/src/config.json` for window styling and UDP bind defaults — when the client spawns the HUD it sets `LENZU_OVERLAY_UDP_PORT` so the port matches `overlay_udp_port`.

```jsonc
{
  // UI
  "lens_size": 400,
  "ui_panel_height": 130,
  "font_size": 13.0,
  "hud_color_hex": "#00FFCC",
  "show_romaji": true,
  "show_furigana": true,
  "result_display_secs": 5,

  // Overlay HUD
  "overlay_enabled": true,
  "overlay_udp_port": 7331,
  "overlay_render_mode": "furigana",

  // Primary backend — local Ollama (no API key needed)
  "llm_api_endpoint": "http://localhost:11434/v1/chat/completions",
  "llm_default_model": "glm-ocr",
  "local_timeout_secs": 3,
  "local_fallback_models": ["gemma4:e2b"],

  // Free remote fallback — OpenRouter free tier (rate-limited without key)
  "free_remote_endpoint": "https://openrouter.ai/api/v1/chat/completions",
  "free_remote_model": "openrouter/free",
  "remote_timeout_secs": 15,

  // Paid remote fallback — requires OPENROUTER_API_KEY env var
  "fallback_llm_api_endpoint": "https://openrouter.ai/api/v1/chat/completions",
  "fallback_llm_model": "google/gemini-2.0-flash-001",
  "fallback_max_dimension": 800,
  "paid_remote_timeout_secs": 60,

  // Text pre-detection (Phase 4 — requires --features onnx build)
  // Set to null to disable; path is relative to working directory
  "text_detection_model": "assets/stabrise-text_detection_dbnet_ml_v02_model.onnx",
  "text_detection_threshold": 0.3,

  // Translation
  "translate_src": "jpn",
  "translate_dest": "eng",
  "translate_extra_prompt": "Each object must also include: furigana (format: 漢字[かんじ]) and romaji fields, plus english translation.",

  // LLM enrichment (post local-OCR) — adds english translation to raw manga-ocr-rs
  // text via text-only Ollama call (no image, no vision model).
  // Furigana and romaji are handled by MeCab (instant, no LLM) — see furigana.rs.
  "enrichment_enabled": true,
  "enrichment_model": "qwen2.5:3b",
  "enrichment_timeout_secs": 30,

  // Token spend warnings — HUD color changes when paid API token usage is high
  "token_warning_threshold": 100000,
  "token_critical_threshold": 500000
}
```

| Field | Description | Default |
|---|---|---|
| `lens_size` | Capture window size in px (square) | `400` |
| `overlay_enabled` | Auto-spawn `lenzu_server` and send results to it | `true` |
| `overlay_udp_port` | UDP port; must match `lenzu_server`'s bind port | `7331` |
| `overlay_render_mode` | What to show in HUD: `original`, `english`, `furigana`, `romaji`, `all`, `debug` | `"furigana"` |
| `llm_default_model` | Primary Ollama model | `"glm-ocr"` |
| `local_fallback_models` | Ordered list of Ollama fallback models | `["gemma4:e2b"]` |
| `local_timeout_secs` | Timeout for each local Ollama call | `3` |
| `remote_timeout_secs` | Timeout for free remote tier | `15` |
| `paid_remote_timeout_secs` | Timeout for paid remote | `60` |
| `text_detection_model` | Path to DBNet ONNX model; `null` disables pre-detection | `null` |
| `text_detection_threshold` | DBNet probability threshold (0–1) | `0.3` |
| `text_detection_oversample_factor` | Capture multiplier for Shift+Click detection (≥1.0; floor at 640 px) | `2.0` |
| `text_detection_max_capture_size` | Hard ceiling on oversample dimension in px | `1600` |
| `text_detection_crop_padding` | Padding px added around detected text union before sending to OCR | `16` |
| `text_detection_debug` | Write annotated debug PNG to `/dev/shm/lenzu/debug_detection.png` after each capture; draw box outlines on lens window | `false` |
| `translate_src` / `translate_dest` | ISO 639-3 language codes (`"jpn"`, `"eng"`, `"kor"`, `"cmn"` …) | `"jpn"` / `"eng"` |
| `translate_extra_prompt` | Appended to base prompt for language-specific fields | furigana/romaji hint |
| `enrichment_enabled` | Enrich local OCR results with english translation via text-only Ollama call. Furigana/romaji are always handled by MeCab (instant, no LLM). | `true` |
| `enrichment_model` | Text-only Ollama model for enrichment (not a vision model); `null` falls back to `llm_default_model` | `"qwen2.5:3b"` |
| `enrichment_timeout_secs` | Timeout for each enrichment request | `30` |
| `enrichment_prompt` | Override the entire enrichment prompt; `null` uses built-in. `{src}`/`{dest}` placeholders resolved. | `null` |
| `furigana_only` | When `true`, use only MeCab furigana — skip romaji and LLM enrichment entirely. Instant results (~5 ms). Override via `--furigana_only` CLI flag. | `false` |
| `token_warning_threshold` | Session paid tokens at which HUD turns orange (0 = disable) | `100000` |
| `token_critical_threshold` | Session paid tokens at which HUD turns red (0 = disable) | `500000` |

### How the prompt is built

The base prompt is hardcoded and language-agnostic:

> "Act as a highly accurate **{src}**-to-**{dest}** OCR and translation engine. Extract ALL text from the image. Return a JSON array of objects, one per line/bubble found. Each object MUST have: `original`, `english`, `debug_info`."

`{src}` / `{dest}` are substituted at runtime from `translate_src` / `translate_dest` (e.g. `"jpn"` → `"Japanese"`). `translate_extra_prompt` is appended for language-specific field additions (furigana/romaji for Japanese, pinyin for Chinese, etc.).

## 🧪 Testing

### Unit tests (no external deps)

```bash
# All unit tests — no onnx feature needed
cargo test -p lenzu

# MeCab furigana/romaji tests (pure-function tests use synthetic MeCab output — no MeCab binary needed)
cargo test -p lenzu -- furigana

# Text detection pure-logic tests (merge, union, intersects, dimensions)
cargo test -p lenzu -- ocr::text_detection

# TextCropper tests (padding, clamping, area filter)
cargo test -p lenzu -- ocr::text_cropper
```

### DBNet image tests (requires `--features onnx`)

These run the full ONNX inference pipeline against the two reference images in `assets/`.
ONNX Runtime is downloaded automatically at build time by the `ort` crate — no system install needed.

```bash
# Both DBNet detection tests
cargo test -p lenzu --features onnx -- ocr::text_detection::tests::test_detect

# Lens-crop image only  →  expects 2 boxes: tategaki separate, yokogaki+tegaki merged
# See full benchmark: https://github.com/HidekiAI/lenzu/blob/trunk/docs/scores.md
cargo test -p jp_detect --features onnx -- test_detect_lens_crop_separates_tategaki

# Fullscreen image only  →  expects 2 boxes  (OCR-Demo-JP2EN.png, 2816×1536)
cargo test -p jp_detect --features onnx -- test_detect_fullscreen_returns_two_boxes

# All onnx tests (detection + any future onnx unit tests)
cargo test -p lenzu --features onnx
```

### Integration tests

```bash
# Mock-server integration test (spins up wiremock, no ollama needed)
cargo test -p lenzu --test integration_test

# Live ollama smoke-test — requires ollama running with gemma4:e2b loaded
OLLAMA_TIMEOUT=600 /usr/local/bin/ollama serve &
cargo test -p lenzu --test integration_test -- --ignored
```

### OCR backend smoke-test binary

A standalone binary to test the OCR chain against a real image without the GTK GUI:

```bash
# Local ollama
cargo run -p lenzu --bin ocr-test -- --image assets/Unit-test-sample-texts.png

# Force remote (OpenRouter)
OPENROUTER_API_KEY=sk-… cargo run -p lenzu --bin ocr-test -- --image assets/Unit-test-sample-texts.png --remote
```

## 🏗 Future Context for Next Session

- **Alignment Status**: The math for `win_x`/`win_y` is 1:1 with the capture box.
- **Threading Status**: `glib` channels keep GTK objects on the main thread; API calls on background thread.
- **Prompting**: Base prompt is language-agnostic; language-specific extras go in `translate_extra_prompt`.
- **Process Lifecycle**: `lenzu` spawns `lenzu_server` via `std::process::Child`; killed on ESC or window close.
