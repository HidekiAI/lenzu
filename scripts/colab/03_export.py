# Cell 3: Export models to ONNX and save to Google Drive
# Paste this into a Colab cell and run after 02_preflight.py.

import shutil
import subprocess
import os
from google.colab import drive

# 1. Mount Drive immediately so we can warp files there instantly
drive.mount('/content/drive', force_remount=True)
DRIVE_PATH = "/content/drive/MyDrive/Lenzu_Exports"
os.makedirs(DRIVE_PATH, exist_ok=True)

models_to_forge = [
    {
        "id": "sbintuitions/sarashina2.2-ocr",
        "name": "ocr_3b",
        "task": "image-to-text",
        "custom": True
    },
    {
        "id": "sbintuitions/sarashina2.2-0.5b-instruct-v0.1",
        "name": "mini_500m",
        "task": "text-generation-with-past",
        "custom": False
    },
    {
        "id": "sbintuitions/sarashina2.2-vision-3b",
        "name": "base_vision_3b",
        "task": "image-to-text",
        "custom": True
    }
]

for model in models_to_forge:
    out_dir = f"./{model['name']}_onnx"
    zip_file = f"{model['name']}_export.zip"

    if os.path.exists(f"{DRIVE_PATH}/{zip_file}"):
        print(f"Skipping {model['name']}, already in Drive.")
        continue

    print(f"Forging {model['name']} with task {model['task']}...")

    try:
        if model["custom"]:
            # Sarashina2-vision registers only AutoModelForCausalLM in its auto_map.
            # optimum has no ONNX config for model_type "sarashina2_vision", so
            # main_export will likely fail at config lookup -- fall back to raw
            # torch.onnx.export if so.
            from transformers import AutoModelForCausalLM, AutoProcessor
            from optimum.exporters.onnx import main_export

            pt_model = AutoModelForCausalLM.from_pretrained(
                model["id"], trust_remote_code=True, torch_dtype="auto"
            )
            processor = AutoProcessor.from_pretrained(
                model["id"], trust_remote_code=True
            )
            main_export(
                model_name_or_path=model["id"],
                output=out_dir,
                task="image-text-to-text",
                model=pt_model,
                trust_remote_code=True,
                device="cuda",
                dtype="fp16",
                no_post_process=False,
            )
            processor.save_pretrained(out_dir)
        else:
            # Standard architecture: CLI works fine
            cmd = (
                f'optimum-cli export onnx'
                f' --model "{model["id"]}"'
                f' --task "{model["task"]}"'
                f' --trust-remote-code'
                f' --device cuda'
                f' --dtype fp16'
                f' "{out_dir}"'
            )
            result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
            if result.returncode != 0:
                print(f"STDOUT: {result.stdout[-2000:]}")
                print(f"STDERR: {result.stderr[-2000:]}")
                raise RuntimeError(f"Export failed with exit code {result.returncode}")

        shutil.make_archive(model['name'], 'zip', out_dir)
        shutil.move(f"{model['name']}.zip", f"{DRIVE_PATH}/{zip_file}")
        shutil.rmtree(out_dir)
        print(f"{model['name']} SUCCESS.")

    except Exception as e:
        print(f"Error during {model['name']}: {e}")
        import traceback
        traceback.print_exc()
