#!/usr/bin/env bash
# Package manga-ocr-rs's encoder/decoder ONNX models as a sidecar tarball
# for AppImage users.  Source: mayocream/manga-ocr-onnx on HuggingFace,
# downloaded into $HOME/.cache/manga-ocr-rs/ by manga-ocr-rs's build.rs.
#
# These models (~440 MB uncompressed, ~340 MB xz-compressed) are too big
# to bundle inside the AppImage and the binary has the build machine's
# absolute cache path baked in via `env!("MANGA_OCR_DEFAULT_MODEL_DIR")`,
# so on any recipient's machine `default_model_dir()` resolves to a
# non-existent path.  The lenzu binary's resolver in local_ocr.rs::new()
# searches the standard data dirs first, so dropping models here works.
#
# Layout of the produced tarball (uncompressed, installed under $HOME):
#
#   .local/share/lenzu/manga-ocr/encoder_model.onnx
#   .local/share/lenzu/manga-ocr/decoder_model.onnx
#   .local/share/lenzu/manga-ocr/vocab.txt
#   .local/share/lenzu/manga-ocr/README.txt
#
# Users untar with:
#
#   tar -xJf lenzu-models-manga-ocr-X.Y.Z.tar.xz -C $HOME
#
# Usage:
#   scripts/build-manga-ocr-tarball.sh <output_dir>
set -euo pipefail

OUT_DIR="${1:-target/appimage}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# manga-ocr-rs's build.rs unconditionally writes to $HOME/.cache/manga-ocr-rs/
# (see https://crates.io/crates/manga-ocr-rs).  Allow override for CI.
SRC_DIR="${MANGA_OCR_CACHE:-$HOME/.cache/manga-ocr-rs}"

ENC="$SRC_DIR/encoder_model.onnx"
DEC="$SRC_DIR/decoder_model.onnx"
VOC="$SRC_DIR/vocab.txt"

for f in "$ENC" "$DEC" "$VOC"; do
    [[ -f "$f" ]] || {
        echo "ERROR: $f not found" >&2
        echo "Run a `cargo build -p lenzu` first to trigger manga-ocr-rs's" >&2
        echo "build.rs to download from mayocream/manga-ocr-onnx." >&2
        exit 3
    }
done

VERSION="0.1.0"
STAGE="$REPO_ROOT/target/manga-ocr-tarball-stage"
PREFIX=".local/share/lenzu/manga-ocr"

mkdir -p "$OUT_DIR"
rm -rf "$STAGE"
mkdir -p "$STAGE/$PREFIX"

cp "$ENC" "$STAGE/$PREFIX/"
cp "$DEC" "$STAGE/$PREFIX/"
cp "$VOC" "$STAGE/$PREFIX/"

cat >"$STAGE/$PREFIX/README.txt" <<'EOF'
manga-ocr ONNX models (kha-white/manga-ocr → mayocream/manga-ocr-onnx).

Usage:    drop-in models for the lenzu*.AppImage's local-first OCR path.
          Without these, every shift-click falls through to the LLM
          backend chain (Ollama / OpenRouter remote).
License:  Apache-2.0 (kha-white/manga-ocr); see HuggingFace model card
          at https://huggingface.co/mayocream/manga-ocr-onnx for details.

The bundled lenzu*.AppImage looks for these files at:
    $XDG_DATA_HOME/lenzu/manga-ocr/   (default: ~/.local/share/lenzu/manga-ocr/)
EOF

TARBALL="$OUT_DIR/lenzu-models-manga-ocr-${VERSION}.tar.xz"
# -T0 = use all CPU cores for xz; manga-ocr models are big.
XZ_OPT="-T0 -9" tar -C "$STAGE" -cJf "$TARBALL" "$PREFIX"
echo "[manga-ocr] sidecar tarball: $TARBALL"
ls -lh "$TARBALL"
