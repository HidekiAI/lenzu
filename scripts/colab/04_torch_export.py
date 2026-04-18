# Cell 4: Raw torch.onnx.export probe for sarashina2_vision
# Run this after 02_preflight.py if optimum-based 03_export.py fails on the
# vision models. optimum has no OnnxConfig for model_type "sarashina2_vision",
# so we bypass it and call torch.onnx.export directly.
#
# This script is a probe: it diagnoses the forward signature, runs a real
# forward pass, then attempts ONNX export. If the export fails on a custom
# op or control flow, the traceback tells us what to patch.

import os
import inspect
import traceback
import torch
from PIL import Image
from transformers import AutoModelForCausalLM, AutoProcessor

MODEL_ID = "sbintuitions/sarashina2.2-ocr"
OUT_DIR = "/content/ocr_3b_torch_onnx"
OUT_PATH = f"{OUT_DIR}/model.onnx"
os.makedirs(OUT_DIR, exist_ok=True)

print(f"HF_HOME: {os.environ.get('HF_HOME', '(default)')}")
print(f"MODEL_ID: {MODEL_ID}")

# --- Load (uses Drive cache if preflight ran) ---
print(f"\n=== Loading {MODEL_ID} ===")
model = AutoModelForCausalLM.from_pretrained(
    MODEL_ID, trust_remote_code=True, torch_dtype=torch.float16
).eval().cuda()
processor = AutoProcessor.from_pretrained(MODEL_ID, trust_remote_code=True)

# --- Phase A: diagnose forward signature ---
sig = inspect.signature(model.forward)
print("\n=== model.forward signature ===")
for name, param in sig.parameters.items():
    default = "<required>" if param.default is inspect.Parameter.empty else repr(param.default)
    print(f"  {name}: default={default}")

# --- Phase B: build realistic inputs via chat template ---
# Sarashina2-vision is Qwen2-VL-style: pixel_values is flat-packed patches
# and the text prompt MUST contain image placeholder tokens that the chat
# template inserts.
dummy_img = Image.new("RGB", (448, 448), color=(200, 200, 200))
messages = [
    {
        "role": "user",
        "content": [
            {"type": "image"},
            {"type": "text", "text": "Read the text in this image."},
        ],
    }
]
prompt = processor.apply_chat_template(
    messages, add_generation_prompt=True, tokenize=False
)
print(f"\n=== chat-templated prompt ===\n{prompt!r}")

inputs = processor(
    images=[dummy_img], text=[prompt], return_tensors="pt"
).to("cuda")

for k in list(inputs.keys()):
    if torch.is_tensor(inputs[k]) and inputs[k].is_floating_point():
        inputs[k] = inputs[k].to(torch.float16)

print("\n=== processor output ===")
for k, v in inputs.items():
    if torch.is_tensor(v):
        print(f"  {k}: shape={tuple(v.shape)}, dtype={v.dtype}")
    else:
        print(f"  {k}: {type(v).__name__} = {v}")

# --- Phase C: sanity-check a real forward before exporting ---
# `lm_kwargs` is marked <required> in the signature; pass an empty dict.
print("\n=== test forward ===")
with torch.no_grad():
    out = model(**inputs, lm_kwargs={})
print(f"  logits shape: {out.logits.shape}, dtype: {out.logits.dtype}")

# --- Phase D: wrap in a positional-args module for ONNX tracing ---
kw_keys = [k for k in inputs.keys() if torch.is_tensor(inputs[k])]
args = tuple(inputs[k] for k in kw_keys)

class ExportWrapper(torch.nn.Module):
    def __init__(self, m, keys):
        super().__init__()
        self.m = m
        self.keys = keys

    def forward(self, *tensors):
        kwargs = dict(zip(self.keys, tensors))
        return self.m(**kwargs, lm_kwargs={}).logits

wrapper = ExportWrapper(model, kw_keys).eval()

dynamic_axes = {"logits": {0: "batch", 1: "seq_len"}}
for k in kw_keys:
    if "input_ids" in k or "attention_mask" in k:
        dynamic_axes[k] = {0: "batch", 1: "seq_len"}
    elif "pixel_values" in k or "image" in k.lower() or "visual_mask" in k:
        dynamic_axes[k] = {0: "batch"}

print(f"\n=== exporting to {OUT_PATH} ===")
print(f"  input_names: {kw_keys}")
print(f"  dynamic_axes: {dynamic_axes}")

try:
    torch.onnx.export(
        wrapper,
        args,
        OUT_PATH,
        input_names=kw_keys,
        output_names=["logits"],
        opset_version=17,
        dynamic_axes=dynamic_axes,
        do_constant_folding=False,
    )
    size_mb = sum(
        os.path.getsize(os.path.join(OUT_DIR, f))
        for f in os.listdir(OUT_DIR)
    ) / (1024 * 1024)
    print(f"\n=== SUCCESS: {OUT_DIR} ({size_mb:.1f} MB total) ===")
    print(f"  files: {os.listdir(OUT_DIR)}")
except Exception as e:
    print(f"\n=== export FAILED: {type(e).__name__} ===")
    traceback.print_exc()
    print("\nNext step: inspect the traceback -- if it's a custom op or")
    print("control-flow issue, we may need torch.onnx.dynamo_export or a")
    print("custom OnnxConfig registered with optimum's TasksManager.")
