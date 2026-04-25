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

    # 1) Rust client.  --all-features turns on `onnx` (DBNet text detection)
    #    so the bundled DBNet sidecar tarball is actually usable; without it
    #    every shift-click falls through to the LLM, defeating --furigana_only.
    #    build-lenzu-appimage.sh then invokes electron-builder --linux dir to
    #    produce the unpacked HUD tree, embeds it under AppDir/usr/lib/lenzu-hud/
    #    and drops a wrapper at AppDir/usr/bin/lenzu-hud (which lenzu's
    #    spawn_server() finds via PATH).  Result: one self-contained
    #    lenzu*.AppImage.
    cargo build --release --all-features -p lenzu
    "$REPO_ROOT/scripts/build-lenzu-appimage.sh" "$OUT_APPIMAGE"

    # 2) AGPL DBNet model as a sidecar tarball (license-isolated; can't ride
    #    inside the MIT-licensed AppImage).
    log "AppImage: packaging DBNet model as sidecar tarball"
    "$REPO_ROOT/scripts/build-dbnet-tarball.sh" "$OUT_APPIMAGE"

    # 3) manga-ocr models as a sidecar tarball (~340 MB xz-compressed; too
    #    big to bundle inside the AppImage, and the binary's embedded
    #    `default_model_dir()` points at the build machine's cache, so a
    #    sidecar at the standard XDG path is the cleanest end-user story).
    log "AppImage: packaging manga-ocr models as sidecar tarball"
    "$REPO_ROOT/scripts/build-manga-ocr-tarball.sh" "$OUT_APPIMAGE"

    # 4) One-file bundle: AppImage + both sidecars + installer + README.
    #    Outer tar is uncompressed since contents are already compressed
    #    (AppImage = SquashFS, sidecars = .tar.xz).  Users grab one file
    #    and run the installer — no juggling individual downloads.
    log "AppImage: rolling everything into a single bundle tar"
    # nullglob: a non-matching glob expands to nothing rather than the literal
    # pattern.  Without it, the upper/lower-case ls falls afoul of pipefail
    # (one of the two args has no match → ls exits 2 → pipefail propagates).
    local appimage_file=""
    shopt -s nullglob
    local _appimg_candidates=("$OUT_APPIMAGE"/lenzu*.AppImage "$OUT_APPIMAGE"/lenzu*.appimage)
    shopt -u nullglob
    [[ ${#_appimg_candidates[@]} -gt 0 ]] || fail "no .AppImage found in $OUT_APPIMAGE — bundle step skipped" 3
    appimage_file="${_appimg_candidates[0]}"
    local bundle_ver
    bundle_ver="$(grep -m1 '^version' "$REPO_ROOT/lenzu/Cargo.toml" | sed -E 's/.*"([^"]+)".*/\1/')"
    local bundle_stage="$REPO_ROOT/target/appimage-bundle-stage"
    rm -rf "$bundle_stage"
    mkdir -p "$bundle_stage"
    cp "$appimage_file" "$bundle_stage/"
    cp "$OUT_APPIMAGE"/lenzu-models-dbnet-*.tar.xz "$bundle_stage/"
    cp "$OUT_APPIMAGE"/lenzu-models-manga-ocr-*.tar.xz "$bundle_stage/"
    cp "$REPO_ROOT/scripts/lenzu-appimage-installer.sh" "$bundle_stage/"
    # run.sh handles the ollama/Docker lifecycle preflight and exec's the
    # AppImage that sits next to it.  No cargo/pnpm needed at runtime.
    cp "$REPO_ROOT/scripts/run.sh" "$bundle_stage/run.sh"
    chmod +x "$bundle_stage/run.sh"
    cat >"$bundle_stage/README.txt" <<EOF
Lenzu single-bundle for x86_64 Linux.

Quick start:
  1.  tar -xf lenzu-bundle-${bundle_ver}.tar
  2.  cd lenzu-bundle-${bundle_ver}/
  3.  LENZU_RELEASE_BASE=file://\$(pwd) ./lenzu-appimage-installer.sh
      (untars both model sidecars to ~/.local/share/lenzu/)
  4.  ./run.sh                       (recommended — checks ollama / Docker
                                      and falls through to OpenRouter if
                                      OPENROUTER_API_KEY is set)
      ./run.sh --furigana_only       (offline path: DBNet + manga-ocr,
                                      no LLM enrichment)

  Or run the AppImage directly (skips the ollama preflight):
      ./$(basename "$appimage_file")
      ./$(basename "$appimage_file") --furigana_only

System dependency:
  sudo apt install mecab mecab-ipadic-utf8

Contents:
  $(basename "$appimage_file")              -- main binary (Rust + Electron HUD)
  lenzu-models-dbnet-*.tar.xz               -- AGPL-3.0 DBNet text-detection
  lenzu-models-manga-ocr-*.tar.xz           -- Apache-2.0 manga-ocr (~340 MB)
  lenzu-appimage-installer.sh               -- runs the model untars
  run.sh                                    -- ollama preflight + run AppImage
EOF

    # Rename the stage dir to a friendlier name so `tar -xf` produces a
    # cleanly-named directory on the user's machine.  Outer tar is
    # uncompressed: the AppImage is SquashFS-compressed and the sidecars
    # are .tar.xz, so an outer .tar.gz/.bz2/.xz would just waste CPU.
    local bundle="$OUT_APPIMAGE/lenzu-bundle-${bundle_ver}.tar"
    local friendly="$REPO_ROOT/target/lenzu-bundle-${bundle_ver}"
    rm -rf "$friendly"
    mv "$bundle_stage" "$friendly"
    tar -C "$(dirname "$friendly")" -cf "$bundle" "$(basename "$friendly")"
    rm -rf "$friendly"

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
