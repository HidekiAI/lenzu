#!/usr/bin/env bash
# Build a SINGLE lenzu.AppImage that bundles both the Rust client and the
# Electron HUD.  No second-file dance for users — download one .AppImage,
# chmod +x, run.
#
# Layout inside the resulting AppImage:
#
#   AppDir/
#   ├── AppRun                              (linuxdeploy-generated)
#   ├── lenzu.desktop
#   ├── lenzu.png
#   ├── usr/
#   │   ├── bin/
#   │   │   ├── lenzu                       (Rust client binary)
#   │   │   └── lenzu-hud                   (wrapper -> usr/lib/lenzu-hud)
#   │   ├── lib/
#   │   │   ├── *.so                        (GTK3, MeCab, ONNX bundled by
#   │   │   │                                linuxdeploy-plugin-gtk)
#   │   │   └── lenzu-hud/                  (electron-builder --linux dir
#   │   │                                    output: electron binary,
#   │   │                                    chromium sandbox, icudtl.dat,
#   │   │                                    resources/app.asar, etc.)
#   │   └── share/
#   │       ├── applications/lenzu.desktop
#   │       ├── icons/hicolor/256x256/apps/lenzu.png
#   │       └── doc/lenzu/NOTICES.md (+ NOTICES.crates.md)
#
# Because linuxdeploy's AppRun puts $APPDIR/usr/bin on PATH, lenzu's
# spawn_server() finds the bundled lenzu-hud wrapper on PATH automatically
# — no change required in lenzu/src/main.rs.
#
# AGPL DBNet model is NOT bundled (license isolation); it's shipped as a
# sidecar tarball by scripts/build-dbnet-tarball.sh.
#
# Usage:
#   scripts/build-lenzu-appimage.sh <output_dir>
#
# Prerequisites:
#   - cargo build --release --all-features -p lenzu  (target/release/lenzu present)
#     (--all-features enables `onnx` so the bundled DBNet sidecar works)
#   - lenzu_server has been pnpm-installed (node_modules/electron present)
set -euo pipefail

OUT_DIR="${1:-target/appimage}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

BINARY="$REPO_ROOT/target/release/lenzu"
[[ -x "$BINARY" ]] || { echo "ERROR: $BINARY not found — run 'cargo build --release -p lenzu' first" >&2; exit 3; }

VERSION="$(grep -m1 '^version' lenzu/Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ARCH="$(uname -m)"
APPDIR="$REPO_ROOT/target/appimage-build/lenzu.AppDir"
TOOLS_DIR="$REPO_ROOT/target/appimage-tools"

mkdir -p "$OUT_DIR" "$TOOLS_DIR"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" \
         "$APPDIR/usr/lib/lenzu-hud" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps" \
         "$APPDIR/usr/share/doc/lenzu"

# --- 1. build the HUD as an unpacked Electron tree --------------------------
# electron-builder --linux dir produces dist-deb/linux-unpacked/ with the
# full Electron runtime + our app.asar — that's the directory we embed.
echo "[appimage] building HUD via electron-builder --linux dir"
( cd lenzu_server && \
  pnpm run build && \
  pnpm exec electron-builder --linux dir )

HUD_UNPACKED="$REPO_ROOT/lenzu_server/dist-deb/linux-unpacked"
[[ -d "$HUD_UNPACKED" ]] || { echo "ERROR: electron-builder did not produce $HUD_UNPACKED" >&2; exit 3; }

# Copy the entire unpacked Electron tree into AppDir.
cp -a "$HUD_UNPACKED"/. "$APPDIR/usr/lib/lenzu-hud/"

