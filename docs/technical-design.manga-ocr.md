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
    → DONE (fully offline, no LLM)
  → gemma4:e2b (Ollama) → glm-ocr (Ollama) → free remote → paid remote (Gemini 2.0 Flash)
```

manga-ocr-rs is the first tier in the pipeline, sitting before all LLM backends. When both detection and OCR confidence scores pass the 71% gate, results are returned immediately — no Ollama, no network. The LLM chain is only reached when confidence is too low.

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

---

## 8. References

- Model: https://huggingface.co/l0wgear/manga-ocr-2025-onnx
- Base fine-tune: https://huggingface.co/jzhang533/manga-ocr-base-2025
- Original: https://github.com/kha-white/manga-ocr
- ORT Rust bindings: https://github.com/pykeio/ort
- HF tokenizers (Rust): https://github.com/huggingface/tokenizers
