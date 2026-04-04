# Lenzu Project Planning & Roadmap

## Current Status (2026-03-22, branch: `RemoteOCR`)
- **Platform**: Linux-primary (X11, GTK3)
- **OCR/Translation**: OpenRouter API → Gemini 2.0 Flash (multimodal); returns structured JSON
- **UI**: GTK3 floating lens window (Cairo + Pango), transparent RGBA
- **Capture**: X11 root window via `x11rb` (bypasses GPU-accelerated windows correctly)
- **Overlay HUD**: Separate Electron process (`lenzu_server`) — transparent window, UDP IPC
- **Interpreter**: Removed (was Kakasi); translation now handled entirely by the LLM
- **Config**: `lenzu_config.json` with `isolang` language codes and `OverlayRenderMode` enum
- **Workspace**: Cargo workspace at repo root; members: `lenzu` (client) and prototypes; `lenzu_server` is a separate Node/Electron project

## Vision
Make **Linux the primary platform** with a robust, performant OCR lens that works across Wayland and X11, using open-source tools while maintaining the offline-first, privacy-respecting design.

## Goals

### Completed

- **Phase 2 — Process Lifecycle** **[COMPLETED: 2026-03-28]**
  - `lenzu` spawns/kills `lenzu_server` (Electron HUD) automatically

- **Phase 3 — Migrate Overlay HUD (Tauri → Electron)** **[COMPLETED]**
  - Tauri/WebKit2GTK dropped due to alpha compositing artefacts on X11; Electron confirmed clean
  - **Known limitation**: thin white titlebar strip on some compositors (mitigated, not eliminated)

---

### Short-term — Phase 4 (dependency-ordered)

> Branch: `feature/ocr-local-remote`  
> Design spec: `docs/technical-design.phase4-predetect.md`

Each step below depends on the previous being working and tested before moving on.

#### Step 1 — Docker / ollama container lifecycle `[M7b-1]`
*Unblocks everything else — no point writing OCR code if the server won't start.*

