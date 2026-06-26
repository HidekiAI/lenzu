# Umi-OCR Evaluation Prototype

Quick evaluation of [Umi-OCR](https://github.com/hiroi-sora/Umi-OCR) (PaddleOCR-based)
for Japanese manga text recognition.

## Results (2026-04-14, pre-rescale images)

Engine: PaddleOCR-json (via Docker, headless HTTP API on port 1224).

> **Note**: These results were obtained with the original oversized images (yokogaki 711×389,
> tategaki/tegaki 2760×1504). The images were rescaled on 2026-04-15 to manga-bubble-realistic
> sizes (yokogaki 360×197, tategaki/tegaki 480×262). Results may improve with smaller inputs.
> Re-run `./eval.sh` with the Umi-OCR server to update.

| Image | Orientation | Expected | Got | Result | Score | Time (ms) |
|---|---|---|---|---|---|---|
| Unit-test-yokogaki.png | Horizontal | `データを正確に読み取る` | `デー々を正確に読み取る` | **FAIL** | 0.961 | 1084 |
| Unit-test-tategaki.png | Vertical | `『言語モデルのテスト』` | `き転でげんの テスイ` | **FAIL** | 0.554, 0.253 | 2360 |
| Unit-test-tegaki.png | Horizontal (calligraphy) | `手書きの文字サンプル` | `チ書きの丈字サンプル` | **FAIL** | 0.903 | 2116 |

**Summary: 0/3 PASS.** See [unified benchmark](https://github.com/CodeMonkeyNinja/lenzu/wiki/scores) for comparison across all engines.

### Analysis

- **Yokogaki** (horizontal): Nearly correct — misread `タ` as `々`. High confidence (96%) but wrong character.
- **Tategaki** (vertical): Complete garbage output. Two low-confidence fragments (55%, 25%).
  Confirms the maintainer's own warning that vertical Japanese is unsupported.
- **Tegaki** (calligraphy): Close but wrong kanji — `手→チ`, `文→丈`. 90% confidence.

**Verdict**: Umi-OCR (PaddleOCR) is not viable for Japanese manga OCR. Vertical text
is broken, and even horizontal text has character-level errors. The existing manga-ocr-rs
pipeline outperforms it on all three test cases.

## Setup

```bash
# Start Umi-OCR in Docker (headless HTTP API on port 1224)
./start-umi-ocr.sh

# Run evaluation against all 3 test images
./eval.sh
```

## Known limitations

The Umi-OCR maintainer has acknowledged that **vertical Japanese text (tategaki)
recognition is poor** across all supported OCR engines (PaddleOCR, RapidOCR, etc.)
and recommends [manga-ocr](https://github.com/kha-white/manga-ocr) for manga.
See: https://github.com/hiroi-sora/Umi-OCR/issues/434

## Alternative models to evaluate

If Umi-OCR performs poorly, these HuggingFace models are worth investigating:

| Model | Type | Notes |
|---|---|---|
| [mayocream/manga-ocr](https://huggingface.co/mayocream/manga-ocr/tree/main) | Original manga-ocr weights | What manga-ocr-rs already uses |
| [xingliao/manga-ocr-onnx-full](https://huggingface.co/xingliao/manga-ocr-onnx-full/tree/main) | ONNX export | Check for quantized/INT8 variant; drop-in for manga-ocr-rs |
| [adambarbato/PaddleOCR-VL-For-Manga-GGUF](https://huggingface.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF/tree/main) | GGUF quantized | Best speed/accuracy — needs llama-cpp-rs instead of ort |
| [jzhang533/PaddleOCR-VL-For-Manga](https://huggingface.co/jzhang533/PaddleOCR-VL-For-Manga/tree/main) | PaddleOCR VL | Base weights for the GGUF above |
| [bluolightning/PaddleOCRv5-Server-Det-For-Manga](https://huggingface.co/bluolightning/PaddleOCRv5-Server-Det-For-Manga/tree/main) | PaddleOCR v5 Det | Lighter detection model, alternative to DBNet |

### Recommended upgrade path

1. **Quick fix**: Use `xingliao/manga-ocr-onnx-full` — check for quantized variant, keep ort backend
2. **Best speed/accuracy**: Use `adambarbato/PaddleOCR-VL-For-Manga-GGUF` — switch to llama-cpp-rs backend
3. **Pure Rust / lightweight**: Use `bluolightning/PaddleOCRv5-Server-Det-For-Manga` — PaddleOCR in Rust, lighter than transformer models
