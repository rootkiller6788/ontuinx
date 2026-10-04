#!/bin/bash
WORKER_ID="${1:-worker-1}"
PID_FILE="/tmp/onto-worker-${WORKER_ID}.pid"
if [ -f "$PID_FILE" ]; then
    PID=$(cat "$PID_FILE")
    kill "$PID" 2>/dev/null && echo "Worker $WORKER_ID stopped (PID $PID)" || echo "Worker not running"
    rm -f "$PID_FILE"
fi