- [ ] `scripts/setup.sh` installs Docker, pulls `ollama/ollama`, pulls `gemma4:e2b` *(scripts written, needs smoke-test on clean machine)*
- [ ] `scripts/run.sh` starts `lenzu-ollama` container before lenzu, stops on exit via `trap`
- [ ] Manual test: `docker ps` confirms container running; `curl http://localhost:11434/` returns healthy
- [ ] Unit test: `start_ollama` idempotent (running twice doesn't create duplicate containers)
- **Done when**: `./scripts/run.sh` reliably starts and stops the container with no orphans

#### Step 2 — `OcrClient` → ollama (Gemma as sole backend) `[M7b-2]`
*Validate that Gemma actually produces usable OCR results before wiring fallback logic.*

- [ ] `AppConfig` defaults change: `llm_api_endpoint` → `http://localhost:11434/v1/chat/completions`, `llm_default_model` → `gemma4:e2b`
- [ ] `OcrClient::call_api` skips `Authorization` header when `api_key` is empty (ollama needs none)
- [ ] `OPENROUTER_API_KEY` not required to start lenzu (warn only)
- [ ] Shift+Click capture → Gemma query → result displayed in HUD (happy-path manual QA)
- [ ] Unit test: empty `api_key` → no `Authorization` header in built request
- [ ] Unit test: old `lenzu_config.json` without new fields still loads with defaults (backward compat)
- **Done when**: Shift+Click shows a translation via local Gemma with OpenRouter key unset

#### Step 3 — `DualOcrClient`: automatic fallback + manual override `[M7b-3]`
*Two trigger paths: automatic (Gemma gave no translation) and manual (user forces remote).*

- [ ] `DualOcrClient` struct wraps primary (`OcrClient` → ollama) + optional fallback (`OcrClient` → OpenRouter)
- [ ] `needs_fallback(results)`: true when result vec is empty or all `english` fields are blank/whitespace
- [ ] Automatic fallback: primary fail or no translation → retry with OpenRouter, log to `/dev/shm/api_debug.txt`
- [ ] **`Ctrl+Shift+Click`** — new hotkey: skips primary entirely, sends directly to OpenRouter
  - Detected in `main.rs` input handler alongside existing `Shift+Click` (add `CONTROL_MASK` check)
  - Requires `OPENROUTER_API_KEY` set; shows error in HUD if not configured
  - Useful for: comparing Gemma vs Gemini output, bypassing local when Gemma is slow
- [ ] `AppConfig` gains `fallback_llm_api_endpoint` + `fallback_llm_model` with `#[serde(default)]`
- [ ] Unit tests for `needs_fallback` (empty vec, all-null, whitespace, partial success)
- [ ] Integration tests: primary no-translation → fallback fires; primary HTTP 503 → fallback fires; both fail → error (wiremock)
- [ ] Integration test: `Ctrl+Shift+Click` path calls fallback directly, primary mock gets zero calls
- **Done when**: automatic fallback works silently; Ctrl+Shift+Click forces remote visibly

#### Step 4 — Fallback image preprocessing `[M7b-4]`
*Only relevant once fallback is confirmed working — polish step.*

- [ ] `raw_to_dynamic_image(raw, w, h) -> DynamicImage` in `utils.rs`
- [ ] `encode_for_fallback(image, max_dim) -> String`: grayscale + proportional downscale
- [ ] `DualOcrClient::call_api` signature changes to accept `&DynamicImage`; encodes colour for primary, grayscale+scaled for fallback
- [ ] `AppConfig` gains `fallback_preprocess_grayscale: bool` (default `true`) + `fallback_max_dimension: u32` (default `800`)
- [ ] Save preprocessed image to `/dev/shm/debug_lens_fallback.png` for dev comparison
- [ ] Unit tests: grayscale output confirmed via PNG colour type; downscale preserves aspect ratio; zero max-dim disables resize; fallback b64 smaller than colour b64
- [ ] Integration test: fallback request body measurably smaller than primary request body
- **Done when**: fallback image is visibly smaller/grayscale in debug file; token savings confirmed in `api_debug.txt`

---

### Deferred from Phase 4 (not yet started)

- **YOLO pre-detection** (`M7`): find text bounding boxes locally before any API call — further reduces tokens by sending only cropped regions. Deferred until M7b-4 is stable; depends on manga-specific ONNX model (COCO YOLOv8n insufficient).

### Short-term — UI/UX
   - Overlay position: configurable top/bottom via `hud_config.json`
   - Click-through mode for `lenzu_server` window (doesn't steal mouse events)

### Medium-term (1-3 Months)
1. **OCR Engine Diversification**
   - Evaluate EasyOCR (Python) via subprocess for Linux (does NOT handle vertical, discard)
   - Research manga-ocr integration (requires Python/PyTorch, heavy)
   - Investigate `rust-ocr` or `paddleocr-rs` alternatives
   - Consider training custom Tesseract model with manga109s dataset

2. **Advanced Features**
   - Text direction detection (vertical/horizontal auto)
   - Multi-line reordering for vertical text (right-to-left → left-to-right)
   - Dictionary lookup (jmdict, etc.)
   - Copy-to-clipboard with furigana toggle
   - History of recognized text

3. **Multi-monitor & HiDPI**
   - Detect all monitors and allow lens to span
   - Handle scaling factors correctly on HiDPI displays
   - Per-monitor DPI awareness

4. **Wayland Security**
   - Work with portals (xdg-desktop-portal) for screen capture
   - Fallback to XWayland compatibility mode if needed
   - Request permission for screen capture gracefully

### Long-term (3-6+ Months)
1. **Alternative OCR Backends**
   - Google Cloud Vision (online, paid) via OAuth2
   - Microsoft Azure Computer Vision (online, paid)
   - Apple Vision framework (macOS port)

2. **Machine Learning Integration**
   - On-device manga-specific model (tiny, fast)
   - Text bubble detection with YOLO/ONNX Runtime
   - Preprocessing CNN for denoising and binarization

3. **Accessibility Features**
   - Screen reader integration (Orca, speech-dispatcher)
   - High contrast mode
   - Configurable font sizes and colors

4. **Community & Ecosystem**
   - Plugin system for interpreters (kakasi, mecab, kuromoji)
   - Shared traineddata repository
   - Translation backends (DeepL, Google Translate, offline dictionaries)

### Known Issues & Blockers

### Overlay HUD (`lenzu_server`)
- **xfwm4 compositor ghosting**: xfwm4's built-in compositor uses alpha-blend accumulation — each frame is blended on top of the previous buffer rather than composited fresh against the desktop, so semi-transparent areas fill with dark ghost pixels over time. The 3-frame erase cycle in `app.js` mitigates this but does not eliminate it. Full fix: disable xfwm4 compositing (`xfconf-query -c xfwm4 -p /general/use_compositing -s false`) and replace with `picom --backend glx --no-use-damage` (`--no-use-damage` forces full-surface redraws). See `lenzu_server/README.md` for complete steps.
- `lenzu_server` must be started before `lenzu` (Phase 2 fixed this with auto-spawn)
- Click-through not yet implemented (window intercepts mouse events)

### Electron Overlay Issues
- **Electron failed to install correctly**: Error `Electron failed to install correctly, please delete node_modules/electron and try installing again`.
  - **Resolution**: Manually delete `lenzu_server/node_modules` and `lenzu_server/pnpm-lock.yaml`, then re-run `lenzu_server/scripts/setup.sh`. This ensures a clean reinstallation of Electron and its dependencies.

### API / Token Cost
- Full lens image sent on every capture — no pre-filtering yet
- Gemini 2.0 Flash is cheap (~$0.0006/hr in practice) but Phase 4 YOLOv8 pre-detection will cut costs further
- **Gemma 4 E2B / E4B** (Google open-weights VLM, `google/gemma-4-E2B-it` / `google/gemma-4-E4B-it`): 140+ language native support, strong OCR and handwriting recognition, runs fully on-device. Ollama: `gemma4:e2b` / `gemma4:e4b`. GGUF (4-bit/8-bit) via Unsloth HuggingFace page. Viable as a zero-cost offline replacement for the remote API. Evaluation tracked in M7b.

### Linux Capture
- Wayland: not yet supported; X11 only via `x11rb`
- Multi-monitor: untested with monitors at non-zero offsets

## Technical Decisions

### UI toolkit: GTK3 (final decision)
- **Decision**: GTK3 (`gtk-rs` 0.18) — permanent choice, not a stepping stone to GTK4
- **Rationale**: GTK4 was evaluated and abandoned — graphene/gobject dep complexity, API churn, prototype build failures. GTK3 provides everything needed and is simpler to build against.

### OCR Backend Selection
- **Linux**: Tesseract CLI (via `rusty-tesseract` or custom wrapper)
- **Fallback**: If Tesseract fails, try OCR with different PSM or grayscale vs color
- **Future**: Allow user to switch between engines (Tesseract, manga-ocr, gcloud)

### Image Preprocessing Pipeline
```
Capture → Grayscale → Denoise (median filter) → Contrast stretch → Binarize (adaptive) → OCR
```
- Use `imageproc` for filters
- Benchmark each step to ensure <100ms total overhead

### Packaging
- **Primary**: Flatpak (Sandboxed, includes all dependencies)
- **Secondary**: Debian/Ubuntu PPA
- **Tertiary**: AppImage for universal Linux distribution

### Installation (`scripts/install.sh`) — planned, not yet in repo

End-user and “single command after clone” flows are not fully covered today. **What exists now** is developer bootstrap only:

| Artifact | Purpose |
|---|---|
| `scripts/setup.sh` | APT packages + runs `lenzu_server/scripts/setup.sh` (Node, pnpm, Electron) |
| `lenzu_server/scripts/setup.sh` | Node toolchain and `pnpm install` / Electron binary |
| `scripts/run.sh` | `cargo build -p lenzu` and run the debug client (client auto-spawns the Electron HUD when enabled) |

**Gap:** there is no `scripts/install.sh` yet — no release build to `~/.local/bin` (or `/usr/local`), no `.desktop` entry, and no documented layout for a relocatable install. The GTK client resolves `lenzu_server` relative to the `lenzu` crate at compile time (`../lenzu_server`); a real installer must either install both trees under a known prefix, set an environment variable, or change the client to resolve the HUD path at runtime (decision TBD when the script lands).

**Planned add:** `scripts/install.sh` (or equivalent) should: optional `setup.sh`, `cargo build --release -p lenzu`, install the `lenzu` binary and ship `lenzu_server` beside it or under a fixed share path, update `PATH` / symlink, and optionally install a desktop file. Formal distribution for non-developers remains **Packaging** above (Flatpak first).

## Dependencies & Tools

### Must-Have (Linux)
- `tesseract-ocr` (>=5.0) with `jpn_vert` traineddata
- `libgtk-3-dev` (GTK3 >=3.24 — **not GTK4**)
- `pkg-config` and `build-essential`

### Optional / Legacy
- `libwebkit2gtk-4.1-dev` — **removed from setup.sh**; was required by the deprecated Tauri overlay (`lenzu_client`). Electron bundles its own Chromium — no system WebKit dependency needed.

### Nice-to-Have
- `opencv` (for advanced preprocessing) - but heavy dependency
- `leptonica` (Tesseract dependency, usually bundled)
- `mecab` (alternative interpreter)
- `jmdict` (dictionary lookup)

## Research Tasks

1. **Tesseract Training**: Investigate manga109s dataset format and training pipeline
2. **Wayland Portals**: Study `xdg-desktop-portal` API for screen capture permissions
3. **Electron overlay positioning**: ArrowUp/Down key cycling (top/center/bottom) already implemented in lenzu_server
4. **Performance Profiling**: Profile capture → OCR pipeline to identify bottlenecks

## Milestones

- [x] **M1**: X11 capture works correctly (bypasses GPU-accelerated windows)
- [x] **M2**: Structured OCR results via OpenRouter/Gemini multimodal API
- [x] **M3**: GTK3 lens window with transparent overlay, spinner, flash feedback
- [x] **M4**: Electron overlay HUD (`lenzu_server`) integrated via UDP IPC (migrated from Tauri/WebKit2GTK due to alpha/transparency bugs)
- [x] **M5**: Configurable language pair, render mode, and prompt via `lenzu_config.json`
- [x] **M6**: `lenzu` auto-spawns/kills `lenzu_server` (Phase 2)
- [ ] **M7b-1**: Docker/ollama container lifecycle — `scripts/setup.sh` + `scripts/run.sh` start/stop `lenzu-ollama` reliably
- [ ] **M7b-2**: `OcrClient` → ollama (Gemma 4 E2B as sole backend, no API key required)
- [ ] **M7b-3**: `DualOcrClient` — automatic fallback to OpenRouter on no-translation; `Ctrl+Shift+Click` manual override to force remote
- [ ] **M7b-4**: Fallback preprocessing — grayscale + proportional downscale before OpenRouter call (~70–80% token reduction)
- [ ] **M7**: YOLOv8 pre-detection — find text bounding boxes locally, send only cropped regions (deferred until M7b-4 stable; needs manga-specific ONNX model)
- [ ] **M8**: Wayland support via portals
- [ ] **M9**: Flatpak packaging
- [ ] **M10**: `scripts/install.sh` (or equivalent) — release binary + `lenzu_server` layout, `PATH` / `.desktop`, runtime HUD path (see **Installation** above)

## Testing Strategy

### Unit Tests
- OCR pipeline with sample images (captured from manga)
- Text reordering algorithm (vertical → horizontal)
- Image preprocessing filters (verify output quality)

### Integration Tests
- Full capture → OCR → interpreter → render flow
- Multi-monitor setup (capture from secondary monitor)
- Wayland vs X11 backend detection

### Manual QA
- Test on various manga styles (shonen, shojo, seinen)
- Different font sizes and qualities (scanned vs digital)
- Mixed vertical/horizontal text layouts

## Success Metrics

1. **Accuracy**: >90% character recognition on clean manga panels
2. **Performance**: <3s from capture to display on 1080p
3. **Usability**: Works out-of-the-box on Ubuntu 22.04+ and Fedora 38+
4. **Accessibility**: Screen reader compatible (Orca)
5. **Privacy**: No network traffic unless user enables online OCR

## References

- GTK3 (gtk-rs): https://gtk-rs.org/gtk3-rs/stable/latest/
- Tesseract Training: https://tesseract-ocr.github.io/tessdoc/Training-Tesseract.html
- Wayland Portals: https://flatpak.org/xdg-desktop-portal/
- Manga109: http://www.manga109.org/
- Kakasi: http://kakasi.namazu.org/

---

**Last Updated**: 2026-04-04
**Maintainer**: Hideki AI
**Status**: Active development — `feature/ocr-local-remote` branch, Phase 4 M7b-1 next

