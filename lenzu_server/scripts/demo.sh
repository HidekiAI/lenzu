#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
UDP_PORT=$(jq -r '.udp_port // 7331' "${PROJECT_ROOT}/hud_config.json" 2>/dev/null || echo 7331)
ORIGINAL_TEXT="Hello world, Hello Shiroe!"
LOREM_WIDE="Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua."

# Ensure netcat (with UDP support) is installed
if [ ! -e "$(which nc 2>/dev/null)" ]; then
    echo "netcat not found, installing netcat-openbsd..."
    sudo apt install -y netcat-openbsd
fi

# Install dependencies before building/running
echo "Installing pnpm dependencies..."
pnpm install --silent

# Send a string to the HUD over UDP loopback
send_hud() {
    printf '%s' "$1" | nc -u -w1 127.0.0.1 "$UDP_PORT"
}

# Warn if picom is not running
if ! pgrep -x picom >/dev/null; then
    echo "WARNING: picom is not running. Start it first:"
    echo "  xfconf-query -c xfwm4 -p /general/use_compositing -s false"
    echo "  picom --backend glx --no-use-damage &"
    echo "Continuing anyway — transparency may not work correctly."
fi

# Kill any existing HUD instances
pkill -f "electron.*dist/main.js" 2>/dev/null || true
sleep 1

# Build and launch the app in the background
cd "$PROJECT_ROOT"
pnpm run build
pnpm exec electron dist/main.js &
APP_PID=$!

# Wait until the UDP port is bound (app is ready to receive messages)
echo "Waiting for app to start..."
until ss -ulnp | grep -q ":${UDP_PORT}"; do
    sleep 1
done
echo "App ready. Sending test messages..."
sleep 1

DELAY=3

send_hud "Get ready..."
sleep $DELAY

send_hud "3"
sleep $DELAY

send_hud "2"
sleep $DELAY

send_hud "1"
sleep $DELAY

# Long line — triggers min_font_size shrink if min_font_size_pt > 0 in hud_config.json
send_hud "$LOREM_WIDE"
sleep $DELAY

send_hud "$ORIGINAL_TEXT"
sleep $DELAY

kill "$APP_PID" 2>/dev/null
wait "$APP_PID" 2>/dev/null

echo "Demo complete."
