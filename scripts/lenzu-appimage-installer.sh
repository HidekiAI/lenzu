#!/usr/bin/env bash
# End-user installer for lenzu*.AppImage runtime data.
#
# The AppImage ships only the binaries (Rust client + Electron HUD).  At
# runtime, lenzu looks for:
#
#   ~/.local/share/lenzu/models/    DBNet ONNX text-detection model
#                                   (AGPL-3.0, license-isolated from MIT
#                                    AppImage — must ship as sidecar)
#   ~/.local/share/lenzu/manga-ocr/ encoder/decoder ONNX + vocab.txt
#                                   (~440 MB uncompressed; too big to
#                                    bundle inside the AppImage)
#
# Plus one system dependency the AppImage cannot provide:
#
#   mecab + mecab-ipadic-utf8       furigana dictionary lookups
#                                   (apt install mecab-ipadic-utf8)
#
# This script downloads the two model sidecars from the lenzu GitHub
# release matching the AppImage version, untars them under $HOME, and
# checks that mecab is available.  It does NOT run sudo or install
# system packages — just prints the command if mecab is missing.
#
# Usage:
#   scripts/lenzu-appimage-installer.sh           # default: latest release
#   scripts/lenzu-appimage-installer.sh v0.1.0    # specific tag
#   LENZU_RELEASE_BASE=file:///path/to/local      # offline install
#       scripts/lenzu-appimage-installer.sh
#
# Exit codes:
#   0 success
#   1 download / extract failure
#   2 missing required tool (curl, tar)
set -euo pipefail

VERSION="${1:-latest}"
RELEASE_BASE="${LENZU_RELEASE_BASE:-https://github.com/HidekiAI/lenzu/releases/download/$VERSION}"

XDG_DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
DBNET_VER="0.2.0"
MANGA_VER="0.1.0"

log()  { printf '\033[1;34m[lenzu-installer]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[lenzu-installer]\033[0m %s\n' "$*" >&2; }
fail() { printf '\033[1;31m[lenzu-installer]\033[0m %s\n' "$*" >&2; exit "${2:-1}"; }

require_tool() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required tool: $1" 2
}
require_tool curl
require_tool tar

fetch_and_extract() {
    local name="$1" url="$2" probe="$3"
    if [[ -f "$probe" ]]; then
        log "$name already installed ($probe), skipping download"
        return 0
    fi
    log "downloading $name from $url"
    local tmp; tmp="$(mktemp --suffix=.tar.xz)"
    trap 'rm -f "$tmp"' RETURN
    if [[ "$url" == file://* ]]; then
        cp "${url#file://}" "$tmp"
    else
        curl -fL --progress-bar -o "$tmp" "$url" || fail "download failed: $url"
    fi
    log "extracting $name → $HOME"
    tar -xJf "$tmp" -C "$HOME" || fail "extract failed: $tmp"
}

log "lenzu AppImage runtime installer (target: $XDG_DATA/lenzu/)"

fetch_and_extract \
    "DBNet text-detection model (AGPL-3.0)" \
    "$RELEASE_BASE/lenzu-models-dbnet-${DBNET_VER}.tar.xz" \
    "$XDG_DATA/lenzu/models/stabrise-text_detection_dbnet_ml_v02_model.onnx"

fetch_and_extract \
    "manga-ocr models (Apache-2.0, ~340MB)" \
    "$RELEASE_BASE/lenzu-models-manga-ocr-${MANGA_VER}.tar.xz" \
    "$XDG_DATA/lenzu/manga-ocr/encoder_model.onnx"

# mecab is a system library — apt-managed, not bundleable into AppImage.
if command -v mecab >/dev/null 2>&1; then
    log "mecab present: $(command -v mecab)"
else
    warn "mecab not found.  Furigana annotation will fail until you run:"
    warn "    sudo apt install mecab mecab-ipadic-utf8"
fi

log "done.  Run the AppImage:"
log "    ./lenzu-X.Y.Z-x86_64.appimage"
log "Or with furigana-only mode (no LLM enrichment):"
log "    ./lenzu-X.Y.Z-x86_64.appimage --furigana_only"
