---
license: apache-2.0
base_model: sbintuitions/sarashina2.2-ocr
model_creator: sbintuitions
model_type: sarashina2
---

# Model Assets Directory

This folder contains the ONNX runtime sessions for the Lenzu. Due to the size of the binary files, they are excluded from Git. You can either download pre-built artifacts or generate them yourself.

## Expected Layout

```text
models/
├── sarashina2.2/
│   ├── encoder_model.onnx
│   ├── decoder_model.onnx
│   ├── decoder_with_past_model.onnx
│   ├── config.json
│   └── tokenizer.json
└── dbnet.onnx
```

**Important:** Ensure the `.onnx` and `.onnx_data` files (if present) remain in the same folder.

---

## Option 1: Setup Script (Recommended)

The repo setup script will download the pre-built ONNX artifacts automatically if they are not already present:

```bash
./scripts/setup.sh
```

To skip the ~30 GB Sarashina download (e.g. on a metered connection):

```bash
./scripts/setup.sh --skip-sarashina
```

## Option 2: Manual Download from Hugging Face

Download the pre-optimized ONNX files from the HidekiAI Hugging Face repository.

### Step-by-Step:

1. **Visit the Repository:** Go to https://huggingface.co/HidekiAI/sarashina2.2-ocr-onnx
2. **Select the Version:** Navigate to the **Files and versions** tab.
3. **Download the ZIP:** Find the relevant bundle (e.g., `sarashina2.2-ocr-fp16.zip`) and click the download icon.
4. **Placement:** Extract the contents of the ZIP into this `models/` directory so the layout matches the tree above.

## Option 3: Manual Generation (DIY)

If you wish to forge your own ONNX files from the original weights using a GPU, follow these steps.

### Step-by-Step Colab Setup:

1. **Open the Notebook:** Open [`notebooks/sarashina_export.ipynb`](../notebooks/sarashina_export.ipynb) in Google Colab (T4 or A100 GPU recommended).
   - Or create a new notebook at [colab.new](https://colab.new) and copy the cells manually.
2. **Set Hardware Accelerator:** Go to `Runtime > Change runtime type` and select **T4 GPU** (or better).
3. **Run All Cells:** The notebook will:
   - Install `optimum` and `transformers`
   - Download the original Sarashina 2.2 weights from [sbintuitions/sarashina2.2-ocr](https://huggingface.co/sbintuitions/sarashina2.2-ocr)
   - Export to ONNX with fp16
   - Zip and save the artifacts to your Google Drive
4. **Retrieve Assets:** Once complete, download the ZIP from your Google Drive and extract it into this `models/` directory.

> **Note:** The notebook will ask for Google Drive permission to save the final ZIP directly to your Drive.

---

## Credits and Attribution

This project uses model weights developed by **SB Intuitions** (https://www.sbintuitions.co.jp/).

- **Base Models:** [sarashina2.2-ocr](https://huggingface.co/sbintuitions/sarashina2.2-ocr) / [sarashina2.2-vision-3b](https://huggingface.co/sbintuitions/sarashina2.2-vision-3b)
- **License:** Apache License 2.0
- **Export Credits:** Conversion to ONNX and quantization was performed by [HidekiAI](https://huggingface.co/HidekiAI).
