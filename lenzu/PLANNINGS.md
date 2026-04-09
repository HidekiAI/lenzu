# Lenzu Project Phases & Planning

## Current Branch: `RemoteOCR`
**Last updated: 2026-03-22**

---

## ✅ Phase 1 — Remote OCR + Structured Results (COMPLETE)

All items below are implemented and on the `RemoteOCR` branch.

### Data Model (`client.rs`)
- [x] `TranslationResult` struct: `original`, `furigana`, `romaji`, `english`, `top_xy`, `bot_xy`, `debug_info`
- [x] OpenRouter multimodal API client (`OcrClient`) with configurable endpoint, model, and prompt
- [x] `normalize_results()` handles both single-object and array JSON responses
- [x] Unit tests for both shapes

### Configuration (`config.rs`)
- [x] `AppConfig` loaded from `lenzu_config.json` (falls back to defaults if absent)
- [x] Language fields use `isolang::Language` (ISO 639-3 codes, serde-compatible)
- [x] `translate_src` / `translate_dest` — configurable language pair
- [x] `translate_extra_prompt` — appended to base prompt for language-specific field hints (e.g. furigana/romaji for Japanese)
- [x] `resolved_prompt()` — injects language names into the hardcoded base prompt at runtime
- [x] `overlay_render_mode: OverlayRenderMode` — controls which field(s) the HUD displays
  - Variants: `original`, `english`, `furigana`, `romaji`, `all`, `debug`
- [x] `overlay_enabled` / `overlay_udp_port` — HUD process integration

### Overlay HUD — UDP Bridge
- [x] `send_to_overlay(text, port)` — fires a UDP datagram to the Electron HUD process
- [x] `format_for_overlay(results, mode)` — formats `Vec<TranslationResult>` according to `OverlayRenderMode`
  - `debug` mode includes bounding boxes and debug_info per result
  - Single-field modes fall back to `original` if the field is absent

### Integration Tests
- [x] `tests/integration_test.rs` — full pipeline mock (wiremock + OcrClient)

---

## ✅ Phase 1B — lenzu_server in Workspace (COMPLETE)

The overlay HUD was originally the `tauri-translucent-desktop-overlay` project (Tauri + WebKit2GTK). It was replaced with an Electron-based implementation (`electron-translucent-desktop-overlay`) due to WebKit2GTK's broken alpha/transparency compositing on X11 — ghost pixels accumulate as text updates because WebKit's dirty-rect optimiser skips repainting transparent-to-transparent regions. Electron (Chromium) composites ARGB windows correctly when a compositor is running.

### Structure
```
./                              ← workspace root (repository root)
├── Cargo.toml                  ← workspace manifest (members: lenzu only; lenzu_server is Node/Electron)
├── lenzu/                      ← OCR lens client (GTK3)
└── lenzu_server/               ← Electron overlay HUD
    ├── package.json            ← electron, esbuild, vitest devDeps
    ├── build.mjs               ← esbuild pipeline
    ├── hud_config.json         ← HUD display settings
    ├── src/
    │   ├── main.ts             ← UDP listener + BrowserWindow; transparent, frameless
    │   ├── preload.ts          ← contextBridge (IPC boundary)
    │   └── renderer/
    │       ├── app.ts          ← listens for "hud-text-changed" IPC event
    │       ├── index.html
    │       └── styles.css
    └── scripts/
        ├── setup.sh            ← Node/pnpm/Electron install + tsc check
        └── demo.sh             ← build + launch + UDP demo sequence
```

### Running standalone (for testing)
```bash
cd lenzu_server
bash scripts/setup.sh   # first time only
bash scripts/demo.sh    # build, launch, send demo messages
```

---

## ✅ Phase 2 — Process Lifecycle Management (COMPLETE)

Split `lenzu` into client + server with automatic lifecycle management.

### Completed
- [x] `lenzu` binary spawns `lenzu_server` on startup (`spawn_server()`)
- [x] `server_process: Option<std::process::Child>` in `AppState`
- [x] `kill_server()` calls `.kill()` + `.wait()` to clean up the child
- [x] ESC handler kills child before `gtk::main_quit()`
- [x] `window.connect_delete_event` kills child then returns `Propagation::Proceed`
- [x] `lenzu` package renamed to `lenzu_client`; binary name stays `lenzu`
- [x] `spawn_server()` checks sibling binary first, falls back to PATH

### Binary discovery (runtime)
```rust
// prefer sibling binary (production), fall back to PATH (dev)
let server_bin = std::env::current_exe()
    .ok()
    .and_then(|p| p.parent().map(|d| d.join("lenzu_server")))
    .filter(|p| p.exists())
    .unwrap_or_else(|| std::path::PathBuf::from("lenzu_server"));
```

