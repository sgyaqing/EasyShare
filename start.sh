#!/bin/sh
# Start the EasyShare server in the background.
# Usage: ./start.sh [port]   (default port: 8972)
set -e
cd "$(dirname "$0")"

PORT="${1:-8972}"
PID_FILE=".easyshare.pid"
OUT_FILE=".easyshare.out"

if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    echo "EasyShare is already running (PID $(cat "$PID_FILE"))"
    exit 0
fi

# Build only when the binary is missing or sources are newer.
if [ ! -x target/debug/easyshare ] || [ -n "$(find src static Cargo.toml Cargo.lock -newer target/debug/easyshare 2>/dev/null)" ]; then
    echo "Building easyshare..."
    cargo build
fi

nohup target/debug/easyshare --port "$PORT" > "$OUT_FILE" 2>&1 &
echo $! > "$PID_FILE"

sleep 1
# The banner proves the process bound the port successfully (a bare kill -0
# can pass briefly even when the process is dying from a bind failure).
if ! kill -0 "$(cat "$PID_FILE")" 2>/dev/null || ! grep -q "Server listening on" "$OUT_FILE"; then
    echo "Failed to start EasyShare. Output:"
    cat "$OUT_FILE"
    rm -f "$PID_FILE"
    exit 1
fi

grep "Server listening on" "$OUT_FILE" || true
echo "EasyShare started (PID $(cat "$PID_FILE"), port $PORT). Stop with: ./stop.sh"
