#!/usr/bin/env bash
# Build the GTK client and run it. The client auto-spawns the Electron HUD (`lenzu_server`)
# when `overlay_enabled` is true — do not start a second overlay process here.
#
# LLM backend (dual-backend):
#   Primary:  glm-ocr via ollama (fast OCR specialist, ~15s)
#   Fallback: gemma4:e2b (offline/no-API-key), then OpenRouter (Gemini 2.0 Flash)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLIENT_BINARY="$REPO_ROOT/target/debug/lenzu"

OLLAMA_CONTAINER="lenzu-ollama"
OLLAMA_IMAGE="ollama/ollama"
OLLAMA_VOLUME="lenzu-ollama-data"
OLLAMA_PORT=11434
OLLAMA_MODEL="${OLLAMA_MODEL:-glm-ocr}"

# GPU/CPU mode written by setup.sh.  Default is cpu — safe on any machine.
MODE_FILE="$REPO_ROOT/.ollama_mode"
OLLAMA_MODE="cpu"
[[ -f "$MODE_FILE" ]] && OLLAMA_MODE=$(cat "$MODE_FILE")

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

        echo "==> Starting ollama container ($OLLAMA_CONTAINER) in ${OLLAMA_MODE} mode..."
        GPU_FLAGS=()
        MODE_ENV=()
        if [[ "$OLLAMA_MODE" == "gpu" ]]; then
            if docker info 2>/dev/null | grep -qi "nvidia"; then
                GPU_FLAGS=(--gpus all)
                echo "    NVIDIA GPU detected — enabling GPU passthrough."
            else
                echo "    WARNING: GPU mode requested but Docker has no NVIDIA runtime."
                echo "             Run 'scripts/setup.sh' (without --gpu) to switch to CPU mode."
            fi
        else
            # CPU mode: tell ollama not to offload any layers to GPU.
            MODE_ENV=(--env OLLAMA_NUM_GPU=0)
            echo "    CPU mode — GPU offload disabled (OLLAMA_NUM_GPU=0)."
        fi
        docker run -d \
            --name "$OLLAMA_CONTAINER" \
            --rm \
            "${GPU_FLAGS[@]+"${GPU_FLAGS[@]}"}" \
            "${MODE_ENV[@]+"${MODE_ENV[@]}"}" \
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

# Ensure the required model is pulled; pull it if missing.
ensure_model() {
    echo -n "==> Checking model ${OLLAMA_MODEL}... "
    if curl -sf "http://localhost:${OLLAMA_PORT}/api/tags" | grep -q "\"name\":\"${OLLAMA_MODEL}\""; then
        echo "present."
    else
        echo "not found — pulling (this may take several minutes)..."
        ollama pull "${OLLAMA_MODEL}"
    fi
}

# Report the active inference mode and warn when reality doesn't match the
# configured mode.  Mode is set by setup.sh and stored in .ollama_mode.
check_gpu_support() {
    echo "==> Ollama mode: ${OLLAMA_MODE} (from .ollama_mode)"

    local ollama_bin
    ollama_bin=$(which ollama 2>/dev/null)

    if [[ "$OLLAMA_MODE" == "gpu" ]]; then
        # Verify the binary has CUDA support by checking for the cuda_v12/v13 runtime
        # directory that the official installer creates.  ldd is unreliable here because
        # ollama loads CUDA via dlopen at runtime, not as a linked dependency.
        if ! [[ -d /usr/local/lib/ollama/cuda_v12 || -d /usr/local/lib/ollama/cuda_v13 ]]; then
            echo "WARNING: GPU mode is configured but ollama has no CUDA support."
            echo "         Re-run to reinstall with the official CUDA build:"
            echo "              scripts/setup.sh --gpu"
        fi
    else
        # CPU mode: nothing to verify — OLLAMA_NUM_GPU=0 is enforced for Docker.
        # For a native ollama that is already running we cannot inject the env var,
        # so check whether the model is unexpectedly using VRAM and warn.
        local ps_resp
        ps_resp=$(curl -sf "http://localhost:${OLLAMA_PORT}/api/ps" 2>/dev/null)
        if echo "$ps_resp" | grep -qv '"size_vram":0'; then
            # Only warn if ps actually shows a loaded model with non-zero VRAM.
            if echo "$ps_resp" | grep -q '"name"' && ! echo "$ps_resp" | grep -q '"size_vram":0'; then
                echo "NOTE: Native ollama is using VRAM even though CPU mode is configured."
                echo "      Restart ollama with OLLAMA_NUM_GPU=0 to enforce CPU-only inference,"
                echo "      or run 'scripts/setup.sh --gpu' to officially switch to GPU mode."
            fi
        fi
    fi
}

# Quick smoke-test: send the bundled unit-test image to the model and verify
# it returns at least one non-empty line of text.  Reports pass/fail without
# blocking the startup if the model is slow.
smoke_test_model() {
    local sample_img="${REPO_ROOT}/assets/Unit-test-sample-texts.png"
    if [[ ! -f "$sample_img" ]]; then
        echo "==> Smoke-test image not found — skipping."
        return
    fi
    echo -n "==> Smoke-testing ${OLLAMA_MODEL} with sample OCR image... "
    local b64
    b64=$(base64 -w0 "$sample_img")
    local response
    # Use || true so a curl failure (timeout, HTTP error, SSE rejection) never exits the script.
    response=$(curl -s -m 30 -X POST "http://localhost:${OLLAMA_PORT}/v1/chat/completions" \
        -H "Content-Type: application/json" \
        -d "{\"model\":\"${OLLAMA_MODEL}\",\"stream\":false,\"temperature\":0.1,
             \"messages\":[{\"role\":\"user\",\"content\":[
               {\"type\":\"text\",\"text\":\"List all text visible in this image, one line per block.\"},
               {\"type\":\"image_url\",\"image_url\":{\"url\":\"data:image/png;base64,${b64}\"}}
             ]}]}" 2>/dev/null) || true
    if echo "$response" | grep -q '"content"'; then
        echo "PASS"
    else
        echo "SKIP (no response within 30s — model may be loading; lenzu will still run)"
    fi
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
    ensure_model
    check_gpu_support
    smoke_test_model
elif command -v docker &>/dev/null && docker_daemon_running; then
    start_ollama
    ensure_model
    check_gpu_support
    smoke_test_model
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
echo "==> Inference mode: ${OLLAMA_MODE}  (change with: scripts/setup.sh [--gpu])"

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
