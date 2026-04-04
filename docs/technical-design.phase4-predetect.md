# Phase 4 Technical Design: Local Pre-Detection + Dual-Backend OCR

> **Status**: Planning / Pre-implementation  
> **Created**: 2026-04-04  
> **Related**: `planning.md` M7 / M7b, `technical-design.md` §3, `lenzu/src/ocr/text_detection.rs`

---

## 1. Problem Statement

Every Shift+Click capture sends the full lens image (400×400 px default) to the Vision-AI endpoint (Gemini 2.0 Flash via OpenRouter). The LLM receives pixel data it doesn't need — backgrounds, UI chrome, whitespace — and must do spatial reasoning that a cheap local model can do faster and for free.

**Cost model (approximate)**

| Image size | PNG bytes (base64) | ~tokens charged |
|---|---|---|
| 400×400 (current lens) | ~60 KB | ~800–1,200 |
| 120×60 cropped bubble | ~4 KB | ~50–80 |
| Savings (1 bubble found) | | **~93% fewer tokens** |

With multiple text regions per capture the savings compound. At current Gemini Flash pricing (~$0.10/M input tokens) the absolute cost is low, but pre-detection also improves **accuracy** — sending a tight crop removes distracting background pixels that can confuse the model on small or dense text.

---

## 2. Proposed Pipeline Change

### Current (Phase I)

```
Shift+Click
  → capture_x11(lens_rect) → BGRA bytes
  → raw_to_rgb + encode_to_base64
  → POST full image to Vision-AI
  → Vec<TranslationResult> (with top_xy / bot_xy returned by LLM)
```

### Phase 4 (with pre-detection)

```
Shift+Click
  → capture_x11(lens_rect) → BGRA bytes
  → raw_to_rgb → DynamicImage
  → TextDetector::detect(image)
      → ONNX inference (local, CPU)
      → NMS → Vec<OcrRect>  (bounding boxes in lens coords)
  ├─ if rects found:
  │    for each rect:
  │      crop image to rect (+padding)
  │      encode_to_base64(crop)
  │      POST crop to Vision-AI
  │      collect TranslationResult, remap coords to lens space
  └─ if no rects (fallback):
       POST full image (current behaviour)
  → Vec<TranslationResult> → HUD / clipboard (unchanged)
```

The HUD and clipboard layers are **unchanged** — the only change is inside the background thread spawned in `main.rs`.

---

## 3. Model Selection

### 3.1 Models already in the repo

| File | Size | Type | Classes |
|---|---|---|---|
| `assets/yolov8n_fp16.onnx` | ~6 MB | YOLOv8-nano, fp16 | COCO 80 classes |
| `assets/yolo11n.onnx` | ~5 MB | YOLO11-nano | COCO 80 classes |
| `assets/model_fp16.onnx` | ~6 MB | YOLOv8 variant, fp16 | COCO 80 classes |
| `lenzu/yolov8n_fp16.onnx` | same | same as above | same |
| `lenzu/yolov8n.onnx` | ~12 MB | YOLOv8-nano, fp32 | COCO 80 classes |

**Critical limitation**: All three are COCO-trained and have no `text`, `speech_bubble`, or `manga_panel` class. They will not reliably detect text regions in manga images.

### 3.2 Model options — comparison

| Option | Accuracy on manga | Size | Latency (CPU) | Notes |
|---|---|---|---|---|
| **YOLOv8n fine-tuned on manga bubbles** | High | ~6 MB | ~30–80 ms | Best fit; requires training data |
| **CRAFT (ONNX export)** | High | ~30 MB | ~200–400 ms | Character-level regions; overkill for bubbles |
| **EAST text detector** | Medium | ~90 MB | ~150–300 ms | Too large for edge deployment |
| **DBNet (ONNX)** | High | ~5 MB | ~50–120 ms | Good option; less documented in Rust |
| **manga-ocr detector** | Very high | ~400 MB | slow on CPU | PyTorch-only; not feasible in Rust |
| **Generic YOLOv8n (COCO)** | Low | 6 MB | ~30 ms | Free but wrong classes |

### 3.3 Recommended approach

**Phase 4a — Proof of concept** (can start immediately):  
Use the existing `yolov8n_fp16.onnx` to validate the plumbing. Map COCO class `book` (id 73) as a crude text proxy for testing. Confirm the crop-and-send pipeline works end-to-end even if detection quality is poor.

**Phase 4b — Real accuracy**:  
Fine-tune YOLOv8n on a manga speech-bubble dataset (Manga109 or similar). Export to ONNX fp16. Drop in as a replacement. The code path is identical.

**Alternative**: Evaluate `DBNet`-based text detector exported to ONNX. DBNet is smaller than CRAFT/EAST and has documented ONNX export paths from the `mmocr` toolkit.

---

## 4. Rust Implementation Plan

### 4.1 Cargo.toml additions (`lenzu/Cargo.toml`)

```toml
[features]
default = []
onnx = ["ort", "ndarray", "half"]

[dependencies]
# existing deps unchanged ...
ort   = { version = "2.0", optional = true }
ndarray = { version = "0.16", optional = true }
half  = { version = "2.4", optional = true }
anyhow = "1.0"
```

Build with: `cargo build -p lenzu --features onnx`

### 4.2 `OcrRect` struct (`lenzu/src/ocr/ocr_traits.rs`)

```rust
/// A bounding box in the coordinate space of the captured image (pixels).
#[derive(Debug, Clone)]
pub struct OcrRect {
    pub x1: u32,  // left
    pub y1: u32,  // top
    pub x2: u32,  // right
    pub y2: u32,  // bottom
    pub confidence: f32,
}

impl OcrRect {
    /// Expand rect by `pad` pixels, clamped to image bounds.
    pub fn padded(&self, pad: u32, img_w: u32, img_h: u32) -> Self {
        OcrRect {
            x1: self.x1.saturating_sub(pad),
            y1: self.y1.saturating_sub(pad),
            x2: (self.x2 + pad).min(img_w),
            y2: (self.y2 + pad).min(img_h),
            confidence: self.confidence,
        }
    }

    pub fn width(&self) -> u32  { self.x2 - self.x1 }
    pub fn height(&self) -> u32 { self.y2 - self.y1 }

    /// Map rect coords from 640×640 model space back to original image dimensions.
    pub fn scale_to(&self, orig_w: u32, orig_h: u32) -> Self {
        OcrRect {
            x1: (self.x1 * orig_w / 640).min(orig_w),
            y1: (self.y1 * orig_h / 640).min(orig_h),
            x2: (self.x2 * orig_w / 640).min(orig_w),
            y2: (self.y2 * orig_h / 640).min(orig_h),
            confidence: self.confidence,
        }
    }
}
```

### 4.3 `TextDetector` — complete implementation (`lenzu/src/ocr/text_detection.rs`)

