# Session Log — 2026-04-14

## Summary

Three main tracks completed: mecab_furigana_rs integration, Umi-OCR evaluation, and
PaddleOCR-VL-For-Manga GGUF evaluation. Plus analysis of replacing Ollama with direct
llama.cpp.

---

## 1. Integrated mecab_furigana_rs into lenzu

**Goal**: Replace 704 lines of inline MeCab logic in `furigana.rs` with the standalone
[`mecab-furigana-rs`](https://crates.io/crates/mecab-furigana-rs) crate (v0.1.0).

**Changes**:
- `lenzu/Cargo.toml` — added `mecab-furigana-rs = "0.1.0"`
- `lenzu/src/furigana.rs` — rewritten from 704 lines to ~130 lines (thin wrapper)
  - Removed `#[cfg(target_os = "linux")]` gate — crate handles cross-platform gracefully
  - `annotate()` and `compare_and_maybe_overwrite()` signatures preserved
  - All 4 tests pass
- `README.md` — added `mecab-furigana-rs` to Related Crates section
- `lenzu/README.md` — updated tech deps table to link the crate instead of raw MeCab

---

## 2. Umi-OCR Evaluation (PaddleOCR-based)

**Goal**: Test [Umi-OCR](https://github.com/hiroi-sora/Umi-OCR) via Docker HTTP API
against 3 standard test PNGs.

**Prototype**: `prototypes/umi-ocr-eval/`

**Results: 0/3 PASS** (pre-rescale oversized images — yokogaki 711×389, tategaki/tegaki 2760×1504)

| Image | Orientation | Expected | Got | Score | Time (ms) |
|---|---|---|---|---|---|
| yokogaki | Horizontal | `データを正確に読み取る` | `デー々を正確に読み取る` | 0.961 | 1084 |
| tategaki | Vertical | `『言語モデルのテスト』` | `き転でげんの テスイ` | 0.554, 0.253 | 2360 |
| tegaki | Calligraphy | `手書きの文字サンプル` | `チ書きの丈字サンプル` | 0.903 | 2116 |

**Verdict**: Not viable for manga OCR. Vertical text completely broken (maintainer
acknowledges this in issue #434). Even horizontal text has character-level errors.
Images were rescaled on 2026-04-15 — re-evaluation with smaller inputs pending.

---

## 3. PaddleOCR-VL-For-Manga GGUF Evaluation

**Goal**: Test [PaddleOCR-VL-For-Manga](https://huggingface.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF)
via llama.cpp server against the same 3 PNGs.

**Prototype**: `prototypes/paddleocr-vl-manga/`

**Results: 2/3 PASS** (CPU-only, no CUDA toolkit — BF16 on Xeon E5-2670v3; pre-rescale images)

| Image | Orientation | Expected | Got | Tokens | Time (ms) |
|---|---|---|---|---|---|
| yokogaki | Horizontal | `データを正確に読み取る` | `データを正確に読み取る` | 13 | 4645 |
| tategaki | Vertical | `『言語モデルのテスト』` | `今回は「言語モデルの テスト」 『言語モデルの テスト』` | 23 | 107418 |
| tegaki | Calligraphy | `手書きの文字サンプル` | `手書きの文字サンプル` | 11 | 104009 |

**Verdict**: Significantly better than Umi-OCR. Yokogaki and tegaki exact match.
Tategaki has correct text present but with hallucinated prefix — potentially fixable
with max_tokens tuning or post-processing. CPU times are slow (100+ seconds) due to
BF16 without CUDA — expect ~1-5s with GPU.
Images were rescaled on 2026-04-15 — re-evaluation with smaller inputs pending.

### Why llama-server, not Ollama?

Ollama can load plain GGUF text models but **vision models need a multimodal projector**
(`--mmproj`), which Ollama's Modelfile has no keyword for. The official
`MedAIBase/PaddleOCR-VL:0.9b` on Ollama was published without the projector — text
completion only, no image input.

---

## 4. HuggingFace Model Links Cataloged

Documented in `docs/model-evaluation.md` under "To evaluate: alternative OCR models":

| Model | Link | Type |
|---|---|---|
| manga-ocr (original) | https://huggingface.co/mayocream/manga-ocr/tree/main | PyTorch weights |
| manga-ocr ONNX full | https://huggingface.co/xingliao/manga-ocr-onnx-full/tree/main | ONNX export |
| PaddleOCR-VL-For-Manga (GGUF) | https://huggingface.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF/tree/main | GGUF quantized |
| PaddleOCR-VL-For-Manga (base) | https://huggingface.co/jzhang533/PaddleOCR-VL-For-Manga/tree/main | Original weights |
| PaddleOCRv5 Det For Manga | https://huggingface.co/bluolightning/PaddleOCRv5-Server-Det-For-Manga/tree/main | Detection model |

Recommended upgrade path (from external advice):
1. Quick fix: `xingliao/manga-ocr-onnx-full` — quantized variant, keep ort backend
2. Best speed/accuracy: `adambarbato/PaddleOCR-VL-For-Manga-GGUF` — llama-cpp-rs backend
3. Lightweight: `bluolightning/PaddleOCRv5-Server-Det-For-Manga` — PaddleOCR in Rust

---

## 5. Ollama-to-llama.cpp Migration Analysis

Full analysis at `docs/planning-ollama-to-llamacpp.md`.

Key findings:
- lenzu already uses OpenAI-compatible `/v1/chat/completions` — same API as llama-server
- Streaming, logprobs, vision payloads all compatible
- Main challenge: model switching (Ollama does it transparently; llama-server needs
  multi-port or model reload strategy)
- Recommended path: `llama-cpp-2` Rust crate for in-process inference (no server overhead)
- VRAM budget: PaddleOCR-VL (1.8 GB) + qwen2.5:3b (1.8 GB) fits in 8 GB with room to spare

---

## 6. Infrastructure Changes

### scripts/setup.sh
- Added `--skip-paddleocr` flag
- Added `nvidia-cuda-toolkit` to apt dependencies
- Added PaddleOCR-VL GGUF download section (~1.8 GB, uses curl with progress)

### .gitignore
- Added `prototypes/paddleocr-vl-manga/model/` to prevent committing GGUF files

---

## 7. Test Image Size Concern (resolved 2026-04-15)

The tategaki and tegaki PNGs were 2760x1504 (4.6-4.7 MB) — full compositor screenshots,
not realistic manga bubble crops. This was fixed on 2026-04-15: images rescaled to
manga-bubble-realistic sizes (yokogaki 360×197, tategaki 480×262, tegaki 480×262,
sample-texts 640×349). With the rescaled images:
- manga-ocr-rs: 3/3 correct text (tategaki bracket-style 「」vs『』 only diff)
- DBNet+manga-ocr pipeline: 3/3 exact match with high confidence (95-99% OCR)
- Inference time per crop: ~1.0-1.5 s (was 25-40 s for oversized images)

---

## Files Changed

| File | Change |
|---|---|
| `lenzu/Cargo.toml` | Added `mecab-furigana-rs = "0.1.0"` |
| `lenzu/src/furigana.rs` | Rewritten as thin wrapper (704 → ~130 lines) |
| `README.md` | Added mecab-furigana-rs to Related Crates |
| `lenzu/README.md` | Updated tech deps table |
| `.gitignore` | Added `prototypes/paddleocr-vl-manga/model/` |
| `scripts/setup.sh` | Added CUDA toolkit, PaddleOCR-VL download, --skip-paddleocr |
| `docs/model-evaluation.md` | Added HuggingFace model links and upgrade path |
| `docs/planning-ollama-to-llamacpp.md` | New — full Ollama replacement analysis |
| `prototypes/umi-ocr-eval/` | New — Umi-OCR Docker prototype + results |
| `prototypes/paddleocr-vl-manga/` | New — PaddleOCR-VL GGUF prototype + results |
