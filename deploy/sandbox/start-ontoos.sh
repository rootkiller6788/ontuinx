#!/bin/bash
# OntoOS Sandbox Deployment — grok-build (default)
# Alternative: deploy/docker/docker-compose.yml
set -e

MODE="${1:-sandbox}"  # sandbox | docker
ONTODIR="/home/admin1/OntoOS"

echo "OntoOS v0.1 Deployment"
echo "Mode: $MODE"
echo "===================="

start_sandbox() {
    local name=$1 profile=$2 cmd=$3

    echo "[$name] Starting with profile=$profile..."

    # grok-build sandbox via nono crate
    # In production: grok run --sandbox $profile -- $cmd
    # For now: direct execution with sandbox profile reference
    nohup $cmd > "/tmp/onto-$name.log" 2>&1 &
    echo $! > "/tmp/onto-$name.pid"
    echo "  PID: $(cat /tmp/onto-$name.pid) | Profile: $profile"
}

start_docker() {
    echo "Starting via Docker Compose..."
    cd "$ONTODIR/deploy/docker"
    docker compose up -d
}

# ── PostgreSQL ──
if [ "$MODE" = "sandbox" ]; then
    # PG 假设已运行 (systemd 或 Docker)
    pg_isready 2>/dev/null || docker run -d --name ontoos-pg -e POSTGRES_PASSWORD=ontoos -p 5432:5432 postgres:16
    echo "[postgres] Ready"
else
    echo "[postgres] Docker Compose managed"
fi

# ── Temporal Server ──
TEMPORAL_CMD="$ONTODIR/temporal/temporal-server --config-file $ONTODIR/temporal/config/development.yaml --allow-no-auth start"
if [ "$MODE" = "sandbox" ]; then
    start_sandbox "temporal" "onto-temporal" "$TEMPORAL_CMD"
else
    start_docker
    exit 0
fi

# ── Authority Service ──
start_sandbox "authority" "onto-authority" \
    "cargo run -p onto-temporal-adapter --bin onto-worker -- --mode authority"

# ── Rust Workers ──
for i in a b c; do
    start_sandbox "worker-$i" "onto-worker" \
        "cargo run -p onto-temporal-adapter --bin onto-worker -- --task-queue onto-loop-v0 --worker-id $i"
done

# ── Status ──
echo ""
echo "All services started. Monitor:"
echo "  tail -f /tmp/onto-*.log"
echo "  cat /tmp/onto-*.pid"
echo ""
echo "Stop all: kill \$(cat /tmp/onto-*.pid)"
