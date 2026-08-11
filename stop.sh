#!/bin/sh
# Stop the EasyShare server started by start.sh.
cd "$(dirname "$0")"

PID_FILE=".easyshare.pid"

if [ ! -f "$PID_FILE" ]; then
    echo "EasyShare is not running (no PID file)"
    exit 0
fi

PID=$(cat "$PID_FILE")
if kill -0 "$PID" 2>/dev/null; then
    kill "$PID"
    echo "EasyShare stopped (PID $PID)"
else
    echo "EasyShare process $PID was not running"
fi
rm -f "$PID_FILE"
