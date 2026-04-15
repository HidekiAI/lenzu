# Technical Design: manga-ocr-rs — local ONNX OCR tier

**Status**: Prototype (`prototypes/manga-ocr-test`)  
**Scope**: Image-to-text only (Japanese OCR). No translation, no furigana stripping.  
**Integration target**: New optional OCR tier in lenzu, sitting before the Ollama tier.

---

## 1. Motivation

The current lenzu OCR pipeline uses LLMs (Ollama / OpenRouter) which:
- Combine OCR + translation in one call — convenient but slow (~3 s locally)
- Require Ollama to be running before lenzu starts
- Are non-deterministic

For users who can read Japanese and want sub-500 ms results without a running LLM,
a local ONNX OCR path provides:
- ~100–500 ms total latency (CPU inference, no GPU required)
- ~140 MB model footprint (encoder 22 MB + decoder 118 MB, FP32)
- Fully offline, no Ollama dependency
- Deterministic output

The translated-text use case is unchanged — Ollama / OpenRouter remain the path
for OCR + translation in one step.

---

## 2. Model

**Source**: [l0wgear/manga-ocr-2025-onnx](https://huggingface.co/l0wgear/manga-ocr-2025-onnx)  
**Based on**: [jzhang533/manga-ocr-base-2025](https://huggingface.co/jzhang533/manga-ocr-base-2025)  
(fine-tune of [kha-white/manga-ocr-base](https://github.com/kha-white/manga-ocr))  
**Architecture**: Vision Encoder-Decoder (ViT + GPT-2 style decoder)  
**Exported via**: Hugging Face Optimum

### 2.1 Files

| File | Size | Role |
|------|------|------|
| `encoder_model.onnx` | 22 MB | ViT image encoder |
| `decoder_model.onnx` | 118 MB | Autoregressive text decoder |
| `tokenizer.json` | 118 KB | BPE tokenizer (loadable via `tokenizers` crate) |

### 2.2 Config values used in code

From `preprocessor_config.json`:
```json
{ "image_processor_type": "ViTImageProcessor",
  "size": { "height": 224, "width": 224 },
  "image_mean": [0.5, 0.5, 0.5],
  "image_std":  [0.5, 0.5, 0.5],
  "do_resize": true, "do_normalize": true }
```

From `generation_config.json`:
```json
{ "decoder_start_token_id": 2,
  "eos_token_id": 3,
  "pad_token_id": 0,
  "max_length": 300,
  "num_beams": 4,
  "length_penalty": 2.0 }
```

The prototype uses greedy decode (argmax) rather than beam search.
Beam search (k=4) is left for the production crate.

---

## 3. Inference pipeline

```
DynamicImage (any size)
    │
    │  preprocess()
    │  ├─ resize_exact(224, 224, Lanczos3)
    │  ├─ to_rgb8()
    │  └─ normalise: pixel = (raw/255 - 0.5) / 0.5  → range [-1, 1]
    │  output: ndarray Array4<f32>  shape [1, 3, 224, 224]
    ▼
encoder_model.onnx
    │  input:  "pixel_values"       [1, 3,   224, 224]  f32
    │  output: "last_hidden_state"  [1, 196, 768]       f32
    │  (14×14 = 196 patches, hidden_dim = 768)
    ▼
decoder_model.onnx  ──── loop until EOS ────────────────────────────┐
    │  inputs:                                                        │
    │    "input_ids"              [1, t]           i64  (grows +1)   │
    │    "encoder_hidden_states"  [1, 196, 768]    f32  (constant)   │
    │    "encoder_attention_mask" [1, 196]         i64  (all ones)   │
    │  output: "logits"           [1, t, vocab]    f32               │
    │                                                                 │
    │  → argmax over logits[:, t-1, :] → next_token_id              │
    │  → if next_token_id == 3 (EOS): break                          │
    └─────────────────────────────────────────────────────────────────┘
    │
    │  tokenizer.decode(ids[1..], skip_special=true)
    ▼
String   (raw Japanese text)
```

### 3.1 Decoder note: non-merged export

The available export is `decoder_model.onnx` (non-merged / without-past).
There is no `decoder_model_merged.onnx` (with-past KV cache).

Consequence: every decode step re-runs full self-attention over the
growing `input_ids` sequence — O(n²) in sequence length.  
For typical manga text (5–30 tokens), this adds ~10–20 ms per call.

A with-past export would halve latency for longer sequences and is worth
requesting upstream or generating locally via `optimum-cli`.

---

## 4. Rust dependencies

| Crate | Version | Role |
|-------|---------|------|
| `ort` | `2.0.0-rc.10` | ONNX Runtime bindings (auto-downloads ORT binary) |
| `ndarray` | `0.15` | Tensor construction and slicing |
| `image` | `0.25` | Open image files, resize, pixel access |
| `tokenizers` | `0.20` | Load `tokenizer.json`, decode token IDs to UTF-8 |
| `anyhow` | `1.0` | Error propagation in the prototype |

`ort` version must stay in sync with what `jp_detect` pulls in — both share
the same ONNX Runtime shared library downloaded at build time.  
The `tokenizers` crate requires a C++ compiler (it vendors parts of Hugging Face's
tokenizers C++ core). No additional system packages are needed beyond `build-essential`.

---

## 5. Integration into lenzu

### 5.1 New OCR tier (IMPLEMENTED)

The current fallback chain is:

```
jp_detect (>= 71% detection confidence)
  → manga-ocr-rs (>= 71% OCR confidence)
    → text enrichment (if enrichment_enabled)
        Send raw text (NOT image) to local Ollama for furigana/romaji/translation.
        Text-to-text only — no vision model needed, fast (~1–5 s).
        If Ollama down/timeout → return raw OCR text as-is (graceful degradation).
    → DONE (local:manga-ocr or local:manga-ocr+enriched)
  → gemma4:e2b (Ollama) → glm-ocr (Ollama) → free remote → paid remote (Gemini 2.0 Flash)
```

manga-ocr-rs is the first tier in the pipeline, sitting before all LLM backends. When both detection and OCR confidence scores pass the 71% gate, results are optionally enriched with furigana/romaji/translation via a text-only Ollama call (no image sent), then returned. The image-based LLM chain is only reached when confidence is too low.

The enrichment prompt uses the same `{src}/{dest}/{extra_prompt}` placeholder mechanism as the OCR prompts, so it works for any language pair (JP→EN, EN→JP, JP→ES, etc.). Config fields: `enrichment_enabled` (default: true), `enrichment_model` (default: null = use `llm_default_model`), `enrichment_timeout_secs` (default: 15).

This tier is active for ALL capture methods (Shift+Click and Ctrl+Shift+Click) when:
- The text_detector is configured (jp_detect DBNet model loaded)
- manga-ocr-rs models are loaded (auto-downloaded on first run, ~140 MB)

Models are loaded once at startup and shared across threads via `Arc`.

### 5.2 Config additions (future)

```jsonc
{
  // Path to the directory containing encoder_model.onnx, decoder_model.onnx,
  // tokenizer.json.  null disables this tier.
  "manga_ocr_model_dir": "assets/manga-ocr-2025",

  // When true, skip the translation step and display raw Japanese.
  // manga_ocr tier is preferred when this is true and model_dir is set.
  "ocr_only": false
}
```

### 5.3 Code structure (future)

```
lenzu/src/ocr/
  mod.rs
  text_detection.rs    ← existing (DBNet via jp_detect)
  text_recognition.rs  ← new: re-exports manga-ocr-rs API
  text_cropper.rs      ← existing
```

`text_recognition.rs` mirrors the `text_detection.rs` pattern:
- `TextRecognizer` trait with `fn recognize(&self, img: &DynamicImage) -> Option<String>`
- `build_text_recognizer(model_dir) -> Option<Arc<dyn TextRecognizer + Send + Sync>>`
- `#[cfg(feature = "onnx")]` gate

---

## 6. Crate extraction plan

The `MangaOcr` struct in `prototypes/manga-ocr-test/src/main.rs` is intentionally
kept at the public API boundary.  Steps to publish as `manga-ocr-rs`:

1. **`lib.rs`** — move `MangaOcr`, `preprocess()`, all constants
2. **Error type** — replace `anyhow::Error` with `thiserror`-based `MangaOcrError`
3. **Trait** — `pub trait TextRecognizer { fn recognize(&self, img: &DynamicImage) -> Result<String, MangaOcrError>; }`
4. **Feature gate** — `[features] default = [] / onnx = ["ort/ndarray", "ndarray", "tokenizers"]`
5. **Beam search** — optional quality improvement; greedy is the v0.1 default
6. **Test** — one integration test with a bundled 10×10 fixture image
7. **Publish** — add to crates.io; lenzu adds `manga-ocr-rs = { version = "0.1", features = ["onnx"] }`

---

## 7. Known limitations & future work

| Item | Confidence Impact | Notes |
|------|-------------------|-------|
| Greedy decode | Slightly lower raw_confidence vs beam search | Beam search (k=4) improves accuracy ~5–10% for ambiguous text |
| No KV cache | None | Decoder is non-merged; each step is O(n²) — acceptable for short text |
| FP32 only | None | No FP16 export available upstream; would save ~70 MB if produced |
| CPU only | None | `ort` feature `cuda` would enable GPU inference; adds ~20 MB ORT binary |
| Furigana | None | Appears in output as regular characters; caller filters if unwanted |
| Vertical text | None | Handled by the model natively (trained on manga); no rotation needed |
| Hallucination on merged regions | OCR confidence drops below 71% gate; `truncated` flag fires | Beam search decoder runs away without EOS when input contains multiple text columns or mixed art — reliably caught by low confidence score |
| Ambiguous/noisy input | Low OCR confidence triggers LLM fallback | Results with < 71% OCR confidence are not trusted; pipeline falls through to Ollama → OpenRouter chain |
| No furigana/translation | N/A — manga-ocr-rs outputs raw text only | Addressed by text enrichment: when `enrichment_enabled`, raw text is sent to local Ollama for furigana/romaji/translation. Graceful degradation if Ollama is unavailable. |
| Input dimensions matter | Oversized inputs degrade accuracy | See §7.1 below |

### 7.1 Input dimension sensitivity (2026-04-15 findings)

The ViT encoder resizes all inputs to 224×224 via `resize_exact`. This makes the model
sensitive to the **aspect ratio and scale** of the source image relative to the text
it contains. Testing revealed stark accuracy differences between oversized "billboard"
images and realistic manga-bubble-sized inputs.

#### Before/after rescaling

Test images were originally generated at full compositor resolution (yokogaki 711×389,
tategaki 2760×1504, tegaki 2760×1504). On 2026-04-15 they were rescaled to
manga-bubble-realistic sizes (yokogaki 360×197, tategaki 480×262, tegaki 480×262).

**manga-ocr-rs standalone** (full image, no DBNet crop):

| Image | Old size | Old result | New size | New result |
|---|---|---|---|---|
| yokogaki | 711×389 | exact match | 360×197 | exact match |
| tategaki | 2760×1504 | `ラスト` instead of `テスト` (character confusion) | 480×262 | correct text, `「」` bracket variant |
| tegaki | 2760×1504 | exact match | 480×262 | exact match |

**DBNet + manga-ocr-rs pipeline** (tight crop, then OCR):

| Image | Size | Det % | OCR % | Text | Time |
|---|---|---|---|---|---|
| yokogaki | 360×197 | 99.5% | 95.0% | `データを正確に読み取る` (exact) | 1.0 s |
| tategaki | 480×262 | 97.0% | 99.5% | `『言語モデルのテスト』` (exact, correct brackets) | 1.1 s |
| tegaki | 480×262 | 99.4% | 88.7% | `手書きの文字サンプル` (exact) | 1.0 s |

#### Why oversized inputs fail

When a 2760×1504 image is squish-resized to 224×224, the text occupies only a
fraction of the 14×14 patch grid (196 patches total). Most patches see blank
background. The model:

1. **Loses fine character detail** — katakana テ and ラ differ by one stroke
   direction; at 224px the compressed patches can't resolve this.
2. **Hallucinates context** — large blank areas trigger the decoder to fill in
   plausible but wrong content (e.g., PaddleOCR-VL hallucinated a prefix before
   the actual text on the oversized tategaki image).
3. **Inference time balloons** — the decoder runs away on ambiguous inputs.
   Per-crop OCR time dropped from 25-40 s (oversized) to ~1.0-1.5 s (realistic).

#### Why the DBNet crop path gives better results than standalone

Even with realistically-sized images, DBNet provides a tighter crop around just
the text region. The tategaki image at 480×262 contains text in only the left
~30% of the frame. Standalone manga-ocr receives the full 480×262 and produces
`「」` brackets (single corner). DBNet crops to 142×262 (just the text column),
and manga-ocr returns the correct `『』` brackets (double corner). The tighter
crop means the text fills more of the 224×224 patch grid, improving accuracy.

#### Design implications

1. **Always crop before OCR** — never send full screenshots or compositor captures
   directly to manga-ocr-rs. DBNet detection → tight crop → OCR is the correct
   pipeline. The standalone model is a fallback, not the primary path.
2. **Pad crops conservatively** — enough padding to avoid clipping characters (the
   current proportional 10% bbox padding), but not so much that text becomes small
   relative to the 224×224 grid.
3. **Test fixtures must be realistic** — use manga-bubble-sized images (200-500px),
   not billboard-scale screenshots. Oversized fixtures give misleading accuracy
   numbers and unrealistic inference times.
4. **Composite images merge at small scale** — the 640×349 sample-texts image
   (3 text regions in one frame) produces 1 merged detection box at dilation=16.
   Individual text images are the correct unit for OCR benchmarking.

---

## 8. References

- Model: https://huggingface.co/l0wgear/manga-ocr-2025-onnx
- Base fine-tune: https://huggingface.co/jzhang533/manga-ocr-base-2025
- Original: https://github.com/kha-white/manga-ocr
- ORT Rust bindings: https://github.com/pykeio/ort
- HF tokenizers (Rust): https://github.com/huggingface/tokenizers
