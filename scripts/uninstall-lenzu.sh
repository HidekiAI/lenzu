#!/bin/bash
# Uninstall lenzu, lenzu-hud, and (if installed) lenzu-models-dbnet.

set -euo pipefail

PKGS=()
for pkg in lenzu lenzu-hud lenzu-models-dbnet; do
    if dpkg -l "$pkg" 2>/dev/null | grep -q "^ii"; then
        PKGS+=("$pkg")
    fi
done

if [[ ${#PKGS[@]} -eq 0 ]]; then
    echo "Nothing to uninstall — lenzu packages not installed."
    exit 0
fi

echo "Removing: ${PKGS[*]}"
sudo apt remove -y "${PKGS[@]}"
