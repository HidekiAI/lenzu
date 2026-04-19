"""Sarashina2.2-vision-3b smoke test.

Loads the vision model once, then runs a list of (image_path, prompt) jobs.
Prints the model output + latency for each job.

Usage:
  python test_vision.py                          # run built-in test battery
  python test_vision.py <image> [prompt]         # run a single image
  python test_vision.py --device cpu|cuda|auto   # override device

The first run downloads ~6 GB of weights to ~/.cache/huggingface.
"""
from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

import torch
from PIL import Image
from transformers import AutoModelForCausalLM, AutoProcessor

MODEL_ID = "sbintuitions/sarashina2.2-vision-3b"

REPO_ROOT = Path(__file__).resolve().parents[2]
ASSETS = REPO_ROOT / "assets"

# (label, image_path, prompt)
DEFAULT_BATTERY = [
    ("tate",    ASSETS / "Unit-test-tategaki.png",    "画像の日本語テキストを読んで、英語に翻訳してください。"),
    ("yoko",    ASSETS / "Unit-test-yokogaki.png",    "画像の日本語テキストを読んで、英語に翻訳してください。"),
    ("te",      ASSETS / "Unit-test-sample-texts.png", "画像の日本語テキストを読んで、英語に翻訳してください。"),
    ("ubunchu", ASSETS / "ubunchu01_02.png",           "画像の日本語テキストを読んで、英語に翻訳してください。"),
]


def pick_device(override: str) -> str:
    if override == "auto":
        return "cuda" if torch.cuda.is_available() else "cpu"
    return override


def load(device: str):
    t0 = time.perf_counter()
    dtype = torch.float16 if device == "cuda" else torch.float32
    processor = AutoProcessor.from_pretrained(MODEL_ID, trust_remote_code=True)
    model = AutoModelForCausalLM.from_pretrained(
        MODEL_ID,
        trust_remote_code=True,
        torch_dtype=dtype,
        device_map=device,
    )
    model.eval()
    print(f"model loaded in {time.perf_counter() - t0:.2f}s on {device} ({dtype})")
    return processor, model


def run_one(processor, model, device: str, image_path: Path, prompt: str, max_new: int = 256) -> tuple[str, float]:
    image = Image.open(image_path).convert("RGB")
    messages = [
        {
            "role": "user",
            "content": [
                {"type": "image", "image": image},
                {"type": "text", "text": prompt},
            ],
        }
    ]
    inputs = processor.apply_chat_template(
        messages,
        add_generation_prompt=True,
        tokenize=True,
        return_tensors="pt",
        return_dict=True,
    ).to(device)

    t0 = time.perf_counter()
    with torch.inference_mode():
        out = model.generate(
            **inputs,
            max_new_tokens=max_new,
            do_sample=False,
        )
    elapsed = time.perf_counter() - t0

    prompt_len = inputs["input_ids"].shape[1]
    new_tokens = out[0][prompt_len:]
    text = processor.decode(new_tokens, skip_special_tokens=True)
    return text, elapsed


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("image", nargs="?", help="single image path (omit for default battery)")
    parser.add_argument("prompt", nargs="?", default="画像の日本語テキストを読んで、英語に翻訳してください。")
    parser.add_argument("--device", choices=["auto", "cpu", "cuda"], default="auto")
    parser.add_argument("--max-new", type=int, default=256)
    args = parser.parse_args()

    device = pick_device(args.device)
    print(f"device: {device}")
    processor, model = load(device)

    if args.image:
        jobs = [("custom", Path(args.image), args.prompt)]
    else:
        jobs = [(label, p, prompt) for (label, p, prompt) in DEFAULT_BATTERY if p.exists()]
        missing = [p for (_, p, _) in DEFAULT_BATTERY if not p.exists()]
        for m in missing:
            print(f"warning: missing {m}", file=sys.stderr)

    for label, image_path, prompt in jobs:
        print(f"\n=== {label}: {image_path.name} ===")
        print(f"prompt: {prompt}")
        try:
            text, elapsed = run_one(processor, model, device, image_path, prompt, args.max_new)
        except Exception as e:
            print(f"ERROR: {e}")
            continue
        print(f"latency: {elapsed:.2f}s")
        print(f"output:\n{text}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
