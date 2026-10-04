#!/bin/bash
# R0: Start Rust OntoLoop Worker
WORKER_ID="${1:-worker-1}"
TASK_QUEUE="${2:-onto-loop-v0}"
LOG="/tmp/onto-worker-${WORKER_ID}.log"
PID_FILE="/tmp/onto-worker-${WORKER_ID}.pid"

echo "Starting OntoLoop Worker $WORKER_ID on queue $TASK_QUEUE..."
cd /home/admin1/OntoOS
nohup cargo run -p onto-temporal-adapter --bin onto-worker -- two-attempt > "$LOG" 2>&1 &
echo $! > "$PID_FILE"
echo "✅ Worker $WORKER_ID started (PID $(cat $PID_FILE))"
