#!/bin/bash
# Stop on any error
set -e

# --- CONFIGURATION ---
ASSETS_DIR="assets"
DET_TAR="ch_PP-OCRv4_det_infer.tar"
REC_TAR="japan_PP-OCRv3_rec_infer.tar"
DICT_FILE="japan_dict.txt"

# 1. PRE-CHECK
# If the converted ONNX files exist, we don't need Python or Downloads.
if [ -f "$ASSETS_DIR/det_model.onnx" ] && [ -f "$ASSETS_DIR/rec_model.onnx" ]; then
    echo "--- ASSETS VERIFIED ---"
    echo "ONNX models already exist in $ASSETS_DIR/. Skipping build steps."
else
    echo "--- STARTING ASSET PREPARATION (RESUME & PERSISTENCE ENABLED) ---"

    # 2. PYTHON ENVIRONMENT SETUP
    export PATH="$HOME/.local/bin:$PATH"

    # Check if paddle2onnx is already installed to skip pip drama
    if ! command -v paddle2onnx &> /dev/null; then
        echo "Installing paddle2onnx via pip with high-persistence flags..."
        # --default-timeout: 100s to handle slow read/connect
        # --resume-retries: 10 attempts to pick up where it left off
        python3 -m pip install paddle2onnx \
            --user \
            --break-system-packages \
            --default-timeout 100 \
            --retries 10
    else
        echo "paddle2onnx is already installed. Proceeding to downloads..."
    fi

    # 3. DIRECTORY SETUP
    mkdir -p "$ASSETS_DIR"
    # We use subshell or direct paths to avoid getting lost in cd calls
    
    # 4. DOWNLOAD & RESUME LOGIC (WGET)
    # -c allows resuming partial downloads
    # --tries=0 means keep retrying forever
    echo "Downloading Detection Model..."
    wget -c -4 --tries=20 -O "$ASSETS_DIR/$DET_TAR" \
        https://paddleocr.bj.bcebos.com/PP-OCRv4/chinese/ch_PP-OCRv4_det_infer.tar

    echo "Downloading Japanese Recognition Model..."
    wget -c -4 --tries=20 -O "$ASSETS_DIR/$REC_TAR" \
        https://paddleocr.bj.bcebos.com/PP-OCRv3/multilingual/japan_PP-OCRv3_rec_infer.tar

    echo "Downloading Dictionary..."
    wget -c -4 --tries=20 -O "$ASSETS_DIR/$DICT_FILE" \
        https://raw.githubusercontent.com/PaddlePaddle/PaddleOCR/main/ppocr/utils/dict/japan_dict.txt

    # 5. EXTRACTION
    echo "Extracting archives..."
    tar -xf "$ASSETS_DIR/$DET_TAR" -C "$ASSETS_DIR"
    tar -xf "$ASSETS_DIR/$REC_TAR" -C "$ASSETS_DIR"

    # 6. CONVERSION TO ONNX
    echo "Converting Paddle models to ONNX..."
    
    # We explicitly define the binary path in case PATH export is acting up
    P2O_BIN="$HOME/.local/bin/paddle2onnx"

    # Detection
    $P2O_BIN --model_dir "$ASSETS_DIR/ch_PP-OCRv4_det_infer" \
             --model_filename inference.pdmodel \
             --params_filename inference.pdiparams \
             --save_file "$ASSETS_DIR/det_model.onnx"

    # Recognition
    $P2O_BIN --model_dir "$ASSETS_DIR/japan_PP-OCRv3_rec_infer" \
             --model_filename inference.pdmodel \
             --params_filename inference.pdiparams \
             --save_file "$ASSETS_DIR/rec_model.onnx"

    # 7. CLEANUP (Surgical)
    echo "Cleaning up heavy native Paddle files..."
    rm "$ASSETS_DIR/$DET_TAR" "$ASSETS_DIR/$REC_TAR"
    rm -rf "$ASSETS_DIR/ch_PP-OCRv4_det_infer/"
    rm -rf "$ASSETS_DIR/japan_PP-OCRv3_rec_infer/"
fi

# 8. SYSTEM DEPENDENCIES & RUST BUILD
echo "Ensuring Linux dependencies are present..."
sudo apt-get install -y libgomp1 libclang-dev

echo "Building Rust Release..."
cargo build --release

echo "--- BUILD COMPLETE ---"
