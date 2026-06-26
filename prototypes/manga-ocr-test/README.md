# manga-ocr-test — OCR prototype using manga-ocr-rs

Prototype for running Japanese manga OCR using the
[`manga-ocr-rs`](https://crates.io/crates/manga-ocr-rs) crate.

Returns raw Japanese text — no translation, no furigana stripping.

---

## Model files

Models (~441 MB) are downloaded automatically on first `cargo build` by
`manga-ocr-rs`'s build script.  They are cached in `~/.cache/manga-ocr-rs/`.

Override the location by setting `MANGA_OCR_MODELS_DIR` before building.

---

## Run

```bash
# default model dir (auto-downloaded by manga-ocr-rs)
cargo run -p manga-ocr-test -- path/to/japanese-text.png

# with explicit model dir
cargo run -p manga-ocr-test -- path/to/image.png /path/to/models
```

---

## Test Results (2026-04-15)

Rescaled test images (manga-bubble-realistic sizes).

| Image | Size | Expected | Got | OCR Time | Result |
|---|---|---|---|---|---|
| Unit-test-yokogaki.png | 360×197 | `データを正確に読み取る` | `データを正確に読み取る` | ~1.4 s | **PASS** (exact) |
| Unit-test-tategaki.png | 480×262 | `『言語モデルのテスト』` | `「言語モデルのテスト」` | ~1.5 s | **PASS** (bracket variant) |
| Unit-test-tegaki.png | 480×262 | `手書きの文字サンプル` | `手書きの文字サンプル` | ~1.5 s | **PASS** (exact) |

**3/3 PASS.** Tategaki reads the correct text but uses single corner brackets `「」`
instead of double `『』` — the bracket style is ambiguous at this resolution.
When cropped tighter by DBNet (142×262), manga-ocr-rs returns the correct `『』`.

See [unified benchmark](https://github.com/CodeMonkeyNinja/lenzu/wiki/scores) for comparison across all OCR engines.

## Credits

ONNX model: [mayocream/manga-ocr-onnx](https://huggingface.co/mayocream/manga-ocr-onnx)  
Original model: [kha-white/manga-ocr](https://github.com/kha-white/manga-ocr)
