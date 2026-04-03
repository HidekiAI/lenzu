#!/usr/bin/env bash
# Build the GTK client and run it. The client auto-spawns the Electron HUD (`lenzu_server`)
# when `overlay_enabled` is true — do not start a second overlay process here.

set -euo pipefail

REPO_ROOT="$(cd "$(git rev-parse --show-toplevel)" && pwd)"
CLIENT_BINARY="$REPO_ROOT/target/debug/lenzu"

if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
  echo "ERROR: OPENROUTER_API_KEY is not set."
  echo "  export OPENROUTER_API_KEY=sk-your-key-here"
  exit 1
fi

echo "==> Building lenzu client..."
cd "$REPO_ROOT"
cargo build -p lenzu

echo "==> Starting lenzu (spawns Electron overlay when overlay_enabled is true)..."
exec "$CLIENT_BINARY" 2>&1
