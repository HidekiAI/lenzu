# Technical Design Document

> **Note (2026-03-22):** Sections 1–11 below describe the original design goals and future architecture. The section immediately below describes the **current, working implementation** on the `RemoteOCR` branch.

---

## 0. Current Implementation (RemoteOCR branch)

### Stack

| Layer | Technology |
|---|---|
| Lens window | GTK3 + Cairo + Pango (floating, RGBA, always-on-top) |
| Screen capture | `x11rb` — X11 root window `GetImage` (ZPixmap) |
| OCR / Translation | OpenRouter API → `google/gemini-2.0-flash-001` (multimodal JSON) |
| HTTP client | `reqwest` (blocking) |
| Overlay HUD | `lenzu_server` — Electron transparent window |
| IPC | UDP loopback (default port 7331; overridable via `overlay_udp_port` / `LENZU_OVERLAY_UDP_PORT`), JSON messages |
| Config | `lenzu_config.json` — `serde_json`, `isolang` for language codes |

### Process architecture

```
lenzu (GTK3 client)
  │
  │  Shift+Click → X11 capture → DBNet detect → refine_boxes → local OCR
  │    ├─ incremental: each box → manga-ocr-rs → MeCab furigana → HUD preview
  │    ├─ if all confident → done (no LLM)
  │    └─ if low confidence → LLM fallback chain → final HUD update
  │
  │  Ctrl+Shift+Click → fullscreen capture → DBNet detect → closest box to cursor
  │    → crop → local OCR or LLM chain → HUD
  │
  └──UDP JSON──► lenzu_server (Electron)
                  transparent BrowserWindow (src/main.js)
                  UDP listener → ipc to renderer
                  renderer.js renders caption lines
```

`lenzu` auto-spawns `lenzu_server` as a child process (`std::process::Child`), sets `LENZU_OVERLAY_UDP_PORT` from `overlay_udp_port`, and kills it on exit (ESC key and window close). Phase 2 lifecycle; Phase 3 switched the HUD to Electron.

### Installation and deployment

| Mode | Mechanism |
|---|---|
| **Developer checkout** | `scripts/setup.sh` then `scripts/run.sh` or `cargo run -p lenzu`. No system install; HUD spawned via `npm run start` inside `lenzu_server` next to the `lenzu` crate (path derived from `CARGO_MANIFEST_DIR` at compile time). |
| **`scripts/build.sh`** | **Not implemented.** Planned CI/release builder: calls `setup.sh`, `cargo build --release`, `pnpm run build`, bundles Electron, produces `dist/lenzu-<version>.tar.gz`. See **Installation — Script Architecture** in `docs/planning.md`. |
| **`scripts/prereqs.sh`** | **Not implemented.** Planned shared downloader (no compiler): ollama native/Docker install, model pull, Electron binary check. Sourced by `setup.sh`, `install.sh`, and `run.sh`. |
| **`scripts/install.sh`** | **Not implemented.** Planned end-user installer: downloads pre-built tarball (or uses `dist/` from `scripts/build.sh`), calls `scripts/prereqs.sh` (ollama + model), extracts to `~/.local`, resolves HUD path via env var or thin wrapper. See **Installation — Script Architecture** in `docs/planning.md`. |
| **Distribution** | Long-term: Flatpak / PPA / AppImage (see `planning.md` **Packaging**). |

### `TranslationResult` (client.rs)

```rust
pub struct TranslationResult {
    pub original: String,           // source text as seen in image
    pub furigana: Option<String>,   // 漢字[かんじ] format
    pub romaji:   Option<String>,   // romanized reading
    pub english:  Option<String>,   // translated text
    pub top_xy:   Option<String>,   // upper-left bounding box (from LLM)
    pub bot_xy:   Option<String>,   // lower-right bounding box (from LLM)
    pub debug_info: Option<String>, // LLM diagnostic notes
}
```

### Prompt design

The base prompt is hardcoded in `config.rs::TRANSLATE_PROMPT` and is language-agnostic. It uses `{src}` / `{dest}` placeholders resolved at runtime via `AppConfig::resolved_prompt()`. Language-specific additions (e.g. furigana/romaji instructions for Japanese) go in the configurable `translate_extra_prompt` field.

### Overlay render modes (`OverlayRenderMode`)

| Value | HUD shows |
|---|---|
| `original` | Source text only |
| `english` | Translation only (fallback: original) |
| `furigana` | Furigana reading (fallback: original) |
| `romaji` | Romanized reading (fallback: original) |
| `all` | All text fields joined with ` | ` |
| `debug` | All fields + bounding boxes + debug_info |

