#!/bin/bash
# Install system dependencies required to build all workspace members.
# Safe to re-run — idempotent at every step.
#
# Covers:
#   lenzu         -- GTK3, Cairo, Pango, x11rb, arboard
#   lenzu_server  -- Electron overlay (Node.js via lenzu_server/scripts/setup.sh)
#   prototypes    -- GTK4 + Graphene
#   ollama        -- local LLM backend (Docker container, gemma4:e2b)
#   manga-ocr-2025 -- ONNX models for prototypes/manga-ocr-test (~140 MB, via curl)
#
# Flags:
#   --skip-ollama      skip Docker/Ollama setup (use if you prefer OpenRouter only)
#   --skip-manga-ocr   skip manga-ocr-2025 model download
#   --gpu              force GPU mode (error if no CUDA GPU found)
#   --no-gpu           force CPU-only mode even when a GPU is present
#
# Default (no flag): auto-detect — uses GPU when CUDA is available, CPU otherwise.
# Re-run at any time to switch modes.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE_FILE="$REPO_ROOT/.ollama_mode"

SKIP_OLLAMA=false
SKIP_MANGA_OCR=false
FORCE_GPU=false
FORCE_CPU=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-ollama)    SKIP_OLLAMA=true ;;
        --skip-manga-ocr) SKIP_MANGA_OCR=true ;;
        --gpu)            FORCE_GPU=true ;;
        --no-gpu)         FORCE_CPU=true ;;
        *) echo "Unknown arg: $1"; exit 1 ;;
    esac
    shift
done

OLLAMA_IMAGE="ollama/ollama"
OLLAMA_VOLUME="lenzu-ollama-data"
# All models pulled during setup.  Listed in priority order (primary first).
#   gemma4:e2b  — primary OCR/translation (large, GPU recommended)
#   glm-ocr     — OCR specialist, fast, great layout understanding (~2.2 GB)
#   qwen2.5:3b  — text-only enrichment model (furigana/romaji/translation after local OCR)
# Excluded:
#   qwen2.5vl:3b  — CPU-bound on 4GB VRAM, 2+ min per query
#   moondream     — captioning model, returns prose not structured OCR JSON
#   Florence-2    — HuggingFace/Python only, not in ollama registry
OLLAMA_MODELS=(
    "gemma4:e2b"
    "glm-ocr"
    "qwen2.5:3b"
)
OLLAMA_MODEL="${OLLAMA_MODELS[0]}"  # legacy var used by version/CUDA checks

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
    mecab \
    mecab-ipadic-utf8 \
    mecab-naist-jdic \
    fonts-noto-cjk \
    fonts-ipafont-gothic

echo ""
echo "System dependencies installed."

# ── Rust compile check ────────────────────────────────────────────────────────
# Build a debug binary — same as run.sh uses. This pre-warms the compile cache
# so the first run.sh invocation is fast. Release builds belong in install.sh.
#
# --features onnx: enables DBNet text-detection pre-processing.
# This causes `ort` (Rust crate) to download the ONNX Runtime shared library from
# GitHub releases automatically during the build — no apt install needed.
# OpenCV is NOT required; all image processing is handled by pure-Rust crates
# (image, imageproc, ndarray).  Requires internet access on first build only
# (the artefact is cached in ~/.cargo after that).
echo "Building lenzu (debug, with onnx feature)..."
cd "$REPO_ROOT"
cargo build -p lenzu --features onnx
cd "$REPO_ROOT"


# ── manga-ocr ONNX models (prototypes/manga-ocr-test) ────────────────────────
# mayocream/manga-ocr-onnx — ONNX export of kha-white/manga-ocr-base.
# Three files, ~440 MB total.  Downloaded once; re-run skips existing files.
echo ""
if [[ "$SKIP_MANGA_OCR" == "true" ]]; then
    echo "Skipping manga-ocr model download (--skip-manga-ocr)."
else
    MANGA_OCR_DIR="$REPO_ROOT/assets/manga-ocr"
    MANGA_OCR_BASE="https://huggingface.co/mayocream/manga-ocr-onnx/resolve/main"
    MANGA_OCR_FILES=("encoder_model.onnx" "decoder_model.onnx" "vocab.txt")

    all_present=true
    for f in "${MANGA_OCR_FILES[@]}"; do
        [[ -f "$MANGA_OCR_DIR/$f" ]] || { all_present=false; break; }
    done

    if [[ "$all_present" == "true" ]]; then
        echo "manga-ocr models already present — skipping."
    else
        echo "Downloading manga-ocr ONNX models (~440 MB)..."
        mkdir -p "$MANGA_OCR_DIR"
        download_ok=true
        for f in "${MANGA_OCR_FILES[@]}"; do
            if [[ -f "$MANGA_OCR_DIR/$f" ]]; then
                echo "  $f — already present, skipping."
                continue
            fi
            echo "  Downloading $f..."
            if ! curl -fL --progress-bar -o "$MANGA_OCR_DIR/$f" "$MANGA_OCR_BASE/$f"; then
                echo "  WARNING: failed to download $f"
                rm -f "$MANGA_OCR_DIR/$f"   # remove partial file
                download_ok=false
            fi
        done
        if [[ "$download_ok" == "true" ]]; then
            echo "manga-ocr models ready."
        else
            echo "  Some files failed. Re-run setup.sh to retry, or download manually:"
            echo "    curl -fL -o assets/manga-ocr/<file> $MANGA_OCR_BASE/<file>"
        fi
    fi
