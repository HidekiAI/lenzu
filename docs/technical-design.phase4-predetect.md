# Phase 4 Technical Design: Local Pre-Detection + Dual-Backend OCR

> **Status**: Prototype validated — implementation in progress
> **Created**: 2026-04-04  
> **Updated**: 2026-04-07 — prototype `prototypes/dbnet-test` validated algorithm and defaults; dilation step added to §4.3; Cargo.toml deps corrected (ort 2.0.0-rc.10, imageproc, no ndarray/half); factory signature extended with dilation/pad params
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

### Phase 4 (with pre-detection + per-region OCR)

```
Shift+Click
  ↓
  adaptive_capture(cursor_pos, lens_size, cfg):
    → capture_x11(cursor, oversample_size × oversample_size)   ← single X11 call
    → TextDetector::detect(oversample_image) → primary_boxes  ← N separate regions
    ├─ if no boxes near cursor: return full lens_size crop (Phase 1 fallback)
    └─ return oversample_image + primary_boxes + screen_origin
  ↓
  TextCropper::crop(oversample_image, primary_boxes)   → N CroppedRegion structs
  ↓
  for each CroppedRegion:                        ← N OCR calls
    OCR(crop, simplified_per_region_prompt)      ← no coord fields needed
    top_xy / bot_xy = DBNet box (precise)        ← NOT guessed by LLM
    remap coords to screen space via screen_origin
  ↓
  Vec<TranslationResult>  (one entry per DBNet region, N entries total)
  → HUD / clipboard (unchanged)

  ↑ fallback when N=0: full lens_size crop → full prompt (Phase 1 behaviour)

Ctrl+Shift+Click  (full-desktop mode — see §4.8)
  ↓
  hide_lens_window()
  → capture_x11(0, 0, screen_w, screen_h)
  show_lens_window()
  → TextDetector::detect(downscaled_to_640)
  → find primary_boxes nearest to lens window position
  → TextCropper::crop → per-region OCR (force-remote backend)
```

The HUD and clipboard layers are **unchanged**.

**Why per-region is the primary path (not union crop):**

DBNet detects each text instance independently — a speech bubble, a subtitle line, a vertical character name, and a sound effect each get their own bounding box. Sending them as a union crop still gives the LLM multiple regions at once; it may still:
- Merge a horizontal subtitle with a nearby vertical name
- Combine handwritten sound effects with dialogue  
- Return wrong reading order for mixed vertical/horizontal layouts

Per-region sends one tight crop per DBNet box. The LLM cannot combine what it cannot see.

**Prompt simplification enabled by DBNet:**  
When the LLM receives a single-region crop, it no longer needs to do spatial reasoning. The prompt loses the coordinate fields entirely — bounding boxes come from DBNet, not from the LLM. See §2.3 below.

### 2.3 DBNet's multi-region capability and what it replaces

DBNet outputs a per-pixel probability map. Every connected region above the threshold becomes a separate contour → separate bounding box. A single capture image containing one speech bubble, one subtitle, and one sound effect produces **three independent bounding boxes** from a single DBNet inference call.

This is the fundamental capability the pipeline relies on:

| Text layout | DBNet output | LLM receives |
|---|---|---|
| Single horizontal line | 1 box | 1 crop |
| Three horizontal lines with gaps | 3 boxes | 3 crops (in order) |
| Vertical Japanese column | 1 box (tall, narrow) | 1 crop |
| Mixed: horizontal subtitle + vertical name label | 2 boxes | 2 crops (separate calls) |
| Handwritten sound effects + dialogue bubble | 2+ boxes | 2+ crops (each isolated) |
| Single large blob with no gaps | 1 box | 1 crop (union of connected region) |

**What this replaces in the LLM prompt:**

Current full-image prompt asks the LLM to perform three tasks simultaneously:
1. **Find** all text regions (spatial reasoning — where is the text?)
2. **Read** each region (OCR)
3. **Separate** them properly (segmentation — which pixels belong to which text instance?)

With DBNet handling task 1 and task 3, the per-region prompt only asks:
- Read this one region (OCR)
- Translate it

**Simplified per-region prompt** (replaces `TRANSLATE_PROMPT` in `config.rs` when `text_detection_model` is set):

```
"Act as a highly accurate {src}-to-{dest} OCR and translation engine.
The image contains exactly one text region. Extract the text and translate it.
Return a JSON object with these fields:
  'original' (string — the exact text as written),
  'debug_info' (string or null).
{extra_prompt}"
```

The `top_xy` / `bot_xy` fields are **removed from the per-region prompt** — they come from DBNet, not the LLM. `TranslationResult.top_xy` and `bot_xy` are populated in Rust from `CroppedRegion.source_box` remapped to screen coordinates, before the LLM result is even parsed. The `coerce_to_opt_string` deserializer complexity (which exists to handle LLMs returning `[x,y]` arrays instead of `"x,y"` strings) becomes irrelevant for the per-region path.

The `translate_extra_prompt` field (e.g., the furigana/romaji extension for Japanese) continues to be appended as `{extra_prompt}` — no change to config.

**Fallback prompt (N=0, full-image path):** unchanged — the existing `TRANSLATE_PROMPT` with `top_xy`/`bot_xy` coord fields continues to be used when DBNet finds no regions.

---

## 3. Model Selection

### 3.1 Models in the repo

| File | Size | Type | Suitable for text detection? |
|---|---|---|---|
| `assets/yolov8n_fp16.onnx` | ~6 MB | YOLOv8-nano, fp16 | **No** — COCO 80 classes, no text class |
| `assets/model_fp16.onnx` | ~6 MB | YOLOv8 variant, fp16 | **No** — COCO 80 classes, no text class |
| `assets/stabrise-text_detection_dbnet_ml_v02_model.onnx` | ~4.7 MB | DBNet text detector | **Yes** — purpose-built for text |

**Why COCO YOLOv8 was ruled out**: All YOLO models in the repo are trained on the 80-class COCO dataset, which has no `text`, `speech_bubble`, or `manga_panel` class. Using `book` (id 73) as a proxy was considered and rejected — it fires on background objects, misses inline text and speech bubbles entirely, and produces bounding boxes at the wrong granularity (one box per book spine, not per text region). Fine-tuning YOLOv8 on a manga dataset (Manga109 balloons) would fix this, but that requires a training pipeline and labelled data that doesn't exist yet.

### 3.2 Candidate comparison

| Option | Accuracy on manga text | Size | Latency (CPU) | Notes |
|---|---|---|---|---|
| **DBNet (ONNX) — chosen** | High | ~5 MB | ~50–120 ms | Purpose-built text detector; model already in assets |
| **YOLOv8n fine-tuned on manga** | High | ~6 MB | ~30–80 ms | Requires training pipeline; no dataset yet |
| **CRAFT (ONNX export)** | High | ~30 MB | ~200–400 ms | Character-level; larger and slower than needed |
| **EAST text detector** | Medium | ~90 MB | ~150–300 ms | Too large for edge deployment |
| **Generic YOLOv8n (COCO)** | **Low — disqualified** | 6 MB | ~30 ms | Wrong training domain; see above |

### 3.3 Decision: DBNet via `stabrise-text_detection_dbnet_ml_v02_model.onnx`

