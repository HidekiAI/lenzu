# OCR Model Evaluation Log

Models tested against `assets/Unit-test-sample-texts.png` (640×349 px, grayscale)
using `scripts/test-ocr.sh --no-timeout` on hardware with 4GB VRAM (NVIDIA).

## Current chain

```
jp_detect (>= 71% det confidence)
  → manga-ocr-rs (>= 71% OCR confidence)
    → text enrichment (if enrichment_enabled): text-only Ollama call
        adds furigana/romaji/translation — no image, no vision model
        graceful degradation if Ollama unavailable
    → DONE (local:manga-ocr or local:manga-ocr+enriched)
  → gemma4:e2b  →  glm-ocr  →  free remote  →  paid remote (Gemini 2.0 Flash)
```

The local-first path (jp_detect + manga-ocr-rs) runs before any image-based LLM.
When both detection and OCR confidence are >= 71%, the raw text is optionally
enriched with furigana/romaji/translation via a text-only Ollama call (no image
sent — just plain text), then returned. The image-based LLM chain is only reached
when confidence is too low.

---

## Tested and dropped

| Model | Usability | OCR Accuracy | Verdict |
|-------|-----------|-------------|---------|
| moondream2 | 0% — wrong output type | 0% — prose, not OCR | Captioning model, useless |
| qwen2.5:1.5b/0.5b | 0% — no vision input | 0% — hallucinates from prompt | Text-only, wrong family |
| qwen2-vl:2b | 0% — doesn't exist | N/A | Wrong model name |
| qwen2.5vl:3b | 0% — always times out | N/A — never completes | CPU-bound on 4 GB VRAM |
| florence2:large | 0% — no ollama package | N/A | Python-only, excluded |
| sarashina2.2-vision-3b | 0% — CPU latency ~150× over budget | N/A — never completed | Too heavy for Lenzu's CPU-only host |

### moondream / moondream2
- **Ollama name**: `moondream`
- **Size**: ~1.7 GB
- **Result**: Returns English prose description of the image ("The image features a white
  background with black text written in an Asian script..."). Does not produce structured
  JSON or OCR output at all.
- **Verdict**: Captioning model, not OCR. Useless for this pipeline.

### qwen2.5:1.5b / qwen2.5:0.5b
- **Ollama name**: `qwen2.5:1.5b`, `qwen2.5:0.5b`
- **Result**: Text-only model — no vision/image input support. Hallucinates OCR output
  from the prompt text rather than reading the image.
- **Verdict**: Wrong model family. qwen2.5 (without `vl`) is text-only.

### qwen2-vl:2b
- **Ollama name**: `qwen2-vl:2b`
- **Result**: Does not exist in ollama's registry (`pull model manifest: file does not exist`).
- **Verdict**: Wrong model name. The correct vision variant is `qwen2.5vl`.

### qwen2.5vl:3b
- **Ollama name**: `qwen2.5vl:3b`
- **Size**: ~3.2 GB
- **Result**: Consistently times out at exactly 30 s via HTTP (`/v1/chat/completions`).
  `ollama run qwen2.5vl:3b "what is 1+1"` took 2+ minutes on CLI — model runs almost
  entirely on CPU because 3.2 GB leaves no headroom in 4 GB VRAM after OS/driver overhead.
- **Verdict**: CPU-bound on 4 GB VRAM. May be viable on 8 GB+ cards.

### florence2:large
- **Ollama name**: `florence2:large`
- **Result**: Does not exist in ollama's registry (`pull model manifest: file does not exist`).
- **Verdict**: Florence-2 (Microsoft) is only available via HuggingFace/Python. No ollama
  package exists. Excluded permanently due to no-Python constraint.

### sarashina2.2-vision-3b (tested 2026-04-19)
- **HuggingFace**: `sbintuitions/sarashina2.2-vision-3b` (~3 B, ~7.1 GiB weights on disk)
- **Runtime**: Python sidecar prototype (`prototypes/sarashina-vision-py/`), `transformers==4.49.0`,
  `AutoModelForCausalLM.from_pretrained(..., trust_remote_code=True)`, CPU `torch.float32`.
  GPU path unavailable — host's Quadro M4000 is sm_5.2, unsupported by modern PyTorch kernels.
- **Test**: standard 4-image battery (tategaki / yokogaki / sample-texts / ubunchu01_02), same
  prompt for each: `画像の日本語テキストを読んで、英語に翻訳してください。`, `max_new_tokens=256`.
- **Result**: **SIGTERM'd at 2 h 34 min** with zero jobs producing visible output (stdout was
  block-buffered and lost on kill — `generate()` was invoked 4 times, so per-job wall time is
  lower-bounded at ~38 min). Sustained 10–12 cores CPU and 8.6–9.6 GiB RSS throughout; system
  load avg 12–13, 16.7 GiB swap in use.
