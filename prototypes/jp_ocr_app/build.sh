#!/bin/bash
set -e
echo "Checking Linux dependencies..."
#sudo apt-get update 
sudo apt-get install -y libgomp1 libclang-dev
mkdir -p tests/data
echo "Building Release..."
cargo build --release
