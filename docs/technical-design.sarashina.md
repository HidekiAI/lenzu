# Technical Design: Sarashina 2.2 Integration

**Status**: Design / prototype planning
**Scope**: Integrate SB Intuitions' Sarashina 2.2 models into Lenzu as a Japanese-native alternative to the current LLM backends.
**Target tier**: New optional enrichment/OCR path, orthogonal to the existing manga-ocr-rs → Ollama → OpenRouter chain.

---

## 1. Motivation

Current enrichment/translation options (Ollama Gemma, OpenRouter) are general-purpose and trained primarily on English. For Japanese-heavy workloads a Japanese-native model should produce better furigana, reading disambiguation, and translation quality.

[SB Intuitions](https://www.sbintuitions.co.jp/) publishes the Sarashina 2.2 family under Apache 2.0:

| Repo | Size | Role | Runtime |
|------|------|------|---------|
| [`sbintuitions/sarashina2.2-0.5b-instruct-v0.1`](https://huggingface.co/sbintuitions/sarashina2.2-0.5b-instruct-v0.1) | ~0.5 B | Text instruct (translation/furigana/romaji) | **ONNX via `ort`** |
| [`sbintuitions/sarashina2.2-ocr`](https://huggingface.co/sbintuitions/sarashina2.2-ocr) | ~3 B | Image → Japanese text (OCR) | **HF `transformers` (Python)** |
| [`sbintuitions/sarashina2.2-vision-3b`](https://huggingface.co/sbintuitions/sarashina2.2-vision-3b) | ~3 B | Image → description/translation | **HF `transformers` (Python)** |

**Design constraint**: We use only these three upstream URLs. No forks, no alternative architecture hacks. If a model cannot be exported cleanly, we honor SB Intuitions' intended runtime instead of reshaping the graph.

---

## 2. Why two runtimes

### 2.1 Text model → ONNX

`sarashina2.2-0.5b-instruct-v0.1` is a standard `LlamaForCausalLM`. `optimum-cli export onnx` converts it cleanly. Two artifacts are produced when a GPU is available on the export host:

- `mini_500m_fp16.zip` — cuda/fp16, smaller, GPU-optimized
- `mini_500m_fp32.zip` — cpu/fp32, larger, portable, best for CPU inference

On a CPU-only export runtime only the fp32 artifact is produced.

Export is performed in `notebooks/sarashina_export.ipynb` (Colab T4 / A100) via the helper scripts in `scripts/colab/`.

### 2.2 Vision models → Python runtime

`sarashina2.2-ocr` and `sarashina2.2-vision-3b` share the custom `sarashina2_vision` architecture. Qwen2-VL-style patterns prevent a clean ONNX export:

- Packed pixel patches with a per-image `image_grid_thw` tensor
- MRoPE requires 3D `position_ids` shape `[3, batch, seq_len]`
- Custom `torch.autograd.Function` with `vmap` rules that the ONNX tracer cannot traverse — attempts hit `RuntimeError: unordered_map::at` inside `custom_function_call_vmap_generate_rule`
- Forcing `attn_implementation="eager"` does not help; the custom ops are upstream of attention

Rather than hack the graph (which would diverge from SB Intuitions' official checkpoints), Lenzu invokes these models through `transformers` in Python — the runtime the authors shipped them for.

---

## 3. Integration plan

### 3.1 Tier placement

```
[current chain — unchanged]
  jp_detect → manga-ocr-rs (ONNX) → text enrichment (Ollama) → DONE
                                  ↘ (timeout / disabled)
  image → gemma4:e2b → glm-ocr → free remote → paid remote

[new Sarashina paths, both optional and independently toggleable]
  text enrichment path:
    manga-ocr-rs output  ──►  sarashina2.2-mini (ONNX, local)  ──►  JP→EN + furigana

  vision-first OCR path (alternative to manga-ocr-rs):
    image  ──►  sarashina2.2-ocr (Python sidecar)  ──►  JP text
             or sarashina2.2-vision-3b (Python sidecar) ──► JP text + translation
```

The text enrichment path is the cheaper, higher-impact win. The vision sidecar path is a heavier experiment.

### 3.2 Config additions (proposed)

```jsonc
{
  // Sarashina ONNX text enrichment
  "sarashina_mini_model_dir": "models/sarashina2.2-mini",  // null disables
  "sarashina_mini_prefer": "fp32",                          // "fp16" | "fp32"

  // Sarashina vision sidecar
  "sarashina_vision_enabled": false,
  "sarashina_vision_variant": "ocr",                        // "ocr" | "vision-3b"
  "sarashina_vision_endpoint": "http://127.0.0.1:8765",     // HTTP sidecar
  "sarashina_vision_timeout_secs": 20
}
```

Default OFF for the vision sidecar — it requires a Python environment the user may not want to install.

---

## 4. Prototype: text-only ONNX tier (`sarashina-mini-rs`)

Mirrors the manga-ocr-rs shape so integration is mechanical.

### 4.1 Crate layout

```
prototypes/sarashina-mini-test/
  Cargo.toml
  src/
    main.rs        # CLI: stdin text → stdout enriched text
    model.rs       # SarashinaMini::new(model_dir), ::generate(prompt, max_new)
    tokenizer.rs   # load tokenizer.json via `tokenizers` crate
    sampling.rs    # greedy + temperature + top-k (start greedy)
  tests/
    fixtures/*.txt
```

### 4.2 Dependencies

| Crate | Role |
|-------|------|
| `ort` | ONNX runtime bindings (share ORT binary with `jp_detect` / `manga-ocr-rs`) |
| `tokenizers` | Load `tokenizer.json`, encode/decode |
| `ndarray` | Tensor I/O |
| `anyhow` | Error handling (prototype only) |

### 4.3 Inference flow

```
prompt string
  ├─ tokenizer.encode(prompt)          → input_ids [1, n]
  ├─ attention_mask = ones_like        → [1, n]
  └─ loop until EOS or max_new_tokens:
       forward pass (with past_key_values)
       next_id = argmax(logits[:, -1, :])
       append to ids, update past_kv
tokenizer.decode(new_ids, skip_special=true)
```

The exported model is `text-generation-with-past` so KV cache reuse is available from the start.

### 4.4 Milestones

1. **M1 — Load & tokenize**: construct session, round-trip "日本語" through tokenizer. Success: encode → decode is lossless.
2. **M2 — Single forward pass**: feed a 32-token prompt, dump logits shape. Success: shape matches `[1, 32, vocab]`.
3. **M3 — Greedy generation, no cache**: naive loop, O(n²). Success: generates coherent continuation of "こんにちは、" within 5 s CPU.
4. **M4 — With-past KV cache**: wire up `past_key_values.*.key/value` I/O binding. Success: 2–4× speedup on M3 benchmark.
5. **M5 — Prompt template**: apply the model's chat template (system/user/assistant) via string format. Success: instruction-following works on "次の文を英訳してください: <text>".
6. **M6 — Bench**: latency on CPU fp32 vs GPU fp16 for a 256-token generation. Success: numbers recorded in §6.

### 4.5 Promotion into lenzu

Once M5 passes, promote to a reusable crate (published as `sarashina-mini-rs`, matching the `manga-ocr-rs` precedent). Wire it into the enrichment slot:

```
manga-ocr-rs output
  ├─ sarashina_mini_model_dir set? → sarashina-mini-rs
  └─ else → existing Ollama text enrichment
```

Ollama enrichment remains the fallback; Sarashina mini is added alongside, not replacing.

---

## 5. Prototype: Python sidecar for vision models

The vision models cannot run in-process from Rust. Three bridging options, in order of decreasing preference:

### 5.1 Option A — HTTP sidecar (recommended)

Small FastAPI / Flask service packaged under `python/sarashina_vision/`:

```
python/sarashina_vision/
  server.py         # FastAPI app
  pyproject.toml    # pinned torch, transformers, accelerate
  README.md
  scripts/run.sh    # uvicorn launcher

Endpoints:
  POST /ocr          body: multipart image
                     resp: { "text": "…", "latency_ms": 1234 }
  POST /vl           body: multipart image + optional "prompt"
                     resp: { "text": "…", "latency_ms": 1234 }
  GET  /health       resp: { "model": "sarashina2.2-ocr", "device": "cuda" }
```

**Why this option**:
- Language boundary is clean — JSON over localhost, no ABI coupling
- Process isolation: a Python crash can't take down lenzu
- Existing pattern in lenzu: `lenzu_server` (Electron HUD) already runs as a spawned sibling process with lifecycle managed by `scripts/run.sh`. Sarashina sidecar reuses that pattern.
- Reuses the same pre-cached HuggingFace weights from the notebook's `HF_HOME`

**Rust side**: a thin `SarashinaVisionClient` in `lenzu/src/ocr/` wraps `reqwest::blocking` (or async if the capture path is async) and returns `Option<String>`.

### 5.2 Option B — PyO3 embedded

Embed CPython in the lenzu binary via `pyo3`. Rejected for prototype because:
- Build complexity (ABI-stable Python, GIL handling per capture)
- Couples lenzu crashes to Python exceptions
- Makes distribution harder (users need a matching Python version at runtime)

Revisit only if sidecar latency proves unacceptable.

### 5.3 Option C — subprocess per call

Spawn `python run_ocr.py <image_path>` per capture. Rejected: cold-start loads the 3 B model every call (~10–30 s). Only viable as a one-off CLI, not a live tier.

### 5.4 Sidecar milestones (Option A)

1. **SM1 — Minimal `/health`**: FastAPI skeleton, loads the model once on startup, responds 200. Success: `curl localhost:8765/health` returns the model name.
2. **SM2 — `/ocr` endpoint**: accepts PNG/JPEG, runs the chat-template + multimodal `{"type": "image"}` pipeline, returns text. Success: matches the reference output from `sarashina2.2-ocr`'s README on the demo image.
3. **SM3 — Rust client**: `SarashinaVisionClient::ocr(&DynamicImage) -> Option<String>`. Success: shift+click → text in HUD.
4. **SM4 — Lifecycle**: `scripts/run.sh` starts the sidecar before lenzu, `trap` kills it on exit (mirror the Ollama/lenzu_server pattern). Success: no orphan Python processes after `ctrl-c`.
5. **SM5 — `/vl` endpoint with prompt**: accept `prompt` field for `vision-3b`. Success: JP→EN translation via a single multimodal call.
6. **SM6 — Bench**: CPU vs GPU latency, memory footprint. Decide whether to ship this tier by default or gate behind a flag.

### 5.5 Packaging & install

- `scripts/setup.sh` gains a `--with-sarashina-vision` flag that creates `python/sarashina_vision/.venv`, `pip install`s pinned deps, and optionally pre-downloads weights via `huggingface-cli`.
- Without the flag, lenzu ignores the vision path entirely (no runtime dependency on Python).

---

## 6. Benchmarks (to populate)

| Config | Model | Device | Latency (256 tok / image) | Memory | Notes |
|---|---|---|---|---|---|
| text fp16 | mini_500m | CUDA T4 | TBD | TBD | generation + KV cache |
| text fp32 | mini_500m | CPU (i7) | TBD | TBD | generation + KV cache |
| vision   | sarashina2.2-ocr | CUDA T4 | TBD | TBD | single OCR call via sidecar |
| vision   | sarashina2.2-ocr | CPU | TBD | TBD | sidecar, may be unusable |
| vision   | vision-3b | CUDA T4 | TBD | TBD | OCR + translation |

Populate after M6 / SM6.

---

## 7. Open questions

1. **Tokenizer parity**: Does `tokenizers 0.20` handle the `sarashina2.2-0.5b-instruct` `tokenizer.json` without warnings? (The Sarashina family historically uses SentencePiece — confirm the HF export is BPE-wrapped.)
2. **Chat template**: Is the template shipped in `tokenizer_config.json` or only in Python `processor.apply_chat_template`? If the latter, we port the template to Rust.
3. **License**: Apache 2.0 on upstream weights is already honored via `models/README.md` frontmatter. Confirm the sidecar README carries the same attribution block.
4. **Default on/off**: Ship with `sarashina_mini_model_dir` auto-detected (default on if artifact present) vs strictly opt-in? Lean toward auto-detect — matches the `manga-ocr-rs` pattern.
5. **GPU on end-user machines**: The `fp16` artifact assumes a CUDA-capable ORT runtime. Most end users will be on CPU — fp32 is the safer default. Ship fp32 in the release zip, offer fp16 as a separate download for GPU users.

---

## 8. References

- Base models:
  - https://huggingface.co/sbintuitions/sarashina2.2-0.5b-instruct-v0.1
  - https://huggingface.co/sbintuitions/sarashina2.2-ocr
  - https://huggingface.co/sbintuitions/sarashina2.2-vision-3b
- Export pipeline: `notebooks/sarashina_export.ipynb`, `scripts/colab/03_export.py`
- Pre-exported artifact (text model): https://huggingface.co/HidekiAI/sarashina2.2-mini-onnx
- Models directory: `models/README.md`
- Sibling design docs: `docs/technical-design.manga-ocr.md`, `docs/technical-design.phase4-predetect.md`
