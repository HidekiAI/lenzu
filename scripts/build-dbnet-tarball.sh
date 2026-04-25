#!/usr/bin/env bash
# Package the AGPL-3.0 DBNet ONNX model as a sidecar tarball for AppImage
# users.  Cannot ride inside the MIT-licensed lenzu.AppImage — copyleft
# contamination via static bundling — so it ships as a separate file the
# user opts into.
#
# Layout of the produced tarball (uncompressed, installed under $HOME):
#
#   .local/share/lenzu/models/stabrise-text_detection_dbnet_ml_v02_model.onnx
#   .local/share/lenzu/models/COPYRIGHT.dbnet
#
# Users untar with:
#
#   tar -xJf lenzu-models-dbnet-X.Y.Z.tar.xz -C $HOME
#
# Usage:
#   scripts/build-dbnet-tarball.sh <output_dir>
set -euo pipefail

OUT_DIR="${1:-target/appimage}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

MODEL_SRC="$REPO_ROOT/assets/stabrise-text_detection_dbnet_ml_v02_model.onnx"
COPYRIGHT_SRC="$REPO_ROOT/packaging/lenzu-models-dbnet/copyright"

[[ -f "$MODEL_SRC"     ]] || { echo "ERROR: $MODEL_SRC not found (LFS pulled?)" >&2; exit 3; }
[[ -f "$COPYRIGHT_SRC" ]] || { echo "ERROR: $COPYRIGHT_SRC not found" >&2; exit 3; }

VERSION="0.2.0"
STAGE="$REPO_ROOT/target/dbnet-tarball-stage"
PREFIX=".local/share/lenzu/models"

mkdir -p "$OUT_DIR"
rm -rf "$STAGE"
mkdir -p "$STAGE/$PREFIX"

cp "$MODEL_SRC"     "$STAGE/$PREFIX/"
cp "$COPYRIGHT_SRC" "$STAGE/$PREFIX/COPYRIGHT.dbnet"

cat >"$STAGE/$PREFIX/README.txt" <<'EOF'
DBNet text-detection ONNX model (StabRise).

License:    AGPL-3.0 — see COPYRIGHT.dbnet next to this file.
Maintained: separately from the MIT-licensed Lenzu binary so the two
            license obligations stay cleanly partitioned.

The bundled lenzu*.AppImage will look for the model file at:
    $XDG_DATA_HOME/lenzu/models/...   (default: ~/.local/share/lenzu/models/)
EOF

TARBALL="$OUT_DIR/lenzu-models-dbnet-${VERSION}.tar.xz"
tar -C "$STAGE" -cJf "$TARBALL" "$PREFIX"
echo "[dbnet] sidecar tarball: $TARBALL"
ls -lh "$TARBALL"
