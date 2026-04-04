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

# Require picom — xfwm4's compositor causes ghost pixels; Electron 36–41 also
# has an ARGB regression.  Without picom the overlay will not be transparent.
if ! pgrep -x picom >/dev/null; then
    echo "WARNING: picom is not running. Start it first:"
    echo "  xfconf-query -c xfwm4 -p /general/use_compositing -s false"
    echo "  picom --backend glx --no-use-damage &"
    echo "EXITING! — transparency will not work correctly."
    exit 1
fi

# Kill any existing HUD instances and free the UDP port
pkill -f "electron.*dist/main.js" 2>/dev/null || true
fuser -k "${UDP_PORT}/udp" 2>/dev/null || true
# Wait until the port is actually free before starting fresh
while ss -ulnp | grep -q ":${UDP_PORT}"; do sleep 0.2; done

# Build and launch the app in the background
cd "$PROJECT_ROOT"
pnpm run build
# Launch Electron directly so APP_PID is Electron's PID (not pnpm's)
GTK_CSD=0 node_modules/.bin/electron dist/main.js &
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
wait "$APP_PID" 2>/dev/null || true

echo "Demo complete."
