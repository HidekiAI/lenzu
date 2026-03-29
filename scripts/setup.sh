#!/bin/bash
# Install system dependencies required to build all workspace members.
# Run once after cloning, or when hitting missing-library build errors.
#
# Covers:
#   lenzu_client  -- GTK3, Cairo, Pango, x11rb, arboard
#   lenzu_server  -- Tauri 2.x (WebKit2GTK 4.1, OpenSSL)
#   prototypes    -- GTK4 + Graphene

set -euo pipefail

# Unless we can `apt update` in IPv4 ONLY, the IPv6 endpoint is heinously slow, in fact, at times will timeout, so I don't do `update` unless I've not done it in months...
echo sudo apt update
sudo apt install -y \
    build-essential \
    pkg-config \
    libgtk-3-dev \
    libcairo2-dev \
    libpango1.0-dev \
    libgdk-pixbuf-2.0-dev \
    libx11-dev \
    libxcb1-dev \
    libssl-dev \
    libwebkit2gtk-4.1-dev \
    libjavascriptcoregtk-4.1-dev \
    libgtk-4-dev \
    libgraphene-1.0-dev \
    fonts-noto-cjk \
    fonts-ipafont-gothic

echo ""
echo "System dependencies installed."
echo ""
echo "Next steps:"
echo "  1. Install Rust:       https://rustup.rs"
echo "  2. Install Tauri CLI:  cargo install tauri-cli --version '^2.0.0' --locked"
echo "  3. Install Node (for lenzu_server): https://nodejs.org  (or: nvm install --lts)"
echo "  4. Set API key:        export OPENROUTER_API_KEY=sk-your-key-here"
echo "  5. Run:                ./scripts/run.sh"
