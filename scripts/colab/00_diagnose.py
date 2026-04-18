# Diagnostic: inspect the model's actual class and auto_map
# Paste into a Colab cell, or run via !python scripts/colab/00_diagnose.py

from transformers import AutoConfig

for model_id in ["sbintuitions/sarashina2.2-ocr", "sbintuitions/sarashina2.2-vision-3b"]:
    print(f"=== {model_id} ===")
    config = AutoConfig.from_pretrained(model_id, trust_remote_code=True)
    print(f"model_type: {config.model_type}")
    print(f"architectures: {config.architectures}")
    print(f"auto_map: {getattr(config, 'auto_map', None)}")
    print()