The prototype at `prototypes/x11-gtk3-lens-test/src/main.rs` already contains a working YOLOv8 fp16 inference loop (`run_inference`). Port and adapt:

```
Key differences from prototype:
- Return Vec<OcrRect> not Option<Detection>
- Apply NMS (Non-Maximum Suppression)
- Scale rects back to original image dimensions
- Filter by a text-relevant class mask (configurable; default: all classes > threshold)
```

**Core logic outline** (`run_inference` → `TextDetector::detect`):

1. Resize input image to 640×640 using Lanczos3.
2. Normalize pixels to `[0.0, 1.0]` fp16, shape `[1, 3, 640, 640]` (CHW).
3. Run ONNX session; output shape is `[1, 84, 8400]` for YOLOv8 with 80 COCO classes.
4. Transpose to `[8400, 84]`; columns 0–3 are `[cx, cy, w, h]`, columns 4–83 are class scores.
5. For each anchor: `conf = max(class_scores[4..])`. Keep anchors where `conf > CONF_THRESHOLD` (default 0.25).
6. Convert `[cx, cy, w, h]` → `[x1, y1, x2, y2]` in 640-space.
7. Apply greedy NMS with IoU threshold 0.45.
8. Scale remaining rects back to original image dimensions via `OcrRect::scale_to`.
9. Return sorted by `y1` (top-to-bottom reading order).

### 4.4 Crop and multi-call logic (`lenzu/src/main.rs`, background thread)

Current thread (around line 504–520):

```rust
// CURRENT
let results = client.call_api(&b64_full_image)?;
```

Phase 4 replacement:

```rust
// PHASE 4 (feature-gated)
#[cfg(feature = "onnx")]
let results = {
    let img = raw_bytes_to_dynamic_image(&raw, w, h);
    let rects = detector.detect(&img).unwrap_or_default();
    if rects.is_empty() {
        // fallback: full image
        client.call_api(&encode_to_base64(&rgb, w, h))?
    } else {
        let mut all = Vec::new();
        for rect in &rects {
            let r = rect.padded(8, w, h);
            let crop = img.crop_imm(r.x1, r.y1, r.width(), r.height());
            let b64 = encode_crop_to_base64(&crop);
            let mut sub = client.call_api(&b64)?;
            // Remap bounding boxes from crop-local → lens-local coords
            for t in &mut sub {
                remap_coords(t, r.x1, r.y1);
            }
            all.extend(sub);
        }
        all
    }
};
#[cfg(not(feature = "onnx"))]
let results = client.call_api(&encode_to_base64(&rgb, w, h))?;
```

### 4.5 `AppState` changes (`lenzu/src/main.rs`)

Add an `Option<TextDetector>` field, initialized at startup if the model file exists:

```rust
struct AppState {
    // ...existing fields...
    #[cfg(feature = "onnx")]
    detector: Option<TextDetector>,
}
```

Model path resolution order:
1. `$LENZU_YOLO_MODEL` env var
2. `<config_dir>/model_fp16.onnx`
3. Executable-relative `../assets/yolov8n_fp16.onnx`
4. No model → disable pre-detection, log at startup

---

## 5. Configuration Additions (`lenzu_config.json`)

```jsonc
{
  // ... existing fields ...
  "predetect_enabled": true,          // default: false until model proven
  "predetect_model_path": "",         // empty = auto-resolve
  "predetect_conf_threshold": 0.25,   // YOLO confidence threshold
  "predetect_iou_threshold": 0.45,    // NMS IoU threshold
  "predetect_padding": 8              // extra pixels around each detected rect
}
```

`AppConfig` in `config.rs` gains corresponding fields with `#[serde(default)]` so existing config files remain valid.

---

## 6. Coordinate Remapping

`top_xy` / `bot_xy` in `TranslationResult` are reported relative to the **lens capture rectangle** (not the full screen). When a crop is sent, the LLM returns coords relative to the crop. Remap:

```
lens_x = crop_origin_x + crop_local_x
lens_y = crop_origin_y + crop_local_y
```

The existing `top_xy`/`bot_xy` string format is `"x,y"` (from `client.rs`). Implement `remap_coords(result, offset_x, offset_y)` in `utils.rs`.

---

## 7. Fallback Strategy

| Condition | Behaviour |
|---|---|
| `onnx` feature not compiled | Always send full image (current behaviour) |
| `predetect_enabled = false` in config | Always send full image |
| Model file not found at startup | Warn to stderr, disable pre-detection |
| `detect()` returns error | Log warning, fall back to full image for this capture |
| `detect()` returns empty `Vec` | Fall back to full image for this capture |
| Crop is smaller than 16×16 | Skip that rect (too small to OCR reliably) |

---

## 8. Open Questions

1. **Model for manga text** — The COCO YOLOv8n will not detect manga speech bubbles reliably. Options:
   - Fine-tune on Manga109 balloon annotations (requires training pipeline)
   - Use `model_fp16.onnx` in assets — unclear what it was trained on; needs evaluation
   - Evaluate DBNet ONNX exports (smaller than CRAFT, designed for text)

2. **Multiple crops vs one merged request** — Sending N crops = N API calls = N round trips. Alternative: pack multiple crops into a single request as a multi-image message (check Gemini API multi-image support). Could reduce latency at the cost of more complex parsing.

3. **Vertical text direction** — YOLOv8 bbox orientation is axis-aligned; vertical Japanese columns may be merged into one tall narrow box. Verify this works well with how Gemini handles narrow vertical crops.

4. **Minimum crop size threshold** — Very small rects (furigana, small kanji) may produce worse results than the full image. Tune `predetect_padding` and min-size filter empirically.

5. **Gemma 4 E2B / E4B as on-device OCR backend** — See §11 below.

6. **`text_detection.rs` is in `ocr/` submodule** — The `ocr/` submodule currently isn't declared in `lib.rs` or `main.rs`. Decide: move to top-level `src/detect.rs` or properly wire the `ocr` submodule into the build. The `ocr/mod.rs` currently declares `image_handling`, `ocr_gcloud`, `ocr_tesseract`, `ocr_traits`, `ocr_winmedia` but NOT `text_detection`.

---

## 9. Testing Plan

### Unit tests

- `OcrRect::scale_to` — verify coordinate math at known values
- `OcrRect::padded` — verify clamping at image edges
- `remap_coords` — round-trip remap with known offsets

### Integration tests

- Load `yolov8n_fp16.onnx` + a test capture PNG; verify `detect()` returns without panic
- Run full pipeline with `predetect_enabled: true` against `wiremock` stub; verify cropped image is smaller than original

### Manual QA

- Enable `predetect_enabled` with existing COCO model; confirm fallback triggers on manga captures (expected: no detections → full image sent)
- After plugging in a manga-specific model: confirm detected rects align visually with speech bubbles (compare against `/dev/shm/debug_lens.png`)

