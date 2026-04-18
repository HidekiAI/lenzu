# Cell 1: Install dependencies with rust-accelerated transfer layer
# Paste this into a Colab cell and run it first.

import subprocess, sys, os

subprocess.check_call([
    sys.executable, "-m", "pip", "install", "-U",
    "optimum[onnxruntime-gpu]", "transformers", "accelerate", "hf_transfer"
])

os.environ["HF_HUB_ENABLE_HF_TRANSFER"] = "1"
print("Setup complete.")
