# PaddleOCR-VL-For-Manga (GGUF) Evaluation Prototype

Evaluates [PaddleOCR-VL-For-Manga](https://huggingface.co/jzhang533/PaddleOCR-VL-For-Manga)
served via llama.cpp as a GGUF model for Japanese manga OCR.

## Model

- **Base**: `PaddlePaddle/PaddleOCR-VL` (0.5B params, 109 languages)
- **Fine-tuned**: `jzhang533/PaddleOCR-VL-For-Manga` (SFT on ~90K Manga109-s crops + 1.5M synthetic)
- **GGUF**: [adambarbato/PaddleOCR-VL-For-Manga-GGUF](https://huggingface.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF/tree/main)
- **Size**: ~936 MB (main) + ~882 MB (mmproj) = ~1.8 GB total
- **Quantization**: BF16 only (no Q4/Q8 variants yet)
- **Accuracy**: 70% exact full-sentence match on Manga109-s (up from 27% base), CER ~10%
- **License**: Apache 2.0

## Prerequisites

- **llama.cpp** (built from source, needs Feb 2026+ with PR #18825 for PaddleOCR arch)
- **huggingface-cli** (`pip install huggingface_hub`)

## Setup

```bash
# 1. Download GGUF model files (~1.8 GB)
./setup.sh

# 2. Start llama-server (OpenAI-compatible API on port 9999)
./serve.sh

# 3. Run evaluation against 3 test PNGs
./eval.sh
```

## Results (2026-04-14, pre-rescale images)

Engine: llama-server (CPU-only, no CUDA toolkit — BF16 on Xeon E5-2670v3 / Quadro M4000).

> **Note**: These results were obtained with the original oversized images (yokogaki 711×389,
> tategaki/tegaki 2760×1504). The images were rescaled on 2026-04-15 to manga-bubble-realistic
> sizes (yokogaki 360×197, tategaki/tegaki 480×262). Smaller inputs should improve tategaki
> accuracy and reduce inference time. Re-run `./eval.sh` with llama-server to update.

| Image | Orientation | Expected | Got | Result | Tokens | Time (ms) |
|---|---|---|---|---|---|---|
| Unit-test-yokogaki.png | Horizontal | `データを正確に読み取る` | `データを正確に読み取る` | **PASS** | 13 | 4645 |
| Unit-test-tategaki.png | Vertical | `『言語モデルのテスト』` | `今回は「言語モデルの テスト」 『言語モデルの テスト』` | **FAIL** | 23 | 107418 |
| Unit-test-tegaki.png | Horizontal (calligraphy) | `手書きの文字サンプル` | `手書きの文字サンプル` | **PASS** | 11 | 104009 |

**Summary: 2/3 PASS.**

### Analysis

- **Yokogaki** (horizontal): Perfect exact match. Fast even on CPU (4.6s).
- **Tategaki** (vertical): Correct text IS present in output (`『言語モデルの テスト』`),
  but model hallucinated extra context before it. Spaces inserted within text. 107s
  due to CPU-only BF16 inference.
- **Tegaki** (calligraphy): Perfect match (trivial leading space). 104s CPU penalty.

### Comparison vs other engines (pre-rescale images)

| Image | Umi-OCR | PaddleOCR-VL GGUF | manga-ocr-rs (2026-04-15, rescaled) |
|---|---|---|---|
| Yokogaki | FAIL (`デー々を...`) | **PASS** (exact) | **PASS** (exact, 95% conf) |
| Tategaki | FAIL (garbage) | **FAIL** (correct text + hallucinated prefix) | **PASS** (exact, 99.5% conf) |
| Tegaki | FAIL (`チ書きの丈字...`) | **PASS** (exact) | **PASS** (exact, 88.7% conf) |

PaddleOCR-VL-For-Manga is significantly better than Umi-OCR. manga-ocr-rs with the rescaled
images achieves 3/3 exact match through the DBNet+OCR pipeline. The Umi-OCR and PaddleOCR-VL
results above are from the pre-rescale oversized images — re-evaluation pending.

See [unified benchmark](https://github.com/HidekiAI/lenzu/blob/trunk/docs/scores.md) for the full cross-engine comparison.

### Performance note

Times are CPU-only (BF16, no CUDA toolkit installed). With GPU acceleration on the
Quadro M4000 (8 GB VRAM), expect ~1-5s per image. The model (1.8 GB) fits comfortably
in 8 GB VRAM.

## How it works

1. `llama-server` loads PaddleOCR-VL GGUF + multimodal projector
2. Eval script sends base64-encoded PNG crops to `/v1/chat/completions`
3. Prompt: `<__media__>OCR:` — model returns raw Japanese text
4. Compare against expected text for each test image

## Why llama-server, not Ollama?

Ollama can load plain GGUF text models via `FROM ./path.gguf` in a Modelfile. However,
**vision models require a separate multimodal projector** (`--mmproj` in llama.cpp), and
Ollama's Modelfile has no keyword for specifying one. The official
`MedAIBase/PaddleOCR-VL:0.9b` on Ollama was published without the projector — it only
supports text completion, not image input.

llama-server's `--mmproj` flag is the correct way to load two-file vision GGUFs.

For production Rust integration, [`llama-cpp-2`](https://crates.io/crates/llama-cpp-2)
provides in-process bindings (no separate server), same pattern as manga-ocr-rs using `ort`.

## Related links

- Training code: https://github.com/jzhang533/PaddleOCR-VL-For-Manga
- Blog: https://pfcc.blog/posts/paddleocr-vl-for-manga
- llama.cpp support PR: https://github.com/ggml-org/llama.cpp/pull/18825
- Ollama base (non-manga, text-only): `MedAIBase/PaddleOCR-VL:0.9b`