---

## 10. Implementation Sequence

1. **Wire the `ocr` module** — add `text_detection` to `ocr/mod.rs`; fix the broken closing brace in current `text_detection.rs` (line 33 `}` is inside the `for y` loop; the `run` call and output parse are unreachable)
2. **Add `ort`, `ndarray`, `half` deps** under `[features] onnx` in `Cargo.toml`
3. **Implement `OcrRect`** in `ocr_traits.rs` (or new `detect_types.rs`)
4. **Rewrite `TextDetector::detect`** using prototype's `run_inference` as reference, returning `Vec<OcrRect>`
5. **Add config fields** to `AppConfig` with `serde` defaults
6. **Integrate into background thread** in `main.rs` behind `#[cfg(feature = "onnx")]`
7. **Implement `remap_coords`** in `utils.rs`
8. **Test with COCO model** — confirm plumbing works; fallback triggers
9. **Decide on final detection model** — fine-tune YOLOv8n on manga speech-bubble data (Manga109) or adopt a dedicated text detector (DBNet)

---

## 11. Gemma 4 E2B / E4B — On-Device OCR Backend

### What it is

Google's Gemma 4 (E2B = ~2B params, E4B = ~4B params) is a multimodal vision-language model with:
- Native support for **140+ languages** baked into the base weights — not added via post-training patches
- Strong OCR, handwriting recognition, and visual data extraction from images
- Small enough to run fully **on-device** (CPU or GPU), with no network dependency
- Both variants are open weights, distributable with the application or downloaded on first run

**Model IDs and distribution**

| Channel | E2B | E4B |
|---|---|---|
| HuggingFace | `google/gemma-4-E2B-it` | `google/gemma-4-E4B-it` |
| Ollama | `gemma4:e2b` | `gemma4:e4b` |
| Kaggle | `kaggle.com/models/google/gemma-4` | same collection |
| GGUF (Unsloth) | 4-bit / 8-bit quantized builds on Unsloth HuggingFace page | same |

The `-it` suffix denotes the instruction-tuned variant — this is the one to use for lenzu (prompt-following, JSON output).

### Architectural impact

Gemma 4 E2B/E4B is not just a cheaper remote API — it enables a **fully offline pipeline**:

```
Current:    YOLO (local) → crop → OpenRouter/Gemini (remote, paid)
With Gemma: YOLO (local) → crop → Gemma 4 E2B (local, free after download)
```

This satisfies the project's offline-first / privacy-first design goal at the OCR+translation layer, not just the detection layer.

### Comparison to current Gemini 2.0 Flash

| Dimension | Gemini 2.0 Flash (current) | Gemma 4 E2B | Gemma 4 E4B |
|---|---|---|---|
| Hosting | Remote (OpenRouter) | Local | Local |
| Cost | ~$0.10/M tokens | Free (after download) | Free (after download) |
| Privacy | Images leave device | Images stay on device | Images stay on device |
| Latency | Network + inference | Inference only (~500ms–2s on CPU) | Inference only (~1–4s on CPU) |
| Languages | Strong (Google training) | 140+ native | 140+ native |
| Japanese OCR | Very good | Strong | Strong |
| Furigana / romaji output | Via prompt | Via prompt | Via prompt |
| Model size | N/A | ~1.5 GB (quantized) | ~3 GB (quantized) |
| GPU acceleration | N/A | Vulkan / Metal / CUDA | Vulkan / Metal / CUDA |

### Integration path in Rust

Gemma 4 can be served in two ways compatible with the existing `client.rs` architecture:

