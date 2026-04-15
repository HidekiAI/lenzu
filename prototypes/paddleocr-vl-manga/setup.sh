#!/usr/bin/env bash
# Download PaddleOCR-VL-For-Manga GGUF model files (~1.8 GB).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MODEL_DIR="${SCRIPT_DIR}/model"
HF_BASE="https://huggingface.co/adambarbato/PaddleOCR-VL-For-Manga-GGUF/resolve/main"

FILES=(
    "PaddleOCR-VL-For-Manga-BF16.gguf"
    "PaddleOCR-VL-For-Manga-mmproj-BF16.gguf"
)

mkdir -p "${MODEL_DIR}"

# Prefer huggingface-cli, fall back to wget/curl
if command -v huggingface-cli &>/dev/null; then
    echo "[setup] downloading via huggingface-cli..."
    huggingface-cli download \
        adambarbato/PaddleOCR-VL-For-Manga-GGUF \
        --local-dir "${MODEL_DIR}" \
        --include "*.gguf"
else
    echo "[setup] huggingface-cli not found, downloading via wget..."
    for f in "${FILES[@]}"; do
        dest="${MODEL_DIR}/${f}"
        if [[ -f "$dest" ]]; then
            echo "[setup] ${f} already exists, skipping"
            continue
        fi
        echo "[setup] downloading ${f}..."
        wget -q --show-progress -O "$dest" "${HF_BASE}/${f}"
    done
fi

echo ""
echo "[setup] model files:"
ls -lh "${MODEL_DIR}"/*.gguf 2>/dev/null || echo "  (no .gguf files found)"

echo ""
echo "[setup] done. Run ./serve.sh to start the server."
