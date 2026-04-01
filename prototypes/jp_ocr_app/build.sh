#!/bin/bash
set -e

echo "--- 1. VERIFYING SYSTEM LIBRARIES ---"
sudo apt-get install -y libx11-dev libssl-dev pkg-config libgtk-3-dev libgdk-pixbuf-2.0-dev \
 fonts-noto-cjk fonts-ipafont-gothic

mkdir -p assets
mkdir -p tests/data

echo "--- 2. RUNNING PIPELINE TESTS ---"
cargo test --manifest-path prototypes/jp_ocr_app/Cargo.toml

echo "--- 3. COMPILING RELEASE BINARY ---"
cargo build --release --manifest-path prototypes/jp_ocr_app/Cargo.toml

echo "------------------------------------------------"
echo "BUILD SUCCESSFUL"
echo "OCR History will be stored in: /dev/shm/ocr_history.txt"
echo "------------------------------------------------"
