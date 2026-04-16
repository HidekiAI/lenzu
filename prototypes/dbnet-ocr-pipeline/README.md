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

| Box | Size | Det % | OCR % | OCR text | Time | Pass? | Notes |
|-----|------|-------|-------|----------|------|-------|-------|
| [0] red | 672x581 | 99% | 13% | `いや、いや...いやいやぁっ、こういう最近人気の...` (hallucinated) | 29 s | No | Merged multiple panels; OCR hallucinated, truncated at 32 chars |
| [1] green | 313x523 | 99% | 90% | `ああたしのオススメはうぶんちゅ` | 1.7 s | Yes | Clean single bubble, both scores well above 71% |
| [2] blue | 117x106 | 84% | 74% | `その` | 0.8 s | **False positive** | DBNet false positive — character hair/line art resembles katakana strokes; manga-ocr-rs hallucinates `その` at 74% (barely above 71% gate) |
| [3] orange | 86x150 | 97% | 100% | `却下!` | 24 s | Yes | Tight single-word box, perfect OCR confidence |
| [4] magenta | 236x218 | 97% | 8% | `よけんなこのっ!!!...` (hallucinated) | 27 s | No | Mixed art/text; OCR confidence collapsed, truncated |
| [5] cyan | 227x195 | 97% | 16% | `いモリなからケーカしないで~~~っ!!!...` (hallucinated) | 39 s | No | Too much content; OCR hallucinated, truncated at 32 chars |
| [6] red | 168x124 | 91% | 8% | `マジいてんんだぞ!!!...` (hallucinated) | 28 s | No | OCR hallucination flagged by low confidence, truncated |
| [7] green | 162x254 | 97% | 98% | `一瞬くらい検討してくださいよー!` | 39 s | Yes | Clean bubble, both scores well above 71% |
| [8] blue | 145x126 | 81% | 16% | (hallucinated) | 38 s | No | Background art misdetected; OCR below gate, truncated |

**Det %** = jp_detect per-box confidence (mean probability of thresholded pixels).
**OCR %** = manga-ocr-rs dimension-adjusted confidence. **Pass** = both >= 71% (the confidence gate used by lenzu's local-first pipeline).

The pattern is clear: boxes that pass both confidence gates with high scores ([1], [3], [7])
produce accurate text. Box [2] is a **false positive** — DBNet detects character hair/line art
as text (the high-contrast strokes resemble katakana), and manga-ocr-rs hallucinates `その`
at 74% confidence, barely clearing the 71% gate. This is a known limitation of DBNet on
manga art where character hair, speed lines, and hatching patterns create text-like edge
gradients. Raising the gate to ~80% would filter it, at the cost of also filtering legitimate
small fragments.

High-confidence crops complete in 0.8–1.7 s. Low-confidence crops take 25–40 s because the
decoder runs away generating garbage — the confidence gate catches these reliably.

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
| <= 1280     | 10       | 0.25      | 32  | Medium screenshots |
| <= 1920     | 6        | 0.35      | 32  | Full HD captures, manga pages |
| <= 2560     | 3        | 0.45      | 40  | 2K/QHD |
| > 2560      | 0        | 0.50      | 48  | 4K — almost no dilation needed |

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
- **High-confidence boxes** ([1], [3], [7]): both scores well above 71% — all produced accurate text
- **False positive** ([2]): det 84%, ocr 74% — DBNet detected character hair as katakana; manga-ocr-rs hallucinated `その` at barely-passing confidence. Art with high-contrast strokes (hair, speed lines, hatching) triggers this.
- **Low-confidence boxes** ([0], [4], [5], [6], [8]): at least one score below 71% — all were hallucinated or partial

The confidence gate at 71% catches most garbage, but borderline false positives like [2]
(art misdetected as text) can slip through when OCR confidence lands in the 71–80% range.
Raising the gate to ~80% would filter these at the cost of also rejecting legitimate
small text fragments.

Size alone doesn't predict accuracy — a large box with a single speech bubble scores high,
while a small box that overlaps art scores low. The confidence scores capture this nuance.

### Performance (CPU, no GPU acceleration)

| Stage | Time |
|-------|------|
| DBNet detection | ~700–1500 ms |
| manga-ocr model load | ~1000–1100 ms |
| OCR per crop (high confidence) | ~0.8–2 s |
| OCR per crop (low confidence, hallucinating) | ~25–40 s |
| Full page (9 crops) | ~229 s total |
| Single panel (1 crop) | ~37 s total |

High-confidence crops (clean single bubbles) complete in 1–2 s. Low-confidence crops
(merged regions, mixed art) take 25–40 s because the beam search decoder runs away
generating garbage — these are reliably caught by the OCR confidence gate (<71%) and
truncated at 32 characters.

Detection is fast (~0.7–1.5 s) with auto-scaled parameters from `detection_params_for_size()`.
For production use, ONNX GPU execution providers (CUDA, TensorRT) would reduce per-crop
latency dramatically.

### lenzu integration (done)

The confidence-gated pipeline is now integrated into `lenzu_client`:

- Both Shift+Click (lens) and Ctrl+Shift+Click (fullscreen) paths try local OCR first
- Detection confidence >= 71% AND OCR confidence >= 71% => result returned immediately, no LLM
- Below the gate => falls through to Ollama -> local fallbacks -> free remote -> paid remote
- manga-ocr-rs models are loaded once at startup and shared across threads via `Arc`

See [unified benchmark](https://github.com/HidekiAI/lenzu/blob/trunk/docs/scores.md) for the full cross-engine accuracy comparison including before/after rescaling.

### Remaining optimizations

1. **GPU acceleration** — enable ONNX CUDA EP for both jp_detect and manga-ocr-rs
2. **Parallel OCR** — process crops concurrently (models are thread-safe)
3. **Panel segmentation** — split manga pages into panels before detection for tighter boxes