**Option A — `ollama` subprocess** (lowest integration effort)
- User runs `ollama run gemma4:e2b` or `ollama run gemma4:e4b` once to pull the model
- `lenzu` calls `http://localhost:11434/v1/chat/completions` (ollama's OpenAI-compatible endpoint)
- Config: set `llm_api_endpoint` to `http://localhost:11434/v1/chat/completions`, `llm_default_model` to `gemma4:e2b`
- `client.rs` already reads endpoint from config — **this requires no code change beyond dropping the auth header**
- Limitation: requires `ollama` daemon running; user must pull model manually (or lenzu can detect and prompt)

**Option B — Direct GGUF via `llama.cpp`** (self-contained)
- Download a quantized GGUF from Unsloth's HuggingFace page (4-bit ~1 GB for E2B, ~2 GB for E4B)
- Spawn `llama-server` as a child process alongside `lenzu_server` (same lifecycle pattern already used for the Electron HUD)
- Exposes the same OpenAI-compatible endpoint as ollama — `client.rs` unchanged
- Preferable for Flatpak/AppImage distribution where bundling `ollama` is impractical

**Option C — ONNX export** (consistent with YOLO pipeline)
- Export Gemma 4 E2B to ONNX via `optimum` and run via `ort`
- Same `ort` dependency already planned for the YOLO detector
- ONNX VLMs are less mature; multimodal (image) input support varies by export toolchain
- Worth revisiting once the ONNX ecosystem matures, but not the near-term path
- **Note**: the export step itself requires Python (`optimum`), but the resulting `.onnx` file is consumed entirely by Rust (`ort`). The model file can be pre-exported and shipped; no Python needed at runtime or build time.

**Language constraint: no Python**

All runtime and build-time code must be Rust or TypeScript. This rules out:
- Any Python subprocess at runtime (no `manga-ocr`, no `paddleocr` Python bindings, no on-the-fly `optimum` export)
- PyTorch-based inference at runtime

It does **not** rule out:
- GGUF files consumed by `llama.cpp` (C++ library, usable from Rust via `llama-cpp-rs` or as a child process)
- ONNX files consumed by `ort` (pure Rust)
- Ollama HTTP API (language-agnostic REST)
- HuggingFace `candle` (pure Rust ML framework — see below)

**Option D — HuggingFace `candle`** (pure Rust, no external process)
- `candle` is HuggingFace's pure-Rust ML inference framework
- Gemma model support exists in `candle-transformers` (Gemma 2 confirmed; Gemma 4 support depends on community PRs)
- Would allow loading weights from HuggingFace Hub directly in Rust, no child process needed
- Most self-contained option if `candle` gains Gemma 4 multimodal support
- Track: `github.com/huggingface/candle` — check for `gemma4` or `paligemma` multimodal examples

### Recommended approach — DECIDED: Option A via Docker

**Decision**: Ollama runs as a Docker container managed by the dev scripts.

- `scripts/setup.sh` — installs Docker if absent, pulls `ollama/ollama` image, creates `lenzu-ollama-data` volume, pulls `gemma4:e2b` model once
- `scripts/run.sh` — auto-detects backend: if `OPENROUTER_API_KEY` is unset, starts the `lenzu-ollama` container before launching lenzu and stops it on exit (`trap`); if key is set, skips ollama entirely and uses OpenRouter as before
- GPU passthrough: `--gpus all` added automatically if NVIDIA runtime is detected in `docker info`
- Model storage is persistent across runs via the named Docker volume `lenzu-ollama-data`

This requires no code change in `client.rs` — the user sets `llm_api_endpoint` to `http://localhost:11434/v1/chat/completions` and `llm_default_model` to `gemma4:e2b` in `lenzu_config.json` when using local mode.

**Future options** (not yet pursued):
- Option B (llama.cpp child process): preferable for Flatpak/AppImage where Docker is unavailable
- Option D (candle): pure Rust, no external process — contingent on Gemma 4 multimodal support landing in `candle-transformers`

### Prompt compatibility

The existing prompt in `config.rs::TRANSLATE_PROMPT` (JSON array of `TranslationResult` objects) should work with Gemma 4 without modification — it's instruction-following capable. Verify that `top_xy` / `bot_xy` coordinate output is consistent; may need a minor prompt tweak since Gemma 4 was not specifically tuned on this schema.

### New config fields

```jsonc
{
  "llm_backend": "openrouter",        // "openrouter" | "ollama" | "local"
  "llm_api_endpoint": "https://openrouter.ai/api/v1/chat/completions",
  "llm_default_model": "google/gemini-2.0-flash-001",
  // For ollama backend (gemma4:e2b or gemma4:e4b):
  // "llm_api_endpoint": "http://localhost:11434/v1/chat/completions",
  // "llm_default_model": "gemma4:e2b"
}
```

The `llm_backend` field drives auth header behaviour (`Bearer <key>` for OpenRouter; no auth for local ollama).

### Planning milestone

Add to `planning.md`:
- **M7b**: Gemma 4 E2B local backend via ollama config option (offline mode)

---

## 12. Dual-Backend Architecture: Gemma Primary, OpenRouter Fallback

### Decision

**Gemma 4 E2B (via ollama) is the primary OCR+translation engine.**  
**OpenRouter (Gemini 2.0 Flash) is the fallback**, invoked only when Gemma cannot produce a translation.

This means:
- `scripts/run.sh` always starts the ollama container (not only when `OPENROUTER_API_KEY` is absent)
- `OPENROUTER_API_KEY` becomes optional — only needed if you want the fallback to work
- Both endpoints are configured simultaneously; `client.rs` gains a `DualOcrClient` wrapper

---

### Fallback trigger conditions

The fallback to OpenRouter fires when Gemma's response meets **any** of these conditions:

| Condition | Meaning |
|---|---|
| `Err(_)` returned | HTTP error, timeout, connection refused, or unparseable JSON |
| `Ok(vec![])` returned | Gemma found no text in the image |
| All results have `english: None` or `english: Some("")` | OCR succeeded but translation was skipped or refused |

A partial result (some items have `english`, some don't) does **not** trigger a full fallback — the items without translation are simply presented as-is. The fallback is a per-capture decision, not per-result.

---

### `DualOcrClient` — design

New struct in `lenzu/src/client.rs`:

```rust
pub struct DualOcrClient {
    primary: OcrClient,           // Gemma via ollama
    fallback: Option<OcrClient>,  // OpenRouter — None if no API key configured
}

impl DualOcrClient {
    pub fn call_api(&self, b64: &str) -> Result<Vec<TranslationResult>, Box<dyn std::error::Error>> {
        match self.try_primary(b64) {
            Ok(results) if self.needs_fallback(&results) => {
                eprintln!("[OCR] primary gave no translations — trying fallback");
                self.try_fallback(b64)
            }
            Ok(results) => Ok(results),
            Err(e) => {
                eprintln!("[OCR] primary failed ({e}) — trying fallback");
                self.try_fallback(b64)
            }
        }
    }

    fn needs_fallback(&self, results: &[TranslationResult]) -> bool {
        // Empty result set, or every english field is blank
        results.is_empty()
            || results.iter().all(|r| {
                r.english.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true)
            })
    }

    fn try_primary(&self, b64: &str) -> Result<Vec<TranslationResult>, Box<dyn std::error::Error>> {
        self.primary.call_api(b64)
    }

    fn try_fallback(&self, b64: &str) -> Result<Vec<TranslationResult>, Box<dyn std::error::Error>> {
        match &self.fallback {
            Some(fb) => fb.call_api(b64),
            None => Err("no translation from primary and no fallback configured".into()),
        }
    }
}
```

`main.rs` constructs a `DualOcrClient` instead of `OcrClient`. The call site is unchanged — `call_api(b64)` is the same method name.

---

### Auth header handling

`OcrClient::call_api` currently always sends `Authorization: Bearer <key>`. Ollama's local endpoint does not use auth. Two approaches:

**Option 1 — Empty key = no header** (minimal change):
```rust
if !self.api_key.is_empty() {
    req = req.header("Authorization", format!("Bearer {}", self.api_key));
}
```

**Option 2 — `is_local_endpoint()` helper**:
```rust
fn is_local(&self) -> bool {
    self.endpoint.starts_with("http://localhost") || self.endpoint.starts_with("http://127.")
}
```

Option 1 is simpler. The primary `OcrClient` is constructed with an empty `api_key`; the fallback `OcrClient` uses `OPENROUTER_API_KEY`.

---

### Config changes (`lenzu_config.json` + `AppConfig`)

New fields added to `AppConfig` with `#[serde(default)]` (backward-compatible):

```jsonc
{
  // Primary (Gemma via ollama) — already exists, defaults change:
  "llm_api_endpoint":   "http://localhost:11434/v1/chat/completions",
  "llm_default_model":  "gemma4:e2b",

  // Fallback (OpenRouter) — new fields:
  "fallback_llm_api_endpoint": "https://openrouter.ai/api/v1/chat/completions",
  "fallback_llm_model":        "google/gemini-2.0-flash-001",
  // fallback API key is read from env var OPENROUTER_API_KEY, not stored in config
}
```

`AppConfig` default changes:
- `llm_api_endpoint` default → `http://localhost:11434/v1/chat/completions`
- `llm_default_model` default → `gemma4:e2b`
- Add `fallback_llm_api_endpoint: String` (default: OpenRouter URL)
- Add `fallback_llm_model: String` (default: `google/gemini-2.0-flash-001`)

The fallback API key is **never stored in config** — always read from the `OPENROUTER_API_KEY` environment variable at startup.

---

### `main.rs` construction

```rust
let cfg = AppConfig::load();
let prompt = cfg.resolved_prompt();
let api_key = std::env::var("OPENROUTER_API_KEY").unwrap_or_default();

let primary = OcrClient::new(
    String::new(),                // no auth for local ollama
    cfg.llm_api_endpoint.clone(),
    cfg.llm_default_model.clone(),
    prompt.clone(),
);
let fallback = if api_key.is_empty() {
    None
} else {
    Some(OcrClient::new(
        api_key,
        cfg.fallback_llm_api_endpoint.clone(),
        cfg.fallback_llm_model.clone(),
        prompt,
    ))
};
let client = DualOcrClient { primary, fallback };
```

---

### `run.sh` changes

The auto-detection logic changes: ollama starts **always** (it's the primary), not only when `OPENROUTER_API_KEY` is absent. `OPENROUTER_API_KEY` becomes a fallback enabler, not a backend selector:

```bash
# OLD: "no API key → use ollama"
# NEW: "always use ollama as primary; API key enables the fallback"

if command -v docker &>/dev/null; then
    trap stop_ollama EXIT
    start_ollama
else
    echo "WARNING: Docker not found — ollama unavailable. Running on OpenRouter only."
fi

if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
    echo "NOTE: OPENROUTER_API_KEY not set — OpenRouter fallback disabled."
fi
```

---

### Logging

When fallback fires, log to `/dev/shm/api_debug.txt` (already used for API response debugging):

```
[14:32:01] [FALLBACK] primary returned no translations — retrying with OpenRouter
```

---

### Planning milestone update

- **M7b** scope expands: not just "ollama config option" but full dual-backend with automatic fallback
- Add to `planning.md` M7b description

---

## 13. Fallback Image Preprocessing (Grayscale + Proportional Downscale)

### Rationale

The OpenRouter (remote) call is billed per token, and vision tokens scale with image byte size. Two cheap preprocessing steps applied **only to the fallback image** cut payload size significantly before it goes over the wire:

1. **Grayscale** — OCR does not need colour. Converting RGB → Luma8 and encoding as a single-channel PNG produces roughly **⅓ the bytes** of the equivalent RGB PNG.
2. **Proportional downscale** — If either dimension exceeds `fallback_max_dimension`, the image is resized (maintaining aspect ratio) before encoding. Fewer pixels = smaller PNG = fewer tokens.

These steps are **not** applied to the primary (Gemma/ollama) call — local inference has no token cost, and colour + full resolution may help Gemma's accuracy.

### Token impact estimate

| Image | Format | Approx PNG bytes | Approx tokens |
|---|---|---|---|
| 400×400 lens capture | RGB colour | ~60 KB | ~800–1,200 |
| 400×400 lens capture | Grayscale | ~20 KB | ~270–400 |
| 400×400 scaled to 300×300 | Grayscale | ~12 KB | ~160–240 |

Savings: **70–80%** on the fallback call compared to the current full-colour send.

### Implementation — `utils.rs`

Two new public functions alongside the existing `encode_to_base64`:

```rust
use image::{DynamicImage, ImageFormat, imageops::FilterType};

/// Convert an RGB DynamicImage to grayscale, optionally downscale, encode as PNG → base64.
/// `max_dim`: longest edge limit in pixels; 0 = no limit.
pub fn encode_for_fallback(image: &DynamicImage, max_dim: u32) -> String {
    // 1. Grayscale
    let gray = image.grayscale();

    // 2. Proportional downscale if over the limit
    let scaled = if max_dim > 0 {
        let (w, h) = gray.dimensions();
        if w > max_dim || h > max_dim {
            gray.resize(max_dim, max_dim, FilterType::Lanczos3)
        } else {
            gray
        }
    } else {
        gray
    };

    // 3. PNG encode → base64
    let mut buf = std::io::Cursor::new(Vec::new());
    scaled.write_to(&mut buf, ImageFormat::Png).unwrap();
    base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
}

/// Convert raw BGRA bytes to a DynamicImage (needed to pass to encode_for_fallback).
pub fn raw_to_dynamic_image(raw: &[u8], w: u32, h: u32) -> DynamicImage {
    let rgb = raw_to_rgb(raw);
    let buf = image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(w, h, rgb)
        .expect("raw_to_dynamic_image: dimensions mismatch");
    DynamicImage::ImageRgb8(buf)
}
```

`DynamicImage::grayscale()` and `resize()` are in the core `image` crate — no new dependencies. The `png` feature already enabled in `Cargo.toml` covers encoding.

### Interface change for `DualOcrClient`

To apply different encoding for primary vs fallback, `DualOcrClient::call_api` accepts a `&DynamicImage` instead of a pre-encoded `&str`. It encodes internally:

```rust
impl DualOcrClient {
    /// `image`: the captured lens region as a DynamicImage.
    pub fn call_api(&self, image: &DynamicImage) -> Result<Vec<TranslationResult>, Box<dyn std::error::Error>> {
        // Primary: full colour, full resolution
        let primary_b64 = encode_to_base64_from_dynamic(image);

        match self.try_primary(&primary_b64) {
            Ok(results) if !self.needs_fallback(&results) => Ok(results),
            primary_result => {
                if let Err(ref e) = primary_result {
                    eprintln!("[OCR] primary failed ({e}) — trying fallback");
                } else {
                    eprintln!("[OCR] primary gave no translations — trying fallback");
                }
                // Fallback: grayscale + downscale to cut token cost
                let fallback_b64 = encode_for_fallback(image, self.fallback_preprocess.max_dimension);
                self.try_fallback(&fallback_b64)
            }
        }
    }
}
```

The `main.rs` background thread call site becomes:

```rust
// Before: client.call_api(&b64_string)
// After:
let dyn_image = raw_to_dynamic_image(&raw, w, h);
let results = client.call_api(&dyn_image)?;
```

### Config additions

```jsonc
{
  // Fallback preprocessing (applied only before OpenRouter calls):
  "fallback_preprocess_grayscale": true,   // default: true
  "fallback_max_dimension": 800            // default: 800; set 0 to disable downscaling
}
```

Added to `AppConfig` with `#[serde(default)]`. Default of 800px means the 400×400 lens capture is untouched (already under limit), but large crops from pre-detection (Phase 4 §4) are still bounded.

### `FallbackPreprocess` struct

```rust
#[derive(Debug, Clone)]
pub struct FallbackPreprocess {
    pub grayscale: bool,       // always true for now; reserved for future toggle
    pub max_dimension: u32,    // 0 = no limit
}

impl Default for FallbackPreprocess {
    fn default() -> Self {
        Self { grayscale: true, max_dimension: 800 }
    }
}
```

Stored in `DualOcrClient`. Read from `AppConfig` at construction time in `main.rs`.

### Interaction with YOLO pre-detection (§4)

When pre-detection is active, multiple small crops are sent individually. Each crop goes through the same dual-backend path — Gemma first (full colour), then fallback if needed (grayscale + scaled). Since crops are already small (typically 60–200px wide), the `max_dimension` cap rarely triggers, but grayscale still cuts their size by ~⅔.

### Debug output

Save the preprocessed fallback image to `/dev/shm/debug_lens_fallback.png` alongside the existing `/dev/shm/debug_lens.png`, so the two can be compared side-by-side during development.

---

## 14. Unit & Integration Tests

### Lesson from Phase 3

The Electron HUD migration (Phase 3) uncovered cascading issues — ghosting artefacts, UDP message timing, IPC race conditions — that were caught late through manual QA. Unit tests on the boundary contracts (message format, port negotiation, lifecycle ordering) would have surfaced these earlier. Phase 4 introduces three new code boundaries (`DualOcrClient`, `encode_for_fallback`, `TextDetector`) that each need tests before integration.

### Coverage map

| Module | New code | Test location |
|---|---|---|
| `utils.rs` | `raw_to_dynamic_image`, `encode_for_fallback` | `utils.rs #[cfg(test)]` |
| `client.rs` | `DualOcrClient`, `needs_fallback`, auth header logic | `client.rs #[cfg(test)]` |
| `config.rs` | new fallback fields + defaults, backward compat | `config.rs #[cfg(test)]` |
| `ocr/text_detection.rs` | `OcrRect::scale_to`, `OcrRect::padded`, `remap_coords` | `text_detection.rs #[cfg(test)]` |
| `tests/integration_test.rs` | dual-backend end-to-end flows | `tests/integration_test.rs` |

---

### `utils.rs` — preprocessing tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::DynamicImage;

    // ── raw_to_dynamic_image ─────────────────────────────────────────────────

    #[test]
    fn test_raw_to_dynamic_image_dimensions() {
        // 2x2 BGRA image
        let raw = vec![0u8; 4 * 2 * 2];
        let img = raw_to_dynamic_image(&raw, 2, 2);
        assert_eq!(img.width(), 2);
        assert_eq!(img.height(), 2);
    }

    #[test]
    fn test_raw_to_dynamic_image_is_rgb() {
        let raw = vec![10, 20, 30, 255]; // 1 BGRA pixel: B=10 G=20 R=30
        let img = raw_to_dynamic_image(&raw, 1, 1);
        // Should be RGB8 (not RGBA, not Luma)
        assert!(matches!(img, DynamicImage::ImageRgb8(_)));
        let pixel = img.as_rgb8().unwrap().get_pixel(0, 0);
        assert_eq!(pixel[0], 30); // R
        assert_eq!(pixel[1], 20); // G
        assert_eq!(pixel[2], 10); // B
    }

    // ── encode_for_fallback — grayscale ──────────────────────────────────────

    #[test]
    fn test_encode_for_fallback_produces_grayscale_png() {
        // Create a small coloured image
        let mut img = image::RgbImage::new(4, 4);
        for p in img.pixels_mut() { *p = image::Rgb([255, 0, 0]); } // solid red
        let dyn_img = DynamicImage::ImageRgb8(img);

        let b64 = encode_for_fallback(&dyn_img, 0); // 0 = no dimension limit
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();

        // Decode the PNG and confirm it is Luma (grayscale)
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(
            matches!(decoded, DynamicImage::ImageLuma8(_) | DynamicImage::ImageLumaA8(_)),
            "encode_for_fallback must produce a grayscale PNG"
        );
    }

    #[test]
    fn test_encode_for_fallback_is_smaller_than_colour() {
        // 400x400 solid-colour image — fallback should be significantly smaller
        let img = DynamicImage::new_rgb8(400, 400);
        let colour_b64 = encode_to_base64(
            &img.to_rgb8().into_raw(), 400, 400
        );
        let fallback_b64 = encode_for_fallback(&img, 0);
        assert!(
            fallback_b64.len() < colour_b64.len(),
            "grayscale b64 ({}) should be smaller than colour b64 ({})",
            fallback_b64.len(), colour_b64.len()
        );
    }

    // ── encode_for_fallback — downscaling ────────────────────────────────────

    #[test]
    fn test_encode_for_fallback_no_resize_when_under_limit() {
        let img = DynamicImage::new_rgb8(100, 80);
        let b64 = encode_for_fallback(&img, 200); // limit 200, image is 100x80
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), 100); // unchanged
        assert_eq!(decoded.height(), 80);
    }

    #[test]
    fn test_encode_for_fallback_downscales_wide_image() {
        let img = DynamicImage::new_rgb8(1000, 400); // wide
        let b64 = encode_for_fallback(&img, 800);
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(decoded.width() <= 800, "width must be clamped to max_dim");
        assert!(decoded.height() <= 800);
        // Aspect ratio preserved: 1000x400 → 800x320
        assert_eq!(decoded.width(), 800);
        assert_eq!(decoded.height(), 320);
    }

    #[test]
    fn test_encode_for_fallback_downscales_tall_image() {
        let img = DynamicImage::new_rgb8(300, 900); // tall
        let b64 = encode_for_fallback(&img, 800);
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(decoded.height() <= 800);
        assert_eq!(decoded.height(), 800);
        assert_eq!(decoded.width(), 266); // 300 * 800/900 ≈ 266
    }

    #[test]
    fn test_encode_for_fallback_zero_max_dim_means_no_resize() {
        let img = DynamicImage::new_rgb8(2000, 2000);
        let b64 = encode_for_fallback(&img, 0); // 0 = no limit
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), 2000);
    }
}
```

---

### `client.rs` — `DualOcrClient` tests

These are unit tests; HTTP calls are replaced by mock `OcrClient` stand-ins. Use `wiremock` for the integration variants, but for unit tests stub `OcrClient` via a trait or by constructing clients that point at a mock server spun up within the test.

```rust
#[cfg(test)]
mod dual_client_tests {
    use super::*;

