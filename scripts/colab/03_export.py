# Cell 3: Export the Sarashina text model to ONNX and save to Google Drive.
# Paste this into a Colab cell and run after 02_preflight.py.
#
# Only the 0.5b instruct model is exported to ONNX. The vision models
# (sarashina2.2-ocr, sarashina2.2-vision-3b) were designed by SB Intuitions
# for the HuggingFace transformers Python runtime, not ONNX, and must be
# run through their intended pipeline.
#
# When a GPU is present we build BOTH artifacts:
#   - mini_500m_fp16.zip  (cuda/fp16 -- smaller, optimized for GPU runtime)
#   - mini_500m_fp32.zip  (cpu/fp32  -- larger, portable, best for CPU runtime)
# On a CPU-only runtime we skip the fp16 build.

import gc
import os
import shutil
import subprocess
import torch
from google.colab import drive

drive.mount('/content/drive', force_remount=True)
DRIVE_PATH = "/content/drive/MyDrive/Lenzu_Exports"
os.makedirs(DRIVE_PATH, exist_ok=True)

MODEL_ID = "sbintuitions/sarashina2.2-0.5b-instruct-v0.1"
MODEL_NAME = "mini_500m"
TASK = "text-generation-with-past"

builds = []
if torch.cuda.is_available():
    builds.append({"device": "cuda", "dtype": "fp16", "suffix": "fp16"})
else:
    print("WARNING: no CUDA -- skipping fp16 build")
builds.append({"device": "cpu", "dtype": "fp32", "suffix": "fp32"})

for i, cfg in enumerate(builds, 1):
    print(f"\n===== BUILD {i}/{len(builds)}: {cfg['device']}/{cfg['dtype']} =====")
    out_dir = f"./{MODEL_NAME}_{cfg['suffix']}_onnx"
    zip_file = f"{MODEL_NAME}_{cfg['suffix']}.zip"
    drive_zip = f"{DRIVE_PATH}/{zip_file}"

    if os.path.exists(drive_zip):
        print(f"Skipping {zip_file}, already in Drive ({os.path.getsize(drive_zip) / 1e6:.1f} MB).")
        continue

    # Clean any leftover output dir from a killed previous attempt.
    if os.path.exists(out_dir):
        shutil.rmtree(out_dir)

    # Free memory between builds so CPU fp32 export isn't squeezed after
    # GPU fp16 export leaves tensors lingering.
    gc.collect()
    if torch.cuda.is_available():
        torch.cuda.empty_cache()

    print(f"Forging {zip_file}... (cpu/fp32 can take 10-20 min, be patient)")
    cmd = [
        "optimum-cli", "export", "onnx",
        "--model", MODEL_ID,
        "--task", TASK,
        "--trust-remote-code",
        "--device", cfg["device"],
        "--dtype", cfg["dtype"],
        out_dir,
    ]
    # Stream output live so Colab shows progress — capture_output=True hides
    # it and makes a slow CPU export look like a hang.
    result = subprocess.run(cmd)
    if result.returncode != 0:
        raise RuntimeError(f"Export failed (exit={result.returncode}) for {cfg}")

    archive_base = f"{MODEL_NAME}_{cfg['suffix']}"
    shutil.make_archive(archive_base, 'zip', out_dir)
    shutil.move(f"{archive_base}.zip", drive_zip)
    shutil.rmtree(out_dir)
    print(f"{zip_file} SUCCESS ({os.path.getsize(drive_zip) / 1e6:.1f} MB).")

print("\nAll builds done.")
