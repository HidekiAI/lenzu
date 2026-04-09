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
- [ ] **INVESTIGATE** — fullscreen scan returns exactly 1 bbox: retry with dilation=0 (see below)

---

## 🐛 Known Bugs

### ~~BUG-1 — Spinning cursor lollipop artefact~~ ✓ FIXED
Cairo's `arc()` draws a connecting line from the current path point (left by the preceding `show_layout` call) to the arc start, producing a lollipop tail. Fixed by inserting `cr.new_sub_path()` before `cr.arc()` to break the path connection.

### ~~BUG-3 — Image not greyscaled before DBNet detection~~ ✓ FIXED
`det.detect()` now receives `dyn_image.grayscale()` (computed once at the top of the worker closure). The fullscreen debug overlay and crop calls retain the original RGB image so bounding-box visualisation stays coloured.

### ~~DBG-1 — Save pre-wire image to debug_lens.png instead of raw capture~~ ✓ FIXED
`save_debug_image()` removed. Replaced by `save_prewire_debug(image)` called at the top of both `DualOcrClient::call_api()` and `call_api_force_fallback()`, writing the greyscale crop (exactly what will be base64-encoded and sent) to `debug_lens.png`.

### BUG-4 — Fullscreen scan over-merges into a single bbox; retry with dilation disabled

**Symptom:** Ctrl+Shift+Click fullscreen scan returns exactly 1 bounding box covering a
large screen area. This is a sign that the morphological dilation (`text_detection_dilation`,
default 16) has merged all nearby text blobs into one giant region. The subsequent crop
sent to remote OCR is oversized and often produces garbage or a timeout.

**Hypothesis:** At 640×640 detection resolution, a dilation radius of 16 px is
proportionally large relative to the full desktop scaled down. Multiple separate UI
text regions bleed together and the connected-components step collapses them into one.

**Proposed fix — detect-and-retry:**

1. Run fullscreen DBNet with the normal config (existing behaviour).
2. If `boxes.len() == 1` **and** the single box covers more than a configurable fraction
   of the screen area (e.g. > 25 % — a heuristic for "this is clearly over-merged"),
   re-run `det.detect()` on the same already-captured `gray_image` with `dilation = 0`.
3. If the second run produces `> 1` box, use those results; otherwise fall back to the
   original single box.

**Touch-points:**
- `jp_detect` crate must expose per-call dilation override, or a second detector instance
  must be constructed with `dilation=0` at startup and stored alongside the primary in
  `AppState` (same `Arc<dyn TextDetector + Send + Sync>`).  Check `jp_detect` API first.
- If a second detector instance is the only option: add
  `text_detector_nodilation: Option<Arc<dyn TextDetector + Send + Sync>>` to `AppState`
  and initialise it unconditionally whenever the primary detector is configured.
- Retry logic lives in the `force_remote` branch of the capture thread in `main.rs`
  (currently around the `is_fullscreen_scan` block).
- Add a config flag `fullscreen_single_bbox_retry: bool` (default `true`) so the retry
  can be disabled if it causes issues.

**Investigation first:** confirm the symptom is reproducible with a known over-merged
screen, compare `fullscreen-debug.png` and `fullscreen-debug.json` between dilation=16
and dilation=0 runs, and verify the retry produces meaningfully more boxes before
committing to the implementation.

### BUG-2 — Lens text box clips long results
The text display area beneath the lens window is too small and clips content when OCR results are long.

**Proposed fixes (pick one or combine):**
- Auto-scroll: slowly scroll down through the text, pause at bottom, reset to top and repeat (marquee-style vertical scroll)
- **Shift+Tab toggle**: swap content between the HUD overlay and the lens text box — what was in the HUD moves to the text box and vice versa, toggling back and forth on each Shift+Tab press

---

## 💡 Future Ideas

### IDEA-2 — Local Ollama payload optimisation

Four targeted reductions to avoid sending oversized images to local Ollama.
Ordered by impact vs. effort; each has a concrete unit-test gate so we
know whether it's actually worth shipping before touching the hot path.

---

#### OPT-1 — Union-bbox crop before Phase 1 fallback

**Problem:** When DBNet detects boxes but all per-region calls fail, the
fallback at `main.rs` (Phase 1 fallback block) still sends the full
`dyn_image` (lens-sized, e.g. 400×400). `compute_union_bbox` is already
re-exported from `jp_detect` via `ocr::text_detection`.

**Fix:** `union = compute_union_bbox(&boxes)` → `TextCropper::crop(&dyn_image, &[union])` →
pass that crop to `call_api` instead of `dyn_image`.

**Touch-points:** `main.rs` fallback block only; no new config, no
changes to `client.rs` or `utils.rs`.

**Unit test to prove value:**
```rust
// ocr/text_cropper.rs or a new tests/opt_union_crop.rs
// Construct a 400×400 DynamicImage with a 40×20 "text region" in the
// corner.  Feed two TextBoundingBoxes that cover that region to
// compute_union_bbox(), then TextCropper::crop().  Assert the resulting
// CroppedRegion dimensions are ≤ 40+2*pad × 20+2*pad (well under 400×400).
// This confirms the crop path produces a smaller image — without any
// Ollama call.
```

---

#### OPT-2 — `primary_max_dimension` config (downscale for local Ollama)

**Problem:** `call_api` calls `encode_as_grayscale(image)` with no size
cap, so a 400×400 or 800×800 (oversampled) grayscale PNG is sent to
every local Ollama call. `encode_for_fallback` already supports a
`max_dim` cap for remote — the same mechanism is absent for primary.

