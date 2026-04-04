#!/usr/bin/env bash
# Build the GTK client and run it. The client auto-spawns the Electron HUD (`lenzu_server`)
# when `overlay_enabled` is true — do not start a second overlay process here.
#
# LLM backend (dual-backend):
#   Primary:  Gemma 4 E2B via ollama Docker container (always started if Docker is available)
#   Fallback: OpenRouter (Gemini 2.0 Flash) — only active when OPENROUTER_API_KEY is set

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLIENT_BINARY="$REPO_ROOT/target/debug/lenzu"

OLLAMA_CONTAINER="lenzu-ollama"
OLLAMA_IMAGE="ollama/ollama"
OLLAMA_VOLUME="lenzu-ollama-data"
OLLAMA_PORT=11434

# ── Ollama lifecycle helpers ──────────────────────────────────────────────────

ollama_running() {
    docker ps --format '{{.Names}}' 2>/dev/null | grep -q "^${OLLAMA_CONTAINER}$"
}

start_ollama() {
    if ollama_running; then
        echo "==> ollama container already running."
        return
    fi
    echo "==> Starting ollama container ($OLLAMA_CONTAINER)..."
    GPU_FLAGS=()
    if docker info 2>/dev/null | grep -qi "nvidia"; then
        GPU_FLAGS=(--gpus all)
        echo "    NVIDIA GPU detected — enabling GPU passthrough."
    fi
    docker run -d \
        --name "$OLLAMA_CONTAINER" \
        --rm \
        "${GPU_FLAGS[@]+"${GPU_FLAGS[@]}"}" \
        -p "${OLLAMA_PORT}:11434" \
        -v "${OLLAMA_VOLUME}:/root/.ollama" \
        "$OLLAMA_IMAGE"
    echo -n "    Waiting for ollama to be ready..."
    for i in $(seq 1 30); do
        if curl -sf "http://localhost:${OLLAMA_PORT}/" &>/dev/null; then
            echo " ready."
            return
        fi
        sleep 1
        echo -n "."
    done
    echo ""
    echo "WARNING: ollama did not become ready within 30s — lenzu will fall back to full-image API."
}

stop_ollama() {
    if ollama_running; then
        echo "==> Stopping ollama container..."
        docker stop "$OLLAMA_CONTAINER"
    fi
}

# ── Backend selection ─────────────────────────────────────────────────────────
# Gemma via ollama is always the primary. Docker availability determines whether
# it can run. OPENROUTER_API_KEY enables the OpenRouter fallback — not required.

if command -v docker &>/dev/null; then
    trap stop_ollama EXIT
    start_ollama
else
    echo "WARNING: Docker not found — ollama (primary) unavailable."
    if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
        echo "ERROR: Docker is not available and OPENROUTER_API_KEY is not set."
        echo "  Install Docker (run scripts/setup.sh) or set OPENROUTER_API_KEY for remote fallback."
        exit 1
    fi
    echo "         Continuing with OpenRouter only (no local fallback)."
fi

if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
    echo "==> OPENROUTER_API_KEY not set — OpenRouter fallback disabled."
fi

# ── Build & run ───────────────────────────────────────────────────────────────

echo "==> Building lenzu_server (Electron HUD)..."
cd "$REPO_ROOT/lenzu_server"
pnpm install --silent
pnpm run build

echo "==> Building lenzu client..."
cd "$REPO_ROOT"
cargo build -p lenzu

echo "==> Starting lenzu (spawns Electron overlay when overlay_enabled is true)..."
exec "$CLIENT_BINARY"
