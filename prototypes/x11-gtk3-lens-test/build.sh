#!/bin/bash
set -e

# THE TARGET: Official YOLOv8n (Nano) Checksum
EXPECTED_HASH="dd48a79dd7fec8ca25fde4eca742ff7bca23b27e2e903eb23bc1d9f83a459bd2"
MODEL_FILE="yolov8n.onnx"

# We try three different sources in order of reliability
URL_1="https://webnn.github.io/webnn-samples/object_detection/models/yolov8n.onnx"
URL_2="https://github.com/ultralytics/assets/releases/download/v8.1.0/yolov8n.onnx"

echo "--- 1. Validating System Libraries ---"
sudo apt update && sudo apt install -y libgtk-3-dev libgdk-pixbuf-2.0-dev libcairo2-dev libpango1.0-dev libxcb-shape0-dev libxcb-xfixes0-dev pkg-config wget curl

echo "--- 2. Validating AI Model ---"

check_hash() {
    if [ ! -f "$MODEL_FILE" ]; then return 1; fi
    ACTUAL_HASH=$(sha256sum "$MODEL_FILE" | cut -d ' ' -f 1)
    if [ "$ACTUAL_HASH" = "$EXPECTED_HASH" ]; then return 0; else return 1; fi
}

if check_hash; then
    echo "✅ Model Verified."
else
    rm -f "$MODEL_FILE"
    echo "🔄 Downloading from WebNN Mirror..."
    if ! curl -L -o "$MODEL_FILE" "$URL_1"; then
        echo "Mirror 1 failed, trying GitHub Assets..."
        curl -L -o "$MODEL_FILE" "$URL_2"
    fi

    if check_hash; then
        echo "✅ Checksum Verified."
    else
        echo "❌ FATAL: Checksum mismatch. File is likely a 404 HTML page."
        # Print the first few lines of the file to see if it's HTML (404 page)
        head -n 5 "$MODEL_FILE"
        exit 1
    fi
fi

echo "--- 3. Building ---"
cargo build --release