### Workspace layout

```
lenzu/                          ← Cargo workspace root
├── Cargo.toml
├── lenzu/                      ← lenzu_client binary ("lenzu")
│   ├── src/
│   │   ├── main.rs             ← GTK event loop, capture, shift/ctrl+shift click handlers
│   │   ├── client.rs           ← OcrClient, LLM fallback chain, response parsing
│   │   ├── config.rs           ← AppConfig, lenzu_config.json deserialization
│   │   ├── utils.rs            ← image encoding, debug image saving (lens/prewire/fullscreen)
│   │   ├── furigana.rs         ← MeCab morphological analysis, furigana/romaji annotation
│   │   └── ocr/
│   │       ├── mod.rs
│   │       ├── local_ocr.rs    ← LocalOcrEngine, refine_boxes(), confidence gate, pipelines
│   │       ├── text_detection.rs  ← re-exports jp_detect (TextDetector, TextBoundingBox, etc.)
│   │       └── text_cropper.rs ← TextCropper, proportional bbox padding, crop extraction
│   ├── tests/integration_test.rs
│   └── lenzu_config.json             ← runtime config (not committed)
├── lenzu_server/               ← Electron overlay HUD
│   ├── package.json
│   └── src/                      ← main.js, preload.js, index.html, renderer.js, config.json
└── prototypes/                 ← standalone test binaries for model evaluation
    ├── manga-ocr-test/
    ├── dbnet-test/
    └── dbnet-ocr-pipeline/
```

---

## 1. Why Offline OCR?

This section explains the rationale for implementing offline OCR in the application, emphasizing performance, privacy, and reliability for Japanese text recognition, particularly for manga and graphic novels. Offline OCR eliminates network latency, ensures user privacy by not transmitting images to external services, and provides consistent performance regardless of internet connectivity.

With the shift to Linux as the primary platform, offline OCR becomes even more critical due to the fragmented nature of Linux desktop environments and the importance of user data sovereignty in open-source ecosystems.

## 2. Libraries

This section outlines the key libraries used in the project:

### Core Dependencies

- **MeCab** (`mecab` + `mecab-ipadic-utf8` / `mecab-naist-jdic`): Context-aware morphological analysis for furigana annotation and romaji generation. Replaces kakasi — MeCab understands word boundaries from neighboring characters, correctly segmenting compound words and conjugated verbs. Per-morpheme katakana readings are converted to hiragana (furigana) and romaji (via pure-Rust Hepburn table) without any external dependency. See `lenzu/src/furigana.rs`.
- ~~**kakasi**~~: Previously used for kanji→hiragana conversion; replaced by MeCab (context-aware, better word boundaries, eliminates CLI dependency).
- **tesseract**: Tesseract OCR engine for Linux and fallback scenarios; requires traineddata including `jpn_vert.traineddata`.
- **windows-rs**: Enables integration with Windows Media OCR via `Media_Ocr` and `Globalization` features (Windows-only).
- **leptonica**: Underlying library for Tesseract; required for building on Windows.

### Rust Implementation Strategy (2026 Maintenance Verified)

