# Lenzu: OCR Screen Lens

Lenzu is a high-performance, real-time screen-capture and OCR utility written in Rust. It provides a "magnifying lens" that follows the mouse cursor, allowing the user to capture and translate text from any window (including browsers and hardware-accelerated apps) using a configurable LLM API (default: Gemini 2.0 Flash via OpenRouter).

Lenzu is two processes:
- **`lenzu`** (`lenzu_client`) — the GTK3 lens window; does capture, OCR, and manages the server lifecycle
- **`lenzu_server`** — a transparent Tauri overlay window that renders the translated text on screen

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

- **Transparent Tauri window** pinned to the bottom of the screen — results appear as subtitles.
- **Configurable render mode**: show `english`, `furigana`, `romaji`, `original`, `all`, or `debug`.
- **UDP IPC**: client sends text datagrams to server on loopback — fire-and-forget, no blocking.

### 4. Core Logic & Threading

- **Thread-Safe API Calls**: `reqwest` calls run in a background thread; results return via `glib::MainContext::channel`.
- **Automated Clipboard**: OCR results copied to system clipboard on success.
- **Persistent Logging**: Every transcription logged with timestamp to `/dev/shm/ocr_history.txt`.

## 🛠 Technical Stack

- **Language**: Rust (Edition 2021)
- **UI Toolkit**: GTK 0.18 (gtk-rs), Cairo & Pango
- **Capture**: x11rb (X11 Rust Bindings)
- **AI Model**: `google/gemini-2.0-flash-001` (via OpenRouter, configurable)
- **Overlay**: Tauri 2.x (`lenzu_server`), Vanilla JS/HTML/CSS
- **Language Codes**: `isolang` (ISO 639-3)

## ⌨️ Controls

- **Mouse Move**: Lens follows cursor.
- **Shift + Left Click**: Capture, flash, and OCR.
- **Esc / Window Close**: Quit (also kills `lenzu_server`).

## 🚀 Running

### Prerequisites

```bash
# System dependencies
sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev \
                 fonts-noto-cjk fonts-ipafont-gothic

# API key
export OPENROUTER_API_KEY=sk-your-key-here
```

### One-shot script (recommended)

A script is provided at the repo root to build and run both processes:

```bash
cd /path/to/lenzu
./scripts/run.sh
```

Or manually (after building `lenzu_server` once):

```bash
# Step 1 — build lenzu_server (only needed once, or after server changes)
cd lenzu_server && npm install && npm run tauri build && cd ..

# Step 2 — copy server binary next to client binary so lenzu can find it
cp lenzu_server/src-tauri/target/release/lenzu_server target/debug/

# Step 3 — run (lenzu auto-spawns lenzu_server)
cargo run -p lenzu_client
```

### Development mode (separate terminals)

If you want `lenzu_server` hot-reloading its UI:

```bash
# Terminal 1 — server with live UI
cd lenzu_server && npm run tauri dev -- -- --port 7331

# Terminal 2 — client (set overlay_enabled = false in lenzu_config.json
#              so it doesn't try to spawn a second server)
cargo run -p lenzu_client
```

## ⚙️ Configuration (`lenzu_config.json`)

Optional file in the working directory. All fields have defaults if the file is absent or a field is omitted. This is the client-side config; the server has its own separate `hud_config.json`.

```json
{
  "lens_size": 400,
  "ui_panel_height": 130,
  "font_size": 13.0,
  "hud_color_hex": "#00FFCC",
  "show_romaji": true,
  "show_furigana": true,
  "overlay_enabled": true,
  "overlay_udp_port": 7331,
  "overlay_render_mode": "furigana",
  "llm_api_endpoint": "https://openrouter.ai/api/v1/chat/completions",
  "llm_default_model": "google/gemini-2.0-flash-001",
  "translate_src": "jpn",
  "translate_dest": "eng",
  "translate_extra_prompt": "Each object must also include: 'furigana' (format: 漢字[かんじ]) and 'romaji'."
}
```

| Field | Description | Default |
|---|---|---|
| `lens_size` | Capture window size in px (square) | `400` |
| `overlay_enabled` | Auto-spawn `lenzu_server` and send results to it | `true` |
| `overlay_udp_port` | UDP port; must match `lenzu_server`'s `--port` | `7331` |
| `overlay_render_mode` | What to show in HUD: `original`, `english`, `furigana`, `romaji`, `all`, `debug` | `"furigana"` |
| `translate_src` / `translate_dest` | ISO 639-3 language codes (e.g. `"jpn"`, `"eng"`, `"kor"`, `"cmn"`) | `"jpn"` / `"eng"` |
| `translate_extra_prompt` | Appended to base prompt for language-specific fields. Empty string = no extension. | furigana/romaji hint |

### How the prompt is built

The base prompt is hardcoded and language-agnostic:

> "Act as a highly accurate **{src}**-to-**{dest}** OCR and translation engine. Extract ALL text from the image. Return a JSON array of objects, one per line/bubble found. Each object MUST have: `original`, `english`, `debug_info`."

`{src}` / `{dest}` are substituted at runtime from `translate_src` / `translate_dest` (e.g. `"jpn"` → `"Japanese"`). `translate_extra_prompt` is appended for language-specific field additions (furigana/romaji for Japanese, pinyin for Chinese, etc.).

## 🏗 Future Context for Next Session

- **Alignment Status**: The math for `win_x`/`win_y` is 1:1 with the capture box.
- **Threading Status**: `glib` channels keep GTK objects on the main thread; API calls on background thread.
- **Prompting**: Base prompt is language-agnostic; language-specific extras go in `translate_extra_prompt`.
- **Process Lifecycle**: `lenzu` spawns `lenzu_server` via `std::process::Child`; killed on ESC or window close.
