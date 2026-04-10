# manga-ocr-test — Vision-Encoder-Decoder OCR prototype

Standalone prototype for running [mayocream/manga-ocr-onnx](https://huggingface.co/mayocream/manga-ocr-onnx)
(the original [kha-white/manga-ocr-base](https://github.com/kha-white/manga-ocr) ONNX export)
entirely in Rust via `ort` (ONNX Runtime).

Returns raw Japanese text — no translation, no furigana stripping.
That is the intended scope: pure image-to-text OCR.

Once validated, the `MangaOcr` struct and its `recognize(&DynamicImage) -> Result<String>` surface
are intended to be extracted into a standalone `manga-ocr-rs` crate for general use.

---

## Model files

Download three files from HuggingFace (~440 MB total) and place them under `assets/manga-ocr/`:

```
assets/
  manga-ocr/
    encoder_model.onnx    (~328 MB)
    decoder_model.onnx    (~113 MB)
    vocab.txt             (~30 KB)
```

```bash
# from the repo root — handled automatically by scripts/setup.sh
bash scripts/setup.sh

# or manually with curl (no Python needed):
mkdir -p assets/manga-ocr
for f in encoder_model.onnx decoder_model.onnx vocab.txt; do
  curl -fL "https://huggingface.co/mayocream/manga-ocr-onnx/resolve/main/$f" \
       -o "assets/manga-ocr/$f"
done
```

---

## Run

```bash
# default model dir (assets/manga-ocr/)
cargo run -p manga-ocr-test -- path/to/japanese-text.png

# inspect model I/O names
cargo run -p manga-ocr-test -- inspect
```

---

## Test results (debug build, beam search k=4)

Fixtures are clean black-on-white IPAGothic / Dejima-Mincho images generated
with ImageMagick — the same class of input the model was trained on (scanned manga):

| Fixture | Size | Expected | Result | Time |
|---------|------|----------|--------|------|
| `Unit-test-tegaki.png` | 500×80 | `手書きの文字サンプル` | ✓ exact | ~1 400 ms |
| `Unit-test-tategaki.png` | 70×450 | `言語モデルのテスト` | ✓ exact | ~12 200 ms |
| `Unit-test-horizontal.png` | 600×80 | `データを正確に読み取る` | ✓ exact | ~34 000 ms |
| `ubunchu01_02.png` (smoke) | 1000×1414 | non-empty Japanese | ✓ `そのためには、自分では自分の` | ~4 500 ms |

Full test run (debug, unoptimized):

```
running 4 tests
tegaki (1394 ms): "手書きの文字サンプル"  (expected: "手書きの文字サンプル")
test test_tegaki ... ok
manga smoke (4454 ms): "そのためには、自分では自分の"
test test_real_manga_smoke ... ok
tategaki (12238 ms): "言語モデルのテスト"  (expected: "言語モデルのテスト")
test test_tategaki ... ok
horizontal (33977 ms): "データを正確に読み取る"  (expected: "データを正確に読み取る")
test test_horizontal ... ok

test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 35.44s
```

The horizontal fixture is slower because beam search explores more candidates for a longer sequence
in a debug (unoptimized) build. Release builds will be significantly faster.

---

## Architecture

```
DynamicImage
    │
    ▼  preprocess()
    │  grayscale → RGB, center-pad to square (white fill)
    │  resize 224×224 Lanczos3, normalize mean=0.5 std=0.5
    │  shape: [1, 3, 224, 224]
    │
    ▼  encoder_model.onnx  (ViT)
    │  last_hidden_state: [1, 196, 768]
    │
    ▼  decoder_model.onnx  (BERT, beam search loop)
    │  4 beams, batched decoder calls per step
    │  no_repeat_ngram_size=3, length_penalty=2.0
    │  stops at EOS (token_id=102) or 300 steps
    │
    ▼  vocab.txt line-indexed decode
    │
    String  (raw Japanese)
```

Beam search parameters match `generation_config.json` from the original model:
`num_beams=4`, `length_penalty=2.0`, `no_repeat_ngram_size=3`.

**Decoder note**: `decoder_model.onnx` is the non-merged (no-KV-cache) export.
Each step re-runs full attention over the growing sequence — O(n²) in token count,
but acceptable for short manga text (~5–30 tokens typical).

---

## Crate extraction plan

The `MangaOcr` struct in `src/lib.rs` is intentionally self-contained.
Extraction to a `manga-ocr-rs` crate requires:

1. Add `#[cfg(feature = "onnx")]` gating (mirror `jp_detect` pattern)
2. Replace `anyhow` with `thiserror` for a library-friendly error type
3. Optionally expose a `TextRecognizer` trait for mock-ability in tests
4. Publish to crates.io; lenzu adds it alongside `jp_detect`

---

## Credits

ONNX model: [mayocream/manga-ocr-onnx](https://huggingface.co/mayocream/manga-ocr-onnx)  
Original model: [kha-white/manga-ocr](https://github.com/kha-white/manga-ocr)

```bibtex
@misc{kha-white2021mangaocr,
  title  = {Manga OCR},
  author = {Maciej Budyś},
  year   = {2021},
  url    = {https://github.com/kha-white/manga-ocr}
}
```
