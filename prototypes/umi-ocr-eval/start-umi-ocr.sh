#!/usr/bin/env bash
# Start Umi-OCR in Docker (headless mode, PaddleOCR backend).
# HTTP API available at http://127.0.0.1:1224
set -euo pipefail

CONTAINER_NAME="umi-ocr-eval"
IMAGE_NAME="umi-ocr-paddle"

# Check if already running
if docker ps --format '{{.Names}}' | grep -q "^${CONTAINER_NAME}$"; then
    echo "[umi-ocr] already running — http://127.0.0.1:1224"
    exit 0
fi

# Remove stopped container if it exists
docker rm -f "${CONTAINER_NAME}" 2>/dev/null || true

# Build image if not present
if ! docker image inspect "${IMAGE_NAME}" &>/dev/null; then
    echo "[umi-ocr] building Docker image (first time, ~5 min)..."
    TMPDIR="$(mktemp -d)"
    wget -q -O "${TMPDIR}/Dockerfile" \
        "https://raw.githubusercontent.com/hiroi-sora/Umi-OCR_runtime_linux/main/Dockerfile"
    docker build -t "${IMAGE_NAME}" "${TMPDIR}"
    rm -rf "${TMPDIR}"
fi

echo "[umi-ocr] starting container..."
docker run -d \
    --name "${CONTAINER_NAME}" \
    -e HEADLESS=true \
    -p 1224:1224 \
    "${IMAGE_NAME}"

# Wait for API to be ready
echo -n "[umi-ocr] waiting for API"
for i in $(seq 1 60); do
    if curl -sf http://127.0.0.1:1224/api/ocr/get_options >/dev/null 2>&1; then
        echo " ready!"
        echo "[umi-ocr] API available at http://127.0.0.1:1224"
        exit 0
    fi
    echo -n "."
    sleep 2
done

echo " TIMEOUT — check: docker logs ${CONTAINER_NAME}"
exit 1
