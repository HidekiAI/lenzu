#!/bin/bash
# Install the lenzu and lenzu-hud .debs in one call.
# Optional --with-dbnet flag also installs the AGPL-3.0 DBNet model package.
#
# Usage:
#   ./scripts/install-lenzu.sh                 # MIT-only install (lenzu + lenzu-hud)
#   ./scripts/install-lenzu.sh --with-dbnet    # also install AGPL DBNet model
#   ./scripts/install-lenzu.sh /path/to/debs   # use a non-default .deb directory

set -euo pipefail

WITH_DBNET=false
DEB_DIR=""

for arg in "$@"; do
    case "$arg" in
        --with-dbnet) WITH_DBNET=true ;;
        -h|--help)
            sed -n '2,9p' "$0"
            exit 0
            ;;
        *) DEB_DIR="$arg" ;;
    esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEB_DIR="${DEB_DIR:-$SCRIPT_DIR/../target/debian}"

CLIENT_DEB=$(ls "$DEB_DIR"/lenzu_*_*.deb 2>/dev/null | grep -v models-dbnet | head -1 || true)
HUD_DEB=$(ls "$DEB_DIR"/lenzu-hud_*.deb 2>/dev/null | head -1 || true)
DBNET_DEB=$(ls "$DEB_DIR"/lenzu-models-dbnet_*.deb 2>/dev/null | head -1 || true)

[[ -f "$CLIENT_DEB" ]] || { echo "ERROR: lenzu client .deb not found in $DEB_DIR" >&2; exit 1; }
[[ -f "$HUD_DEB" ]]    || { echo "ERROR: lenzu-hud .deb not found in $DEB_DIR" >&2; exit 1; }

PKGS=("$HUD_DEB" "$CLIENT_DEB")  # HUD first — lenzu Depends: on it
if [[ "$WITH_DBNET" == "true" ]]; then
    [[ -f "$DBNET_DEB" ]] || { echo "ERROR: lenzu-models-dbnet .deb not found (--with-dbnet requested)" >&2; exit 1; }
    PKGS+=("$DBNET_DEB")
    echo "Installing lenzu + lenzu-hud + lenzu-models-dbnet (AGPL-3.0)..."
else
    echo "Installing lenzu + lenzu-hud (MIT only; no DBNet model)..."
fi

# apt resolves shlib deps from $auto and electron-builder's Depends.
sudo apt install -y "${PKGS[@]}"

echo
echo "Done. Run with: lenzu"