**Fix:**
- Add `primary_max_dimension: u32` to `AppConfig` (default `0` = no
  limit, suggested starting value `512`).
- Add `primary_max_dimension: u32` field to `DualOcrClient`.
- In `call_api`, replace `encode_as_grayscale(image)` with
  `encode_for_fallback(image, self.primary_max_dimension)` (reuses
  existing downscale logic; `max_dim=0` is a no-op so no behaviour
  change for users who don't set it).

**Touch-points:** `config.rs` (+1 field), `client.rs` (+1 field +
constructor arg + one call-site change), all `DualOcrClient::new()`
call-sites in `main.rs` (+1 arg, ~3 places).

**Unit test to prove value:**
```rust
// utils.rs tests block
// Create a 600×400 DynamicImage.  Call encode_for_fallback(img, 512).
// Decode the resulting base64 PNG and assert longest edge ≤ 512 and
// shortest edge is proportionally scaled (not distorted).
// Then call encode_for_fallback(img, 0) and assert dimensions are
// unchanged (600×400) — proving max_dim=0 is truly a no-op.
```

---

#### OPT-3 — Skip Ollama call entirely when DBNet finds zero boxes

**Problem:** If DBNet is configured and detects no boxes in the lens
capture, the image almost certainly has no readable text, yet the code
falls through to a full-image Ollama round-trip anyway.

**Fix:** After `det.detect(&gray_image)` returns an empty `boxes` vec
in the `!force_remote` branch, return `Err("No text detected".into())`
(or a dedicated empty-result `Ok`) immediately, without constructing a
`DualOcrClient` or calling `call_api`.

Make this opt-in: add `skip_ocr_if_no_boxes: bool` to `AppConfig`
(default `false`) so users can enable it once they're confident in their
DBNet model's recall.

**Touch-points:** `config.rs` (+1 bool field), `main.rs` (~5 lines in
the `!force_remote` branch).

**Unit test to prove value:**
```rust
// No real test needed for the guard itself (trivial branch).
// The valuable test: create a solid-colour 400×400 DynamicImage with no
// text-like content, run it through a mock TextDetector that returns
// vec![], and assert the function returns before constructing a
// DualOcrClient.  Use a mock/spy on DualOcrClient::new() or count
// HTTP calls via wiremock — zero calls expected.
```

---

#### OPT-4 — Downscale oversampled capture before handing to Ollama

**Problem:** `text_detection_oversample_factor` (default 2.0) captures
up to `max(lens_size × factor, 640)` pixels for DBNet accuracy.  That
oversized image is then passed as `dyn_image` into `call_api` — Ollama
receives an 800px image when the text region (after crop) may be 60px.
OPT-1 + OPT-2 together largely neutralise this, but for the no-detector
fallback path it still applies.

**Fix:** After capture and before any `call_api` call on the non-cropped
path, resize `dyn_image` back down to `lens_size × lens_size` (or
`primary_max_dimension` from OPT-2, whichever is smaller).  This is
already implicit if OPT-2 is implemented with a sensible
`primary_max_dimension` value.

**Note:** Implement OPT-1 + OPT-2 first; OPT-4 may become a no-op.

**Unit test to prove value:**
```rust
// Confirm encode_for_fallback(oversampled_image, lens_size) produces
// output whose decoded dimensions equal lens_size × lens_size (or the
// proportionally-scaled equivalent).  Verifies the downscale path
// triggers correctly when the capture exceeds the cap.
// (Reuses the encode_for_fallback test from OPT-2 with a larger input.)
```

---

### IDEA-1 — Dynamic lens resize on Ctrl+Shift hover

When Ctrl+Shift is held (no click), the lens window resizes to match whichever detected bounding box the cursor is hovering over. Resets to `config.lens_size` when the cursor is not inside any bbox, and again when the OCR result arrives.

**Interaction flow:**
1. Hold Ctrl+Shift → trigger a fullscreen DBNet detect-only scan (background thread, no OCR call)
2. Timer loop checks cursor against cached boxes each tick:
   - Cursor inside a bbox → resize lens to that bbox's `(w, h)`
   - Cursor outside all boxes → lens = default `config.lens_size`
3. Ctrl+Shift+Click on a highlighted box → OCR that region (existing flow)
4. OCR result received → clear cached boxes, reset lens to default

**Implementation touch-points (`main.rs` only; no changes to `client.rs` / `config.rs`):**
- `AppState`: add `cached_boxes`, `cached_boxes_origin: (i32, i32)`, `lens_override: Option<(i32, i32)>`, `ctrl_shift_scan_done: bool`
- Shared `Arc<Mutex<Option<(Vec<TextBoundingBox>, (i32, i32))>>>` to pass detect results from background thread to timer loop
- Timer loop: spawn detect-only thread on Ctrl+Shift rising edge; drain shared mutex each tick; hit-test cursor against boxes; call `window.resize(eff_w, eff_h + ui_panel_height)` and `window.move_(x - eff_w/2, y - eff_h/2)`
- Draw callback: replace all bare `s.config.lens_size` refs with `s.eff_w()` / `s.eff_h()` helpers
- RX handler: `s.lens_override = None; s.cached_boxes.clear();` on result

**Coordinate note:** using the fullscreen-scan path means boxes are in screen coordinates (origin `(0, 0)`), so the cursor hit-test is a direct `x1 <= cx <= x2 && y1 <= cy <= y2` with no offset math.

**Estimated scope:** ~100 lines added/changed, all in `main.rs`.

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
