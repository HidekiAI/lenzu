#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# ── Node.js via nvm ────────────────────────────────────────────────────────────
export NVM_DIR="$HOME/.nvm"
if [ ! -d "$NVM_DIR" ]; then
    echo "Installing nvm..."
    curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
fi
# shellcheck source=/dev/null
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh"

# Install & activate the Node version pinned in .nvmrc
nvm install   # reads .nvmrc
nvm use       # reads .nvmrc
echo "Node: $(node --version)"

# ── pnpm ──────────────────────────────────────────────────────────────────────
if ! command -v pnpm >/dev/null 2>&1; then
    echo "Installing pnpm..."
    npm install -g pnpm
fi
echo "pnpm: $(pnpm --version)"

# ── Project dependencies ───────────────────────────────────────────────────────
echo "Installing project dependencies..."
pnpm install

# ── Electron binary ───────────────────────────────────────────────────────────
# pnpm v10 blocks build scripts by default; run them explicitly so the
# Electron binary is actually downloaded (not just the npm package metadata).
ELECTRON_BINARY="node_modules/electron/dist/electron"

echo "Checking Electron binary..."
if [ ! -x "$ELECTRON_BINARY" ]; then
    echo "Electron binary not found — running install script..."
    node node_modules/electron/install.js
fi

if [ ! -x "$ELECTRON_BINARY" ]; then
    echo "ERROR: Electron binary still missing after install. Check network or proxy settings." >&2
    exit 1
fi

ELECTRON_VERSION=$(node -e "process.stdout.write(require('./node_modules/electron/package.json').version)")
echo "Electron: ${ELECTRON_VERSION} ($(realpath "$ELECTRON_BINARY"))"

# ── esbuild binary ────────────────────────────────────────────────────────────
if [ ! -f "node_modules/esbuild/bin/esbuild" ]; then
    echo "esbuild binary not found — running install script..."
    node node_modules/esbuild/install.js
fi

# Install picom to fix the X11 transparency issues (if not already installed)
if ! command -v picom >/dev/null 2>&1; then
    echo "Installing picom for X11 transparency support..."
    sudo apt install -y picom
fi  
picom --backend glx --no-use-damage &

echo ""
echo "Setup complete."
echo ""
echo "To run the demo:"
echo "  nvm use && bash scripts/demo.sh"
echo ""
echo "To start the app directly:"
echo "  nvm use && pnpm start"