---

## ✅ Phase 3 — Documentation & Polish (COMPLETE)

- [x] `lenzu/README.md` — config reference, overlay HUD section, prompt design
- [x] `lenzu/PLANNINGS.md` — this file, updated to reflect reality
- [x] `docs/planning.md` — high-level roadmap updated, GTK4 removed, M6 checked
- [x] `docs/technical-design.md` — Section 0 current implementation added; GTK4 references removed throughout
- [x] `lenzu_server/README.md` — standalone server usage, `--port` arg, hud_config.json, picom/xfwm4 fix

---

## 🔶 Phase 4 — Detection Pipeline (Partially Complete)

Core detection is live via `jp_detect 0.2.0` (DBNet, published crate). Remaining work:

- [x] DBNet text detection via `jp_detect` crate (replaces planned YOLOv8n path)
- [x] Ctrl+Shift+Click fullscreen scan → crop to nearest text region → remote OCR
- [x] Lens-capture mode: per-region detection + individual crops sent to API
- [ ] `top_xy` / `bot_xy` from `TranslationResult` not yet used to anchor HUD text to screen coordinates
- [ ] YOLOv8n (`yolov8n_fp16.onnx`) still in repo — evaluate whether it adds value over DBNet or can be removed
- [ ] Fullscreen detection parameter tuning: live desktop (taskbars, UI chrome) causes over-merging at default dilation=16; consider separate config for fullscreen vs lens-crop paths

---

## 🐛 Known Bugs

### BUG-1 — Spinning cursor lollipop artefact
The animated spinner cursor has a visible tail/disfigurement — appears as a lollipop shape instead of a clean spinning circle. Likely a leftover artefact from a previous frame not being cleared before drawing the next.

### ~~BUG-3 — Image not greyscaled before DBNet detection~~ ✓ FIXED
`det.detect()` now receives `dyn_image.grayscale()` (computed once at the top of the worker closure). The fullscreen debug overlay and crop calls retain the original RGB image so bounding-box visualisation stays coloured.

### ~~DBG-1 — Save pre-wire image to debug_lens.png instead of raw capture~~ ✓ FIXED
`save_debug_image()` removed. Replaced by `save_prewire_debug(image)` called at the top of both `DualOcrClient::call_api()` and `call_api_force_fallback()`, writing the greyscale crop (exactly what will be base64-encoded and sent) to `debug_lens.png`.

### BUG-2 — Lens text box clips long results
The text display area beneath the lens window is too small and clips content when OCR results are long.

**Proposed fixes (pick one or combine):**
- Auto-scroll: slowly scroll down through the text, pause at bottom, reset to top and repeat (marquee-style vertical scroll)
- **Shift+Tab toggle**: swap content between the HUD overlay and the lens text box — what was in the HUD moves to the text box and vice versa, toggling back and forth on each Shift+Tab press

---

## 🧹 Housekeeping / Chores

### CHORE-1 — Rename `./lenzu` → `./lenzu_client`
The main client crate lives in `./lenzu/` but should be `./lenzu_client/` to match the naming of `./lenzu_server/` and make the workspace layout self-documenting.

**Touch-points to update:**
- `lenzu/Cargo.toml` → `lenzu_client/Cargo.toml` (package `name` field, path refs)
- Root `Cargo.toml` `[workspace] members` entry
- `build.rs` (if it references the directory by name)
- Shell scripts / Makefiles referencing `./lenzu/`
- `README.md` / docs referencing the old path
- Any `#[path]` or `include!` macros that embed the old directory name
- CI/CD workflows (`.github/workflows/`) that reference `lenzu/`

---

## Architecture Overview (Current)

```
┌─────────────────────────────────┐
│         lenzu (client)          │
│  GTK3 lens window               │
│  X11 capture (x11rb)            │
│  OpenRouter API (reqwest)       │
│  → format_for_overlay()         │
│  → send_to_overlay() via UDP    │
└────────────┬────────────────────┘
             │ UDP loopback (default :7331)
             ▼
┌─────────────────────────────────┐
│     lenzu_server (Electron)     │
│  Transparent overlay window     │
│  UDP listener (dgram)           │
│  IPC: "hud-text-changed"        │
│  app.ts renders text in HUD     │
└─────────────────────────────────┘
```

### IPC: why UDP?
- Zero setup: no socket files, no ports to register
- Fire-and-forget: client never blocks waiting for the server
- Datagram model matches our use case: one capture → one text update
- Works even if `lenzu_server` isn't running (packet silently dropped)
