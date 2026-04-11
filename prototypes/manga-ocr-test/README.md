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

## Credits

ONNX model: [mayocream/manga-ocr-onnx](https://huggingface.co/mayocream/manga-ocr-onnx)  
Original model: [kha-white/manga-ocr](https://github.com/kha-white/manga-ocr)