- **Verdict**: Unusable. ~150× over Lenzu's 15 s local-fallback budget, and eats enough RAM +
  CPU that the primary manga-ocr-rs tier can't run in parallel. The same custom architecture
  is used by `sarashina2.2-ocr`, so that variant is ruled out by extension. **Do not re-prototype
  sarashina vision on CPU hardware.** Full run notes: `prototypes/sarashina-vision-py/README.md`.
  The text-only tier (`sarashina-mini-rs`, ONNX via `ort`) is unaffected and remains the
  green-light path.

---

## Retained models

### jp_detect + manga-ocr-rs (local-first, no LLM)
- **Crates**: `jp_detect >= 0.2.2`, `manga-ocr-rs >= 0.1.1`
- **Size**: ~4.7 MB (DBNet) + ~140 MB (manga-ocr encoder+decoder)
- **Speed**: ~700-1500 ms detection + ~1.0-1.5 s OCR per crop (CPU); much faster with GPU
- **Quality**: Excellent for clean, isolated text regions. Per-box confidence scores
  (0.0-1.0) reliably predict accuracy — boxes passing the 71% gate on both detection
  and OCR are consistently correct. Fails gracefully on merged/ambiguous regions by
  reporting low confidence, triggering fallback to the LLM chain.
- **Benchmark (2026-04-15)** — rescaled manga-bubble-sized test images:

  | Image | Size | Det % | OCR % | Text | Time |
  |---|---|---|---|---|---|
  | yokogaki | 360×197 | 99.5% | 95.0% | `データを正確に読み取る` (exact) | 1.0 s |
  | tategaki | 480×262 | 97.0% | 99.5% | `『言語モデルのテスト』` (exact) | 1.1 s |
  | tegaki | 480×262 | 99.4% | 88.7% | `手書きの文字サンプル` (exact) | 1.0 s |

  All three pass the 71% confidence gate on both detection and OCR.
- **Confidence scores**: Detection confidence = mean probability of thresholded DBNet pixels.
  OCR confidence = dimension-adjusted geometric mean of per-token probabilities.
  `truncated` flag fires when decoder runs away without EOS (strong hallucination signal).
- **Text enrichment**: When `enrichment_enabled` (default: true), raw OCR text is sent to
  local Ollama as text-only (no image) for furigana/romaji/translation. Uses `enrichment_model`
  (default: `llm_default_model`) with `enrichment_timeout_secs` (default: 15 s). If enrichment
  fails, raw text is returned as-is. The enrichment prompt supports `{src}/{dest}/{extra_prompt}`
  placeholders for any language pair.
- **Notes**: Loaded once at startup, shared via `Arc`. No network, no API key for OCR itself.
  This is the preferred path — the image-based LLM chain exists as fallback for low-confidence cases only.

### gemma4:e2b (primary local LLM fallback)
- **Size**: ~7.2 GB
- **Speed**: 47–110 s on partial GPU (4 GB VRAM)
- **Quality**: Inconsistent block detection (2–5 blocks per run); misses vertical CJK text
  reliably; good translation quality when it does detect.
- **Notes**: SSE streaming (`"stream": true`) required to keep connection alive past
  ollama's 30 s server-side write timeout during slow inference.

### glm-ocr (local fallback)
- **Ollama name**: `glm-ocr`
- **Size**: ~2.2 GB
- **Speed**: 10–20 s on GPU
- **Quality**: Lumps all visible text into one block (no spatial separation), but text
  content is accurate. Misses vertical CJK text. Acceptable for single-region captures
  where block splitting is less important.