[StabRise/text_detection_dbnet_ml_v0.2](https://huggingface.co/StabRise/text_detection_dbnet_ml_v0.2) is a DBNet-based model exported to ONNX, specifically trained for multi-language text detection. Key properties:

- Input: `[1, 3, 640, 640]` float32 tensor, ImageNet-normalized
- Output: `[1, 1, 640, 640]` probability map — each pixel is the likelihood it belongs to a text region
- Post-processing: threshold the probability map → binary mask → contour extraction → bounding boxes
- No NMS required (DBNet produces a smooth probability map, not discrete anchor proposals)
- The model file is already committed at `assets/stabrise-text_detection_dbnet_ml_v02_model.onnx`
- The model path is configured in `lenzu_config.json` (`text_detection_model`) — not hard-coded

**ImageNet normalization constants used during inference**:

| Channel | Mean | Std |
|---|---|---|
| R | 123.675 | 58.395 |
| G | 116.28 | 57.12 |
| B | 103.53 | 57.375 |

---

## 4. Rust Implementation Plan

### 4.1 Cargo.toml additions (`lenzu/Cargo.toml`)

```toml
[features]
default = []
onnx = ["ort", "imageproc"]

[dependencies]
# existing deps unchanged ...
anyhow   = "1.0"
ort      = { version = "2.0.0-rc.10", features = ["ndarray"], optional = true }
imageproc = { version = "0.25", optional = true }
```

Build with: `cargo build -p lenzu --features onnx`

> **Note — why not `ndarray` / `half` as separate deps?**  
> The prototype (`prototypes/dbnet-test`) uses `ort::value::Tensor::from_array` with a raw `Vec<f32>` rather than an `ndarray::Array4`, so `ndarray` and `half` are not required as direct dependencies.  The `ort` crate's own `ndarray` feature is kept to enable its internal ndarray integration.

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

### 4.3 `TextDetector` trait + `DbNetDetector` (`lenzu/src/ocr/text_detection.rs`)

The module exposes a **trait** so the backend can be swapped without changing callers:

```rust
/// Axis-aligned bounding box in the coordinate space of the *original* captured image.
pub struct TextBoundingBox {
    pub x1: u32,  // left edge
    pub y1: u32,  // top edge
    pub x2: u32,  // right edge
    pub y2: u32,  // bottom edge
}

/// Swappable text-detection backend.
pub trait TextDetector: Send + Sync {
    fn detect(&self, image: &DynamicImage) -> Vec<TextBoundingBox>;
}
```

**`DbNetDetector` — DBNet-specific algorithm**:

1. Record the original `(width, height)` for later scaling.
2. Resize to 640×640 using `FilterType::Triangle` (bilinear — faster than Lanczos3, adequate for detection).
3. For each pixel `(x, y)`:  
   `input[0, c, y, x] = (channel_value − mean[c]) / std[c]`  
   using ImageNet constants (R/G/B mean `[123.675, 116.28, 103.53]`, std `[58.395, 57.12, 57.375]`).  
   Shape: `[1, 3, 640, 640]` float32.
4. Run ONNX session; output is `[1, 1, 640, 640]` probability map (`prob_map[[0, 0, y, x]]`).
5. Threshold: for each pixel, `if prob >= threshold { 255 } else { 0 }` → build a `GrayImage` mask.
6. **Dilation** (if `dilation > 0`): `imageproc::morphology::dilate(&mask, Norm::L1, dilation)`.  
   Merges nearby character blobs and compensates for DBNet's slightly-shrunk training targets.
   The radius is in 640×640 pixels; default 16 ≈ 2.5% of image width.
7. `imageproc::contours::find_contours::<u32>(&mask)` → `Vec<Contour<u32>>`.
8. Filter: skip contours with fewer than 4 points (stray noise).
9. For each surviving contour, compute `min_x, min_y, max_x, max_y` of its `.points`.
   Also skip contours where `(max_x - min_x) < 5 || (max_y - min_y) < 5` (single-pixel noise).
10. Scale back:  
    `x_orig = (x_640 as f32 * scale_x).round() as u32`  
    where `scale_x = orig_width as f32 / 640.0` (same for y).
11. Apply padding: expand by `pad_x`/`pad_y` on each side, clamped to image bounds:  
    `x1 = x1.saturating_sub(pad_x); x2 = (x2 + pad_x).min(orig_w);` (same for y).
12. Collect into `Vec<TextBoundingBox>`; run union-merge to collapse overlapping boxes.
13. Sort top-to-bottom, left-to-right (`y1` ascending, then `x1`).

**Why axis-aligned AABB from contour min/max?**  
The downstream consumers (`TextCropper` and the HUD overlay `top_xy`/`bot_xy`) both use axis-aligned rectangles. Computing the convex hull or rotated minimum bounding box would add complexity without benefit here; for dense manga text the AABB is tight enough.

**Factory function** (used by `main.rs`):
```rust
/// Returns None if model_path is None (pre-detection disabled).
/// Returns Err if the model file cannot be loaded.
pub fn build_text_detector(
    model_path: Option<&str>,
    threshold: f32,
    dilation: u8,
    pad_x: u32,
    pad_y: u32,
) -> anyhow::Result<Option<Box<dyn TextDetector>>>
```
The `DbNetDetector` struct and its `TextDetector` impl are `#[cfg(feature = "onnx")]`.  
`build_text_detector` is always present: without the feature it always returns `Ok(None)` and logs a warning if `model_path` is `Some`.

### 4.4 `TextCropper` — per-region crop dispatch (`lenzu/src/ocr/text_cropper.rs`)

**Primary OCR path**: DBNet produces N bounding boxes; `TextCropper` turns them into N individual crops, each of which is sent to the LLM as a separate call with the simplified per-region prompt (§2.3). The LLM receives one region and cannot combine it with anything else.

A separate struct responsible for taking a detected `Vec<TextBoundingBox>` and producing
padded, validated image crops ready for OCR.  Keeping it separate from `TextDetector` means
the crop logic can be tested and tuned independently of the detection model.

```rust
pub struct CroppedRegion {
    /// The cropped sub-image, ready for base64 encoding and OCR.
    pub image: DynamicImage,
    /// Top-left corner of this crop in original image coordinates.
    /// Used by `remap_coords` to translate LLM-reported positions back to lens space.
    pub origin_x: u32,
    pub origin_y: u32,
    /// The source bounding box *before* padding was applied.
    pub source_box: TextBoundingBox,
}

pub struct TextCropper {
    /// Extra pixels added on all four sides of each bounding box.
    /// Default: 8 px.  Larger values include more context for the LLM.
    pub pad: u32,
    /// Skip crops with area (width × height) below this pixel threshold.
    /// Default: 256 px² (16×16).  Prevents sending furigana / single-character noise.
    pub min_area: u32,
}

impl TextCropper {
    pub fn new(pad: u32, min_area: u32) -> Self;

    /// Crop `image` at each bounding box, apply padding (clamped to image bounds),
    /// filter by `min_area`, and return the surviving crops.
    pub fn crop(&self, image: &DynamicImage, boxes: &[TextBoundingBox]) -> Vec<CroppedRegion>;
}
```

**Padding and clamping**:
```
padded_x1 = box.x1.saturating_sub(self.pad)
padded_y1 = box.y1.saturating_sub(self.pad)
padded_x2 = (box.x2 + self.pad).min(image_width)
padded_y2 = (box.y2 + self.pad).min(image_height)
crop_w    = padded_x2 - padded_x1
crop_h    = padded_y2 - padded_y1
```
`image.crop_imm(padded_x1, padded_y1, crop_w, crop_h)` does the actual slice.

**Area filter**: `crop_w * crop_h < self.min_area` → skip.

**`origin_x` / `origin_y`**: set to `padded_x1` / `padded_y1` (the crop's top-left in original image space), not to `box.x1`/`box.y1`.  The padded origin is what's needed for `remap_coords`.

### 4.5 Crop and multi-call logic (`lenzu/src/main.rs`, background thread)

Current thread (around line 504–520):

```rust
// CURRENT
let results = client.call_api(&b64_full_image)?;
```

Phase 4 replacement — using `TextDetector` + `TextCropper`:

```rust
// PHASE 4 (feature-gated)
#[cfg(feature = "onnx")]
let results = {
    let boxes = dual.detect_text(&dyn_image);  // calls TextDetector if configured
    if boxes.is_empty() {
        // No regions found — send full image (same as current behaviour)
        dual.call_api(&dyn_image)?
    } else {
        let cropper = TextCropper::new(/* pad */ 8, /* min_area */ 256);
        let crops = cropper.crop(&dyn_image, &boxes);
        let mut all = Vec::new();
        for crop in crops {
            let mut sub = dual.call_api(&crop.image)?;
            // Remap LLM-reported coords from crop-local space → lens space
            for t in &mut sub.0 {
                remap_coords(t, crop.origin_x, crop.origin_y);
            }
            all.extend(sub.0);
        }
        all
    }
};
#[cfg(not(feature = "onnx"))]
let results = dual.call_api(&dyn_image)?;
```

`dual.detect_text` is a thin helper on `DualOcrClient` that returns `vec![]` when no detector is configured, keeping the feature-gate at one call site.

### 4.6 `AppState` / `DualOcrClient` changes

`DualOcrClient` (in `client.rs`) gains an optional detector field:

```rust
pub struct DualOcrClient {
    // ...existing fields...
    text_detector: Option<Box<dyn TextDetector + Send + Sync>>,
}
```

`DualOcrClient::new` gains one extra parameter:
```rust
text_detector: Option<Box<dyn TextDetector + Send + Sync>>
```

`DualOcrClient::detect_text` is a thin delegate:
```rust
pub fn detect_text(&self, image: &DynamicImage) -> Vec<TextBoundingBox> {
    self.text_detector
        .as_ref()
        .map(|d| d.detect(image))
        .unwrap_or_default()
}
```

The detector is constructed in `main.rs` at startup via `build_text_detector`:
```rust
let text_detector = ocr::text_detection::build_text_detector(
    config.text_detection_model.as_deref(),
    config.text_detection_threshold,
)?;
```

Model path resolution: the value comes directly from `lenzu_config.json → text_detection_model`.
Recommended default value: `"assets/stabrise-text_detection_dbnet_ml_v02_model.onnx"` (relative to CWD).
`None` / absent field → pre-detection disabled, fall back to full-image OCR.

### 4.7 Adaptive Capture Area — Virtual Lens Stretch (`lenzu/src/adaptive_capture.rs`)

#### The fundamental problem

The lens window displays `lens_size × lens_size` pixels. When the user Shift+Clicks, we capture exactly that area and run DBNet on it. If the text under the cursor extends beyond the lens boundary — a long subtitle, a wide banner, a tall manga page column — DBNet will detect boxes that clip the capture edge. We now know text continues past the boundary, but we do not know by how much.

This is a **chicken-and-egg problem**: to know the full text extent you need to capture it, but to know how much to capture you need to know the extent.

#### Approaches considered

| Approach | How it works | Pros | Cons |
|---|---|---|---|
| **Iterative expand** *(previous design, now dropped)* | Detect clipping edges → expand by the overhang amount → re-capture → repeat | Minimal over-capture when text is small | Multiple X11 round-trips; expansion amount is a guess; text may still clip after one expansion |
| **Full desktop capture** | Capture entire screen; run DBNet at reduced resolution; find text near cursor | Catches text of any size | Heavy X11 bandwidth (≥8 MB per click); DBNet on a downscaled full screen loses detection resolution; privacy concern |
| **User click-and-drag** *(xfce4-screenshooter style)* | User manually draws the capture rectangle | Exact; no ambiguity | Requires user effort; breaks "just Shift+Click" flow; reference: https://gitlab.xfce.org/apps/xfce4-screenshooter |
| **Fixed oversample — chosen** | Always capture a larger fixed area; run DBNet once; filter boxes near cursor | Single X11 capture; single DBNet run; simple; configurable ceiling | Over-captures on every click; text larger than the oversample ceiling still clips (handled by fallback) |

#### Chosen approach: fixed oversample, single capture

Always capture `oversample_size × oversample_size` centered on the cursor, **regardless of whether the text looks like it will fit inside the lens**. Run DBNet once on this larger image. The lens window is not shown at this size — the oversample exists only as data for detection.

**Why the oversample size is `max(lens_size × factor, 640)`**:
DBNet always resizes its input to 640×640 before inference. If we capture at 400×400 (the default lens), we are *upscaling* to 640×640 — which degrades detection quality. Capturing at ≥640 px in each axis means DBNet is downscaling, which is always better. The factor (default 2.0) means the oversample at a 400px lens is 800×800, giving DBNet a slightly downscaled 640×640 input and 2× the spatial context.

**Finding the right text cluster ("primary cluster")**:
The oversample area may contain multiple unrelated text regions (subtitles from another window, desktop icons, other speech bubbles). We only want the text the user actually clicked on.

Selection rule: keep only boxes that intersect or touch the original `lens_size × lens_size` area centered on the cursor.

```
lens_rect = { x: (oversample_w - lens_size) / 2,
              y: (oversample_h - lens_size) / 2,
              w: lens_size, h: lens_size }

primary_boxes = boxes where box.intersects(lens_rect)
```

A box "intersects" the lens rect when any part of it overlaps the original lens area — it does not have to be fully contained. This means:
- Small text fully inside the original lens → selected
- Text that started inside the lens and extended past the edge → selected (the oversample captured the extension)
- Unrelated text elsewhere in the oversample area → excluded

If `primary_boxes` is empty (DBNet found nothing near the cursor), fall back to the full lens-sized center crop of the oversample — same as the current no-detection behaviour.

**When text still clips the oversample boundary**:
If the union of `primary_boxes` still touches the edge of the oversample image, the text is larger than `oversample_size`. This is the case the design cannot automatically handle.

Behaviour: send the union crop (even if text is cut off) and log `[adaptive] text exceeds oversample boundary — partial capture`. The user can increase `text_detection_oversample_factor` in config, or use the deferred manual selection feature (see below).

#### Structs

```rust
/// The final image + metadata produced by adaptive_capture().
pub struct AdaptiveCaptureResult {
    /// Cropped image to send to the OCR backend.
    /// Sized to the primary text cluster union, padded and clamped.
    pub image: DynamicImage,
    /// Top-left corner of `image` in screen coordinates.
    /// Subtracted from cursor position and added to LLM-reported coords by remap_coords.
    pub screen_origin_x: i32,
    pub screen_origin_y: i32,
    /// Detected text boxes inside `image` (image-local coordinates).
    /// Empty iff DBNet found nothing near the cursor.
    pub text_boxes: Vec<TextBoundingBox>,
    /// True if the union touched the oversample boundary (text may be cut off).
    pub clipped: bool,
}

/// Loaded from AppConfig; controls the oversample and crop behaviour.
pub struct AdaptiveCaptureConfig {
    /// Oversample multiplier applied to lens_size in each axis.
    /// The actual capture size is max(lens_size * factor, 640), clamped to max_capture_size.
    /// Default: 2.0
    pub oversample_factor: f32,
    /// Hard ceiling on oversample dimension (width or height).  Default: 1600.
    pub max_capture_size: u32,
    /// Padding (px) added around the union crop on all sides.  Default: 16.
    pub crop_padding: u32,
}
```

#### `compute_union_bbox` helper (`ocr/text_detection.rs`)

```rust
pub fn compute_union_bbox(boxes: &[TextBoundingBox]) -> Option<TextBoundingBox> {
    let x1 = boxes.iter().map(|b| b.x1).min()?;
    let y1 = boxes.iter().map(|b| b.y1).min()?;
    let x2 = boxes.iter().map(|b| b.x2).max()?;
    let y2 = boxes.iter().map(|b| b.y2).max()?;
    Some(TextBoundingBox { x1, y1, x2, y2 })
}
```

#### `merge_overlapping_boxes` — union merge pass (`ocr/text_detection.rs`)

DBNet occasionally splits a single physical text region into multiple overlapping
bounding boxes — most commonly when vertical Japanese columns produce per-character
probability blobs, or when uneven probability mass causes a line to fragment after
thresholding. After padding is applied these fragments often overlap. The union
merge pass collapses them before boxes are returned to the caller.

**Algorithm**: iteratively scan all box pairs; when two boxes overlap, replace them
with their axis-aligned bounding union (upper-left-most corner, bottom-right-most
corner). Repeat until a full pass produces zero merges. Typically converges in 1–2
passes for the number of boxes DBNet produces.

```rust
fn overlaps(a: &TextBoundingBox, b: &TextBoundingBox) -> bool {
    a.x1 < b.x2 && a.x2 > b.x1 && a.y1 < b.y2 && a.y2 > b.y1
}

fn union_bbox(a: &TextBoundingBox, b: &TextBoundingBox) -> TextBoundingBox {
    TextBoundingBox {
        x1: a.x1.min(b.x1),
        y1: a.y1.min(b.y1),
        x2: a.x2.max(b.x2),
        y2: a.y2.max(b.y2),
    }
}

pub fn merge_overlapping_boxes(mut boxes: Vec<TextBoundingBox>) -> Vec<TextBoundingBox> {
    loop {
        let mut merged = Vec::with_capacity(boxes.len());
        let mut consumed = vec![false; boxes.len()];
        let mut any = false;
        for i in 0..boxes.len() {
            if consumed[i] { continue; }
            let mut current = boxes[i].clone();
            for j in (i + 1)..boxes.len() {
                if consumed[j] { continue; }
                if overlaps(&current, &boxes[j]) {
                    current = union_bbox(&current, &boxes[j]);
                    consumed[j] = true;
                    any = true;
                }
            }
            merged.push(current);
        }
        boxes = merged;
        if !any { break; }
    }
    boxes
}
```

This should be called at the end of `DbNetDetector::detect()`, after padding is
applied and before the result is returned. It is distinct from `compute_union_bbox`
(which folds *all* boxes into one for the oversample crop path); this function only
merges pairs that actually overlap and leaves well-separated boxes independent.

#### `adaptive_capture` algorithm

```
fn adaptive_capture(
    detector:        &dyn TextDetector,
    cursor_screen_x: i32,
    cursor_screen_y: i32,
    lens_size:       u32,
    cfg:             &AdaptiveCaptureConfig,
) -> AdaptiveCaptureResult
```

1. Compute `oversample_size = clamp(lens_size as f32 * cfg.oversample_factor, 640.0, cfg.max_capture_size as f32) as u32`
2. `os_screen_x = cursor_screen_x - (oversample_size / 2) as i32`
   `os_screen_y = cursor_screen_y - (oversample_size / 2) as i32`
3. `oversample_image = capture_x11(os_screen_x, os_screen_y, oversample_size, oversample_size)`
4. `all_boxes = detector.detect(&oversample_image)`
5. Compute `lens_rect` in oversample-local coords:
   `lens_x = (oversample_size - lens_size) / 2`
   `lens_y = (oversample_size - lens_size) / 2`
6. `primary_boxes = all_boxes.filter(|b| b.intersects(lens_x, lens_y, lens_size, lens_size))`
7. If `primary_boxes` is empty:
   → crop oversample to the central `lens_size × lens_size` area (original lens)
   → return with empty `text_boxes`, `clipped = false`
8. `union = compute_union_bbox(&primary_boxes)` (always `Some` here)
9. Detect boundary clip: `clipped = union touches any edge of oversample_image`
10. Pad and clamp union:
    ```
    px1 = union.x1.saturating_sub(cfg.crop_padding)
    py1 = union.y1.saturating_sub(cfg.crop_padding)
    px2 = min(union.x2 + cfg.crop_padding, oversample_size)
    py2 = min(union.y2 + cfg.crop_padding, oversample_size)
    ```
11. Minimum size guard: if `(px2 - px1) < 16 || (py2 - py1) < 16` → use central lens_size crop
12. `crop = oversample_image.crop_imm(px1, py1, px2 - px1, py2 - py1)`
13. `screen_origin_x = os_screen_x + px1 as i32`
    `screen_origin_y = os_screen_y + py1 as i32`
14. Return `AdaptiveCaptureResult { image: crop, screen_origin_x, screen_origin_y, text_boxes: primary_boxes, clipped }`

Note: `adaptive_capture` takes the cursor position and calls `capture_x11` internally. It does **not** receive a pre-captured image — that was a design mistake in the earlier draft. The oversample must be the very first capture; there is no "initial lens capture" anymore when detection is enabled.

#### Deferred: user click-and-drag selection

For text that exceeds `max_capture_size`, the automatic approach cannot help. The correct long-term solution is a manual selection mode, as used by `xfce4-screenshooter` (https://gitlab.xfce.org/apps/xfce4-screenshooter): the user holds Shift and drags a rectangle rather than clicking a point. Lenzu would enter a "selection mode" overlay where the cursor becomes a crosshair, and the capture area is defined by the drag rect.

This is **deferred** — it requires significant GTK3 interaction changes (draw-on-drag, crosshair cursor). Until it is implemented, users with consistently large text should increase `text_detection_oversample_factor` or `text_detection_max_capture_size` in config.

#### Where this lives in the call chain

`adaptive_capture` replaces the `capture_x11` call + DBNet path in the `main.rs` background thread when `text_detection_model` is set:

```rust
// With detection enabled:
let result = adaptive_capture(
    &*detector,
    cursor_screen_x, cursor_screen_y,
    config.lens_size as u32,
    &adaptive_cfg,
);
if result.clipped {
    eprintln!("[adaptive] text exceeds oversample boundary — partial capture");
}
let (mut ocr_results, meta) = dual.call_api(&result.image)?;
for t in &mut ocr_results {
    remap_coords(t, result.screen_origin_x, result.screen_origin_y);
}

// Without detection (text_detection_model = null):
// capture_x11 at lens_size as before, no adaptive logic.
```

No change to `DualOcrClient::call_api` signature.

### 4.8 Ctrl+Shift+Click — Full-Desktop DBNet Mode

#### Motivation

The oversample approach (§4.7) has a hard ceiling (`max_capture_size`, default 1600 px). Text blocks larger than that — long subtitles running the full screen width, multi-panel manga spreads — cannot be auto-sized. The user needs a way to say "go wider than the normal oversample and find the text yourself".

Ctrl+Shift+Click currently forces the remote OCR backend (`call_api_force_fallback`). That behaviour is **retained and stacked**: Ctrl+Shift+Click = full-desktop DBNet capture **and** force-remote OCR.

#### Why full desktop is feasible here

DBNet always resizes its input to 640×640 before inference. Whether the input image is 800×800 or 1920×1080, inference time is the same (~50–120 ms CPU). The extra cost is only in the X11 `GetImage` call (typically 20–60 ms for a full 1080p desktop). This is acceptable for a deliberate Ctrl+Shift+Click; it would be too slow for every Shift+Click.

#### The lens-window obstruction problem

At the moment of Ctrl+Shift+Click, the lens window is visible on screen. X11 `GetImage` on the root window captures the composited display — including the lens window's own pixels. If the lens is sitting over the text the user wants to OCR, the captured image will contain the lens frame, the spinner, or the previous OCR result, **not** the text underneath.

**Solution: hide the lens window before the full-desktop capture, show it again immediately after.**

GTK3 provides `widget.hide()` / `widget.show()`. Between `hide()` and the X11 `GetImage` there must be a compositor sync — at minimum one `gdk::flush()` + a brief yield — so the compositor has time to re-paint the desktop without the lens overlay before we snapshot it.

Timing requirement: two frames (at 60 Hz, ~33 ms) is enough for most compositors. A `std::thread::sleep(Duration::from_millis(50))` before capture is a simple and reliable guard.

#### Algorithm

```
on Ctrl+Shift+Click:
  1. Record current lens position (screen_center_x, screen_center_y)
  2. hide_lens_window()                    // GTK: window.hide(); gdk::flush()
  3. sleep(50ms)                           // allow compositor to repaint
  4. full_capture = capture_x11(0, 0, screen_w, screen_h)
  5. show_lens_window()                    // GTK: window.show()
  6. downscale full_capture to 640×640 for DBNet input
     (keep the full-res copy; DBNet receives the downscaled version)
  7. boxes = detector.detect(&downscaled)  // returns boxes in 640×640 space
  8. scale boxes back to full-screen coordinates:
       box_screen.x1 = (box_640.x1 as f32 / 640.0 * screen_w as f32) as u32
       (same for y, x2, y2)
  9. primary_boxes = boxes where centroid distance to lens center < selection_radius
     (selection_radius default: lens_size / 2; configurable)
 10. if primary_boxes empty → use lens_size crop centered on cursor (fallback)
 11. union = compute_union_bbox(&primary_boxes)
 12. crop full_capture to union + crop_padding (clamp to screen bounds)
 13. screen_origin = (union.x1 - crop_padding, union.y1 - crop_padding)
 14. call_api_force_fallback(&crop)        // force remote OCR, existing behaviour
 15. remap_coords(results, screen_origin_x - screen_center_x + lens_size/2,
                           screen_origin_y - screen_center_y + lens_size/2)
```

Step 6 is a new `downscale_for_detection(image: &DynamicImage) -> DynamicImage` helper in `utils.rs` — it resizes to 640×640 using `FilterType::Triangle` (same as DBNet preprocessing), producing the image the detector should see. The full-res image is kept in a separate binding for the final crop in step 12.

#### Screen dimensions

`screen_w` and `screen_h` are available at startup from GTK's default screen:
```rust
let screen = gdk::Screen::default().expect("no default screen");
let screen_w = screen.width() as u32;
let screen_h = screen.height() as u32;
```

Multi-monitor note: `gdk::Screen::width()` returns the combined virtual desktop width. On a dual-monitor setup this captures both monitors. This is correct behaviour — text may be on either monitor.

#### Open question: what happens to old Ctrl+Shift+Click "force remote" shortcut?

Before this change, Ctrl+Shift+Click was the only way to force the remote backend when the local ollama is slow or wrong. With this change, Ctrl+Shift+Click implies full-desktop capture **plus** force-remote. If the user wants force-remote **without** the full-desktop capture (e.g., text is small and they just want a better LLM), they have no dedicated shortcut.

Options (decide before implementing):
- **A**: Ctrl+Shift+Click = full-desktop + force-remote (as designed here). Old force-remote-only behaviour is gone or merged.
- **B**: Ctrl+Shift+Click = full-desktop; Ctrl+Alt+Shift+Click = force-remote-only (adds a third chord).
- **C**: Ctrl+Shift+Click = full-desktop (force-remote is always implied when the result needs it, via the fallback chain — so the user doesn't need a dedicated force-remote shortcut).

Option C is the cleanest: the existing automatic fallback chain already tries remote when local fails. The manual "force remote" shortcut was a debug convenience. Full-desktop mode for Ctrl+Shift+Click is a more useful assignment.

### 4.9 Detection Debug Visualization

When `text_detection_debug: true` is set in config, detected bounding boxes are rendered visually so the developer can verify DBNet is finding the right regions.

#### Why not a live on-screen overlay?

A transparent full-screen GTK3 window with drawn rectangles would be the most interactive option but carries the same alpha-compositing / ghosting risk that caused the Tauri→Electron migration (see `planning.md` — Overlay HUD history). The compositor layer is not a safe place to invest in for a debug-only feature.

The Electron `lenzu_server` overlay is pinned to the bottom of the screen as a subtitle band — it is not positioned or sized for full-screen annotation.

#### Chosen approach: two complementary outputs

**1. Annotated debug PNG** — covers Ctrl+Shift+Click (full-desktop) and Shift+Click alike

After every detection run (regardless of whether boxes were found), save an annotated copy of the detection image:

```
/dev/shm/lenzu/debug_detection.png
```

- For **Shift+Click**: the annotated image is the `oversample_size × oversample_size` capture with a yellow rectangle marking the original lens region and green rectangles for each detected box.
- For **Ctrl+Shift+Click**: the annotated image is the full desktop capture (downscaled to 640×640, since that is what DBNet actually saw) with green boxes drawn.

Implementation:
```rust
// imageproc is already a planned dep (--features onnx)
use imageproc::drawing::draw_hollow_rect_mut;
use imageproc::rect::Rect;
use image::Rgb;

fn save_debug_image(
    detection_input: &DynamicImage,   // the image DBNet received (640×640 for full-desktop)
    boxes: &[TextBoundingBox],
    lens_rect: Option<(u32, u32, u32, u32)>,  // Some((x,y,w,h)) for Shift+Click only
) {
    let mut annotated = detection_input.to_rgb8();
    // Yellow rectangle = original lens area (Shift+Click only)
    if let Some((x, y, w, h)) = lens_rect {
        draw_hollow_rect_mut(&mut annotated, Rect::at(x as i32, y as i32).of_size(w, h), Rgb([255, 255, 0]));
    }
    // Green rectangles = detected text boxes
    for b in boxes {
        draw_hollow_rect_mut(&mut annotated, Rect::at(b.x1 as i32, b.y1 as i32).of_size(b.x2 - b.x1, b.y2 - b.y1), Rgb([0, 255, 0]));
    }
    let _ = std::fs::create_dir_all("/dev/shm/lenzu");
    let _ = DynamicImage::ImageRgb8(annotated).save("/dev/shm/lenzu/debug_detection.png");
    eprintln!("[debug] detection image saved → /dev/shm/lenzu/debug_detection.png  ({} boxes)", boxes.len());
}
```

**2. Cairo box overlays on the lens window** — covers Shift+Click only

For Shift+Click, after DBNet detects boxes, scale the `primary_boxes` from oversample space to lens display space and draw them as colored outlines in the lens window's `connect_draw` callback using the existing Cairo context:

```rust
// In the lens window draw callback (main.rs), when debug mode is on:
if config.text_detection_debug {
    cr.set_source_rgba(0.0, 1.0, 0.2, 0.8);  // green, 80% opacity
    cr.set_line_width(2.0);
    let scale_x = lens_size as f64 / oversample_size as f64;
    let scale_y = scale_x;
    let lens_offset_x = (oversample_size - lens_size) / 2;
    let lens_offset_y = (oversample_size - lens_size) / 2;
    for b in &state.last_detection_boxes {
        // Transform: oversample coords → lens-display coords
        let x = (b.x1 as i32 - lens_offset_x as i32) as f64 * scale_x;
        let y = (b.y1 as i32 - lens_offset_y as i32) as f64 * scale_y;
        let w = (b.x2 - b.x1) as f64 * scale_x;
        let h = (b.y2 - b.y1) as f64 * scale_y;
        cr.rectangle(x, y, w, h);
        cr.stroke().unwrap_or(());
    }
}
```

`state.last_detection_boxes` is a `Vec<TextBoundingBox>` stored in `AppState`, updated after each Shift+Click detection. Boxes fully outside the original lens rect appear outside the lens window boundary (clipped by GTK).

For Ctrl+Shift+Click: the boxes are in full-screen space and the lens is 400×400 — scaling a 1920×1080 scene into 400×400 makes individual boxes 1–2 px tall. The debug PNG is the right tool here; the Cairo overlay is not added for the full-desktop path.

#### Summary

The **annotated debug PNG is the primary debug tool** — it works for both Shift+Click and Ctrl+Shift+Click, requires no GTK interaction changes, and gives a complete picture of what DBNet saw and detected. The Cairo overlay in the lens window is a lightweight bonus for Shift+Click; it is not added for the full-desktop path where the scale is impractical.

A real-time transparent overlay drawn over the live screen was considered and rejected: it carries the same alpha-compositing / compositor ghosting risk that caused the Tauri→Electron migration, and the debug PNG already provides all the information needed.

| Debug tool | Shift+Click | Ctrl+Shift+Click | Effort | Status |
|---|---|---|---|---|
| **Annotated PNG** `/dev/shm/lenzu/debug_detection.png` | Yes (oversample image + box outlines) | Yes (640×640 DBNet input + box outlines) | ~20 lines; `imageproc` already a dep | **Primary; implement first** |
| Cairo box overlay on lens window | Yes (boxes scaled to lens display) | No (1920→400 scale makes boxes 1–2 px) | ~15 lines; existing Cairo context | Secondary; easy add-on |
| Transparent full-screen GTK3 overlay | Possible | Possible | High effort; compositor risk | **Deferred indefinitely** |

Config flag: `text_detection_debug: bool` (default `false`). All debug outputs disabled when `false`.

---

## 5. Configuration Additions (`lenzu_config.json`)

```jsonc
{
  // ... existing fields ...

  // Path to the text-detection ONNX model (relative to CWD or absolute).
  // Omit or set to null to disable pre-detection and adaptive capture entirely.
  // Default: null
  "text_detection_model": "assets/stabrise-text_detection_dbnet_ml_v02_model.onnx",

  // DBNet probability-map binarization threshold (0.0–1.0).
  // Pixels with probability > threshold are marked as text.
  // Lower = more sensitive (more regions detected, more noise); higher = stricter.
  // Default: 0.3
  "text_detection_threshold": 0.3,

  // ── Adaptive capture (virtual lens stretch) ───────────────────────────────
  // Multiplier applied to lens_size to compute the oversample capture area.
  // The oversample is always taken at max(lens_size * factor, 640) to ensure
  // DBNet gets at least native-resolution input.  Higher values catch larger
  // text at the cost of more X11 bandwidth per click.  Default: 2.0
  "text_detection_oversample_factor": 2.0,

  // Hard ceiling on the oversample dimension (width or height) in pixels.
  // Text larger than this cannot be auto-detected; use manual selection.
  // Default: 1600
  "text_detection_max_capture_size": 1600,

  // Padding (px) added around the detected text union on all four sides
  // before sending the crop to OCR.  Provides context for the LLM.  Default: 16.
  "text_detection_crop_padding": 16,

  // When true: saves an annotated detection image to /dev/shm/lenzu/debug_detection.png
  // after every Shift+Click or Ctrl+Shift+Click, and draws green box outlines on the
  // lens window for Shift+Click detections.  Default: false.
  "text_detection_debug": false
}
```

Note: there is no `predetect_iou_threshold` for DBNet (IoU/NMS is a YOLO concept — DBNet outputs a smooth probability map, not anchor boxes, so NMS is not needed).

`AppConfig` in `config.rs` gains all new fields with `#[serde(default)]` so existing config files remain valid without any migration.

**Prompt changes in `config.rs`**: a second prompt constant is added alongside `TRANSLATE_PROMPT`:

```rust
/// Used when text_detection_model is set and DBNet has found a region.
/// Bounding box is already known from DBNet — coordinates are NOT requested from the LLM.
const PER_REGION_PROMPT: &str =
    "Act as a highly accurate {src}-to-{dest} OCR and translation engine. \
    The image contains exactly one isolated text region. \
    Extract the text and translate it. \
    Return a JSON object with EXACTLY these fields: \
    'original' (string), \
    'debug_info' (string or null). \
    Do NOT include top_xy or bot_xy — they are determined externally.";
```

`AppConfig::resolved_prompt()` gains a boolean parameter (or separate method `resolved_per_region_prompt()`). When `text_detection_model` is `Some`, the per-region prompt is used for individual crop calls; the full `TRANSLATE_PROMPT` is used only for the fallback full-image path.

`translate_extra_prompt` (e.g., `"…furigana and romaji fields…"`) is still appended to both prompts.

---

## 6. Coordinate Systems and Scaling

### 6.1 The four coordinate spaces

Every bounding box in the pipeline lives in exactly one of these spaces. Getting the space wrong silently produces off-by-N-× errors that are hard to spot without the debug PNG.

| Space | Size | Origin | Lifetime |
|---|---|---|---|
| **DBNet input space** | always 640×640 | top-left of resized image | Internal to `DbNetDetector::detect()` only — never escapes |
| **Capture space** | oversample size (square) or full desktop (non-square) | top-left of `capture_x11` rect | Output of `detect()`; input to `TextCropper` |
| **Screen space** | full desktop (e.g. 1920×1080) | (0,0) = monitor top-left | `AdaptiveCaptureResult.screen_origin_*`; `remap_coords` offset |
| **Crop space** | individual `CroppedRegion` dimensions | top-left of padded crop | LLM input; LLM-reported `top_xy`/`bot_xy` before remapping |

### 6.2 The cardinal rule: `detect()` returns boxes in input-image space

`TextDetector::detect(image: &DynamicImage) -> Vec<TextBoundingBox>` **always returns boxes in the coordinate space of `image`**, regardless of the internal 640×640 resize. The caller never needs to know the model's input resolution.

This is enforced inside `DbNetDetector`: after computing contours in 640×640 space, every point is scaled back before returning:

```rust
// Inside DbNetDetector::detect, at the end of post-processing:
let scale_x = orig_width  as f32 / 640.0;
let scale_y = orig_height as f32 / 640.0;

TextBoundingBox {
    x1: (box_640.x1 as f32 * scale_x) as u32,
    y1: (box_640.y1 as f32 * scale_y) as u32,
    x2: ((box_640.x2 as f32 * scale_x) as u32).min(orig_width),
    y2: ((box_640.y2 as f32 * scale_y) as u32).min(orig_height),
}
```

**Why separate `scale_x` and `scale_y`**: for the square oversample capture `scale_x == scale_y`, so they are interchangeable. For the full-desktop Ctrl+Shift+Click capture (e.g. 1920×1080), `scale_x = 3.0` but `scale_y = 1.6875`. Using a single scale factor here would shift boxes sideways or vertically. The scaling must always use the per-axis ratio.

**Aspect ratio distortion**: DBNet's `resize_exact(640, 640)` distorts non-square images. On a 1920×1080 desktop the horizontal axis is compressed more than vertical (ratio 3.0 vs 1.6875). Detection quality may degrade on very wide/narrow text. If this proves problematic, letterboxing can be added: pad the shorter axis to make the image square before resize, then subtract the pad offset when scaling back. For now, accept the distortion — it rarely matters for axis-aligned Latin/CJK text.

### 6.3 Coordinate chain: Shift+Click (oversample path)

```
capture_x11(os_screen_x, os_screen_y, OS, OS)   → oversample image (OS×OS)
  └─ OS = max(lens_size × factor, 640), e.g. 800

detector.detect(&oversample_image)               → boxes in OS×OS space
  └─ internally: resize to 640×640 → contours → scale back by OS/640

TextCropper::crop(oversample_image, boxes)        → CroppedRegion
  └─ origin_x, origin_y in OS×OS space

screen_origin_x = os_screen_x + origin_x         → screen space
screen_origin_y = os_screen_y + origin_y

LLM receives crop image (crop space)
  LLM-reported top_xy "cx,cy" is in crop space
  remap: screen_x = screen_origin_x + cx          → screen space
         screen_y = screen_origin_y + cy

DBNet-provided top_xy (primary path, §2.3):
  box in OS space → subtract origin_x/y → crop space
  "cx,cy" = (box.x1 - origin_x, box.y1 - origin_y)
  (no LLM coordinate guessing needed)
```

### 6.4 Coordinate chain: Ctrl+Shift+Click (full-desktop path)

```
capture_x11(0, 0, screen_w, screen_h)            → full desktop image (W×H, e.g. 1920×1080)

detector.detect(&full_desktop_image)              → boxes in W×H space
  └─ internally: resize to 640×640 → contours → scale back by W/640, H/640

Filter: keep boxes where centroid is within selection_radius of cursor

TextCropper::crop(full_desktop_image, boxes)      → CroppedRegion
  └─ origin_x, origin_y in W×H space = screen space (capture origin is (0,0))

screen_origin_x = 0 + origin_x = origin_x        → screen space
screen_origin_y = 0 + origin_y = origin_y

LLM receives crop (crop space); remap same as above
```

Because the full-desktop capture starts at (0,0), capture space == screen space for Ctrl+Shift+Click. No extra offset addition.

### 6.5 Cairo overlay scaling (debug, Shift+Click only)

The lens window displays the captured area at `lens_size × lens_size` pixels, but the underlying data is `OS × OS`. To draw DBNet boxes on the lens window:

```rust
let scale    = lens_size as f64 / oversample_size as f64;  // e.g. 400/800 = 0.5
let offset_x = (oversample_size - lens_size) / 2;          // center of oversample
let offset_y = (oversample_size - lens_size) / 2;

// For each box (in OS space):
let display_x = (box.x1 as i32 - offset_x as i32) as f64 * scale;
let display_y = (box.y1 as i32 - offset_y as i32) as f64 * scale;
let display_w = (box.x2 - box.x1) as f64 * scale;
let display_h = (box.y2 - box.y1) as f64 * scale;
```

Boxes outside the original lens area produce negative `display_x`/`display_y` — they are clipped by the lens window boundary automatically. This correctly shows that part of a text region extended past the lens edge.

### 6.6 `remap_coords` — fallback path only

**When DBNet is active (per-region path)**: the LLM receives a single tight crop and is not asked for coordinates at all (§2.3 prompt has no `top_xy`/`bot_xy` fields). Bounding boxes are set in Rust directly from the DBNet box scaled to screen space. `remap_coords` is **not called** on this path.

**When DBNet is not active (fallback full-image path)**: the LLM receives the full lens image and is asked to report `top_xy`/`bot_xy` in crop space. `remap_coords` translates those to screen space.

```rust
/// Translate LLM-reported top_xy / bot_xy from crop space to screen space.
/// offset_x = screen_origin_x, offset_y = screen_origin_y of the crop.
/// Only called on the fallback full-image path — NOT on the per-region DBNet path.
pub fn remap_coords(result: &mut TranslationResult, offset_x: i32, offset_y: i32) {
    result.top_xy = add_offset(&result.top_xy, offset_x, offset_y);
    result.bot_xy = add_offset(&result.bot_xy, offset_x, offset_y);
}

fn add_offset(xy: &Option<String>, dx: i32, dy: i32) -> Option<String> {
    let s = xy.as_deref()?;
    let (x, y) = parse_xy(s)?;       // reuse existing parse_xy from client.rs
    Some(format!("{},{}", x + dx, y + dy))
}
```

`remap_coords` is only called on the **fallback full-image path** (when DBNet finds nothing and the LLM reports its own coordinate guesses). On the per-region path, `top_xy`/`bot_xy` are populated directly in Rust from the DBNet box, already in screen space — `remap_coords` is skipped.

---

## 7. Fallback Strategy

| Condition | Behaviour |
|---|---|
| `onnx` feature not compiled | Capture at `lens_size`; send full image (current behaviour) |
| `text_detection_model` is `null` / absent in config | Same as above — no adaptive capture |
| Model file not found at startup | Warn to stderr; disable adaptive capture for the session; fall back to lens_size |
| `detect()` returns an error | Log warning; send central `lens_size` crop of the oversample image |
| No boxes intersect the original lens rect | No text detected near cursor; send central `lens_size` crop |
| Union crop smaller than 16×16 px | Degenerate box (noise); send central `lens_size` crop |
| `oversample_size` clamped to `max_capture_size` | Text may extend past the oversample; log `[adaptive] clipped`; send best-effort union crop |
| Ctrl+Shift+Click (full-desktop mode); `detect()` finds nothing near lens | Send full `lens_size` crop centered on cursor; still force-remote backend |

---

## 8. Open Questions

1. **Model accuracy on manga** — DBNet (`stabrise-text_detection_dbnet_ml_v02_model.onnx`) is
   trained on multi-language printed text; its accuracy on manga-style hand-lettered text and
   vertical Japanese columns is unproven.  Manual QA is required: enable detection, capture a
   representative manga page, and verify that detected bounding boxes align with speech bubbles
   and inline text.  If quality is poor, fine-tuning DBNet on Manga109 annotations is the
   next option.

2. **Multiple crops vs one merged request** — Sending N crops = N API calls = N round trips. Alternative: pack multiple crops into a single request as a multi-image message (check Gemini API multi-image support). Could reduce latency at the cost of more complex parsing.

3. **Vertical text direction** — DBNet bbox orientation is axis-aligned; vertical Japanese columns may be merged into one tall narrow box. Verify this works well with how Gemini handles narrow vertical crops.

4. **Minimum crop size threshold** — Very small rects (furigana, single kanji) may produce worse results than the full image. Tune `text_detection_threshold` and `TextCropper::min_area` empirically.

5. **Gemma 4 E2B / E4B as on-device OCR backend** — See §11 below.

6. **`ocr/mod.rs` currently declares non-existent modules** — `image_handling`, `ocr_gcloud`, `ocr_tesseract`, `ocr_traits`, `ocr_winmedia` are all listed but the files don't exist on disk. This causes compile errors. The fix (remove stubs, add `text_detection` and `text_cropper`) is tracked in §10.

---

## 9. Testing Plan

### Unit tests

**`text_detection.rs`**
- `compute_union_bbox` — empty input returns `None`; single box returns itself; overlapping boxes return their union; non-overlapping boxes span all four extremes
- `merge_overlapping_boxes` — empty input returns empty; single box returns itself unchanged; two non-overlapping boxes returned as-is; two overlapping boxes collapsed into one union; three-way chain (A∩B, B∩C but not A∩C) collapses all three in second pass

**`text_cropper.rs`** (primary OCR dispatch path)
- `TextCropper::crop`: empty `boxes` → empty vec; padding applied correctly; clamped at image boundary; boxes below `min_area` filtered

**`adaptive_capture.rs`** (oversample approach)
- `text_fits_in_lens`: oversample captures text; union well inside the central `lens_size` rect; crop is tight; `screen_origin` equals oversample top-left + padded union position
- `text_larger_than_lens`: white rect spans from center to right edge of oversample; union.x2 < oversample edge → included; `clipped = false`; crop is wider than `lens_size`
- `text_clips_oversample_edge`: white rect touches right edge of oversample image; `clipped = true`; crop is still returned (best-effort, text is cut off)
- `no_boxes_near_cursor`: all detected boxes are in corners, outside the `lens_size` filter rect; `primary_boxes` is empty; returns central `lens_size` crop
- `degenerate_union (< 16×16)`: falls back to central `lens_size` crop
- `oversample_size_formula`: `lens_size=400, factor=2.0` → `oversample=800`; `lens_size=200, factor=2.0` → `oversample=640` (clamped to DBNet min); `lens_size=900, factor=2.0` → `oversample=1600` (clamped to max)

**`remap_coords`**
- Positive offset: `"10,20"` + origin `(50, 30)` → `"60,50"`
- Zero offset: coords unchanged
- Negative offset (crop origin left/above cursor center): correct subtraction
- `None` top_xy / bot_xy: not modified (no panic)

**`config.rs`**
- Old config without any `text_detection_*` fields deserializes with all defaults
- `text_detection_model: null` → `Option::None` in Rust
- Three new adaptive capture fields default to `2.0` / `1600` / `16`

### Integration tests

- Load `stabrise-text_detection_dbnet_ml_v02_model.onnx` + a real capture PNG; verify `DbNetDetector::detect()` returns `Vec<TextBoundingBox>` without panic
- `adaptive_capture` with a synthetic 800×800 image containing a white rectangle at center: confirm `primary_boxes` contains the box; confirm returned crop is tighter than the full 800×800
- End-to-end with `wiremock` stub: `text_detection_model` set → union crop posted; size smaller than `lens_size × lens_size`; `screen_origin` correct

### Manual QA

- Shift+Click on a single kanji: confirm OCR receives a tight crop (log shows `[adaptive] crop: WxH`)
- Shift+Click on a long subtitle that extends beyond the lens but within the oversample: confirm crop is wider than `lens_size`, `clipped = false`
- Shift+Click where text exceeds the oversample ceiling: confirm `[adaptive] clipped` log line appears
- Shift+Click on empty area: confirm `[adaptive] no text near cursor — using lens crop` log line
- **Ctrl+Shift+Click**: confirm lens window hides before capture, full desktop captured, nearest text found, window reappears

---

## 10. Implementation Sequence

1. **Fix `ocr/mod.rs`** — remove the five non-existent stub module declarations; replace with `pub mod text_detection;` and `pub mod text_cropper;`
2. **Add deps to `Cargo.toml`**:
   - Always: `anyhow = "1.0"`
   - Under `[features] onnx`: `ort = { version = "2.0.0-rc.12", features = ["ndarray"] }`, `ndarray = "0.15"`, `imageproc = "0.25"`
   - Build: `cargo build -p lenzu --features onnx` (see §11 for why no system libs needed)
3. **Write `text_detection.rs`** — `TextBoundingBox`, `compute_union_bbox`, `TextDetector` trait, `DbNetDetector`, `build_text_detector`; unit tests for `compute_union_bbox`
4. **Write `text_cropper.rs`** — `CroppedRegion`, `TextCropper`; unit tests; this is the primary OCR dispatch path (one crop per DBNet box)
5. **Write `adaptive_capture.rs`** — `AdaptiveCaptureResult`, `AdaptiveCaptureConfig`, `adaptive_capture()`; unit tests per §9
6. **Add config fields** — five fields to `AppConfig` with `#[serde(default)]` per §5
7. **Implement `remap_coords`** in `utils.rs`
8. **Wire Shift+Click path in `main.rs`** — replace `capture_x11` + encode with `adaptive_capture()`; add `remap_coords` call; handle `result.clipped` log
9. **Wire Ctrl+Shift+Click path in `main.rs`** — hide lens → full-desktop `capture_x11` → DBNet nearest-box → show lens → force-remote OCR (per §4.8)
10. **Implement `save_debug_image` in `utils.rs`** — annotated PNG to `/dev/shm/lenzu/debug_detection.png`; gated on `text_detection_debug`; ~20 lines using `imageproc::drawing` (already a planned dep)
11. **Wire Cairo box overlay in `main.rs` draw callback** — store `last_detection_boxes: Vec<TextBoundingBox>` in `AppState`; draw green outlines when `text_detection_debug = true` (per §4.9)
12. **Remove `detect_text()` from `DualOcrClient`** — detection now lives in `adaptive_capture.rs`; simplify `DualOcrClient::new` signature
13. **Manual QA** — set `text_detection_debug: true`; Shift+Click a manga panel; inspect `/dev/shm/lenzu/debug_detection.png` and lens window overlays; tune `text_detection_threshold` and `oversample_factor`

---

## 11. Dependencies and System Libraries

### Is OpenCV required?

**No.** The DBNet inference pipeline uses only pure-Rust crates:

| Task | Library | Installed how |
|---|---|---|
| ONNX model inference | `ort` (Rust crate wrapping ONNX Runtime C API) | Cargo downloads ONNX Runtime binaries automatically at build time |
| N-dimensional arrays | `ndarray` | Cargo (pure Rust) |
| Resize / pixel access | `image` | Cargo (already a dep, pure Rust) |
| Contour finding on binary mask | `imageproc` | Cargo (pure Rust) |

OpenCV is a C++ library that provides all of the above plus much more. Since we only need
the ONNX inference + image manipulation subset, and Rust crates cover it, OpenCV is not a
dependency and does not need to be installed.

### `ort` and ONNX Runtime

The `ort` crate v2.0.0-rc.12 (same version already used in `prototypes/x11-gtk3-lens-test`)
downloads the pre-built ONNX Runtime shared library from GitHub releases during the
`cargo build` step. No `apt install` is required.

Required for the download to succeed at build time:
- Internet access (for first build; artefact is cached by Cargo after that)
- `libstdc++` / `glibc` — already present on any Debian/Ubuntu system

### `setup.sh` changes needed

The `cargo build -p lenzu` line in `setup.sh` must pass `--features onnx` to compile the
detection code.  Since the DBNet model is already committed to `assets/`, we always enable
the feature:

```bash
# Was:
cargo build -p lenzu

# Becomes:
cargo build -p lenzu --features onnx
```

The `run.sh` script (and any CI build commands) need the same update.

### Summary: no new `apt` packages needed for ONNX/DBNet

The existing `apt install` block in `setup.sh` already covers all system deps.  The only
change is the `--features onnx` flag on the Cargo build invocation.

---

## 12. Gemma 4 E2B / E4B — On-Device OCR Backend

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
Current:     DBNet (local) → crop → OpenRouter/Gemini (remote, paid)
With Gemma:  DBNet (local) → crop → Gemma 4 E2B (local, free after download)
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
- Same `ort` dependency already used for the DBNet text detector
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

There are two paths to the OpenRouter fallback:

**Automatic** — Gemma's response meets any of these conditions:

| Condition | Meaning |
|---|---|
| `Err(_)` returned | HTTP error, timeout, connection refused, or unparseable JSON |
| `Ok(vec![])` returned | Gemma found no text in the image |
| All results have `english: None` or `english: Some("")` | OCR succeeded but translation was skipped or refused |

A partial result (some items have `english`, some don't) does **not** trigger a full fallback — the items without translation are simply presented as-is. The fallback is a per-capture decision, not per-result.

**Manual — `Ctrl+Shift+Click`** — user explicitly forces the remote backend:
- Skips `DualOcrClient` primary path entirely; calls OpenRouter directly
- Useful for comparing Gemma vs Gemini output, or when Gemma is slow/unavailable
- Requires `OPENROUTER_API_KEY` set; displays an error message in the HUD if the key is absent
- Detected in `main.rs` input handler (around line 438) by adding `CONTROL_MASK` to the existing `SHIFT_MASK + BUTTON1_MASK` check
- Existing `Shift+Click` (no Ctrl) always uses the primary (Gemma) path as before

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

OCR does not need colour — text contrast is carried entirely by luminance. Converting to grayscale immediately after capture is essentially free (sub-millisecond) and benefits all backends by removing irrelevant colour information that can confuse VLM attention heads. Two preprocessing steps are applied, but at different scopes:

1. **Grayscale** — applied to **all captures** (both local Gemma and remote OpenRouter). Converting RGB → Luma8 reduces PNG size by roughly **⅓** and focuses the model on luminance contrast only.
2. **Proportional downscale** — applied **only before fallback (OpenRouter) calls**. The remote call is billed per token and vision tokens scale with image byte size. If either dimension exceeds `fallback_max_dimension`, the image is resized (maintaining aspect ratio) before encoding. Fewer pixels = smaller PNG = fewer tokens.

Local inference (Gemma/ollama) receives a **grayscale, full-resolution** image. Remote inference (OpenRouter) receives a **grayscale, downscaled** image.

### Token impact estimate

| Image | Path | Format | Approx PNG bytes | Approx tokens |
|---|---|---|---|---|
| 400×400 lens capture | (original) | RGB colour | ~60 KB | ~800–1,200 |
| 400×400 lens capture | Local (Gemma) | Grayscale, full res | ~20 KB | ~270–400 |
| 400×400 lens capture | Remote (OpenRouter) | Grayscale + scaled ≤800px | ~12 KB | ~160–240 |

Savings on the fallback call: **70–80%** vs. the original full-colour send. Local call benefits from grayscale alone (~66% reduction) at zero token cost.

### Implementation — `utils.rs`

Three new public functions alongside the existing `encode_to_base64`:

```rust
use image::{DynamicImage, ImageFormat, imageops::FilterType};

/// Convert raw BGRA bytes to a DynamicImage (used at capture time before encoding).
pub fn raw_to_dynamic_image(raw: &[u8], w: u32, h: u32) -> DynamicImage {
    let rgb = raw_to_rgb(raw);
    let buf = image::ImageBuffer::<image::Rgb<u8>, _>::from_raw(w, h, rgb)
        .expect("raw_to_dynamic_image: dimensions mismatch");
    DynamicImage::ImageRgb8(buf)
}

/// Encode a DynamicImage as grayscale PNG → base64.
/// Applied to ALL captures (both local and remote) immediately after capture.
pub fn encode_as_grayscale(image: &DynamicImage) -> String {
    let gray = image.grayscale();
    let mut buf = std::io::Cursor::new(Vec::new());
    gray.write_to(&mut buf, ImageFormat::Png).unwrap();
    base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
}

/// Encode a DynamicImage as grayscale + proportionally downscaled PNG → base64.
/// `max_dim`: longest edge limit in pixels; 0 = no limit.
/// Applied ONLY before remote (OpenRouter/fallback) calls to reduce token cost.
pub fn encode_for_fallback(image: &DynamicImage, max_dim: u32) -> String {
    let gray = image.grayscale();

    // Proportional downscale if over the limit
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

    let mut buf = std::io::Cursor::new(Vec::new());
    scaled.write_to(&mut buf, ImageFormat::Png).unwrap();
    base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
}
```

`DynamicImage::grayscale()` and `resize()` are in the core `image` crate — no new dependencies. The `png` feature already enabled in `Cargo.toml` covers encoding.

### Interface change for `DualOcrClient`

To apply different encoding for primary vs fallback, `DualOcrClient::call_api` accepts a `&DynamicImage` instead of a pre-encoded `&str`. It encodes internally, applying grayscale to both paths and downscale only to the fallback path:

```rust
impl DualOcrClient {
    /// `image`: the captured lens region as a DynamicImage (raw RGB, full resolution).
    pub fn call_api(&self, image: &DynamicImage) -> Result<Vec<TranslationResult>, Box<dyn std::error::Error>> {
        // Primary: grayscale, full resolution — colour info not needed for OCR
        let primary_b64 = encode_as_grayscale(image);

        match self.try_primary(&primary_b64) {
            Ok(results) if !self.needs_fallback(&results) => Ok(results),
            primary_result => {
                if let Err(ref e) = primary_result {
                    eprintln!("[OCR] primary failed ({e}) — trying fallback");
                } else {
                    eprintln!("[OCR] primary gave no translations — trying fallback");
                }
                // Fallback: grayscale + downscale to cut remote token cost
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
  // Preprocessing applied to ALL captures (local + remote):
  //   grayscale conversion is always-on and has no config knob.

  // Additional preprocessing applied ONLY before fallback (OpenRouter) calls:
  "fallback_max_dimension": 800            // default: 800; set 0 to disable downscaling
}
```

Added to `AppConfig` with `#[serde(default)]`. Default of 800px means the 400×400 lens capture is untouched (already under limit), but large crops from pre-detection (Phase 4 §4) are still bounded.

> **Why remove `fallback_preprocess_grayscale` as a config knob?** Grayscale now applies to all captures, so it is no longer specific to the fallback path. Keeping a toggle would imply it could be disabled for primary too, which is not the intent. If a future use case needs per-path control, this can be re-introduced then.

### `FallbackPreprocess` struct

```rust
#[derive(Debug, Clone)]
pub struct FallbackPreprocess {
    pub max_dimension: u32,    // 0 = no limit; applies only to fallback (remote) calls
}

impl Default for FallbackPreprocess {
    fn default() -> Self {
        Self { max_dimension: 800 }
    }
}
```

Grayscale is always applied at capture time (both paths) and is not stored here. `FallbackPreprocess` tracks only the downscale limit, which is fallback-specific. Stored in `DualOcrClient`; read from `AppConfig` at construction time in `main.rs`.

### Interaction with YOLO pre-detection (§4)

When pre-detection is active, multiple small crops are sent individually. Each crop goes through the same dual-backend path — grayscale is applied to all crops before Gemma (local) and before OpenRouter (remote); downscale is applied only for the remote path. Since crops are already small (typically 60–200px wide), the `max_dimension` cap rarely triggers, but grayscale still cuts their size by ~⅔ in both paths.

### Debug output

Save the preprocessed fallback image to `/dev/shm/debug_lens_fallback.png` alongside the existing `/dev/shm/debug_lens.png`, so the two can be compared side-by-side during development.

---

## 14. Unit & Integration Tests

### Lesson from Phase 3

The Electron HUD migration (Phase 3) uncovered cascading issues — ghosting artefacts, UDP message timing, IPC race conditions — that were caught late through manual QA. Unit tests on the boundary contracts (message format, port negotiation, lifecycle ordering) would have surfaced these earlier. Phase 4 introduces three new code boundaries (`DualOcrClient`, `encode_as_grayscale`/`encode_for_fallback`, `TextDetector`) that each need tests before integration.

### Coverage map

| Module | New code | Test location |
|---|---|---|
| `utils.rs` | `raw_to_dynamic_image`, `encode_as_grayscale`, `encode_for_fallback` | `utils.rs #[cfg(test)]` |
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

    // ── encode_as_grayscale — used for ALL captures (local + remote primary) ──

    #[test]
    fn test_encode_as_grayscale_produces_grayscale_png() {
        let mut img = image::RgbImage::new(4, 4);
        for p in img.pixels_mut() { *p = image::Rgb([0, 128, 255]); } // solid blue
        let dyn_img = DynamicImage::ImageRgb8(img);

        let b64 = encode_as_grayscale(&dyn_img);
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(
            matches!(decoded, DynamicImage::ImageLuma8(_) | DynamicImage::ImageLumaA8(_)),
            "encode_as_grayscale must produce a grayscale PNG"
        );
    }

    #[test]
    fn test_encode_as_grayscale_preserves_dimensions() {
        let img = DynamicImage::new_rgb8(100, 80);
        let b64 = encode_as_grayscale(&img);
        let bytes = base64::engine::general_purpose::STANDARD.decode(&b64).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), 100);
        assert_eq!(decoded.height(), 80);
    }

    #[test]
    fn test_encode_as_grayscale_smaller_than_colour() {
        let img = DynamicImage::new_rgb8(400, 400);
        let colour_b64 = encode_to_base64(&img.to_rgb8().into_raw(), 400, 400);
        let gray_b64 = encode_as_grayscale(&img);
        assert!(
            gray_b64.len() < colour_b64.len(),
            "grayscale b64 ({}) should be smaller than colour b64 ({})",
            gray_b64.len(), colour_b64.len()
        );
    }

    // ── encode_for_fallback — grayscale + downscale (remote only) ────────────

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
    fn test_encode_for_fallback_is_smaller_than_primary() {
        // 400x400 image — fallback (grayscale+downscale) must be smaller than primary (grayscale only)
        let img = DynamicImage::new_rgb8(400, 400);
        let primary_b64 = encode_as_grayscale(&img);
        let fallback_b64 = encode_for_fallback(&img, 300); // force downscale
        assert!(
            fallback_b64.len() < primary_b64.len(),
            "fallback b64 ({}) should be smaller than primary b64 ({}) after downscale",
            fallback_b64.len(), primary_b64.len()
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

    // Use a larger image so the size difference is visible.
    // primary gets: grayscale, full-resolution (400×400)
    // fallback gets: grayscale + downscaled to max_dimension (default 800, so 400×400 is
    //   under the limit here — use fallback_max_dimension=300 via new_for_test to force resize)
    let img = image::DynamicImage::new_rgb8(400, 400);
    let client = DualOcrClient::new_for_test_with_max_dim(
        &primary_server.uri(), &fallback_server.uri(), "sk", 300
    );
    let _ = client.call_api(&img);

    let primary_reqs = primary_server.received_requests().await.unwrap();
    let fallback_reqs = fallback_server.received_requests().await.unwrap();
    assert_eq!(primary_reqs.len(), 1);
    assert_eq!(fallback_reqs.len(), 1);

    let primary_body_len = primary_reqs[0].body.len();
    let fallback_body_len = fallback_reqs[0].body.len();
    assert!(
        fallback_body_len < primary_body_len,
        "fallback payload ({fallback_body_len}B) must be smaller than primary ({primary_body_len}B) \
         — both are grayscale; fallback is additionally downscaled"
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
| Primary path sends colour instead of grayscale | `test_encode_as_grayscale_produces_grayscale_png` |
| Grayscale dimensions change unexpectedly | `test_encode_as_grayscale_preserves_dimensions` |
| Fallback sends larger image than primary | `test_fallback_image_sent_is_smaller_than_primary` (integration) + `test_encode_for_fallback_is_smaller_than_primary` (unit) |
| Fallback grayscale conversion regresses to colour | `test_encode_for_fallback_produces_grayscale_png` |
| Aspect ratio broken by downscale | `test_encode_for_fallback_downscales_wide_image` + tall variant |
| Auth header sent to ollama (local) | `test_ocr_client_empty_key_omits_auth_header` |

---

## 15. Gemma-Based Detection Pass (YOLO-Free Interim Path)

### Motivation

The YOLO pre-detection path (§2–§4) requires a fine-tuned model for manga speech bubbles — the existing COCO-trained models have no `text` or `bubble` class. Fine-tuning requires training data (Manga109 balloon annotations) and a separate model file to ship with the application. Until that work is done, there is a simpler **interim path** that uses Gemma itself as the text-region detector, without YOLO.

### Design

The pipeline becomes a two-request sequence per capture, both served by `DualOcrClient`:

```
Shift+Click
  → capture lens → DynamicImage

  Pass 1 — Region detection (single request):
    → prompt: "Find all text regions. Return a JSON array of {top_xy, bot_xy} only.
               Do not read or transcribe the text. Return ONLY the JSON array."
    → model returns: [{top_xy:"x,y", bot_xy:"x,y"}, ...]

  Pass 2 — OCR per crop (one request per region, parallelizable):
    → for each rect from pass 1:
         crop DynamicImage to rect (+padding)
         encode_as_grayscale(crop)
         prompt: "Read the Japanese text in this image. Return JSON:
                  {original, furigana, english, romaji}"
         model returns: one TranslationResult
    → remap top_xy / bot_xy from crop-local → lens-local coords (same as §6)
    → collect all TranslationResults
```

### Why this is faster than the current single-request approach

| Step | Current | Gemma 2-pass |
|---|---|---|
| Images sent | 1 full image | 1 full image (pass 1) + N small crops (pass 2) |
| Output tokens (pass 1) | — | ~10–20 tokens (just coordinates) |
| Output tokens (pass 2) | All text + coords in one go | ~20–40 tokens per crop |
| Vision tokens (pass 2) | Full image × 1 | Crop (~100×50px) × N |
| VRAM pressure | Full image in KV cache | Tiny crops; short context per call |
| Parallelism | None | Pass 2 calls are independent → `tokio::spawn` |

The key saving is that each pass-2 crop is a **tiny image with a short output** — the vision encoder processes far fewer pixels, and the model produces fewer tokens. For a capture with 3 text regions, 3 parallel crop requests will typically complete faster than 1 request asking the model to read and locate everything at once.

### Comparison: YOLO vs Gemma for pass 1

| | YOLO/ONNX | Gemma prompt |
|---|---|---|
| Latency (pass 1) | ~30–80 ms (local CPU) | ~500ms–2s (LLM inference) |
| Model accuracy on manga | Low (COCO classes) → needs fine-tune | Reasonable (can reason about visual layout) |
| Required setup | ONNX model file on disk | Nothing new — already running |
| Network required | No | No (ollama is local) |
| Bounding box precision | Tight (pixel-level) | Approximate (model guesses, not measures) |
| When to prefer | After fine-tuned model is available | Now, as an interim path |

**Interim strategy**: ship Gemma-based detection for the initial release. Add YOLO when a manga-specific model is ready to swap in. The pass-2 crop-and-OCR logic is identical either way — only the source of `Vec<OcrRect>` changes.

### Prompt for pass 1

```
Find all regions in this image that contain text.
Return a JSON array of objects, one per text region.
Each object must have:
  top_xy  — upper-left corner of the region as string "x,y"
  bot_xy  — lower-right corner of the region as string "x,y"
Do NOT read or transcribe the text. Do NOT include any other fields.
Return ONLY the JSON array, no markdown fences.
```

This is a simpler task than OCR — the model only needs to draw rectangles, not recognize characters. The shorter output reduces inference time.

### Config knob

```jsonc
{
  "detection_pass": "gemma",    // "gemma" | "yolo" | "none"
                                // "gemma" = two-request pipeline (this section)
                                // "yolo"  = ONNX pre-detection (§4)
                                // "none"  = single full-image OCR (current behaviour)
}
```

`"gemma"` becomes the default once the two-pass plumbing is in place. `"yolo"` replaces it when a fine-tuned model is available. `"none"` is the fallback for minimal-install scenarios.

### Parallelism

Pass-2 requests are independent and can be dispatched concurrently. In `main.rs` the background thread would use `tokio::task::spawn` or `futures::future::join_all`:

```rust
let crop_futures: Vec<_> = rects.iter().map(|rect| {
    let r = rect.padded(8, w, h);
    let crop = dyn_image.crop_imm(r.x1, r.y1, r.width(), r.height());
    let client = client.clone();  // Arc<DualOcrClient>
    tokio::spawn(async move {
        let b64 = encode_as_grayscale(&crop);
        let mut results = client.call_api_b64(&b64)?;
        remap_coords(&mut results, r.x1 as i64, r.y1 as i64);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(results)
    })
}).collect();
let all_results: Vec<TranslationResult> = futures::future::join_all(crop_futures)
    .await
    .into_iter()
    .flatten()
    .flatten()
    .collect();
```

**Note**: ollama processes requests sequentially (one model, one GPU). Parallel requests will queue internally. The concurrency benefit is real only if a remote backend is used for pass 2 (OpenRouter handles parallel requests). With a local ollama backend, serial dispatch is equivalent.

---

## 16. Smoke Test: Dual-Backend Testing (`scripts/test-ocr.sh`)

### What changed (2026-04-05)

`scripts/test-ocr.sh` was updated to test both the local-ollama and remote-OpenRouter backends in a single run, with separate timing for each.

### Invocation

```bash
# Test both backends (default):
./scripts/test-ocr.sh

# Test local only:
./scripts/test-ocr.sh --skip-remote

# Test remote only:
./scripts/test-ocr.sh --skip-ollama --remote-key "$OPENROUTER_API_KEY"

# Custom models:
./scripts/test-ocr.sh --model gemma4:e4b --remote-model google/gemini-2.0-flash-001
```

If `OPENROUTER_API_KEY` is not set and `--remote-key` is not passed, the remote backend is **skipped with a notice** (not a failure) — local-only setups are not penalized.

### Coordinate checks are informational only

The expected JSON (`assets/Unit-test-sample-texts.json`) stores bounding-box coordinates for the original 2816×1536 source image. The model receives a scaled-down copy (default: 640px longest edge) — a scale factor of ~4.4×. Even at the same scale, VLMs do not reliably reproduce exact pixel coordinates; they approximate spatial layout. Coordinates are therefore printed as `[WARN]` lines and do not contribute to the pass/fail count.

**The only checked assertions are:**
1. HTTP 200 response
2. Response parses as a JSON array
3. Expected text substrings are present in the results

### Performance tuning applied

Two ollama-specific options reduce VRAM pressure on cards with limited headroom:

| Parameter | Previous | Current | Reason |
|---|---|---|---|
| `max_dim` (image longest edge) | 896 px | **640 px** | Smaller image = smaller vision encoder activation = less VRAM |
| `options.num_ctx` | (unset, ~4096) | **2048** | Smaller KV cache = more VRAM free for computation |

These are passed in the `options` block of the ollama API request. OpenRouter ignores unknown `options` fields, so the same request JSON is safe to send to both backends.

### Measured effect (Quadro M4000, 8 GB VRAM, gemma4:e2b)

| Configuration | Inference time |
|---|---|
| 896 px, default num_ctx, CPU (linuxbrew) | ~4 minutes |
| 896 px, default num_ctx, GPU (official binary) | ~28 s |
| 640 px, num_ctx=2048, GPU | target < 10 s (to be measured) |

The Quadro M4000 has ~389 MB VRAM remaining after the model loads (7.9 GB model in 8 GB VRAM). Vision encoder activations for a 640px image consume less of that headroom than a 896px image.

### Adding a future `--two-pass` flag

Once the Gemma detection pass (§15) is implemented, `test-ocr.sh` should gain a `--two-pass` flag that:
1. Sends pass-1 (bounding-box only) request and prints detected rects
2. For each rect, sends a crop and verifies the expected text is present in that crop's response
3. Reports timing for each pass separately
