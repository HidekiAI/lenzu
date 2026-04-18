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

for cfg in builds:
    out_dir = f"./{MODEL_NAME}_{cfg['suffix']}_onnx"
    zip_file = f"{MODEL_NAME}_{cfg['suffix']}.zip"
    drive_zip = f"{DRIVE_PATH}/{zip_file}"

    if os.path.exists(drive_zip):
        print(f"Skipping {zip_file}, already in Drive.")
        continue

    print(f"Forging {zip_file} on {cfg['device']}/{cfg['dtype']}...")
    cmd = (
        f'optimum-cli export onnx'
        f' --model "{MODEL_ID}"'
        f' --task "{TASK}"'
        f' --trust-remote-code'
        f' --device {cfg["device"]}'
        f' --dtype {cfg["dtype"]}'
        f' "{out_dir}"'
    )
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"STDOUT: {result.stdout[-2000:]}")
        print(f"STDERR: {result.stderr[-2000:]}")
        raise RuntimeError(f"Export failed with exit code {result.returncode}")

    archive_base = f"{MODEL_NAME}_{cfg['suffix']}"
    shutil.make_archive(archive_base, 'zip', out_dir)
    shutil.move(f"{archive_base}.zip", drive_zip)
    shutil.rmtree(out_dir)
    print(f"{zip_file} SUCCESS.")