1. **Display Capture**:
   - [`x11rb`](https://crates.io/crates/x11rb) (MIT) - Modern X11 protocol implementation
     - Last updated: 2023-11-15 (v0.11.0)
     - GitHub: [psychon/x11rb](https://github.com/psychon/x11rb) (last commit 2024-02-19)
     - Status: **Actively maintained** (20 open issues, CI passing)
   - [`gtk` 0.18 (gtk-rs)](https://crates.io/crates/gtk) (LGPL) - GTK3 bindings — **GTK3 only, GTK4 abandoned**
     - Multi-monitor geometry: `gdk::Display::default()` + `gdk::Monitor` (GTK3 API)
     - Status: Stable, actively maintained
   - [`cairo-rs`](https://crates.io/crates/cairo-rs) (LGPL) - Surface rendering
     - Last updated: 2024-01-28 (v0.18.3)
     - GitHub: [gtk-rs/cairo-rs](https://github.com/gtk-rs/cairo-rs) (last commit 2024-03-10)
     - Status: **Very active** (10 open issues, frequent releases)

2. **Extension Crates Needed**:
   - `x11rb-xfixes` - Will extend x11rb for cursor capture
   - `x11rb-xshape` - Will extend x11rb for window shapes

3. **Existing Alternatives**:
   - [`x11`](https://crates.io/crates/x11) (legacy bindings, last updated 2021)
   - [`xcb`](https://crates.io/crates/xcb) (XCB bindings, actively maintained)

4. **Window Management**:
   - GTK3 (`gtk-rs` 0.18) — permanent choice. GTK4 was evaluated and abandoned (graphene/gobject dep complexity, API churn, prototype build failures).

5. **Optional Services**:
   - Google Cloud Vision/Azure Computer Vision via OAuth2
   - Online translation APIs

### License Strategy
- Core implementation remains MIT-licensed
- GPL/LGPL components isolated in separate modules
- Clear documentation of license requirements

## 3. Text Detection and Recognition Pipeline

This section outlines the modern approach to Japanese text extraction in manga, which separates text detection from recognition for improved accuracy and performance. This separation also enables significant token cost reductions when using cloud-based AI services by only sending detected text regions rather than full images.

### Token Cost Optimization Strategy

**Phase I (Current):**

- Processes full captured images
- High token costs (1 token ≈ 4 chars of base64 encoded image)
- Example: 1080p screenshot ≈ 300KB → ~75,000 tokens

**Phase II (Optimized):**

1. Local text detection (EAST/CRAFT)
2. Extract only text-containing regions
3. Send regions to cloud OCR
4. Estimated 80-90% token reduction

**Hybrid Architecture Benefits:**

- Privacy: Most image processing stays local
- Cost: Only pay for text region analysis
- Performance: Parallel local/cloud processing

**Technical Challenges:**

1. **Detection Accuracy vs Performance Tradeoff**
   - Lightweight models (EAST) may miss small/dense text
   - Heavy models (CRAFT) require GPU acceleration
   - Minimum viable accuracy threshold: 85% recall

2. **Platform Compatibility**
   - Windows: DirectML acceleration for ONNX models
   - Linux: Vulkan/Metal fallbacks
   - CPU-only mode requirements

3. **Text Region Processing**
   - Merging adjacent regions without losing context
   - Handling overlapping text bubbles
   - Direction detection (vertical vs horizontal)

4. **Fallback Mechanisms**
   - Confidence scoring for detected regions (jp_detect: per-box detection confidence 0–100%; manga-ocr-rs: per-result OCR confidence 0–100%)
   - Confidence gate: both scores must be > 70% for local results to be accepted
   - **Weak box drop**: boxes below the detection confidence gate are silently dropped — they no longer drag confident boxes into the LLM fallback. Only when zero boxes pass the gate does the pipeline fall through.
   - Progressive enhancement:
     1. Try local detection (jp_detect DBNet) + bbox refinement + local OCR (manga-ocr-rs) — if confident boxes exist with OCR >= 71%:
        - **Phase 1 — incremental preview**: each box is OCR'd one at a time; after each, MeCab furigana is applied and the accumulated results are sent to the HUD as a preview. Text grows on screen as boxes complete (~0.8–1.5 s per box). This prevents the HUD from appearing hung during multi-box processing. Even low-confidence garbage text is shown as a "system is working" signal.
        - **Phase 2 — MeCab annotation** (instant, ~5 ms): morphological analysis produces furigana (`最初[さいしょ]`) and romaji. No LLM, no network — pure dictionary lookup with context-aware word boundaries. HUD updates in-place.
        - **Phase 3 — LLM enrichment** (if `enrichment_enabled` and not `furigana_only`): send raw text (NOT image) to local Ollama for english translation. Text-to-text call — no vision model needed. If Ollama is down or times out, MeCab-annotated text is returned as-is (graceful degradation).
        - Done — no image-based LLM call needed
     2. If low confidence → local LLM (Ollama) with image
     3. If local LLM fails → free remote tier
     4. Final fallback → paid remote (Gemini 2.0 Flash)

5. **Performance Benchmarks**
   - Target: <500ms detection time on 1080p image (Core i5)
   - Memory: <500MB RAM footprint
   - Model size: <50MB for edge deployment

### Text Detection (First Stage)

- **EAST (Efficient and Accurate Scene Text Detector)**: Lightweight CNN for arbitrary-shaped text detection (speech bubbles, curved text)
- **CRAFT (Character Region Awareness for Text Detection)**: Detects character-level regions for precise text boundary detection
- **DBNet (Differentiable Binarization)**: State-of-the-art text detection with adaptive binarization

### Text Recognition (Second Stage)

- **PaddleOCR (ONNX)**: Modern OCR engine optimized for Japanese (both horizontal and vertical); highly recommended for its accuracy in "wild" scenarios like manga. Can run efficiently on CPU via ONNX Runtime.
- **Manga-OCR**: A specialized Vision Transformer-based model specifically for manga. Considered the "gold standard" for accuracy, though it requires more resources (PyTorch/ONNX).
- **Tesseract (Fallback)**: Open-source OCR engine with significant limitations in vertical Japanese text and layout analysis. It will be used ONLY as a lightweight recognition-only fallback for clean text regions.
- **Windows Media OCR (Legacy)**: High accuracy for Japanese text, but Windows-only. Deprecated in favor of cross-platform ONNX-based engines.

### Bounding Box Refinement Pipeline (2026-04-16)

DBNet can produce problematic detections: overlapping boxes (speech bubble + text inside it as two detections), false positives on high-contrast panel edges/art, or stacked speech bubbles merged into a single tall box. The `refine_boxes()` function in `local_ocr.rs` addresses all three problems:

```
Raw DBNet boxes
  │
  ├─ Step 1: Cluster overlapping boxes (IoU > 15%) via union-find
  │
  ├─ Step 2a: For each cluster (≥2 boxes):
  │    ├─ Compute union bbox of the cluster
  │    ├─ Crop union region from the original image
  │    ├─ Re-run DBNet on the crop → remap coordinates back
  │    └─ If re-detect finds boxes → use those; else keep smallest original
  │
  ├─ Step 2b: For each singleton:
  │    ├─ Check aspect ratio (longest/shortest edge)
  │    ├─ If > 3.5 → suspicious (stacked bubbles merged)
  │    │    ├─ Re-run DBNet on that region → may split into multiple
  │    │    └─ If no split → keep as-is
  │    └─ If ≤ 3.5 → pass through unchanged
  │
  └─ Step 3: Sort refined boxes right-to-left, top-to-bottom
     (Japanese manga reading order: descending X midpoint, then ascending Y)
```

**Examples of problems solved:**
- 4 raw boxes on a single text bubble ("敵探知"): 2 overlapping (bubble 99% + text 87%) + 2 false positives (35%, 50%). After refinement: 1 box on the text. Confidence gate drops the false positives, re-detection on the cluster yields the tight text box.
- Two vertically stacked bubbles merged into one tall box (aspect 4:1): re-detection splits them into two separate boxes, each with its own text.

### Early Decoder Bailout (manga-ocr-rs 0.1.3, 2026-04-16)

The manga-ocr-rs beam search decoder can "run away" on ambiguous or garbage input, producing 40+ hallucinated tokens over 16–23 seconds before hitting `max_length`. This is waste — the output is garbage and the confidence will be low.

**Solution**: After 16 tokens, check the best active beam's geometric mean per-token probability. If below 30%, abort immediately.

```
Constants:
  EARLY_BAILOUT_MIN_TOKENS = 16
  EARLY_BAILOUT_CONFIDENCE = 0.30

Check (after each decoder step, once num_generated ≥ 16):
  running_conf = exp(score / num_generated)
  if running_conf < 0.30 → break
```

**Observed improvement**: box OCR time dropped from ~23 s (full hallucination) to ~1.8 s (bailed at 38 tokens, 28.2% confidence). The `truncated` flag is set, signaling the LLM fallback chain that local OCR was unreliable.

### Pre-scaling Large Crops (2026-04-15)

manga-ocr-rs squish-resizes all input to 224×224 using bilinear interpolation internally. When a large crop (e.g. 600×400) is resized directly, fine kanji strokes are lost. The `OCR_MAX_EDGE` constant (448 px, 2× the model input) gates a Lanczos3 pre-downscale in `recognize_crop()` — crops larger than 448px on their longest edge are proportionally shrunk with high-quality resampling before the model's internal resize. This preserves stroke detail that bilinear would blur.

### Low-Confidence Text Truncation (2026-04-16)

When OCR confidence is below the gate and the text is long (hallucination), the result is truncated to `low_conf_max_chars` (default: 64, configurable via `lenzu_config.json`). This prevents runaway garbage from flooding the HUD while still showing enough text to be useful as a "system is working" indicator.

### Resilient LLM Response Parsing (2026-04-16)

LLM responses returning JSON arrays are now deserialized element-by-element. If one element in the array is malformed (missing fields, wrong types), it is logged and skipped — the valid elements are kept. Previously, a single malformed entry discarded the entire array. This applies to both direct arrays and wrapper-key paths (`results`, `items`, `data`, etc.) in `client.rs::normalize_results()`.

### Pipeline Benefits

1. **Performance**: Detection reduces processing area by 80-90%, making recognition much faster
2. **Accuracy**: Detection models trained on manga data handle speech bubbles and onomatopoeia better
3. **Flexibility**: Different recognition engines can be swapped based on platform/preference
4. **Scalability**: Detection can run on CPU while recognition can leverage GPU if available

## 4. Image Capture Strategy

This section describes the phased approach to screen capture.

### X11 Implementation Insights (from xfce4-screenshooter)

Key findings from analyzing xfce4-screenshooter's X11 implementation:

- **Core Capture Components** (`src/screenshooter-capture-x11.c`):
  - Line 142-210: `screenshooter_capture_screenshot_x11()` handles unified coordinate space (0,0 at primary display top-left)
  - Line 89-140: `screenshooter_get_screen_geometry()` gets combined display dimensions
  - Line 215-230: Negative coordinates clamped via `CLAMP()` macro to prevent off-screen captures

- **Multi-Monitor Handling** (`src/screenshooter-utils.c`):
  - Line 45-78: Scaling handled via `gdk_window_get_scale_factor()`
  - Line 112-125: Window positions obtained through `gdk_window_get_origin()`
  - All coordinates in display pixels (accounts for HiDPI)

- **Key Technologies**:
  - X11/Xlib primitives (Line 50-85 in `capture-x11.c`)
  - XFixes extension for cursor capture (Line 180-195 with GDK fallback)
  - XShape for window border management (Line 200-215)
  - GDK/GTK for image manipulation (Line 230-250)
  - Cairo for surface rendering (Line 260-275)

### Phase I (Baseline Implementation)

- **Whole-screen Capture**: Capture entire desktop across all monitors simultaneously using `x11rb`.
- **Multi-monitor Support**:
  - Uses `gdk::Display::default()` + `gdk::Monitor` (GTK3) to determine individual monitor boundaries.
  - X11 implementation captures from the Root Window using `GetImage` (ZPixmap format), handling unified coordinate spaces (negative offsets for monitors relative to primary).
- **Pixel Conversion**:
  - Implements BGRA to RGBA conversion for compatibility with the `image` crate.
  - Ensures 32-bit alignment for high-performance memory operations.
- **Performance Target**: <100ms capture latency for 1080p regions on X11.
- **Reference Implementation**: Inspired by `xfce4-screenshooter` focus management and X11 capture logic, ported to safe Rust via `x11rb`.

### Phase II (Enhanced Implementation)

- **Smart Capture**: On-demand region capture with mouse hover
- **Compositor Integration**: Use Wayland/X11 protocol to capture only visible windows
- **Performance Target**:500ms capture latency for 4K regions

## 5. System Architecture

![Hybrid Architecture](assets/architecture.png)

1. **Capture Layer**:
   - Phase I: Fullscreen capture via GTK3 window + `x11rb` root `GetImage`
   - Phase II: Region capture with compositor integration

2. **Processing Layer**:
   - Text detection (EAST/CRAFT)
   - Image preprocessing
   - OCR engine selection

3. **Output Layer**:
   - Text rendering with furigana
   - History tracking
   - Clipboard integration

## 6. User Interface Design

### Lens Interaction Mode

1. **Manual Region Selection**:
   - User moves transparent lens window over text regions
   - On click, captures only lens area (+5px padding)
   - Reduces processing area by ~95% vs full-screen capture
   - Enables precise text targeting in dense layouts

2. **Hotkey Behaviors**:
   - **Right-click**: Standard furigana/romaji conversion
   - **Shift+Right-click**: Direct translation to configured language (default: English)
   - **Ctrl+Right-Click**: Freeze current OCR result
   - **Mouse Wheel** (or PgUp|PgDn): Adjust lens magnification (1x-4x)
   - **Tab**: Scan entire desktop (upper left to lower right of currently focosed desktop) for Japanese text and save to local buffer (/dev/shm/) the discovered text-rectangles ; each Tab-toggle will erase temp files but it is useful for diagnostics/debugging

3. **Translation Pipeline**:
   - OCR → Kakasi conversion → Dictionary lookup → Translation API
   - Caches recent translations locally
   - Supports offline dictionary fallback

4. **Performance Benefits**:
   - Smaller image regions → faster processing
   - Reduced token costs for cloud services
   - Lower memory/CPU usage on edge devices

### Implementation Requirements

1. **Lens Window**:
   - Always-on-top transparent overlay
   - Configurable size (200x200px to 800x800px)
   - Visual feedback on capture (border highlight)
   - Focus-based activation (similar to xfce4-screenshooter):
     - Requires application focus to remain active
     - Disappears when losing focus to prevent interference
     - Floating toolbar maintains lens control state
   - Source Inspiration:
     - Xfce4-screenshooter's focus management (GPL-licensed) - https://gitlab.xfce.org/apps/xfce4-screenshooter
     - GTK3 adaptation of their focus tracking approach
     - Will analyze their implementation for reference

2. **Translation Service**:
   - Default: Google Translate API
   - Fallback: Offline jmdict dictionary
   - User-configurable target language

3. **Accessibility**:
   - Keyboard navigation for lens positioning
   - Screen reader support for translations
   - High-contrast mode for visibility

## 6. Technical Decisions

### Capture Approach

- **Decision**: Prioritize full-screen capture for Phase I
- **Rationale**:
  - Reduces complexity in early development
  - Leverages GTK3/X11 capture capabilities (`x11rb` root `GetImage`)
  - Provides maximum coverage for manga users
  - Allows progressive refinement through Phase II

### Multi-monitor Handling

- **Strategy**:
  - Capture all monitors simultaneously using GTK3's `GdkDisplay` API
  - Handle scaling factors per-monitor
  - Provide unified coordinate system across displays

### Performance Optimization

- **Technical**:
  - Use `imageproc` for efficient region-of-interest processing
  - Implement capture rate limiting (12fps default, configurable)
  - Prioritize vertical text detection for manga content

## 7. Sample Outputs and Results

Examples of OCR results from different methods including text recognition accuracy and processing times:

- **Debug Text Representation**: Detailed breakdown of recognized text with line and word coordinates.
- **Visual Results**: Demo GIF showing real-time OCR processing and furigana conversion.
- **Comparative Results**: Side-by-side comparisons of different OCR engines on the same test images.

## 8. Other Considerations

Discussion of various considerations for the design:

- **Offline-First Design**: Rationale for prioritizing offline functionality for performance and privacy.
- **Privacy Concerns**: Implications of using online OCR services and data handling.
- **Multi-monitor Support**: Plans for better support across multiple displays.
- **Image Preprocessing**: Potential integration of OpenCV for image filtering and enhancement.
- **Future Training**: Plans to train Tesseract with manga109s dataset for improved accuracy.
- **Translation Integration**: Potential integration with dictionary services for translation.

## 8. Hardware and Privacy Considerations

- **CPU-First Design**: Full pipeline (detection + OCR) works entirely on CPU, ensuring compatibility with any hardware
- **GPU Acceleration (Optional)**: Optional integration of YOLOv8-tiny models for detection acceleration on systems with compatible GPUs
- **Privacy Guarantee**: Images are never transmitted off-device; only text results may be processed online if user opts for premium services
- **Performance Tiers**: Users can choose between CPU-only mode (slower, fully offline) or GPU-accelerated mode (faster, optional online enhancement)
- **Model Selection**: YOLOv8-tiny for detection on consumer hardware; ONNX Runtime integration for efficient execution
- **Hardware Adaptability**: Pipeline scales gracefully across low-end and high-end hardware

## 9. TODO

A list of pending tasks and future enhancements:

- Train manga-109s dataset for Tesseract to improve Linux OCR accuracy.
- Implement better multi-monitor desktop support.
- Add online OCR fallback option via OAuth2 (Google Cloud Vision).
- Develop image preprocessing pipeline (grayscale, denoise, contrast adjustment).
- Integrate dictionary lookup for enhanced translation capabilities.
- ~~Replace the fake kakasi crate with the official version.~~ Resolved: kakasi replaced entirely by MeCab morphological analysis (2026-04-12).

## 10. Post Mortem

Reflections on the development process including challenges faced and lessons learned:

- Debugging Windows Media OCR integration revealed issues with stream management.
- Tesseract training data mismatch required careful consideration of font compatibility.
- Microsoft documentation quality issues required extensive experimentation.
- Importance of offline functionality for user trust and performance.

## 11. Build/Compile Notes

Notes on building and compiling the project including dependencies and platform-specific instructions:

- **Target platform**: Debian-family Linux (`apt`/`dpkg`) is the only supported platform — Ubuntu 22.04+, Debian 12+, Mint, Pop!_OS, and derivatives. Fedora-family (`dnf`) is a planned future target. Windows support is historical only (MinGW64 + `Media_Ocr`; no longer maintained).
- **Linux (Debian)**: Install `mecab`, `mecab-ipadic-utf8`, `mecab-naist-jdic` via apt; use cargo build commands. (kakasi is no longer required — MeCab handles all furigana/romaji annotation.)
- **Linux install script**: See `docs/planning.md` — Installation section. `scripts/install.sh` and `scripts/build.sh` are planned but not yet implemented; architecture is documented.
- **Debugging**: Conditional compilation writes `recognized_image.png` for offline inspection; use debug builds for testing.

## 12. Comparative Analysis

Performance benchmarks and accuracy assessments across OCR engines:

| Engine | Latency | Confidence Scoring | Accuracy | Notes |
|---|---|---|---|---|
| jp_detect + manga-ocr-rs (local, no LLM) | ~1–5 s (CPU) | Det 0–100%, OCR 0–100%; >= 71% both = pass | High for clean text regions | Fully offline; preferred path |
| Windows Media OCR | ~2 s | None | High for manga text | Windows-only; legacy |
| Tesseract (Linux, PSM 5) | ~5 s | None | Medium with preprocessing | Requires traineddata files |
| Tesseract (Linux, default) | ~34 s | None | Low without tuning | Default settings unusable for manga |
| Manga-OCR (Python) | ~32 s | None | High | Complex installation; replaced by manga-ocr-rs |
| gemma4:e2b (Ollama) | ~15–110 s | N/A (LLM fallback) | Inconsistent block detection | Misses vertical CJK |
| glm-ocr (Ollama) | ~10–20 s | N/A (LLM fallback) | Accurate text, poor segmentation | Lumps text into one block |
| Gemini 2.0 Flash (remote) | ~3–5 s | N/A (LLM fallback) | Best overall | Requires API key; images leave device |

The confidence-gated local pipeline (jp_detect + manga-ocr-rs) eliminates the need for image-based LLM calls when both detection and OCR confidence scores pass the 71% gate. After local OCR succeeds, a multi-phase progressive rendering pipeline updates the HUD:

**Multi-bbox shift-click path (incremental rendering, 2026-04-16):**

1. **Bbox refinement** (~50 ms): `refine_boxes()` clusters overlapping detections, re-runs DBNet on union regions, checks singleton aspect ratios, drops weak boxes.
2. **Sort** — right-to-left, top-to-bottom (Japanese manga reading order).
3. **Per-box loop**: for each box:
   - **OCR** (~0.8–1.5 s): manga-ocr-rs with early bailout at 16 tokens if confidence < 30%.
   - **MeCab furigana** (~5 ms): annotate OCR result with furigana/romaji.
   - **HUD preview**: send accumulated results to HUD (text grows on screen). Even low-confidence garbage is shown as a "system is working" signal while the LLM fallback chain runs.
4. **Phase 3 — LLM enrichment** (if `enrichment_enabled` and not `furigana_only`, ~15–30 s): text-only Ollama call for english translation. Skipped in `furigana_only` mode.

**Single-box path (fullscreen Ctrl+Shift+Click):**

1. **Phase 1 — raw text** (instant): OCR result displayed immediately.
2. **Phase 2 — MeCab furigana + romaji** (~5 ms): morphological analysis annotates kanji with hiragana readings (`最初[さいしょ]`) and generates romaji via pure-Rust Hepburn conversion. No LLM, no network — deterministic dictionary lookup with context-aware word boundaries.
3. **Phase 3 — LLM enrichment** (if `enrichment_enabled` and not `furigana_only`, ~15–30 s): text-only Ollama call for english translation. If Ollama is down or times out, Phase 2 results are shown as-is.

The lens stays modal until the final phase completes, preventing request stacking. Low-confidence results fall through to the image-based LLM chain automatically.

**Observability**: All LLM backends (local and remote) now report mean token probability (via logprobs) and `finish_reason` in stderr logs. Paid remote backends additionally log per-request and session-cumulative token counts (`prompt_tokens`, `completion_tokens`). The HUD color changes from configured → orange → red as session paid token usage crosses configurable thresholds (`token_warning_threshold`, `token_critical_threshold`).

**Local OCR logging** (2026-04-16): The local pipeline logs total boxes found, processes each with a `[box N]` prefix (1-indexed), shows full OCR text with character count, and explicitly logs truncation when applied (`truncated 142 → 64 chars`). Bbox refinement logs cluster merges, re-detection results, and aspect ratio checks.

**Debug files** (`/dev/shm/lenzu/`):

| File | Written by | Contents |
|---|---|---|
| `debug_lens.png` | `save_lens_debug()` | RGB lens capture with color-coded 2px bbox rectangles (6-color cycling palette). Written after DBNet detection + refinement. |
| `debug_lens.json` | `save_lens_debug()` | JSON with box count, coordinates, dimensions, and confidence for each bbox. |
| `debug_prewire.png` | `save_prewire_debug()` | Greyscale image of the exact crop sent to the LLM — reflects the actual OCR payload. Separate path from lens debug to prevent overwriting. |
| `fullscreen-debug.png` | `save_fullscreen_debug()` | Fullscreen capture with color-coded bbox rectangles (Ctrl+Shift+Click path). |
| `fullscreen-debug.json` | `save_fullscreen_debug()` | JSON with box count and coordinates for fullscreen scan. |
| `api_debug.txt` | `client.rs` | Raw LLM API request/response payloads for debugging. |

## 13. windows-rs Integration

Details on integrating Windows Media OCR via the `windows-rs` crate:

- **Setup**: Enable `Media_Ocr` and `Globalization` features in `Cargo.toml`.
- **Data Flow**: Uses `InMemoryRandomAccessStream` for passing image data to the OCR engine.
- **Debugging Challenges**: Issues with stream detachment and resource management required careful handling during development.

## 14. MeCab Morphological Analysis for Furigana

MeCab replaced kakasi as the furigana engine (2026-04-12). The improvement is fundamental: kakasi is a character-by-character converter that cannot distinguish word boundaries, while MeCab performs context-aware morphological analysis — it understands that `知らない天井だ` is `知ら|ない|天井|だ` (four morphemes), not a flat string.

### How It Works

MeCab is invoked as a subprocess (`mecab -Oyomi` for the annotation path is NOT used — we use the default output format for per-morpheme control). For each morpheme, MeCab returns:

```
surface\tPOS,sub1,sub2,sub3,conj_type,conj_form,base,reading,pronunciation
```

Field index 7 (0-based) is the katakana reading. The pipeline:

1. **Spawn MeCab** with the best available UTF-8 dictionary (naist-jdic preferred for person names, ipadic-utf8 fallback). Six standard paths are searched across `/usr/share`, `/usr/lib`, `/var/lib`.
2. **Parse each morpheme**: surface (as-is text) + katakana reading from field 7.
3. **Bracket kanji**: if the surface contains kanji (CJK Unified Ideographs U+4E00–U+9FFF), append `[hiragana]` — e.g. `天井[てんじょう]`. Kana-only surfaces pass through unchanged.
4. **Romaji** (optional): a pure-Rust Hepburn romanization table converts the katakana reading to ASCII. Handles digraphs (`キャ`→`kya`), gemination (`ッ`→doubled consonant), long vowels (`ー`→repeat), and loanword extensions (`ファ`→`fa`).

### Why Not an LLM?

MeCab furigana is deterministic, instant (~5 ms), and correct. LLM-based furigana (tested with qwen2.5:3b) produced garbage readings (`ふところ[もく]まとって`) that failed even the 71% confidence gate. Dictionary-based morphological analysis is the right tool for this job — it's not a generation task, it's a lookup.

### `furigana_only` Mode

When `furigana_only: true` (config) or `--furigana_only` (CLI flag), the entire LLM enrichment pipeline is skipped. MeCab furigana is the final result — no romaji, no translation, no network call. This makes the capture-to-HUD latency effectively the OCR time (~1.7 s) plus ~5 ms for MeCab. For users who read Japanese and just need kanji readings, this is the ideal mode.

### Implementation

All MeCab logic lives in `lenzu/src/furigana.rs` — Linux-only (`#[cfg(target_os = "linux")]`), no-op stub on other platforms. Key functions:

- `parse_mecab_output()` — pure function, fully unit-testable without MeCab binary
- `kata_to_hira()` — Unicode shift (katakana → hiragana, -0x60)
- `kata_to_romaji()` — pure-Rust Hepburn table, no external dependency
- `has_kanji()` — CJK range check
- `annotate()` — public API, annotates `TranslationResult` array in-place

### MeCab Overwrite Mode (2026-04-13)

When `--mecab_overwrite` is active (default: true), every LLM-produced furigana result is compared against MeCab's output. If they differ, MeCab's version replaces the LLM's. This catches LLM hallucinations in furigana annotations (e.g. LLM returning `ふところ[もく]まとって` vs MeCab's correct `懐[ふところ]`). Timing and MATCH/MISMATCH status are logged for each comparison.

### Companion Crates

All three companion crates are published on crates.io and used as dependencies:

| Crate | Version | Role |
|---|---|---|
| `jp_detect` | 0.2.3 | DBNet scene text detection (4.7 MB ONNX model). Orientation-aware merging (tategaki never merges with yokogaki), graduated padding scale table, confidence scores, contour polygons. |
| `manga-ocr-rs` | 0.1.3 | ViT-based manga OCR (140 MB ONNX models). Beam search (k=4), confidence scoring, early bailout on hallucination (16 tokens / 30% threshold), configurable max decode steps. |
| `mecab-furigana-rs` | 0.1.0 | MeCab morphological analysis. Furigana annotation (`漢字[かんじ]`), romaji (Hepburn), word segmentation with POS tags. Cross-platform (Linux/macOS), `MECAB_DICT_DIR` env var support. |
