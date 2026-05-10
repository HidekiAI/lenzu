# lenzu-prototypes (formerly `HidekiAI/lenzu`)

> [!IMPORTANT]
> **Lenzu (the product) has moved to [CodeMonkeyNinja/lenzu](https://github.com/CodeMonkeyNinja/lenzu).**
>
> This repo is now an **archive** of the prototype crates that informed
> Lenzu's design. The shippable product — Rust GTK lens, Electron HUD,
> AppImage/.deb packaging, release pipeline — lives at the new home above.

## What's here

`prototypes/` contains experimental crates that explored various corners of
the Lenzu problem space. Several fed insights into the production crate now
at [CodeMonkeyNinja/lenzu](https://github.com/CodeMonkeyNinja/lenzu); many
never went anywhere and remain only as historical record.

Notable prototypes (each has its own `README.md` under `prototypes/<name>/`):

- `manga-ocr-test` — manga-ocr-rs integration harness (now shipped as the
  [`manga-ocr-rs`](https://github.com/CodeMonkeyNinja/manga-ocr-rs) crate)
- `dbnet-test`, `dbnet-ocr-pipeline` — DBNet text-detection experiments
- `sarashina-onnx-test`, `sarashina-vision-py` — Sarashina LLM evaluation
- `mecab-test` — MeCab morphological analysis (now shipping as the
  [`mecab-furigana-rs`](https://crates.io/crates/mecab-furigana-rs) crate)
- `winit-test`, `gtk_gdk_test`, `gtk4_dialogbox_test`, `x11-gtk3-lens-test`
  — windowing/compositor experiments (X11 vs Wayland, GTK3 vs GTK4)
- `jp_ocr_app` — early YOLO-based detector (superseded by DBNet)
- `paddleocr-vl-manga`, `umi-ocr-eval` — third-party OCR evaluations
- `text-enrichment-test`, `kakasi-cli-test` — text-processing experiments

## Shared infrastructure

A few directories outside `prototypes/` survive in this archive because
the prototypes depend on them:

- `assets/` — test fixtures (`Unit-test-{yokogaki,tategaki,tegaki}.png`,
  `ubunchu01_02.png`, etc.) and demo media
- `models/` — Apache 2.0 LICENSE/NOTICE for Sarashina models pulled
  on-demand by `setup.sh`
- `scripts/` — `setup.sh` (model fetcher), `test-ocr.sh`, `colab/`
- `docs/` — design docs (`technical-design.{manga-ocr,phase4-predetect,sarashina}.md`),
  benchmark log (`scores.md`), and historical planning

## License

Rust prototype code: MIT (see [`LICENSE`](./LICENSE)).

Individual prototypes may depend on third-party models or libraries
with their own licenses — notably AGPL-3.0 for Ultralytics YOLO and
StabRise DBNet ONNX models that several prototypes pulled at runtime.
See each prototype's `README.md` for specifics.

---

*Active product home: [CodeMonkeyNinja/lenzu](https://github.com/CodeMonkeyNinja/lenzu)*
