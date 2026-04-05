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
# Build a debug binary — same as run.sh uses. This pre-warms the compile cache
# so the first run.sh invocation is fast. Release builds belong in install.sh.
echo "Building lenzu (debug)..."
cd "$REPO_ROOT"
cargo build -p lenzu
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
    echo "Setting up Ollama (local LLM backend)..."

    # ── helpers ───────────────────────────────────────────────────────────────
    ollama_api_up()      { curl -sf "http://localhost:11434/" &>/dev/null; }
    ollama_binary_ok()   { command -v ollama &>/dev/null; }
    docker_ok()          { command -v docker &>/dev/null && docker info &>/dev/null; }

    pull_model_native() {
        echo "  Pulling $OLLAMA_MODEL (no-op if already present)..."
        ollama pull "$OLLAMA_MODEL"
        echo "  Model ready."
    }

    pull_model_docker() {
        # 'ollama pull' is a client command — requires the server running.
        # Start a temporary container, wait for healthy, pull via exec, stop.
        local TMP="lenzu-ollama-setup"
        # Clean up any leftover from a previous interrupted run.
        docker rm -f "$TMP" &>/dev/null || true
        docker run -d --name "$TMP" \
            -p "11434:11434" \
            -v "${OLLAMA_VOLUME}:/root/.ollama" \
            "$OLLAMA_IMAGE"
        echo -n "  Waiting for temporary ollama container..."
        for i in $(seq 1 30); do
            if ollama_api_up; then echo " ready."; break; fi
            sleep 1; echo -n "."
        done
        docker exec "$TMP" ollama pull "$OLLAMA_MODEL"
        docker stop "$TMP"
        echo "  Model ready."
    }

    # ── decision tree ─────────────────────────────────────────────────────────
    #
    #  Priority:
    #   1. ollama API already up (native running or Docker container running)
    #      → just pull the model, nothing else to install
    #   2. ollama binary installed but not running
    #      → start it in the background, pull model
    #   3. Docker available
    #      → pull image + model into a named volume
    #   4. Nothing available
    #      → install native ollama via the official install script, then pull model

    if ollama_api_up; then
        echo "  ollama already running at http://localhost:11434/"
        if ollama_binary_ok; then
            pull_model_native
        else
            # API is up (probably a Docker container someone started manually)
            # Pull via the REST API using a running container exec if possible,
            # otherwise just warn — run.sh will still work for whatever model is loaded.
            echo "  NOTE: ollama binary not in PATH; model pull skipped."
            echo "        Run 'ollama pull $OLLAMA_MODEL' manually if needed."
        fi

    elif ollama_binary_ok; then
        echo "  ollama installed ($(ollama --version)) but not running — starting it..."
        ollama serve &>/dev/null &
        echo -n "  Waiting for ollama to start..."
        for i in $(seq 1 15); do
            if ollama_api_up; then echo " ready."; break; fi
            sleep 1; echo -n "."
        done
        if ! ollama_api_up; then
            echo ""
            echo "  WARNING: ollama did not start in time. Try 'ollama serve' manually, then re-run setup.sh."
        else
            pull_model_native
        fi

    elif docker_ok; then
        echo "  Docker found — using Docker for ollama."
        echo "  Pulling $OLLAMA_IMAGE image..."
        docker pull "$OLLAMA_IMAGE"
        echo "  Creating persistent volume '$OLLAMA_VOLUME'..."
        docker volume create "$OLLAMA_VOLUME" &>/dev/null || true
        pull_model_docker

    else
        echo "  Neither ollama nor Docker found — installing native ollama..."
        curl -fsSL https://ollama.com/install.sh | sh
        echo "  Starting ollama..."
        ollama serve &>/dev/null &
        echo -n "  Waiting for ollama to start..."
        for i in $(seq 1 15); do
            if ollama_api_up; then echo " ready."; break; fi
            sleep 1; echo -n "."
        done
        if ! ollama_api_up; then
            echo ""
            echo "  WARNING: ollama did not start. Try 'ollama serve' manually, then re-run setup.sh."
        else
            pull_model_native
        fi
    fi
fi

echo ""
echo "Next steps:"
echo "  1. Install Rust:       https://rustup.rs"
echo "  2. Install Node (for lenzu_server): https://nodejs.org  (or: nvm install --lts)"
echo "  3a. Local backend:     ./scripts/run.sh          (uses ollama + $OLLAMA_MODEL)"
echo "  3b. Remote backend:    export OPENROUTER_API_KEY=sk-your-key-here && ./scripts/run.sh"
