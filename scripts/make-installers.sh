#!/usr/bin/env bash
# Build Lenzu installer artifacts: .deb, AppImage, and (eventually) Flatpak.
#
# This is the single entrypoint for both local installer builds and GitHub
# CI — `--ci` mode does the AppImage path only, matching what we publish
# from Actions.
#
# Usage:
#   scripts/make-installers.sh --deb              # MIT pair + AGPL DBNet .deb
#   scripts/make-installers.sh --appimage         # lenzu + lenzu-hud AppImages
#   scripts/make-installers.sh --flatpak          # NOT YET IMPLEMENTED
#   scripts/make-installers.sh --all              # everything above
#   scripts/make-installers.sh --ci               # AppImage only (CI mode)
#   scripts/make-installers.sh --help
#
# Outputs land in target/debian/  (.deb)
#                  target/appimage/ (.AppImage + DBNet model tarball)
#                  target/flatpak/  (.flatpak bundle)
#
# Exit codes:
#   0  success
#   1  unknown flag / usage error
#   2  required tool missing
#   3  build failure
#   10 Flatpak path requested (not yet implemented)
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

OUT_DEB="$REPO_ROOT/target/debian"
OUT_APPIMAGE="$REPO_ROOT/target/appimage"
OUT_FLATPAK="$REPO_ROOT/target/flatpak"

DO_DEB=0
DO_APPIMAGE=0
DO_FLATPAK=0
CI_MODE=0

usage() {
    sed -n '2,/^set -euo/p' "$0" | sed 's/^# \?//' | head -n -1
    exit "${1:-0}"
}

# --- arg parsing ------------------------------------------------------------
[[ $# -eq 0 ]] && usage 1
while [[ $# -gt 0 ]]; do
    case "$1" in
        --deb)      DO_DEB=1 ;;
        --appimage) DO_APPIMAGE=1 ;;
        --flatpak)  DO_FLATPAK=1 ;;
        --all)      DO_DEB=1; DO_APPIMAGE=1; DO_FLATPAK=1 ;;
        --ci)       CI_MODE=1; DO_APPIMAGE=1 ;;
        -h|--help)  usage 0 ;;
        *)          echo "unknown flag: $1" >&2; usage 1 ;;
    esac
    shift
done

log()  { printf '\033[1;34m[make-installers]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[make-installers]\033[0m %s\n' "$*" >&2; }
fail() { printf '\033[1;31m[make-installers]\033[0m %s\n' "$*" >&2; exit "${2:-3}"; }

require_tool() {
    command -v "$1" >/dev/null 2>&1 || fail "missing required tool: $1 ($2)" 2
}

# --- .deb path --------------------------------------------------------------
build_deb() {
    log ".deb: delegating to 'make deb' (lenzu, lenzu-hud, lenzu-models-dbnet)"
    require_tool cargo "install rustup"
    require_tool dpkg-deb "apt-get install dpkg-dev"
    make deb
    log ".deb artifacts in $OUT_DEB:"
    ls -1 "$OUT_DEB"/*.deb 2>/dev/null || warn "no .deb files produced"
}

# --- AppImage path ----------------------------------------------------------
build_appimage() {
    log "AppImage: building single bundle (lenzu + lenzu-hud)"
    require_tool cargo "install rustup"
    require_tool curl "apt-get install curl"
    require_tool pnpm "npm install -g pnpm"

    mkdir -p "$OUT_APPIMAGE"

    # 1) Rust client.  build-lenzu-appimage.sh internally invokes
    #    electron-builder --linux dir to produce the unpacked HUD tree, then
    #    embeds it under AppDir/usr/lib/lenzu-hud/ and drops a wrapper at
    #    AppDir/usr/bin/lenzu-hud (which lenzu's spawn_server() finds via
    #    PATH).  Result: one self-contained lenzu*.AppImage.
    cargo build --release -p lenzu
    "$REPO_ROOT/scripts/build-lenzu-appimage.sh" "$OUT_APPIMAGE"

    # 2) AGPL DBNet model as a sidecar tarball (license-isolated; can't ride
    #    inside the MIT-licensed AppImage).
    log "AppImage: packaging DBNet model as sidecar tarball"
    "$REPO_ROOT/scripts/build-dbnet-tarball.sh" "$OUT_APPIMAGE"

    log "AppImage artifacts in $OUT_APPIMAGE:"
    ls -1 "$OUT_APPIMAGE"/ 2>/dev/null
}

# --- Flatpak path -----------------------------------------------------------
build_flatpak() {
    fail "Flatpak path not yet implemented (manifest + portal wiring TBD)" 10
}

# --- dispatch ---------------------------------------------------------------
[[ $CI_MODE -eq 1 ]] && log "running in CI mode (AppImage only)"

[[ $DO_DEB      -eq 1 ]] && build_deb
[[ $DO_APPIMAGE -eq 1 ]] && build_appimage
[[ $DO_FLATPAK  -eq 1 ]] && build_flatpak

log "done."
