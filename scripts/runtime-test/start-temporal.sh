#!/bin/bash
# R0: Start Temporal Server with OntoFlow CHASM registration
set -e

BINARY="${TEMPORAL_BIN:-/tmp/temporal-server}"
CONFIG="${TEMPORAL_CONFIG:-/home/admin1/OntoOS/temporal/config/development.yaml}"
LOG="${TEMPORAL_LOG:-/tmp/temporal-runtime.log}"
PID_FILE="${TEMPORAL_PID:-/tmp/temporal-server.pid}"

if [ -f "$PID_FILE" ]; then
    OLD_PID=$(cat "$PID_FILE")
    if kill -0 "$OLD_PID" 2>/dev/null; then
        echo "Temporal Server already running (PID $OLD_PID)"
        exit 0
    fi
fi

echo "Starting Temporal Server..."
cd /home/admin1/OntoOS/temporal
nohup "$BINARY" --config-file "$CONFIG" --allow-no-auth start > "$LOG" 2>&1 &
echo $! > "$PID_FILE"

# Wait for healthy
for i in $(seq 1 30); do
    if grep -q "OntoFlow library registered" "$LOG" 2>/dev/null; then
        echo "✅ Temporal Server ready (PID $(cat $PID_FILE))"
        exit 0
    fi
    sleep 1
done
echo "❌ Temporal Server failed to start"
exit 1
