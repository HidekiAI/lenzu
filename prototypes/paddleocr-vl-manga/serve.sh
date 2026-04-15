#!/usr/bin/env bash
# Start llama-server with PaddleOCR-VL-For-Manga GGUF model.
# OpenAI-compatible API at http://127.0.0.1:9999
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MODEL_DIR="${SCRIPT_DIR}/model"
MODEL="${MODEL_DIR}/PaddleOCR-VL-For-Manga-BF16.gguf"
MMPROJ="${MODEL_DIR}/PaddleOCR-VL-For-Manga-mmproj-BF16.gguf"
PORT=9999

# Verify model files exist
for f in "$MODEL" "$MMPROJ"; do
    if [[ ! -f "$f" ]]; then
        echo "ERROR: missing $(basename "$f") — run ./setup.sh first" >&2
        exit 1
    fi
done

# Verify llama-server is installed
if ! command -v llama-server &>/dev/null; then
    echo "ERROR: llama-server not found on PATH" >&2
    echo "" >&2
    echo "Install llama.cpp (needs Feb 2026+ build with PaddleOCR arch support):" >&2
    echo "  git clone https://github.com/ggml-org/llama.cpp" >&2
    echo "  cd llama.cpp && cmake -B build -DGGML_CUDA=ON && cmake --build build -j" >&2
    echo "  # binary at build/bin/llama-server" >&2
    exit 1
fi

echo "[serve] starting llama-server on port ${PORT}..."
echo "[serve] model:  $(basename "$MODEL")"
echo "[serve] mmproj: $(basename "$MMPROJ")"
echo ""

exec llama-server \
    -m "$MODEL" \
    --mmproj "$MMPROJ" \
    --host 0.0.0.0 \
    --port "$PORT" \
    --n-gpu-layers 999 \
    -c 32768 \
    --temp 0
