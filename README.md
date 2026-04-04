# lenzu

Desktop OCR lens — a transparent floating window that follows the mouse cursor, captures the region under it on demand, and sends it to an LLM API for OCR and translation. Results appear in a separate transparent overlay HUD (`lenzu_server`).

The key difference from browser extensions like Yomitan/Rikaichan: this operates on **images** (GPU-rendered video, game windows, PDFs, anything on screen), not UTF-8 text.

![beta demo](docs/lenzu-beta-demo.gif)

> **Architecture note**: The Windows/winit/GTK4 experiments are archived in `prototypes/`. The active implementation uses **GTK3** (`gtk-rs` 0.18) on Linux/X11. GTK4 was evaluated and abandoned due to integration complexity — GTK3 provides everything needed and is simpler to build against. See [Technical Design](./docs/technical-design.md) for current architecture.

## Architecture (Current)

```
lenzu (GTK3 client)               lenzu_server (Electron)
  floating lens window     UDP     transparent overlay HUD
  X11 root capture       ──────►  renders translated text
  OpenRouter/Gemini API            ArrowUp/Down moves position
  manages server lifecycle
```

1. **Capture**: `x11rb` captures the X11 root window directly — bypasses GPU-accelerated and hardware-rendered windows correctly.
2. **OCR/Translation**: Image sent as base64 PNG to OpenRouter (Gemini 2.0 Flash). Returns structured JSON with `original`, `furigana`, `romaji`, `english`, bounding boxes.
3. **Overlay**: Formatted text sent via UDP loopback to `lenzu_server`, an Electron transparent window pinned to screen edge.

## Hardware and Privacy

- **Cloud OCR**: Images are sent to OpenRouter (Gemini) by default. Set `overlay_enabled = false` and/or use a local LLM endpoint to keep everything on-device.
- **GPU Acceleration**: Optional YOLOv8n pre-detection (model in repo) to crop text regions before API call, reducing token cost ~80%.

## Libraries & Dependencies

- [`gtk` 0.18](https://crates.io/crates/gtk) — GTK3 bindings (gtk-rs). **GTK3, not GTK4.**
- [`x11rb`](https://crates.io/crates/x11rb) — X11 protocol (screen capture)
- [`cairo-rs`](https://crates.io/crates/cairo-rs) — 2D drawing
- [`pango`](https://crates.io/crates/pango) / [`pangocairo`](https://crates.io/crates/pangocairo) — text layout and CJK rendering
- [`reqwest`](https://crates.io/crates/reqwest) — HTTP client (OpenRouter API)
- [`isolang`](https://crates.io/crates/isolang) — ISO 639-3 language codes
- Electron (`lenzu_server`) — transparent overlay window

## Build & Run

```bash
# 1. Install system dependencies
./scripts/setup.sh

# 2. Set API key
export OPENROUTER_API_KEY=sk-your-key-here

# 3. Build and run (builds lenzu_server on first run)
./scripts/run.sh
```

See [`lenzu/README.md`](lenzu/README.md) for full configuration reference and controls.

## TODO

- YOLOv8n pre-detection to crop text regions before API call (reduce token cost)
- Wayland support via xdg-desktop-portal
- Multi-monitor capture at non-zero offsets