    // ── needs_fallback ───────────────────────────────────────────────────────

    #[test]
    fn test_needs_fallback_empty_vec() {
        let c = make_dual_client();
        assert!(c.needs_fallback(&[]));
    }

    #[test]
    fn test_needs_fallback_all_null_english() {
        let results = vec![
            TranslationResult { original: "hello".into(), english: None, ..Default::default() },
            TranslationResult { original: "world".into(), english: Some("".into()), ..Default::default() },
        ];
        let c = make_dual_client();
        assert!(c.needs_fallback(&results));
    }

    #[test]
    fn test_needs_fallback_some_english_present() {
        let results = vec![
            TranslationResult { original: "a".into(), english: Some("A".into()), ..Default::default() },
            TranslationResult { original: "b".into(), english: None, ..Default::default() },
        ];
        let c = make_dual_client();
        // At least one result has english → partial success, no fallback
        assert!(!c.needs_fallback(&results));
    }

    #[test]
    fn test_needs_fallback_whitespace_only_english() {
        // "  " should count as empty
        let results = vec![
            TranslationResult { original: "x".into(), english: Some("   ".into()), ..Default::default() },
        ];
        assert!(make_dual_client().needs_fallback(&results));
    }

    // ── auth header behaviour ────────────────────────────────────────────────

