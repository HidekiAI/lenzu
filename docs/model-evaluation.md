# OCR Model Evaluation Log

Models tested against `assets/Unit-test-sample-texts.png` (640px max-dim, grayscale)
using `scripts/test-ocr.sh --no-timeout` on hardware with 4GB VRAM (NVIDIA).

## Current chain

```
jp_detect (>= 71% det confidence)
  → manga-ocr-rs (>= 71% OCR confidence)
    → DONE (fully offline, no LLM)
  → gemma4:e2b  →  glm-ocr  →  free remote  →  paid remote (Gemini 2.0 Flash)
```

The local-first path (jp_detect + manga-ocr-rs) runs before any LLM. When both
detection and OCR confidence are >= 71%, results are returned immediately — no
Ollama, no network. The LLM chain is only reached when confidence is too low.

---

## Tested and dropped

| Model | Usability | OCR Accuracy | Verdict |
|-------|-----------|-------------|---------|
| moondream2 | 0% — wrong output type | 0% — prose, not OCR | Captioning model, useless |
| qwen2.5:1.5b/0.5b | 0% — no vision input | 0% — hallucinates from prompt | Text-only, wrong family |
| qwen2-vl:2b | 0% — doesn't exist | N/A | Wrong model name |
| qwen2.5vl:3b | 0% — always times out | N/A — never completes | CPU-bound on 4 GB VRAM |
| florence2:large | 0% — no ollama package | N/A | Python-only, excluded |

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

---

## Retained models

### jp_detect + manga-ocr-rs (local-first, no LLM)
- **Crates**: `jp_detect >= 0.2.2`, `manga-ocr-rs >= 0.1.1`
- **Size**: ~4.7 MB (DBNet) + ~140 MB (manga-ocr encoder+decoder)
- **Speed**: ~50-120 ms detection + ~1-42 s OCR per crop (CPU); much faster with GPU
- **Quality**: Excellent for clean, isolated text regions. Per-box confidence scores
  (0.0-1.0) reliably predict accuracy — boxes passing the 71% gate on both detection
  and OCR are consistently correct. Fails gracefully on merged/ambiguous regions by
  reporting low confidence, triggering fallback to the LLM chain.
- **Confidence scores**: Detection confidence = mean probability of thresholded DBNet pixels.
  OCR confidence = dimension-adjusted geometric mean of per-token probabilities.
  `truncated` flag fires when decoder runs away without EOS (strong hallucination signal).
- **Notes**: Loaded once at startup, shared via `Arc`. No network, no API key. This is
  the preferred path — the LLM chain exists as fallback for low-confidence cases only.

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
