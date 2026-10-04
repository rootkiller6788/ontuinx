#!/bin/bash
# Start OntoRuntime as a persistent background service (FULL product stack).
# Run ONCE in a terminal. ontotest connects as a client.

TOKEN=$(cat /home/admin1/.ironclaw/reborn/webui-token 2>/dev/null || echo "ontotest-$(date +%s)")

# ── Full stack config ──
export DATABASE_URL="${DATABASE_URL:-postgres://ironclaw:ironclaw@localhost:5432/ironclaw}"
export OPENAI_API_KEY="${OPENAI_API_KEY:-sk-6278327262384ea29c5c9b06e669f8f3}"
export OPENAI_BASE_URL="${OPENAI_BASE_URL:-https://api.deepseek.com/v1}"
export OPENAI_MODEL="${OPENAI_MODEL:-deepseek-v4-flash}"
export IRONCLAW_REBORN_WEBUI_TOKEN="$TOKEN"
export RUST_LOG="${RUST_LOG:-info}"

LOG="/tmp/ironclaw_service.log"
PIDFILE="/tmp/ironclaw_service.pid"

if [ -f "$PIDFILE" ] && kill -0 $(cat "$PIDFILE") 2>/dev/null; then
    echo "OntoRuntime already running (PID $(cat $PIDFILE))"
    exit 0
fi

echo "Starting OntoRuntime (full product stack)..."
echo "  DB: $DATABASE_URL"
echo "  LLM: $OPENAI_MODEL @ $OPENAI_BASE_URL"
nohup /home/admin1/ironclaw-main/target/debug/ironclaw serve > "$LOG" 2>&1 &
PID=$!
echo $PID > "$PIDFILE"

for i in $(seq 1 30); do
    if curl -sf http://127.0.0.1:3000/api/health > /dev/null 2>&1; then
        echo "OntoRuntime ready (PID $PID, ${i}s)"
        echo "  Log: $LOG"
        echo "  Stop: bash ontotest/stop_ironclaw.sh"
        echo "  Test: python3 ontotest/runner.py --suite e2e"
        exit 0
    fi
    sleep 1
done
echo "ERROR: OntoRuntime failed to start. Check $LOG"
tail -20 "$LOG"
exit 1
