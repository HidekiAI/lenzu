# Cell 2: Pre-flight -- mount Drive and pre-cache model weights
# Paste this into a Colab cell and run after 01_setup.py.

from google.colab import drive
import os

# 1. Mount your 20TB Drive
drive.mount('/content/drive')

# 2. Point the Hugging Face cache to your Drive
# This way, the 8GB is saved PERMANENTLY in your 20TB pool
os.environ["HF_HOME"] = "/content/drive/MyDrive/HF_Cache"
os.environ["HF_HUB_ENABLE_HF_TRANSFER"] = "1"

from huggingface_hub import snapshot_download

models = ["sbintuitions/sarashina2.2-ocr", "sbintuitions/sarashina2.2-vision-3b"]
for m in models:
    print(f"Downloading {m} directly to Google Drive...")
    snapshot_download(repo_id=m, cache_dir="/content/drive/MyDrive/HF_Cache")
    print(f"{m} is now safely stored in your Drive!")
