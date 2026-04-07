# dbnet-test — DBNet text detection prototype

Standalone prototype for validating DBNet ONNX inference before integrating into `lenzu`.

## Usage

```bash
# Run both built-in test cases
cargo run -p dbnet-test -- --test [model] [threshold] [dilation] [pad_x] [pad_y]

# Run on any image
cargo run -p dbnet-test -- <image_path> [model] [threshold] [dilation] [pad_x] [pad_y]
```

**Defaults**

| param       | default                                          | description |
|-------------|--------------------------------------------------|-------------|
| `model`     | `assets/stabrise-text_detection_dbnet_ml_v02_model.onnx` | ONNX model; bare filename resolved against `assets/` |
| `threshold` | `0.2`                                            | Probability map cut-off (lower = more pixels detected) |
| `dilation`  | `16`                                             | Binary mask expansion radius at 640×640 scale; merges nearby text blobs |
| `pad_x`     | `32`                                             | Pixels added to each horizontal side of every box, at original-image scale |
| `pad_y`     | `32`                                             | Pixels added to each vertical side of every box, at original-image scale |

**Tuned values** that gave the best results on the sample assets (as of prototype):

```bash
cargo run -p dbnet-test -- --test stabrise-text_detection_dbnet_ml_v02_model.onnx 0.2 16 32 32
```

Output PNGs are written to `/dev/shm/lenzu/`.

## Built-in test cases (`--test`)

| Case | Image | Simulates |
|------|-------|-----------|
| `3-texts` | `assets/Unit-test-sample-texts.png` | Lens-crop: text fills most of the frame |
| `fullscreen` | `assets/OCR-Demo-JP2EN.png` | Desktop capture: real game screenshot with multiple text regions |

## Pipeline

```
DynamicImage
  │
  ▼ resize_exact(640×640, Triangle filter)
  │ normalize per channel: (px − mean) / std   ← ImageNet constants
  │ layout: CHW float32 tensor [1, 3, 640, 640]
  │
  ▼ ONNX inference (DBNet)
  │
  ▼ probability map [1, 1, 640, 640]  (each pixel = P(text))
  │
  ▼ threshold → binary GrayImage
  │
  ▼ morphological dilation (Norm::L1, radius = dilation param)
  │   compensates for DBNet's slightly-shrunk training targets;
  │   also merges character-level blobs into word/line-level regions
  │
  ▼ find_contours::<u32>  (imageproc)
  │   skip contours with < 4 points (stray noise)
  │
  ▼ AABB per contour → scale back to original-image coordinates
  │   pad_x / pad_y added to each side, clamped to image bounds
  │
  ▼ union merge (overlapping box collapse)
  │   iteratively merges any two boxes that overlap until none remain;
  │   each pair → take upper-left-most (x,y) and bottom-right-most (x+w, y+h)
  │   → single enclosing box
  │
  ▼ Vec<BBox> sorted top-to-bottom, left-to-right
```

## Union merge algorithm

DBNet occasionally splits a single text region into multiple overlapping boxes
(e.g. one per character row for vertical text, or due to uneven probability mass).
After padding is applied, these boxes often overlap. The union merge pass collapses
them:

```rust
fn overlaps(a, b) -> bool {
    a.x < b.x + b.w && a.x + a.w > b.x &&
    a.y < b.y + b.h && a.y + a.h > b.y
}

fn union(a, b) -> BBox {
    x      = min(a.x, b.x)
    y      = min(a.y, b.y)
    right  = max(a.x + a.w, b.x + b.w)
    bottom = max(a.y + a.h, b.y + b.h)
    BBox { x, y, w: right − x, h: bottom − y }
}
```

The loop runs until a full pass produces zero merges (typically 1–2 passes for
a handful of boxes). The result is re-sorted top-to-bottom.

## Why these parameter values?

- **threshold 0.2** — lower than the DBNet paper's 0.3 default; the StabRise
  multi-language model produces sparser probability maps on JP text than on
  Latin script, so a lower threshold recovers more of the text mass.
- **dilation 16** — at 640×640 scale this is ~4% of the image width; enough to
  merge per-character blobs for vertical Japanese columns without bleeding across
  separate text regions.
- **pad 32×32** — applied at original-image scale after dilation; provides the
  ascender/descender headroom that DBNet crops tightly around. The asymmetry
  (more vertical than horizontal) was validated empirically: Japanese text is
  taller relative to its probability mass than wide.

## Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Full pipeline implementation |
| `../../assets/stabrise-text_detection_dbnet_ml_v02_model.onnx` | DBNet model (~4.7 MB) |
| `../../assets/Unit-test-sample-texts.png` | 3-texts lens-crop test image |
| `../../assets/OCR-Demo-JP2EN.png` | Fullscreen game screenshot test image |