# Wrapper script on PATH.  spawn_server() in lenzu/src/main.rs already
# tries `lenzu-hud` on PATH first, so dropping this wrapper at
# $APPDIR/usr/bin/lenzu-hud is all the integration that's needed.
#
# --no-sandbox is required because Chromium's setuid sandbox helper
# (chrome-sandbox) cannot run from inside a FUSE-mounted AppImage; we
# fall back to the kernel's user namespace sandbox instead.
cat >"$APPDIR/usr/bin/lenzu-hud" <<'WRAPPER'
#!/usr/bin/env bash
HERE="$(dirname "$(readlink -f "$0")")"
APPDIR_ROOT="$(cd "$HERE/../.." && pwd)"
exec "$APPDIR_ROOT/usr/lib/lenzu-hud/lenzu-hud" --no-sandbox "$@"
WRAPPER
chmod +x "$APPDIR/usr/bin/lenzu-hud"

# --- 2. drop the lenzu binary + desktop + icon ------------------------------
cp "$BINARY" "$APPDIR/usr/bin/lenzu"
cp "$REPO_ROOT/packaging/lenzu/lenzu.desktop" "$APPDIR/usr/share/applications/lenzu.desktop"
cp "$REPO_ROOT/packaging/lenzu/lenzu.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/lenzu.png"
# linuxdeploy looks for icon + desktop at AppDir root.
cp "$REPO_ROOT/packaging/lenzu/lenzu.desktop" "$APPDIR/lenzu.desktop"
cp "$REPO_ROOT/packaging/lenzu/lenzu.png" "$APPDIR/lenzu.png"

# Notices (curated + auto-generated crate licenses).
cp "$REPO_ROOT/lenzu/NOTICES.md" "$APPDIR/usr/share/doc/lenzu/" 2>/dev/null || echo "WARN: NOTICES.md missing"
if [[ -f "$REPO_ROOT/lenzu/NOTICES.crates.md" ]]; then
    cp "$REPO_ROOT/lenzu/NOTICES.crates.md" "$APPDIR/usr/share/doc/lenzu/"
else
    make notices && cp "$REPO_ROOT/lenzu/NOTICES.crates.md" "$APPDIR/usr/share/doc/lenzu/"
fi

# --- 3. fetch linuxdeploy if not cached -------------------------------------
LD="$TOOLS_DIR/linuxdeploy-${ARCH}.AppImage"
LD_GTK="$TOOLS_DIR/linuxdeploy-plugin-gtk.sh"

if [[ ! -x "$LD" ]]; then
    echo "[appimage] fetching linuxdeploy"
    curl -fsSL -o "$LD" "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-${ARCH}.AppImage"
    chmod +x "$LD"
fi
if [[ ! -x "$LD_GTK" ]]; then
    echo "[appimage] fetching linuxdeploy-plugin-gtk"
    curl -fsSL -o "$LD_GTK" "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh"
    chmod +x "$LD_GTK"
fi

# --- 4. pack with linuxdeploy + GTK plugin ----------------------------------
export LINUXDEPLOY_OUTPUT_VERSION="$VERSION"
export LINUXDEPLOY_OUTPUT_APP_NAME="lenzu"
export NO_STRIP=1
export DEPLOY_GTK_VERSION=3
# Tell linuxdeploy to walk only the Rust binary's deps — the bundled
# Electron runtime under usr/lib/lenzu-hud/ is self-contained and must
# NOT have its libs duplicated/relinked.
export LDAI_EXCLUDE_LIBRARIES_FROM_DEPLOYMENT="usr/lib/lenzu-hud"

( cd "$OUT_DIR" && \
  "$LD" --appdir "$APPDIR" \
        --executable "$APPDIR/usr/bin/lenzu" \
        --plugin gtk \
        --output appimage )

# Normalize filename to lowercase for predictability.
shopt -s nullglob
for f in "$OUT_DIR"/Lenzu*.AppImage "$OUT_DIR"/lenzu*.AppImage; do
    base="$(basename "$f")"
    target="$(echo "$base" | tr '[:upper:]' '[:lower:]')"
    [[ "$base" != "$target" ]] && mv "$f" "$OUT_DIR/$target"
done
shopt -u nullglob

echo "[appimage] done — single bundle in $OUT_DIR/"
ls -1 "$OUT_DIR"/lenzu*.AppImage 2>/dev/null
