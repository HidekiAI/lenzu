---
license: apache-2.0
base_model: sbintuitions/sarashina2.2-0.5b-instruct-v0.1
model_creator: sbintuitions
model_type: sarashina2
---

# Model Assets Directory

This folder holds the runtime assets for Lenzu. Binary weights are excluded from Git; you can download pre-built artifacts or generate them yourself.

## ONNX vs Python Runtime

Lenzu uses two runtimes depending on the model:

| Model | Runtime | Why |
| --- | --- | --- |
| `sarashina2.2-0.5b-instruct-v0.1` | ONNX (via `ort`) | Standard `LlamaForCausalLM`; exports cleanly. |
| `sarashina2.2-ocr` | HuggingFace `transformers` (Python) | Custom `sarashina2_vision` architecture; SB Intuitions designed it for their Python pipeline. |
| `sarashina2.2-vision-3b` | HuggingFace `transformers` (Python) | Same. |

The vision models use Qwen2-VL-style patterns (packed patches, MRoPE, custom `autograd.Function` ops) that the ONNX tracer cannot traverse. Rather than hack around SB Intuitions' intended design, Lenzu invokes them through their official runtime.

## Expected Layout

```text
models/
├── sarashina2.2-mini/           # 0.5b instruct, ONNX
│   ├── model.onnx
│   ├── model.onnx_data          # if present
│   ├── config.json
│   └── tokenizer.json
└── dbnet.onnx
```

The vision models (`sarashina2.2-ocr`, `sarashina2.2-vision-3b`) are not stored here; they are pulled into the HuggingFace cache on first use by the Python runtime.

**Important:** Keep each ONNX file and its sibling `.onnx_data` file (if present) in the same folder.

---

## Option 1: Setup Script (Recommended)

The repo setup script downloads the pre-built ONNX artifact if it is not already present:

```bash
./scripts/setup.sh
```

To skip the Sarashina download (e.g. on a metered connection):

```bash
./scripts/setup.sh --skip-sarashina
```

## Option 2: Manual Download from Hugging Face

Download the pre-exported ONNX file from the HidekiAI Hugging Face repository.

1. Visit https://huggingface.co/HidekiAI/sarashina2.2-mini-onnx
2. Go to the **Files and versions** tab.
3. Download the bundled zip.
4. Extract into this `models/` directory so the layout matches the tree above.

## Option 3: Manual Generation (DIY)

Forge the ONNX file from the original weights on a GPU.

1. Open [`notebooks/sarashina_export.ipynb`](../notebooks/sarashina_export.ipynb) in Google Colab (T4 or better).
2. Set `Runtime > Change runtime type` to **T4 GPU**.
3. Run all cells. The notebook will:
   - Install `optimum` and `transformers`
   - Download the original weights from [sbintuitions/sarashina2.2-0.5b-instruct-v0.1](https://huggingface.co/sbintuitions/sarashina2.2-0.5b-instruct-v0.1)
   - Export to ONNX with fp16
   - Zip and save the artifact to your Google Drive
   - Pre-cache the vision models to Drive for later Python-runtime use
4. Download the ZIP from your Google Drive and extract into this `models/` directory.

> **Note:** The notebook asks for Google Drive permission to save artifacts.

---

## Credits and Attribution

This project uses model weights developed by **SB Intuitions** (https://www.sbintuitions.co.jp/).

- **Base Models:**
  - [sarashina2.2-0.5b-instruct-v0.1](https://huggingface.co/sbintuitions/sarashina2.2-0.5b-instruct-v0.1)
  - [sarashina2.2-ocr](https://huggingface.co/sbintuitions/sarashina2.2-ocr)
  - [sarashina2.2-vision-3b](https://huggingface.co/sbintuitions/sarashina2.2-vision-3b)
- **License:** Apache License 2.0
- **Export Credits:** Conversion of the 0.5b text model to ONNX was performed by [HidekiAI](https://huggingface.co/HidekiAI). The vision models are used unchanged.
