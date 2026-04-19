#!/usr/bin/env bash
# Kill orphaned lenzu_server (Electron HUD) processes.
#
# Use this when lenzu exits without firing its cleanup trap (panic, SIGKILL,
# OS reboot with a stuck zombie, etc.) and the HUD is still running.
#
# Detection:
#   - processes whose cmdline contains "electron" AND the repo's lenzu_server path
#   - whoever is bound to UDP port 7331 (the HUD's configured listen port)
#
# Usage:
#   ./scripts/kill-hud.sh            # dry-run + kill any found
#   ./scripts/kill-hud.sh --check    # report only, exit 0 if clean, 1 if orphans found
#
# Exit codes:
#   0 — nothing to kill, or kill succeeded
#   1 — orphans found in --check mode, or kill failed

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HUD_DIR="$REPO_ROOT/lenzu_server"
UDP_PORT="${LENZU_HUD_UDP_PORT:-7331}"

CHECK_ONLY=0
if [[ "${1:-}" == "--check" ]]; then
    CHECK_ONLY=1
fi

# Collect candidate PIDs.
#
# Match either:
#   - an electron process whose cmdline references this repo's lenzu_server,
#   - OR the process listening on the HUD UDP port.
#
# Electron spawns several helper subprocesses (zygote, GPU, renderer). pgrep -f
# catches them all; we kill the tree rather than try to identify just the main
# process — simpler and safer for cleanup.

mapfile -t PID_BY_CMDLINE < <(pgrep -f "electron.*${HUD_DIR}/dist/main\.js" || true)

PID_BY_PORT=""
if command -v ss >/dev/null 2>&1; then
    # ss output for UDP listeners — extract pid=N from "users:((\"electron\",pid=12345,fd=22))"
    PID_BY_PORT=$(ss -ulnpH "sport = :${UDP_PORT}" 2>/dev/null \
        | grep -oE 'pid=[0-9]+' | cut -d= -f2 | sort -u | tr '\n' ' ' || true)
fi

# Merge + dedupe. Use awk (never non-zero on no-match, unlike grep under pipefail).
ALL_PIDS=$(printf "%s\n" "${PID_BY_CMDLINE[@]}" $PID_BY_PORT | awk '/^[0-9]+$/ && !seen[$0]++')

if [[ -z "$ALL_PIDS" ]]; then
    echo "No lenzu_server HUD processes found — clean."
    exit 0
fi

echo "Found lenzu_server HUD process(es):"
# shellcheck disable=SC2086
ps -o pid,ppid,etime,rss,cmd -p $ALL_PIDS 2>/dev/null || true
echo

if [[ "$CHECK_ONLY" -eq 1 ]]; then
    echo "--check mode — not killing. $(wc -w <<<"$ALL_PIDS") pid(s) above."
    exit 1
fi

echo "Sending SIGTERM..."
# shellcheck disable=SC2086
kill -TERM $ALL_PIDS 2>/dev/null || true

# Wait up to 5 seconds for graceful exit.
for _ in 1 2 3 4 5; do
    REMAINING=$(for pid in $ALL_PIDS; do kill -0 "$pid" 2>/dev/null && echo "$pid"; done)
    [[ -z "$REMAINING" ]] && break
    sleep 1
done

if [[ -n "$REMAINING" ]]; then
    echo "Still alive after SIGTERM, sending SIGKILL to: $REMAINING"
    # shellcheck disable=SC2086
    kill -KILL $REMAINING 2>/dev/null || true
    sleep 1
fi

# Final check.
STILL=$(for pid in $ALL_PIDS; do kill -0 "$pid" 2>/dev/null && echo "$pid"; done)
if [[ -n "$STILL" ]]; then
    echo "ERROR: could not kill: $STILL" >&2
    exit 1
fi

echo "All HUD processes terminated."
