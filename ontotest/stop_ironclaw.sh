#!/bin/bash
PIDFILE="/tmp/ironclaw_service.pid"
if [ -f "$PIDFILE" ]; then
    PID=$(cat "$PIDFILE")
    echo "Stopping OntoRuntime (PID $PID)..."
    kill -TERM $PID 2>/dev/null
    sleep 3
    kill -KILL $PID 2>/dev/null
    rm -f "$PIDFILE"
    echo "Stopped."
else
    echo "No PID file found."
fi