    #[test]
    fn test_ocr_client_empty_key_omits_auth_header() {
        // OcrClient with empty api_key must NOT add Authorization header.
        // Verified by inspecting generated_payload / request headers.
        let c = OcrClient::new(String::new(), "http://localhost:11434/v1".into(), "gemma4:e2b".into(), "p".into());
        // Build the request without sending; inspect header presence.
        // (Use reqwest::blocking::Client::request() + RequestBuilder::build() to inspect)
        let req = c.build_request("dGVzdA==").build().unwrap();
        assert!(
            req.headers().get("authorization").is_none(),
            "empty api_key must not produce an Authorization header"
        );
    }

    #[test]
    fn test_ocr_client_non_empty_key_includes_auth_header() {
        let c = OcrClient::new("sk-abc".into(), "https://openrouter.ai/v1".into(), "gemini".into(), "p".into());
        let req = c.build_request("dGVzdA==").build().unwrap();
        let auth = req.headers().get("authorization").unwrap().to_str().unwrap();
        assert_eq!(auth, "Bearer sk-abc");
    }
}
```

> **Note**: `build_request` is a new private helper that returns a `RequestBuilder` (extracted from `call_api`) so headers can be inspected in tests without making a real HTTP call.

---

### `config.rs` — new field defaults and backward compatibility

```rust
#[cfg(test)]
mod tests {
    // ... existing tests ...

