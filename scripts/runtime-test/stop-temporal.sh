#!/bin/bash
PID_FILE="${TEMPORAL_PID:-/tmp/temporal-server.pid}"
if [ -f "$PID_FILE" ]; then
    PID=$(cat "$PID_FILE")
    kill "$PID" 2>/dev/null && echo "Temporal Server stopped (PID $PID)" || echo "Temporal Server not running"
    rm -f "$PID_FILE"
fi
