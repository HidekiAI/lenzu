#!/bin/bash
set -e

echo "--- 1. VERIFYING SYSTEM LIBRARIES ---"
sudo apt-get install -y libx11-dev libssl-dev pkg-config libgtk-3-dev libgdk-pixbuf-2.0-dev \
 fonts-noto-cjk fonts-ipafont-gothic \
    libopencv-dev clang libclang-dev    \
    pkg-config

export OPENCV_LINK_LIBS=opencv_4
export PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig:/usr/share/pkgconfig

mkdir -p assets
mkdir -p tests/data

echo "--- 2. RUNNING PIPELINE TESTS ---"
cargo test

echo "--- 3. COMPILING RELEASE BINARY ---"
cargo build --release

echo "------------------------------------------------"
echo "BUILD SUCCESSFUL"
echo "OCR History will be stored in: /dev/shm/ocr_history.txt"
echo "------------------------------------------------"