    #[test]
    fn test_fallback_endpoint_default_is_openrouter() {
        let cfg = AppConfig::default();
        assert!(cfg.fallback_llm_api_endpoint.contains("openrouter.ai"));
    }

    #[test]
    fn test_fallback_model_default_is_gemini_flash() {
        let cfg = AppConfig::default();
        assert!(cfg.fallback_llm_model.contains("gemini"));
    }

    #[test]
    fn test_primary_endpoint_default_is_localhost_ollama() {
        let cfg = AppConfig::default();
        assert!(cfg.llm_api_endpoint.contains("localhost:11434"));
    }

    #[test]
    fn test_fallback_max_dimension_default_is_800() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.fallback_max_dimension, 800);
    }

    #[test]
    fn test_old_config_without_fallback_fields_loads_with_defaults() {
        // Simulate a pre-Phase-4 config file that has no fallback_* fields.
        let old_json = r#"{
            "lens_size": 400,
            "ui_panel_height": 130,
            "font_size": 13.0,
            "hud_color_hex": "#00FFCC",
            "show_romaji": true,
            "show_furigana": true,
            "overlay_enabled": true,
            "overlay_udp_port": 7331,
            "llm_api_endpoint": "https://openrouter.ai/api/v1/chat/completions",
            "llm_default_model": "google/gemini-2.0-flash-001",
            "translate_src": "jpn",
            "translate_dest": "eng",
            "translate_extra_prompt": "",
            "overlay_render_mode": "furigana"
        }"#;
        let cfg: AppConfig = serde_json::from_str(old_json)
            .expect("old config without fallback fields must still deserialize");
        // New fields should have their defaults
        assert_eq!(cfg.fallback_max_dimension, 800);
        assert!(!cfg.fallback_llm_api_endpoint.is_empty());
    }
}
```

---

### `ocr/text_detection.rs` — `OcrRect` geometry tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // ── scale_to ─────────────────────────────────────────────────────────────

    #[test]
    fn test_scale_to_identity() {
        let r = OcrRect { x1: 100, y1: 50, x2: 200, y2: 150, confidence: 1.0 };
        let scaled = r.scale_to(640, 640); // same dimensions as model space
        assert_eq!(scaled.x1, 100);
        assert_eq!(scaled.y1, 50);
    }

    #[test]
    fn test_scale_to_half_size_original() {
        // Original image is 320x320 (half of 640 model space)
        let r = OcrRect { x1: 320, y1: 160, x2: 480, y2: 320, confidence: 0.9 };
        let scaled = r.scale_to(320, 320);
        assert_eq!(scaled.x1, 160); // 320 * 320/640
        assert_eq!(scaled.y1, 80);
        assert_eq!(scaled.x2, 240);
        assert_eq!(scaled.y2, 160);
    }

    #[test]
    fn test_scale_to_clamps_to_image_bounds() {
        // A rect at the very edge of model space
        let r = OcrRect { x1: 600, y1: 600, x2: 640, y2: 640, confidence: 0.5 };
        let scaled = r.scale_to(400, 400); // smaller than model space
        assert!(scaled.x2 <= 400);
        assert!(scaled.y2 <= 400);
    }

    // ── padded ───────────────────────────────────────────────────────────────

    #[test]
    fn test_padded_expands_rect() {
        let r = OcrRect { x1: 10, y1: 10, x2: 50, y2: 50, confidence: 1.0 };
        let padded = r.padded(5, 400, 400);
        assert_eq!(padded.x1, 5);
        assert_eq!(padded.y1, 5);
        assert_eq!(padded.x2, 55);
        assert_eq!(padded.y2, 55);
    }

    #[test]
    fn test_padded_clamps_at_zero() {
        // Rect near top-left corner
        let r = OcrRect { x1: 3, y1: 2, x2: 50, y2: 50, confidence: 1.0 };
        let padded = r.padded(10, 400, 400);
        assert_eq!(padded.x1, 0); // saturating_sub clamps at 0
        assert_eq!(padded.y1, 0);
    }

    #[test]
    fn test_padded_clamps_at_image_edge() {
        // Rect near bottom-right corner
        let r = OcrRect { x1: 350, y1: 350, x2: 395, y2: 395, confidence: 1.0 };
        let padded = r.padded(10, 400, 400);
        assert_eq!(padded.x2, 400); // .min(img_w)
        assert_eq!(padded.y2, 400);
    }

    // ── remap_coords ─────────────────────────────────────────────────────────

    #[test]
    fn test_remap_coords_adds_offset() {
        let mut result = TranslationResult {
            top_xy: Some("10,20".into()),
            bot_xy: Some("50,80".into()),
            ..Default::default()
        };
        remap_coords(&mut result, 100, 200); // crop origin was (100,200)
        assert_eq!(result.top_xy.as_deref(), Some("110,220"));
        assert_eq!(result.bot_xy.as_deref(), Some("150,280"));
    }

    #[test]
    fn test_remap_coords_handles_missing_coords() {
        let mut result = TranslationResult { top_xy: None, bot_xy: None, ..Default::default() };
        remap_coords(&mut result, 50, 50); // must not panic
        assert!(result.top_xy.is_none());
    }

    #[test]
    fn test_remap_coords_handles_array_format() {
        // Coords coerced from [x,y] array still remap correctly
        let mut result = TranslationResult {
            top_xy: Some("[79,48]".into()),
            bot_xy: Some("[167,181]".into()),
            ..Default::default()
        };
        remap_coords(&mut result, 20, 30);
        assert_eq!(result.top_xy.as_deref(), Some("99,78"));
        assert_eq!(result.bot_xy.as_deref(), Some("187,211"));
    }
}
```

