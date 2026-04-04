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

# ── Approve build scripts (pnpm v10 blocks them by default) ───────────────────
# Run approve-builds so Electron's post-install download script is allowed on
# subsequent installs. This is interactive — select 'electron' with <space> then
# press <enter>. If running non-interactively the fallback below handles it.
if [ -t 0 ]; then
    echo "Approving pnpm build scripts (select 'electron', press <space> then <enter>)..."
    pnpm approve-builds || true
fi

# ── Electron binary ───────────────────────────────────────────────────────────
# Fallback: if approve-builds was skipped or the binary still isn't present,
# run the install script directly.
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

# ── TypeScript compile check ──────────────────────────────────────────────────
echo "Running TypeScript type check..."
pnpm exec tsc --noEmit

echo ""
echo "Setup complete."
echo ""
echo "To run the demo:"
echo "  nvm use && bash scripts/demo.sh"
echo ""
echo "To start the app directly:"
echo "  nvm use && pnpm start"
