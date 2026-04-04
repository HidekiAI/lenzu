#!/bin/bash
# Install system dependencies required to build all workspace members.
# Run once after cloning, or when hitting missing-library build errors.
#
# Covers:
#   lenzu         -- GTK3, Cairo, Pango, x11rb, arboard
#   lenzu_server  -- Electron overlay (Node.js via lenzu_server/scripts/setup.sh)
#   prototypes    -- GTK4 + Graphene
#   ollama        -- local LLM backend (Docker container, gemma4:e2b)
#
# Flags:
#   --skip-ollama   skip Docker/Ollama setup (use if you prefer OpenRouter only)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SKIP_OLLAMA=false
for arg in "$@"; do [[ "$arg" == "--skip-ollama" ]] && SKIP_OLLAMA=true; done

OLLAMA_IMAGE="ollama/ollama"
OLLAMA_VOLUME="lenzu-ollama-data"
OLLAMA_MODEL="gemma4:e2b"

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
    libgtk-4-dev \
    libgraphene-1.0-dev \
    fonts-noto-cjk \
    fonts-ipafont-gothic

echo ""
echo "System dependencies installed."

# ── Rust compile check ────────────────────────────────────────────────────────
echo "Running cargo check for lenzu..."
cd "$REPO_ROOT/lenzu"
cargo check
cd "$REPO_ROOT"

echo ""
echo "Setting up Node.js and pnpm for lenzu_server (Electron)..."
cd "$REPO_ROOT/lenzu_server"
./scripts/setup.sh
cd "$REPO_ROOT"
# ── Docker / Ollama ───────────────────────────────────────────────────────────
if [[ "$SKIP_OLLAMA" == "true" ]]; then
    echo ""
    echo "Skipping Ollama setup (--skip-ollama)."
else
    echo ""
    echo "Setting up Ollama (local LLM backend via Docker)..."

    if ! command -v docker &>/dev/null; then
        echo "  Docker not found — installing docker.io..."
        sudo apt install -y docker.io
        sudo usermod -aG docker "$USER"
        echo ""
        echo "  IMPORTANT: Docker group membership requires a log-out/log-in to take effect."
        echo "  After re-login, re-run this script to continue Ollama setup."
        echo "  Or run the remaining steps as root: sudo docker pull $OLLAMA_IMAGE"
    else
        echo "  Docker found: $(docker --version)"

        echo "  Pulling $OLLAMA_IMAGE image..."
        docker pull "$OLLAMA_IMAGE"

        echo "  Creating persistent volume '$OLLAMA_VOLUME' for model storage..."
        docker volume create "$OLLAMA_VOLUME" &>/dev/null || true

        echo "  Pulling model $OLLAMA_MODEL (this may take several minutes on first run)..."
        docker run --rm \
            -v "${OLLAMA_VOLUME}:/root/.ollama" \
            "$OLLAMA_IMAGE" \
            ollama pull "$OLLAMA_MODEL"

        echo "  Ollama + $OLLAMA_MODEL ready."
    fi
fi

echo ""
echo "Next steps:"
echo "  1. Install Rust:       https://rustup.rs"
echo "  2. Install Node (for lenzu_server): https://nodejs.org  (or: nvm install --lts)"
echo "  3a. Local backend:     ./scripts/run.sh          (uses ollama + $OLLAMA_MODEL)"
echo "  3b. Remote backend:    export OPENROUTER_API_KEY=sk-your-key-here && ./scripts/run.sh"
