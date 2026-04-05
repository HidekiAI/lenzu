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

_Unblocks everything else — no point writing OCR code if the server won't start._

- [ ] `scripts/setup.sh` installs Docker, pulls `ollama/ollama`, pulls `gemma4:e2b` _(scripts written, needs smoke-test on clean machine)_
- [ ] `scripts/run.sh` starts `lenzu-ollama` container before lenzu, stops on exit via `trap`
- [ ] Manual test: `docker ps` confirms container running; `curl http://localhost:11434/` returns healthy
- [ ] Unit test: `start_ollama` idempotent (running twice doesn't create duplicate containers)
- **Done when**: `./scripts/run.sh` reliably starts and stops the container with no orphans

#### Step 2 — `OcrClient` → ollama (Gemma as sole backend) `[M7b-2]`

_Validate that Gemma actually produces usable OCR results before wiring fallback logic._

- [ ] `AppConfig` defaults change: `llm_api_endpoint` → `http://localhost:11434/v1/chat/completions`, `llm_default_model` → `gemma4:e2b`
- [ ] `OcrClient::call_api` skips `Authorization` header when `api_key` is empty (ollama needs none)
- [ ] `OPENROUTER_API_KEY` not required to start lenzu (warn only)
- [ ] Shift+Click capture → Gemma query → result displayed in HUD (happy-path manual QA)
- [ ] Unit test: empty `api_key` → no `Authorization` header in built request
- [ ] Unit test: old `lenzu_config.json` without new fields still loads with defaults (backward compat)
- **Done when**: Shift+Click shows a translation via local Gemma with OpenRouter key unset

#### Step 3 — `DualOcrClient`: automatic fallback + manual override `[M7b-3]`

_Two trigger paths: automatic (Gemma gave no translation) and manual (user forces remote)._

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

#### Step 4 — Image preprocessing `[M7b-4]`

_Grayscale applies to all captures (local + remote); downscale applies only before remote calls._

- [ ] `raw_to_dynamic_image(raw, w, h) -> DynamicImage` in `utils.rs`
- [ ] `encode_as_grayscale(image) -> String` in `utils.rs`: grayscale PNG, full resolution — used for primary (Gemma) path
- [ ] `encode_for_fallback(image, max_dim) -> String` in `utils.rs`: grayscale + proportional downscale — used for fallback (OpenRouter) path
- [ ] `DualOcrClient::call_api` signature changes to accept `&DynamicImage`; calls `encode_as_grayscale` for primary, `encode_for_fallback` for fallback
- [ ] `AppConfig` gains `fallback_max_dimension: u32` (default `800`) — downscale limit for remote calls only
- [ ] Save preprocessed fallback image to `/dev/shm/debug_lens_fallback.png` for dev comparison
- [ ] Unit tests: grayscale output confirmed via PNG colour type for both paths; downscale preserves aspect ratio; zero max-dim disables resize; fallback b64 smaller than primary b64 (downscale effect)
- [ ] Integration test: fallback request body measurably smaller than primary request body
- **Done when**: local path sends grayscale full-res; fallback path sends grayscale downscaled; debug file confirms visually; token savings confirmed in `api_debug.txt`

---

### Deferred from Phase 4 (not yet started)

- **YOLO pre-detection** (`M7`): find text bounding boxes locally before any API call — further reduces tokens by sending only cropped regions. Deferred until M7b-4 is stable; depends on manga-specific ONNX model (COCO YOLOv8n insufficient).

### Short-term — UI/UX

- Overlay position: configurable top/bottom via `hud_config.json`
- Click-through mode for `lenzu_server` window (doesn't steal mouse events)
- **Lens-box resize** (post-YOLO): when YOLO pre-detection is active, allow the user to dynamically resize the lens capture box (click-drag) so text that doesn't fit inside the default box can be included. Box should never shrink below a configurable minimum size. Auto-adjust option: YOLO bounding boxes could be used to suggest an optimal crop size.

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

### Installation — Script Architecture (planned)

#### Platform scope

All scripts (`setup.sh`, `build.sh`, `prereqs.sh`, `run.sh`, `install.sh`) target **Debian-family Linux** (`apt`, `dpkg`) as the only supported platform. This covers Ubuntu, Debian, Mint, Pop!\_OS, and derivatives. Fedora-family (`dnf`/`rpm`) is a planned future target; no other distros are in scope. Scripts may assert this with a friendly error if `/etc/debian_version` is absent.

---

#### What exists today (developer bootstrap only)

| Script                          | Purpose                                                                                          |
| ------------------------------- | ------------------------------------------------------------------------------------------------ |
| `scripts/setup.sh`              | APT packages + debug `cargo build` + calls `lenzu_server/scripts/setup.sh` + ollama/Docker setup |
| `lenzu_server/scripts/setup.sh` | nvm → Node → pnpm → `pnpm install` → Electron binary                                             |
| `scripts/run.sh`                | ollama lifecycle (start/stop container) + `cargo build -p lenzu` (debug) + run client            |

**Gap:** no release build, no installable package, no `.desktop` entry, and `lenzu` resolves `lenzu_server` via `CARGO_MANIFEST_DIR` at compile time — which breaks outside a git checkout.

---

#### Planned script set

The core problem: end users don't want `rustc`, `cargo`, `tsc`, `pnpm`, or `nvm`. But ollama/Electron still need to be present at runtime. Splitting into four scripts keeps each one focused:

```
scripts/
├── prereqs.sh    ← NEW: runtime downloader (ollama, model, Electron). No compiler.
├── setup.sh      ← EXISTING: dev toolchain (APT, rustc, Node, pnpm). Calls prereqs.sh.
├── build.sh      ← NEW: release builder + packager. Calls setup.sh → produces dist/
├── run.sh        ← EXISTING: dev loop. Calls prereqs.sh for ollama, then cargo build + run.
└── install.sh    ← NEW: end-user installer. Calls prereqs.sh, extracts dist package.
```

**Dependency flow:**

```
install.sh ──calls──► prereqs.sh
setup.sh   ──calls──► prereqs.sh
build.sh   ──calls──► setup.sh ──calls──► prereqs.sh
run.sh     ──calls──► prereqs.sh  (ollama lifecycle already inline; may refactor to call prereqs.sh)
```

---

#### `scripts/prereqs.sh` — shared developer downloader

> **Scope: developer scripts only** (`setup.sh`, `build.sh`, `run.sh`). `install.sh` does NOT call this.

Idempotent (skips steps already done). No compiler logic. Shared to avoid duplicating the ollama/Docker decision tree that currently lives in both `setup.sh` and `run.sh`.

Responsibilities:

- **ollama** (developer path — Docker or native, user's choice): same decision tree currently in `setup.sh`/`run.sh` (Docker first, then native install)
- **ollama model pull**: `gemma4:e2b` against whatever backend is running
- **Electron binary**: checks `lenzu_server/node_modules/electron/dist/electron`; if absent, runs `node lenzu_server/node_modules/electron/install.js` (Node must already be on PATH from `setup.sh`)

Does NOT install: `rustc`, `cargo`, `node`, `pnpm`, `nvm`, APT packages.

Flags: `--skip-ollama` (for CI or OpenRouter-only dev), `--skip-electron` (for CI builds where the Electron binary is irrelevant)

---

#### `scripts/build.sh` — release builder + packager

For developers and CI. Produces a redistributable `dist/` package consumed by `install.sh`.

Steps:

1. Call `scripts/setup.sh` (ensures dev toolchain present; `setup.sh` calls `prereqs.sh`)
2. `cargo build --release -p lenzu`
3. `cd lenzu_server && pnpm install --frozen-lockfile && pnpm run build`
4. Download the ollama Linux binary for `x86_64` and `aarch64` from the ollama GitHub releases page into `dist/` — **these become part of the package** so `install.sh` never needs to figure out where to get ollama
5. Assemble `dist/lenzu-<version>/` to match the installed layout exactly:
   ```
   dist/lenzu-<version>/
   ├── install.sh                          ← copy of scripts/install.sh (self-contained)
   ├── bin/
   │   └── lenzu-core                      ← release binary
   ├── libexec/
   │   └── ollama                          ← ollama binary for target arch (bundled, no Docker)
   └── share/
       └── overlay/                        ← HUD renderer (Electron app — internal name only)
           ├── dist/                       ← esbuild output
           ├── src/                        ← static assets
           └── node_modules/electron/dist/ ← bundled Electron runtime (~200 MB)
   ```
   Note: the directory is named `overlay/` in the dist package, not `lenzu_server/`, so the user-facing concept is "the overlay" not "the server". Internal code and developer docs may still use `lenzu_server` as the project name.
6. Create `dist/lenzu-<version>-linux-x86_64.tar.gz` (and `aarch64` variant)

The package is self-contained: no Docker, no Node, no pnpm, no system package manager needed to install.

Flags:

- `--skip-setup` — skip `setup.sh` (CI fast-path, toolchain already present)
- `--skip-package` — build only, no tarball (for local test runs)

---

#### `scripts/install.sh` — zero-knowledge end-user installer

**Design principle:** the user should not need to know what Docker, ollama, Node, cargo, or pnpm are. If they would have to search the internet to understand an error message, the script has failed. Model this on `brew install` and `snap install`: everything is managed silently, entirely in userspace, no `sudo` required.

**Audience:** anyone who can open a terminal and run a single command. Technical pre-knowledge: none.

---

**What install.sh must never do:**

- Ask the user about any software: Docker, ollama, Node, Electron, or anything else
- Require or request `sudo` (one exception: if a system library is missing, it prints the exact command for the user to run — see step 3 below)
- Detect or reuse any existing software the user may already have installed; lenzu's install is entirely self-contained
- Expose flags or options whose names require technical knowledge (`--skip-electron`, `--use-docker`, etc.) — the only acceptable flags are human-language options like `--prefix` or `--no-desktop-shortcut`
- Print error messages containing binary names, paths, or jargon that require interpretation
- Leave the user at a prompt asking “which option do you want?”
- Emit more than one screen of output for a successful install

---

**Install layout — fully userspace, fully scoped to lenzu:**

```
~/.local/
├── bin/
│   └── lenzu                           ← launcher (shell wrapper — the only file the user ever touches)
└── share/
    └── lenzu/
        ├── bin/
        │   └── lenzu-core              ← main application binary
        ├── libexec/
        │   └── ollama                  ← bundled AI runtime (private to lenzu)
        ├── ollama-models/              ← AI model storage (private to lenzu)
        ├── overlay/                    ← HUD renderer (private to lenzu; user never interacts with this)
        └── config/
            └── lenzu_config.json       ← user-editable settings
```

Everything under `~/.local/share/lenzu/` is implementation detail. The user sees one command: `lenzu`. All sub-processes (AI runtime, overlay renderer) are managed by lenzu itself — invisible to the user.

The AI runtime binary and model storage are private to lenzu. Lenzu does not share, detect, or conflict with anything the user may already have installed.

**Steps:**

1. Detect arch (`uname -m`); abort with a human message if unsupported (e.g. `arm32`)
2. Locate the release package:
   - If the script is extracted from a tarball, the package is in the same directory
   - Otherwise, download `lenzu-<latest>-linux-<arch>.tar.gz` from the GitHub releases API (no auth required for public releases) with a visible progress bar
3. Check for required system libraries (`libgtk-3.so`, `libcairo.so`, `libpango-1.0.so`):
   - These are provided by the user's desktop environment — any GNOME, XFCE, or KDE system will have them
   - If any are missing: print one friendly sentence (“Lenzu needs one system library. Please run the command below, then re-run this installer:”) followed by the exact `apt install` command. Exit cleanly. Do not try to install them automatically (that requires `sudo`).
4. Extract the package to `~/.local/share/lenzu/`
5. Write `~/.local/bin/lenzu` as a thin shell wrapper:
   ```sh
   #!/bin/sh
   export LENZU_SERVER_PATH=”$HOME/.local/share/lenzu/lenzu_server”
   export OLLAMA_MODELS=”$HOME/.local/share/lenzu/ollama-models”
   export OLLAMA_HOST=”127.0.0.1:11435” # private port, avoids colliding with user's system ollama
   exec “$HOME/.local/share/lenzu/bin/lenzu-core” “$@”
   ```
   (The wrapper is how HUD path resolution and private ollama port are injected — no compile-time baking needed.)
6. First-run model download (one time):
   - Start the private ollama (`libexec/ollama serve`) in the background
   - Pull `gemma4:e2b` into `ollama-models/` with a human progress message (“Downloading AI model — this is ~2 GB and happens only once…”)
   - Stop private ollama
7. Add `~/.local/bin` to PATH if missing: append to `~/.bashrc` and `~/.zshrc` silently; print one note at the end
8. Optionally install `~/.local/share/applications/lenzu.desktop`
9. Print a single success line: `Lenzu is ready. Type 'lenzu' to start.`

**The lenzu wrapper is also responsible at runtime for:**

- Starting the private ollama before lenzu and stopping it on exit (mirrors what `run.sh` does for dev, but using the private binary and port)
- This means `lenzu-core` can always assume `OLLAMA_HOST` is running — no Docker, no system-wide service

---

#### Ollama port isolation

`install.sh` uses port `11435` (not `11434`) for lenzu's private ollama. This avoids:

- Conflicts with a developer's own system ollama on `11434`
- Any interaction with `run.sh`'s Docker container
- The user needing to know why lenzu is “using” a port they already have occupied

`AppConfig` must support `llm_api_endpoint` pointing to `http://127.0.0.1:11435/v1/chat/completions`. The wrapper script sets `OLLAMA_HOST` and the default config is written to point at port `11435` during install.

---

#### Open design decisions (resolve before implementing)

| Decision                       | Options                                                                                                                           | Status                                                        |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| HUD path resolution            | Thin shell wrapper at `~/.local/bin/lenzu` sets `LENZU_SERVER_PATH`; `lenzu-core` reads it at runtime                             | **Decided** — wrapper approach; no compile-time path baking   |
| Electron bundling size         | (A) ship `node_modules/electron/dist/` as-is (~200 MB); (B) `electron-builder` AppImage (~120 MB)                                 | Option A first; Option B when Flatpak packaging begins        |
| System library check           | Check `ldconfig -p` for GTK3/Cairo/Pango; if missing, print exact `sudo apt install libgtk-3-0 libcairo2 libpango-1.0-0` and exit | Debian-family only (`apt`); Fedora (`dnf`) is a future target |
| `run.sh` → `build.sh` refactor | keep `cargo build` inline in `run.sh` (fast dev loop) vs. call `build.sh --debug`                                                 | Keep inline; revisit when `build.sh` lands                    |
| Model download progress        | `ollama pull` prints its own progress; wrap with a friendly preamble explaining the size                                          | Use ollama CLI directly, prepend message                      |

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
- [ ] **M10**: `scripts/prereqs.sh` + `scripts/build.sh` + `scripts/install.sh` — release binary packaged with bundled Electron, `~/.local` install, `PATH` / `.desktop`, runtime HUD path resolution (see **Installation — Script Architecture** above)

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
3. **Usability**: Works out-of-the-box on Debian-family systems (Ubuntu 22.04+, Debian 12+, Mint, Pop!\_OS, etc.); Fedora-family (`dnf`) is a planned future target
4. **Accessibility**: Screen reader compatible (Orca)
5. **Privacy**: No network traffic unless user enables online OCR

## References

- GTK3 (gtk-rs): https://gtk-rs.org/gtk3-rs/stable/latest/
- Tesseract Training: https://tesseract-ocr.github.io/tessdoc/Training-Tesseract.html
- Wayland Portals: https://flatpak.org/xdg-desktop-portal/
- Manga109: http://www.manga109.org/
- Kakasi: http://kakasi.namazu.org/

---

**Last Updated**: 2026-04-05
**Maintainer**: Hideki AI
**Status**: Active development — `feature/ocr-local-remote` branch, Phase 4 M7b-1 next

Addendum:
Notes added that should be later updated to other docs (including this doc):

- Q: How do we generate packages of both lenzu_client and lenzu_server, let alone, other dependency such as ollama, docker, etc?