- **Notes**: Returns a single JSON object (not an array) — handled by `normalize_results()`
  in `client.rs`.

### remote: google/gemini-2.0-flash-001 (remote fallback)
- **Endpoint**: OpenRouter (`https://openrouter.ai/api/v1/chat/completions`)
- **Speed**: 3–5 s
- **Quality**: Detects all 3 text blocks including vertical CJK; correct text; best results.
- **Notes**: Only used when both local models fail or `OPENROUTER_API_KEY` is set and
  Ctrl+Shift+Click override is active. Images leave the device on this path.

---

## Considered but gated

### yomitoku (2026-04-19)

- **Repo**: https://github.com/kotaro-kinoshita/yomitoku
- **Status**: **Not prototyped.** Gated on a pure-Rust implementation path.
- **Why gated**: yomitoku is a Python library — PyTorch + onnxruntime pipeline with
  detector, recognizer, layout analysis, table detection, and reading-order logic,
  all orchestrated in Python. Prior user feedback already flagged it as a memory
  and CPU hog for full-page JP OCR use. Even if the underlying ONNX model weights
  could be extracted and served from Rust via `ort`, the pre/post-processing and
  reading-order pipeline would have to be reimplemented — that's multi-prototype
  work, not a quick port.
- **Gate to open this**: concrete evidence the Rust port is cheap, e.g., upstream
  publishes ONNX bundles, or someone has already shipped a Rust implementation
  of the pipeline. Absent that evidence, assume no cheap port exists and leave
  yomitoku off the candidate list.
- **Do not revisit without meeting the gate** — this was reviewed against Lenzu's
  <15 s local-fallback budget and the no-Python preference, and found wanting on
  both. The point of this entry is to save future-me (or an LLM picking up cold)
  from re-raising yomitoku as a fresh suggestion.

## To evaluate: alternative OCR models

### Umi-OCR (PaddleOCR-based)

- **Repo**: https://github.com/hiroi-sora/Umi-OCR
- **Engine**: PaddleOCR-json (PaddlePaddle C++ inference) or RapidOCR-json (ONNX Runtime)
- **Status**: Not yet tested. Maintainer warns vertical Japanese is poor (issue #434).
  Recommends manga-ocr for manga. Worth a quick eval via Docker HTTP API.
- **Prototype**: `prototypes/umi-ocr-eval/`

### HuggingFace manga OCR models

Models worth evaluating as potential upgrades or alternatives to the current manga-ocr-rs pipeline:

| Model | Link | Type | Integration path |
|---|---|---|---|
| manga-ocr (original) | https://huggingface.co/mayocream/manga-ocr/tree/main | PyTorch weights | What manga-ocr-rs already uses |
| manga-ocr ONNX full | https://huggingface.co/xingliao/manga-ocr-onnx-full/tree/main | ONNX export | Check for quantized/INT8 variant; drop-in for manga-ocr-rs via ort |
| PaddleOCR-VL-For-Manga (GGUF) | https://huggingface.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF/tree/main | GGUF quantized | Best speed/accuracy — needs llama-cpp-rs instead of ort |
| PaddleOCR-VL-For-Manga (base) | https://huggingface.co/jzhang533/PaddleOCR-VL-For-Manga/tree/main | Original weights | Base model for the GGUF above |
| PaddleOCRv5 Det For Manga | https://huggingface.co/bluolightning/PaddleOCRv5-Server-Det-For-Manga/tree/main | Detection model | Lighter alternative to DBNet for text detection |

### Recommended upgrade path (from external advice)

1. **Quick fix now**: `xingliao/manga-ocr-onnx-full` — check for quantized variant, keep ort backend, ensure `max_decode_steps=50` to prevent 40s hangs
2. **Best speed/accuracy**: `adambarbato/PaddleOCR-VL-For-Manga-GGUF` — switch backend from ort to llama-cpp-rs; this model is specifically built for manga OCR
3. **Pure Rust / lightweight**: `bluolightning/PaddleOCRv5-Server-Det-For-Manga` — PaddleOCR Rust implementation, much lighter than transformer-based models