---

### `tests/integration_test.rs` — dual-backend end-to-end

These tests use `wiremock` to stub both the ollama and OpenRouter endpoints.

```rust
// ── helper ───────────────────────────────────────────────────────────────────

fn make_small_image() -> image::DynamicImage {
    image::DynamicImage::new_rgb8(4, 4)
}

fn gemma_response_with_translation() -> serde_json::Value {
    json!({
        "choices": [{ "message": { "content":
            r#"[{"original":"こんにちは","english":"Hello","furigana":"今日[こんにち]は","romaji":"Konnichiwa","top_xy":"0,0","bot_xy":"10,10"}]"#
        }}]
    })
}

fn gemma_response_no_translation() -> serde_json::Value {
    json!({
        "choices": [{ "message": { "content":
            r#"[{"original":"こんにちは","english":null}]"#
        }}]
    })
}

fn openrouter_response() -> serde_json::Value {
    json!({
        "choices": [{ "message": { "content":
            r#"[{"original":"こんにちは","english":"Hello (fallback)","top_xy":"0,0","bot_xy":"10,10"}]"#
        }}]
    })
}

// ── tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dual_client_uses_primary_when_it_succeeds() {
    let primary_server = MockServer::start().await;
    let fallback_server = MockServer::start().await;

    Mock::given(method("POST")).respond_with(
        ResponseTemplate::new(200).set_body_json(gemma_response_with_translation())
    ).mount(&primary_server).await;

    // fallback must NOT be called
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&fallback_server).await;

    let client = DualOcrClient::new_for_test(&primary_server.uri(), &fallback_server.uri(), "sk-test");
    let results = client.call_api(&make_small_image()).unwrap();

    assert_eq!(results[0].english.as_deref(), Some("Hello"));
}

#[tokio::test]
async fn test_dual_client_falls_back_when_primary_returns_no_translation() {
    let primary_server = MockServer::start().await;
    let fallback_server = MockServer::start().await;

    Mock::given(method("POST")).respond_with(
        ResponseTemplate::new(200).set_body_json(gemma_response_no_translation())
    ).mount(&primary_server).await;

    Mock::given(method("POST")).respond_with(
        ResponseTemplate::new(200).set_body_json(openrouter_response())
    ).mount(&fallback_server).await;

    let client = DualOcrClient::new_for_test(&primary_server.uri(), &fallback_server.uri(), "sk-test");
    let results = client.call_api(&make_small_image()).unwrap();

    assert_eq!(results[0].english.as_deref(), Some("Hello (fallback)"));
}

#[tokio::test]
async fn test_dual_client_falls_back_when_primary_errors() {
    let primary_server = MockServer::start().await;
    let fallback_server = MockServer::start().await;

    Mock::given(method("POST")).respond_with(ResponseTemplate::new(503))
        .mount(&primary_server).await;

    Mock::given(method("POST")).respond_with(
        ResponseTemplate::new(200).set_body_json(openrouter_response())
    ).mount(&fallback_server).await;

    let client = DualOcrClient::new_for_test(&primary_server.uri(), &fallback_server.uri(), "sk-test");
    let results = client.call_api(&make_small_image()).unwrap();

    assert_eq!(results[0].english.as_deref(), Some("Hello (fallback)"));
}

#[tokio::test]
async fn test_dual_client_errors_when_both_fail() {
    let primary_server = MockServer::start().await;
    let fallback_server = MockServer::start().await;

    Mock::given(method("POST")).respond_with(ResponseTemplate::new(503))
        .mount(&primary_server).await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(502))
        .mount(&fallback_server).await;

    let client = DualOcrClient::new_for_test(&primary_server.uri(), &fallback_server.uri(), "sk-test");
    assert!(client.call_api(&make_small_image()).is_err());
}

#[tokio::test]
async fn test_dual_client_no_fallback_configured_returns_error_on_primary_failure() {
    let primary_server = MockServer::start().await;
    Mock::given(method("POST")).respond_with(ResponseTemplate::new(503))
        .mount(&primary_server).await;

    let client = DualOcrClient::new_for_test_no_fallback(&primary_server.uri());
    assert!(client.call_api(&make_small_image()).is_err());
}

#[tokio::test]
async fn test_fallback_image_sent_is_smaller_than_primary() {
    // Capture the request bodies from both servers and compare sizes
    let primary_server = MockServer::start().await;
    let fallback_server = MockServer::start().await;

    Mock::given(method("POST")).respond_with(
        ResponseTemplate::new(200).set_body_json(gemma_response_no_translation())
    ).mount(&primary_server).await;
    Mock::given(method("POST")).respond_with(
        ResponseTemplate::new(200).set_body_json(openrouter_response())
    ).mount(&fallback_server).await;

    // Use a larger image so the size difference is visible
    let img = image::DynamicImage::new_rgb8(400, 400);
    let client = DualOcrClient::new_for_test(&primary_server.uri(), &fallback_server.uri(), "sk");
    let _ = client.call_api(&img);

    let primary_reqs = primary_server.received_requests().await.unwrap();
    let fallback_reqs = fallback_server.received_requests().await.unwrap();
    assert_eq!(primary_reqs.len(), 1);
    assert_eq!(fallback_reqs.len(), 1);

    let primary_body_len = primary_reqs[0].body.len();
    let fallback_body_len = fallback_reqs[0].body.len();
    assert!(
        fallback_body_len < primary_body_len,
        "fallback payload ({fallback_body_len}B) must be smaller than primary ({primary_body_len}B)"
    );
}
```

> `DualOcrClient::new_for_test` and `new_for_test_no_fallback` are `#[cfg(test)]`-only constructors that accept explicit endpoint URLs, bypassing config loading. This avoids test coupling to the filesystem.

---

### What these tests guard against

| Risk | Caught by |
|---|---|
| Fallback fires unnecessarily (primary was fine) | `test_dual_client_uses_primary_when_it_succeeds` + `.expect(0)` on fallback mock |
| Fallback silently swallowed (never fires on failure) | `test_dual_client_falls_back_when_primary_errors` |
| Partial translations incorrectly trigger fallback | `test_needs_fallback_some_english_present` |
| Whitespace-only translations treated as success | `test_needs_fallback_whitespace_only_english` |
| Old config files break on upgrade | `test_old_config_without_fallback_fields_loads_with_defaults` |
| Coord remapping wrong after crop offset | `test_remap_coords_adds_offset` + array format variant |
| Fallback sends larger image than primary | `test_fallback_image_sent_is_smaller_than_primary` |
| Grayscale conversion regresses to colour | `test_encode_for_fallback_produces_grayscale_png` |
| Aspect ratio broken by downscale | `test_encode_for_fallback_downscales_wide_image` + tall variant |
| Auth header sent to ollama (local) | `test_ocr_client_empty_key_omits_auth_header` |
