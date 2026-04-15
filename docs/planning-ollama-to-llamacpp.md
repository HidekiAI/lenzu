# Planning: Replace Ollama with direct llama.cpp

Status: **analysis only** — no code changes yet.

## Motivation

Ollama wraps llama.cpp in a Go HTTP server with model management, scheduling, and a
registry system. For a single-purpose OCR pipeline that loads models at startup and
runs them until exit, this overhead is unnecessary. Direct llama.cpp (via `llama-cpp-2`
Rust crate or `llama-server` binary) is lighter and gives full control over model
loading, VRAM management, and inference parameters.

Analogy: Ollama is a "truck" (heavy, carries everything); llama.cpp is a "bike"
(lightweight, fast, purpose-built).

## Current Ollama usage in lenzu

### API format

All calls use OpenAI-compatible `/v1/chat/completions` on `localhost:11434`.
This is the **same API** that llama-server exposes — high compatibility.

### Models (referenced by Ollama name)

| Model | Type | Role | Size |
|---|---|---|---|
| `glm-ocr` | Vision | Primary LLM OCR (fast, ~15s) | ~2.2 GB |
| `gemma4:e2b` | Vision | LLM fallback OCR (~50s) | ~7.2 GB |
| `qwen2.5:3b` | Text-only | Post-OCR enrichment (furigana/romaji/translation) | ~1.8 GB |
| `qwen2.5vl:7b` | Vision | Candidate local fallback | ~4.5 GB |

### Fallback chain (client.rs)

```
1. Primary local:   glm-ocr        @ localhost:11434
2. Local fallback:  gemma4:e2b     @ localhost:11434  (same port, Ollama switches model)
3. Free remote:     openrouter/free
4. Paid remote:     google/gemini-2.0-flash-001
```

### Features used

| Feature | Where | llama-server equivalent |
|---|---|---|
| `/v1/chat/completions` | client.rs | Same endpoint (compatible) |
| `stream: true` (SSE) | client.rs | Same (compatible) |
| `logprobs: true` | client.rs | Same (compatible) |
| `options.num_ctx` | config.rs | `-c` flag at server start |
| Model switching by name | client.rs fallback chain | Different model = different server port or model reload |
| Runner process killing | client.rs:642-651 | Standard process management |
| Health check `/api/tags` | text-enrichment-test | `/health` endpoint |
| Vision image payloads | client.rs | Same `image_url` format (compatible) |

### Ollama-specific code to remove

- `config.rs`: `options.num_ctx` injection (~lines 134-138, 181-182, 218-219)
- `client.rs`: `pkill -KILL -f "ollama runner.*--model"` (~line 642-651)
- `scripts/setup.sh`: Ollama install, Docker setup, model pulling (~lines 146-369)
- `scripts/run.sh`: Ollama startup/health checks

## What would change

### Model references

Ollama uses names (`gemma4:e2b`). llama-server uses GGUF file paths. Need a mapping:

```
glm-ocr       → /path/to/glm-ocr.gguf
gemma4:e2b    → /path/to/gemma4-e2b.gguf
qwen2.5:3b   → /path/to/qwen2.5-3b.gguf
```

GGUF files can be extracted from Ollama's blob store (`~/.ollama/models/blobs/`) or
downloaded directly from HuggingFace.

### Model switching (the main challenge)

Ollama transparently switches models on the same port. With llama-server, options:

**Option A: One server per model, different ports**
```
llama-server -m glm-ocr.gguf        --port 9001
llama-server -m gemma4-e2b.gguf     --port 9002
llama-server -m qwen2.5-3b.gguf     --port 9003
```
- Pro: All models hot, instant switching
- Con: VRAM — all models loaded simultaneously. 2.2 + 7.2 + 1.8 = ~11 GB (won't fit 8 GB)

**Option B: Single server, reload on fallback**
```
llama-server -m glm-ocr.gguf --port 9000
# On fallback: kill, restart with gemma4-e2b.gguf
```
- Pro: Single port, VRAM-friendly
- Con: Reload latency (~5-10s per model switch)

**Option C: In-process via llama-cpp-2 Rust crate**
```rust
// Load model in-process, no HTTP server
let model = LlamaModel::load("glm-ocr.gguf")?;
let result = model.inference(prompt, image)?;
```
- Pro: No server, no HTTP overhead, can swap models programmatically
- Con: Requires Rust bindings integration, more code
- This is the long-term ideal — same pattern as manga-ocr-rs using `ort`

**Option D: Hybrid — primary in-process, fallbacks via server**
- PaddleOCR-VL (primary OCR) loaded in-process via `llama-cpp-2`
- Text enrichment model loaded in-process (small, text-only)
- Vision fallback models (gemma4) only loaded on demand
- Remote fallbacks unchanged (OpenRouter/Gemini)

### VRAM budget (Quadro M4000, 8 GB)

| Configuration | VRAM needed | Fits? |
|---|---|---|
| manga-ocr-rs (ONNX) + PaddleOCR-VL (GGUF) | ~140 MB + ~1.8 GB | Yes |
| + qwen2.5:3b (enrichment) | + ~1.8 GB | Yes (~3.7 GB) |
| + glm-ocr (vision fallback) | + ~2.2 GB | Tight (~5.9 GB) |
| + gemma4:e2b | + ~7.2 GB | No (>8 GB) |

Realistic deployment: PaddleOCR-VL + qwen2.5:3b in-process, with gemma4/glm-ocr as
on-demand fallbacks (loaded only when primary fails).

## Migration path

1. **Phase 0 (now)**: PaddleOCR-VL prototype via llama-server — done, evaluated
2. **Phase 1**: Add `llama-cpp-2` as dependency, load PaddleOCR-VL in-process alongside manga-ocr-rs
3. **Phase 2**: Move text enrichment (qwen2.5:3b) from Ollama to in-process llama-cpp-2
4. **Phase 3**: Move vision fallback (glm-ocr) from Ollama to llama-server or in-process
5. **Phase 4**: Remove Ollama dependency entirely from setup.sh and run.sh

Each phase is independently shippable. Ollama can coexist with llama-cpp during migration
since both speak the same OpenAI-compatible API.

## Files affected

- `lenzu/Cargo.toml` — add `llama-cpp-2` dependency
- `lenzu/src/config.rs` — model paths instead of Ollama names, remove `num_ctx` option
- `lenzu/src/client.rs` — replace HTTP calls with in-process inference (or point to llama-server ports)
- `scripts/setup.sh` — download GGUFs directly, build llama.cpp, remove Ollama install
- `scripts/run.sh` — start llama-server instances instead of Ollama daemon
