#!/usr/bin/env bash
# Run lenzu (client + server) in one shot.
#
# First run: builds lenzu_server via Tauri CLI (slow — bundles the UI).
# Subsequent runs: reuses the existing binary unless --rebuild is passed.
#
# Usage:
#   ./scripts/run.sh              # build if needed, then run
#   ./scripts/run.sh --rebuild    # force rebuild of lenzu_server
#   ./scripts/run.sh --dev        # run lenzu_server via 'tauri dev' (hot-reload UI)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVER_SRC="$REPO_ROOT/lenzu_server"
SERVER_BIN="$SERVER_SRC/src-tauri/target/release/lenzu_server"
CLIENT_TARGET="$REPO_ROOT/target/debug"

DEV_MODE=false
REBUILD=false
for arg in "$@"; do
  case "$arg" in
    --dev)     DEV_MODE=true ;;
    --rebuild) REBUILD=true ;;
  esac
done

if [[ -z "${OPENROUTER_API_KEY:-}" ]]; then
  echo "ERROR: OPENROUTER_API_KEY is not set."
  echo "  export OPENROUTER_API_KEY=sk-your-key-here"
  exit 1
fi

if $DEV_MODE; then
  echo "==> Dev mode: starting lenzu_server via 'cargo tauri dev' in background"
  echo "    (set overlay_enabled = false in lenzu_config.json if lenzu also tries to spawn it)"
  cd "$SERVER_SRC"
  npm run tauri dev -- -- --port 7331 &
  SERVER_PID=$!
  cd "$REPO_ROOT"
  echo "    lenzu_server PID: $SERVER_PID"
  echo "    Waiting 5s for server to start..."
  sleep 5
  trap "kill $SERVER_PID 2>/dev/null; wait $SERVER_PID 2>/dev/null" EXIT
  cargo run -p lenzu_client
else
  # Production-like: build lenzu_server once, copy next to client binary, run client
  if $REBUILD || [[ ! -f "$SERVER_BIN" ]]; then
    echo "==> Building lenzu_server (Tauri release build)..."
    cd "$SERVER_SRC"
    npm install --silent
    npm run tauri build
    cd "$REPO_ROOT"
  else
    echo "==> lenzu_server already built (pass --rebuild to force)"
  fi

  echo "==> Building lenzu client..."
  cargo build -p lenzu_client

  echo "==> Copying lenzu_server binary next to lenzu..."
  mkdir -p "$CLIENT_TARGET"
  cp "$SERVER_BIN" "$CLIENT_TARGET/lenzu_server"

  echo "==> Starting lenzu (will auto-spawn lenzu_server)..."
  "$CLIENT_TARGET/lenzu"
fi
