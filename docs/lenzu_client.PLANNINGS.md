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
- [x] `send_to_overlay(text, port)` — fires a UDP datagram to the Tauri HUD process
- [x] `format_for_overlay(results, mode)` — formats `Vec<TranslationResult>` according to `OverlayRenderMode`
  - `debug` mode includes bounding boxes and debug_info per result
  - Single-field modes fall back to `original` if the field is absent

### Integration Tests
- [x] `tests/integration_test.rs` — full pipeline mock (wiremock + OcrClient)

---

## ✅ Phase 1B — lenzu_server in Workspace (COMPLETE)

The `tauri-translucent-desktop-overlay` project has been brought into this workspace as `lenzu_server`.

### Structure
```
lenzu/                          ← workspace root
├── Cargo.toml                  ← workspace (members: lenzu, lenzu_server/src-tauri)
├── lenzu/                      ← OCR lens client (GTK3)
└── lenzu_server/
    ├── package.json            ← @tauri-apps/cli devDep
    ├── src-tauri/
    │   ├── Cargo.toml          ← name = "lenzu_server"
    │   ├── tauri.conf.json     ← productName = "lenzu_server", identifier = com.hidekiai.lenzu-server
    │   ├── capabilities/
    │   ├── icons/
    │   └── src/main.rs         ← UDP listener + Tauri window; accepts --port CLI arg
    └── ui/
        ├── index.html
        ├── app.js              ← listens for "hud-text-changed" Tauri event
        └── styles.css
```

### Key change from original overlay
- `lenzu_server` accepts `--port <N>` CLI argument, which overrides `hud_config.json`.
  This lets `lenzu_client` pass its configured `overlay_udp_port` at spawn time.

### Running standalone (for testing)
```bash
cd lenzu_server
npm install
npm run tauri dev -- -- --port 7331
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

## 🔲 Phase 4 — Detection Pipeline (Future)

Once the client/server lifecycle is stable, revisit the original vision of local text detection before sending to the LLM:

- Pre-screen the lens capture with YOLOv8n (already in repo: `yolov8n_fp16.onnx`) to find text bounding boxes
- Only send cropped text regions to the API → lower token cost, higher accuracy
- `top_xy` / `bot_xy` fields in `TranslationResult` will anchor results to screen coordinates for future overlay positioning

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
│       lenzu_server (Tauri)      │
│  Transparent overlay window     │
│  UDP listener thread            │
│  Emits "hud-text-changed" event │
│  app.js renders text in HUD     │
└─────────────────────────────────┘
```

### IPC: why UDP?
- Zero setup: no socket files, no ports to register
- Fire-and-forget: client never blocks waiting for the server
- Datagram model matches our use case: one capture → one text update
- Works even if `lenzu_server` isn't running (packet silently dropped)
