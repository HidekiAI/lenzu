# Cell 3: Export the Sarashina text model to ONNX and save to Google Drive.
# Paste this into a Colab cell and run after 02_preflight.py.
#
# Only the 0.5b instruct model is exported to ONNX. The vision models
# (sarashina2.2-ocr, sarashina2.2-vision-3b) were designed by SB Intuitions
# for the HuggingFace transformers Python runtime, not ONNX, and must be
# run through their intended pipeline.

import os
import shutil
import subprocess
from google.colab import drive

drive.mount('/content/drive', force_remount=True)
DRIVE_PATH = "/content/drive/MyDrive/Lenzu_Exports"
os.makedirs(DRIVE_PATH, exist_ok=True)

MODEL_ID = "sbintuitions/sarashina2.2-0.5b-instruct-v0.1"
MODEL_NAME = "mini_500m"
TASK = "text-generation-with-past"

# fp16 only works on CUDA; fall back to fp32/CPU if GPU is unavailable
import torch
if torch.cuda.is_available():
    DEVICE, DTYPE = "cuda", "fp16"
else:
    DEVICE, DTYPE = "cpu", "fp32"
    print("WARNING: no CUDA -- falling back to CPU/fp32 (slower, larger file)")

out_dir = f"./{MODEL_NAME}_onnx"
zip_file = f"{MODEL_NAME}_export.zip"

if os.path.exists(f"{DRIVE_PATH}/{zip_file}"):
    print(f"Skipping {MODEL_NAME}, already in Drive.")
else:
    print(f"Forging {MODEL_NAME} with task {TASK} on {DEVICE}/{DTYPE}...")
    cmd = (
        f'optimum-cli export onnx'
        f' --model "{MODEL_ID}"'
        f' --task "{TASK}"'
        f' --trust-remote-code'
        f' --device {DEVICE}'
        f' --dtype {DTYPE}'
        f' "{out_dir}"'
    )
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"STDOUT: {result.stdout[-2000:]}")
        print(f"STDERR: {result.stderr[-2000:]}")
        raise RuntimeError(f"Export failed with exit code {result.returncode}")

    shutil.make_archive(MODEL_NAME, 'zip', out_dir)
    shutil.move(f"{MODEL_NAME}.zip", f"{DRIVE_PATH}/{zip_file}")
    shutil.rmtree(out_dir)
    print(f"{MODEL_NAME} SUCCESS.")
