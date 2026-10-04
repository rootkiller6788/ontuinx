#!/bin/bash
# Collect runtime evidence from all logs
OUT_DIR="${1:-/home/admin1/OntoOS/evidence/ontoflow-v0.1/r0-worker}"
mkdir -p "$OUT_DIR"
cp /tmp/temporal-runtime.log "$OUT_DIR/temporal-server.log" 2>/dev/null
cp /tmp/onto-worker-*.log "$OUT_DIR/" 2>/dev/null
echo "Evidence collected in $OUT_DIR"
