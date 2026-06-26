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

## Design docs & learnings

All technical design documents, investigation notes, and lessons learned from
these prototypes live in the **[CodeMonkeyNinja/lenzu wiki](https://github.com/CodeMonkeyNinja/lenzu/wiki)**. The docs there cross-reference prototype source code by GitHub link.

Per-prototype doc coverage:

| Prototype | Relevant pages in [lenzu wiki](https://github.com/CodeMonkeyNinja/lenzu/wiki) |
|-----------|------------------------------------------------------------------------------------------|
| [`jp_ocr_app`](prototypes/jp_ocr_app/) | [prototypes-desktop-issues](https://github.com/CodeMonkeyNinja/lenzu/wiki/prototypes-desktop-issues) — GTK4 migration, Cairo spinner fix, flash_alpha white-window fix, async capture |
| [`x11-gtk-lens-test`](prototypes/x11-gtk-lens-test/) | [prototypes-desktop-issues](https://github.com/CodeMonkeyNinja/lenzu/wiki/prototypes-desktop-issues) — GTK4 migration, transparent draw fix, SCIM noise |
| [`x11-gtk3-lens-test`](prototypes/x11-gtk3-lens-test/) | [technical-design.phase4-predetect](https://github.com/CodeMonkeyNinja/lenzu/wiki/technical-design.phase4-predetect) — ort version notes |
| [`gtk4_dialogbox_test`](prototypes/gtk4_dialogbox_test/) | [prototypes-desktop-issues](https://github.com/CodeMonkeyNinja/lenzu/wiki/prototypes-desktop-issues) — GTK4 migration decisions |
| [`gtk_gdk_test`](prototypes/gtk_gdk_test/) | [prototypes-desktop-issues](https://github.com/CodeMonkeyNinja/lenzu/wiki/prototypes-desktop-issues) — GTK4 migration decisions |
| [`manga-ocr-test`](prototypes/manga-ocr-test/) | [technical-design.manga-ocr](https://github.com/CodeMonkeyNinja/lenzu/wiki/technical-design.manga-ocr) — full ONNX OCR tier design |
| [`dbnet-test`](prototypes/dbnet-test/) | [technical-design.phase4-predetect](https://github.com/CodeMonkeyNinja/lenzu/wiki/technical-design.phase4-predetect) — DBNet algorithm, defaults, Cargo deps |
| [`dbnet-ocr-pipeline`](prototypes/dbnet-ocr-pipeline/) | [scores](https://github.com/CodeMonkeyNinja/lenzu/wiki/scores), [technical-design](https://github.com/CodeMonkeyNinja/lenzu/wiki/technical-design) — benchmark results, architecture |
| [`sarashina-vision-py`](prototypes/sarashina-vision-py/) | [technical-design.sarashina](https://github.com/CodeMonkeyNinja/lenzu/wiki/technical-design.sarashina), [model-evaluation](https://github.com/CodeMonkeyNinja/lenzu/wiki/model-evaluation) — vision-3b CPU rejection |
| [`sarashina-onnx-test`](prototypes/sarashina-onnx-test/) | [prototypes-desktop-issues](https://github.com/CodeMonkeyNinja/lenzu/wiki/prototypes-desktop-issues) — workspace dependency graph |
| [`umi-ocr-eval`](prototypes/umi-ocr-eval/) | [model-evaluation](https://github.com/CodeMonkeyNinja/lenzu/wiki/model-evaluation), [session-log-2026-04-14](https://github.com/CodeMonkeyNinja/lenzu/wiki/session-log-2026-04-14) — eval results (0/3 pass) |
| [`paddleocr-vl-manga`](prototypes/paddleocr-vl-manga/) | [session-log-2026-04-14](https://github.com/CodeMonkeyNinja/lenzu/wiki/session-log-2026-04-14) — eval results (2/3 pass) |
| [`winit-test`](prototypes/winit-test/) | [lenzu-desktop-issues](https://github.com/CodeMonkeyNinja/lenzu/wiki/lenzu-desktop-issues) — GTK replacement evaluation |
| [`text-enrichment-test`](prototypes/text-enrichment-test/) | [planning-ollama-to-llamacpp](https://github.com/CodeMonkeyNinja/lenzu/wiki/planning-ollama-to-llamacpp) — Ollama health-check API notes |

## Shared infrastructure

A few directories outside `prototypes/` survive in this archive because
the prototypes depend on them:

- `assets/` — test fixtures (`Unit-test-{yokogaki,tategaki,tegaki}.png`,
  `ubunchu01_02.png`, etc.) and demo media
- `models/` — Apache 2.0 LICENSE/NOTICE for Sarashina models pulled
  on-demand by `setup.sh`
- `scripts/` — `setup.sh` (model fetcher), `test-ocr.sh`, `colab/`

## License

Rust prototype code: MIT (see [`LICENSE`](./LICENSE)).

Individual prototypes may depend on third-party models or libraries
with their own licenses — notably AGPL-3.0 for Ultralytics YOLO and
StabRise DBNet ONNX models that several prototypes pulled at runtime.
See each prototype's `README.md` for specifics.

---

*Active product home: [CodeMonkeyNinja/lenzu](https://github.com/CodeMonkeyNinja/lenzu)*
