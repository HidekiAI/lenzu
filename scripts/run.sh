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

# Tracks whether THIS script started the Docker container.
# Only true when we did — prevents stopping a pre-existing native/Docker ollama.
OLLAMA_STARTED_BY_US=false

# ── Ollama lifecycle helpers ──────────────────────────────────────────────────

docker_daemon_running() {
    docker info &>/dev/null
}

# True if the ollama API is responding — regardless of whether it is a Docker
# container or a native host install.
ollama_api_healthy() {
    curl -sf "http://localhost:${OLLAMA_PORT}/" &>/dev/null
}

# True only if the container WE manage is currently running.
our_container_running() {
    docker ps --format '{{.Names}}' 2>/dev/null | grep -q "^${OLLAMA_CONTAINER}$"
}

start_ollama() {
    # Case 1: already healthy (native install or someone else's container) — reuse it.
    if ollama_api_healthy; then
        echo "==> ollama already running on port ${OLLAMA_PORT} — using existing instance."
        return
    fi

    # Case 2: our container is listed but not yet healthy — wait for it.
    if our_container_running; then
        echo "==> ollama container starting up — waiting..."
    else
        # Case 3: nothing running — start our container.
        if ! command -v docker &>/dev/null || ! docker_daemon_running; then
            return 1   # caller handles the no-docker path
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
        OLLAMA_STARTED_BY_US=true
    fi

    echo -n "    Waiting for ollama to be ready..."
    for i in $(seq 1 30); do
        if ollama_api_healthy; then
            echo " ready."
            return
        fi
        sleep 1
        echo -n "."
    done
    echo ""
    echo "WARNING: ollama did not become ready within 30s — lenzu will fall back to OpenRouter if key is set."
}

stop_ollama() {
    # Only stop the container if this script started it — never kill a
    # pre-existing native ollama or another user's container.
    if [[ "$OLLAMA_STARTED_BY_US" == "true" ]] && our_container_running; then
        echo "==> Stopping ollama container (started by this session)..."
        docker stop "$OLLAMA_CONTAINER"
    fi
}

# ── Backend selection ─────────────────────────────────────────────────────────
# Priority order:
#   1. Native ollama already running on port 11434 → use it, don't touch it on exit
#   2. Docker available → start our container, stop it on exit
#   3. Neither → warn; require OPENROUTER_API_KEY for remote-only mode

# NOTE: do NOT use 'exec' to launch lenzu below — exec replaces this shell,
# preventing the EXIT trap from firing and orphaning the container.
trap stop_ollama EXIT

if ollama_api_healthy; then
    echo "==> ollama already available at http://localhost:${OLLAMA_PORT}/ — skipping Docker."
elif command -v docker &>/dev/null && docker_daemon_running; then
    start_ollama
else
    echo "WARNING: ollama not running and Docker unavailable."
    if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
        echo "ERROR: No local ollama and no OPENROUTER_API_KEY set."
        echo "  Option A: start ollama natively or via Docker"
        echo "  Option B: export OPENROUTER_API_KEY=sk-... for remote-only mode"
        echo "  Run scripts/setup.sh to install Docker + ollama."
        exit 1
    fi
    echo "         Continuing with OpenRouter only."
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
# Do NOT use 'exec' here — it would replace this shell, preventing the EXIT
# trap from firing and leaving the ollama container running after lenzu exits.
"$CLIENT_BINARY"