fi

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
    OLLAMA_MIN_VERSION="0.20.0"   # gemma4 requires at least this version

    ollama_api_up()      { curl -sf "http://localhost:11434/" &>/dev/null; }
    ollama_binary_ok()   { command -v ollama &>/dev/null; }
    docker_ok()          { command -v docker &>/dev/null && docker info &>/dev/null; }

    # Returns 0 when an NVIDIA GPU is reachable and libcuda is findable.
    # On Debian/Ubuntu, libcuda is often installed via /etc/alternatives and is
    # NOT listed by ldconfig -p, so we fall back to explicit -e file tests.
    detect_gpu() {
        command -v nvidia-smi &>/dev/null || return 1
        nvidia-smi --query-gpu=name --format=csv,noheader &>/dev/null 2>&1 || return 1
        ldconfig -p 2>/dev/null | grep -q "libcuda\.so" && return 0
        local f
        for f in /usr/lib/x86_64-linux-gnu/libcuda.so \
                 /usr/lib/x86_64-linux-gnu/libcuda.so.1 \
                 /usr/lib/aarch64-linux-gnu/libcuda.so \
                 /usr/lib/aarch64-linux-gnu/libcuda.so.1 \
                 /usr/local/cuda/lib64/libcuda.so \
                 /usr/lib/libcuda.so; do
            [[ -e "$f" ]] && return 0
        done
        return 1
    }

    # Returns 0 when the official ollama install (with CUDA runtime libraries) is present.
    # The official installer puts CUDA runner libs in /usr/local/lib/ollama/cuda_v12/.
    # The linuxbrew build has no such directory — it's a CPU-only binary.
    # Using strings/ldd is unreliable: both binaries contain "cudaMalloc failed" as an
    # error message string, causing false positives.
    ollama_has_cuda() {
        [[ -d /usr/local/lib/ollama/cuda_v12 ]] || [[ -d /usr/local/lib/ollama/cuda_v13 ]]
    }

    # Returns 0 if installed ollama meets the minimum version requirement.
    ollama_version_ok() {
        ollama_binary_ok || return 1
        local ver
        ver=$(ollama --version 2>/dev/null | grep -oP '\d+\.\d+\.\d+' | head -1)
        [[ -z "$ver" ]] && return 1
        # Compare semver: split into parts and compare numerically.
        local IFS=.
        read -ra cur  <<< "$ver"
        read -ra min  <<< "$OLLAMA_MIN_VERSION"
        for i in 0 1 2; do
            local c=${cur[$i]:-0} m=${min[$i]:-0}
            (( c > m )) && return 0
            (( c < m )) && return 1
        done
        return 0   # equal
    }

    # Always use the official installer — it produces a CUDA-capable binary when
    # CUDA is present, and a CPU binary when it isn't.  The linuxbrew package is
    # CPU-only regardless of hardware, so we never use it for upgrades.
    upgrade_ollama_native() {
        echo "  Upgrading ollama via official installer (current: $(ollama --version 2>/dev/null), required: >= $OLLAMA_MIN_VERSION)..."
        curl -fsSL https://ollama.com/install.sh | sh
        echo -n "  Waiting for upgraded ollama to start..."
        for i in $(seq 1 15); do
            if ollama_api_up; then echo " ready."; return; fi
            sleep 1; echo -n "."
        done
        echo ""
        echo "  WARNING: ollama did not start after upgrade. Try 'ollama serve' manually."
    }

    pull_model_native() {
        for model in "${OLLAMA_MODELS[@]}"; do
            # Always run `ollama pull` — it's idempotent: fully-present models
            # return instantly ("up to date"), partial downloads resume.
            # We cannot rely on `ollama list` because it shows models whose
            # manifest was fetched even when layers are only partially downloaded.
            echo "  Pulling $model..."
            if ollama pull "$model"; then
                echo "  $model ready."
            else
                echo "  WARNING: failed to pull $model — run 'ollama pull $model' manually to retry."
            fi
        done
        echo "  All models done."
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
        for model in "${OLLAMA_MODELS[@]}"; do
            if docker exec "$TMP" ollama pull "$model"; then
                echo "  $model ready."
            else
                echo "  WARNING: failed to pull $model — run 'ollama pull $model' manually to retry."
            fi
        done
        docker stop "$TMP"
        echo "  All models done."
    }

    # ── GPU / CPU mode selection ──────────────────────────────────────────────
    #
    # Priority: --gpu / --no-gpu flags > auto-detect > CPU fallback.
    # The chosen mode is written to .ollama_mode and read by run.sh.
    # Re-running at any time changes the mode.

    # Read the previously-written mode (if any) so we can detect a change.
    PREVIOUS_MODE=""
    [[ -f "$MODE_FILE" ]] && PREVIOUS_MODE=$(cat "$MODE_FILE")

    if [[ "$FORCE_CPU" == "true" ]]; then
        REQUESTED_MODE="cpu"
        echo "  CPU-only mode forced via --no-gpu."
    elif [[ "$FORCE_GPU" == "true" ]]; then
        echo -n "  --gpu: checking GPU availability... "
        if detect_gpu; then
            echo "OK (NVIDIA GPU + libcuda found)."
            REQUESTED_MODE="gpu"
        else
            echo "FAILED."
            echo ""
            echo "ERROR: --gpu requested but no CUDA-capable GPU was found."
            echo "       Requirements: NVIDIA GPU, nvidia-smi in PATH, libcuda.so in ldconfig."
            echo "       Run without any flag to let setup auto-detect."
            exit 1
        fi
    else
        # Auto-detect: use GPU when available, fall back to CPU silently.
        echo -n "  Auto-detecting GPU... "
        if detect_gpu; then
            echo "NVIDIA GPU + CUDA found — enabling GPU mode."
            REQUESTED_MODE="gpu"
        else
            echo "no CUDA GPU found — using CPU mode."
            REQUESTED_MODE="cpu"
        fi
    fi

    if [[ -n "$PREVIOUS_MODE" && "$PREVIOUS_MODE" != "$REQUESTED_MODE" ]]; then
        echo "  Mode change: $PREVIOUS_MODE → $REQUESTED_MODE"
        echo "  NOTE: If ollama is already running you must restart it for the new mode to take effect."
        echo "        For native ollama: 'pkill -x ollama && ollama serve'"
        echo "        For Docker: run.sh handles this automatically via OLLAMA_NUM_GPU."
    fi

    echo "$REQUESTED_MODE" > "$MODE_FILE"
    echo "  Ollama mode: $REQUESTED_MODE (saved to .ollama_mode)"

    # ── ensure ollama is installed ──────────────────────────────────────────
    #
    # 1. Install if missing (native preferred, Docker fallback)
    # 2. Upgrade if below minimum version
    # 3. Replace with CUDA build if GPU mode and binary lacks CUDA
    # 4. Kill any running instance and start fresh
    # 5. If it doesn't come up → something is seriously wrong, exit

    if ! ollama_binary_ok && ! docker_ok; then
        echo "  Neither ollama nor Docker found — installing native ollama..."
        curl -fsSL https://ollama.com/install.sh | sh
    fi

    if ollama_binary_ok; then
        if ! ollama_version_ok; then
            upgrade_ollama_native
        fi

        # GPU mode: replace binary with CUDA build if needed
        if [[ "$REQUESTED_MODE" == "gpu" ]] && ! ollama_has_cuda; then
            echo "  GPU mode selected but current binary ($(which ollama)) has no CUDA support."
            echo "  Installing official ollama (CUDA-enabled)..."
            curl -fsSL https://ollama.com/install.sh | sh
        fi

        # ── fresh restart ────────────────────────────────────────────────────
        # Kill whatever is running and start clean.  Guarantees the running
        # process matches the binary we just verified/installed.
        echo "  Restarting ollama..."
        pkill -x ollama 2>/dev/null || true
        sleep 1
        ollama serve &>/dev/null &
        echo -n "  Waiting for ollama..."
        for i in $(seq 1 20); do
            if ollama_api_up; then echo " ready."; break; fi
            sleep 1; echo -n "."
        done

        if ! ollama_api_up; then
            echo ""
            echo "  ERROR: ollama failed to start. Check 'ollama serve' manually."
            exit 1
        fi

        pull_model_native

    elif docker_ok; then
        echo "  Docker found — using Docker for ollama."
        echo "  Pulling $OLLAMA_IMAGE image..."
        docker pull "$OLLAMA_IMAGE"
        echo "  Creating persistent volume '$OLLAMA_VOLUME'..."
        docker volume create "$OLLAMA_VOLUME" &>/dev/null || true
        pull_model_docker

    else
        echo "  ERROR: ollama installation failed and Docker is unavailable."
        exit 1
    fi
fi

echo ""
echo "Next steps:"
echo "  1. Install Rust:       https://rustup.rs"
echo "  2. Install Node (for lenzu_server): https://nodejs.org  (or: nvm install --lts)"
echo "  3a. Local backend:     ./scripts/run.sh          (uses ollama + $OLLAMA_MODEL)"
echo "  3b. Remote backend:    export OPENROUTER_API_KEY=sk-your-key-here && ./scripts/run.sh"
echo "  4. manga-ocr prototype test (once models downloaded):"
echo "       cargo test -p manga-ocr-test -- --nocapture"
echo ""
echo "Ollama mode: $(cat "$MODE_FILE" 2>/dev/null || echo "unknown")"
echo "  Auto-detect (recommended): ./scripts/setup.sh"
echo "  Force GPU:                 ./scripts/setup.sh --gpu"
echo "  Force CPU:                 ./scripts/setup.sh --no-gpu"
