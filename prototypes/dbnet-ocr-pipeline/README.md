# dbnet-ocr-pipeline

Fully offline Japanese OCR pipeline combining two crates:

1. **[`jp_detect`](https://crates.io/crates/jp_detect) >= 0.2.2** (DBNet) — locates text bounding boxes with per-box confidence scores
2. **[`manga-ocr-rs`](https://crates.io/crates/manga-ocr-rs) >= 0.1.1** — recognizes Japanese text from each cropped region with confidence scores

No LLM, no cloud API. Raw Japanese text output.

### Confidence-gated pipeline

Both crates now expose confidence scores (0.0-1.0). The pipeline uses these to:
- **Retry detection** at 0.5x / 1.5x image scales when detection confidence is low (<= 70%)
- **Truncate OCR output** at 32 characters when OCR confidence is low (<= 70%) and the text is long (garbage guessing)
- Report both `detect_confidence` and `ocr_confidence` in JSON output

## Usage

```bash
cargo run -p dbnet-ocr-pipeline -- <image> [options]

# Options:
#   --json                output results as JSON array
#   --threshold <0.0-1.0> DBNet confidence threshold (auto-scaled by image size)
#   --dilation <pixels>   DBNet dilation radius (auto-scaled by image size)
#   --pad <pixels>        crop padding in each direction (auto-scaled by image size)
#   --save-boxes <path>   save image with bounding box overlay
#   --save-crops <dir>    save individual crop images to directory
```

## Results

### Single panel crop (187x286)

Auto-scaled params: threshold=0.2, dilation=16, pad=32 (longest edge 286 — small crop bucket)

![Panel bounding boxes](docs/panel-boxes.png)

The entire panel is detected as one box. OCR result: `最近人気のデスクトップなリナックスです!` — **perfect**.

For small crops (lens-sized captures), high dilation merges nearby character blobs into
a single region, which is exactly what we want — one box per speech bubble.

### Full manga page (1246x1635)

Auto-scaled params: threshold=0.35, dilation=6, pad=16 (longest edge 1635 — medium bucket)

![Full page bounding boxes](docs/fullpage-boxes.png)

9 boxes detected. Results by box:

| Box | Size | Det % | OCR % | OCR text | Pass? | Notes |
|-----|------|-------|-------|----------|-------|-------|
| [0] red | 672x581 | low | low | `いや、いや...いやいやぁっ、こういう最近人気の...` (hallucinated) | No | Merged multiple panels; both scores below gate |
| [1] green | 313x523 | high | high | `ああたしのオススメはうぶんちゅ` | Yes | Clean single bubble, both scores above 71% |
| [2] blue | 117x106 | mid | mid | `その` | Partial | Small fragment; detection confident but OCR uncertain |
| [3] orange | 86x150 | high | high | `却下!` | Yes | Tight single-word box, high confidence both |
| [4] magenta | 236x218 | mid | low | `よけんなこのっ!!!...` (hallucinated) | No | Mixed art/text; OCR confidence collapsed |
| [5] cyan | 227x195 | mid | low | `いモリなからケーカしないで~~~っ!!!...` (hallucinated) | No | Too much content; OCR score below gate, truncated at 32 chars |
| [6] red | 168x124 | mid | low | `マジいてんんだぞ!!!...` (hallucinated) | No | OCR hallucination flagged by low confidence |
| [7] green | 162x254 | high | high | `一瞬くらい検討してくださいよー!` | Yes | Clean bubble, both scores well above 71% |
| [8] blue | 145x126 | low | low | (hallucinated) | No | Background art misdetected; both scores below gate |

**Det %** = jp_detect per-box confidence (mean probability of thresholded pixels).
**OCR %** = manga-ocr-rs dimension-adjusted confidence. **Pass** = both >= 71% (the confidence gate used by lenzu's local-first pipeline).

The pattern is clear: boxes that pass both confidence gates ([1], [3], [7]) are the accurate ones.
Boxes with low detection or OCR confidence reliably indicate merged regions or hallucinated output.

Individual crops (generated via `--save-crops`):

| crop_01 (box [1]) | crop_03 (box [3]) | crop_07 (box [7]) |
|:-:|:-:|:-:|
| ![crop_01](docs/full-page-crops/crop_01.png) | ![crop_03](docs/full-page-crops/crop_03.png) | ![crop_07](docs/full-page-crops/crop_07.png) |
| `ああたしのオススメはうぶんちゅ` | `却下!` | `一瞬くらい検討してくださいよー!` |

## Discoveries

### Auto-scaling detection parameters by image size

DBNet always runs at 640x640 internally. A dilation of N pixels at 640-res represents
`N * (original / 640)` pixels in the original image — much more blur for large inputs.
Parameters must scale inversely with image size.

The pipeline uses the same lookup table as the main lenzu app:

| Longest edge | Dilation | Threshold | Pad | Use case |
|-------------|----------|-----------|-----|----------|
| <= 800      | 16       | 0.20      | 32  | Small lens crops — merge nearby character blobs |
| <= 1280     | 10       | 0.25      | 24  | Medium screenshots |
| <= 1920     | 6        | 0.35      | 16  | Full HD captures, manga pages |
| <= 2560     | 3        | 0.45      | 12  | 2K/QHD |
| > 2560      | 0        | 0.50      | 8   | 4K — almost no dilation needed |

CLI flags `--threshold`, `--dilation`, `--pad` override the auto-scaled values.

### Small crops: high dilation works perfectly

For small images (lens-sized captures, single panels), dilation=16 merges all character
blobs in a speech bubble into one box. manga-ocr-rs handles the full bubble as a single
input and produces accurate results. The 187x286 panel crop was recognized perfectly as
one box.

### Large images: merged boxes cause hallucination

For full manga pages, even dilation=6 can merge neighboring speech bubbles or capture
art alongside text. When a crop contains too much content (multiple text columns, mixed
art and text), `manga-ocr-rs` generates endless filler Japanese text — hundreds of characters
of grammatically plausible but meaningless output.

This is a known limitation of the ViT+BERT architecture: the beam search decoder has no
length constraint and will fill the output when the input is ambiguous.

**Key insight**: the OCR model works well when each crop contains a single, tight text region.
The quality bottleneck is detection precision, not OCR accuracy.

### Confidence scores predict accuracy

From the full-page run:
- **High-confidence boxes** ([1], [3], [7]): both detection and OCR scores above 71% — all produced accurate text
- **Low-confidence boxes** ([0], [4], [5], [6], [8]): at least one score below 71% — all were hallucinated or partial

The confidence gate at 71% cleanly separates accurate results from garbage. This is the
threshold used by lenzu's local-first pipeline to decide whether manga-ocr-rs output can be
trusted or must fall through to the LLM chain.

Size alone doesn't predict accuracy — a large box with a single speech bubble scores high,
while a small box that overlaps art scores low. The confidence scores capture this nuance.

### Performance (CPU, no GPU acceleration)

| Stage | Time |
|-------|------|
| DBNet detection | ~800-1800 ms |
| manga-ocr model load | ~1100-1300 ms |
| OCR per crop | ~1-42 s (varies with content) |
| Full page (9 crops) | ~240 s total |
| Single panel (1 crop) | ~44 s total |

OCR is the bottleneck — beam search decoding is CPU-intensive. Detection is fast (~1-2s).
For production use, ONNX GPU execution providers (CUDA, TensorRT) would reduce per-crop
latency dramatically.

### lenzu integration (done)

The confidence-gated pipeline is now integrated into `lenzu_client`:

- Both Shift+Click (lens) and Ctrl+Shift+Click (fullscreen) paths try local OCR first
- Detection confidence >= 71% AND OCR confidence >= 71% => result returned immediately, no LLM
- Below the gate => falls through to Ollama -> local fallbacks -> free remote -> paid remote
- manga-ocr-rs models are loaded once at startup and shared across threads via `Arc`

### Remaining optimizations

1. **GPU acceleration** — enable ONNX CUDA EP for both jp_detect and manga-ocr-rs
2. **Parallel OCR** — process crops concurrently (models are thread-safe)
3. **Panel segmentation** — split manga pages into panels before detection for tighter boxes
