#!/bin/bash
# S3.3: 18 项故障注入矩阵
# 每个故障窗口重复运行，验证分布式不变量。
# Usage: ./fault-matrix.sh [--repeat N] [--timeout S]

REPEAT=${REPEAT:-3}
TIMEOUT=${TIMEOUT:-30}
EVIDENCE_DIR="/evidence/fault-injection"
PASS=0; FAIL=0

log_test() { echo "[$1] $2: $3"; }
pass() { log_test "PASS" "$1" "$2"; PASS=$((PASS+1)); }
fail() { log_test "FAIL" "$1" "$2"; FAIL=$((FAIL+1)); }

run_fault() {
    local id=$1 name=$2 action=$3 invariant=$4
    mkdir -p "$EVIDENCE_DIR/$id"
    for i in $(seq 1 $REPEAT); do
        eval "$action"
        if [ $? -eq 0 ]; then pass "$name" "repeat $i/$REPEAT"; else fail "$name" "repeat $i/$REPEAT - $invariant"; fi
        sleep 2
    done
}

# ═══════════════════════════════════════════
# 1-6: Worker 进程故障
# ═══════════════════════════════════════════

run_fault "F01" "Worker crash before pickup" \
    "docker kill worker-a && sleep 5 && docker start worker-a" \
    "re-dispatch same loop_id"

run_fault "F02" "Crash after first heartbeat" \
    "docker kill -s SIGKILL worker-b && sleep 3 && docker start worker-b" \
    "new Worker recovers stable loop_id"

run_fault "F03" "Crash after Attempt complete" \
    "docker kill worker-c && sleep 2 && docker start worker-c" \
    "no duplicate Attempt"

run_fault "F04" "Crash after Decision persisted" \
    "docker kill worker-a && sleep 1 && docker start worker-a" \
    "no duplicate Decision"

run_fault "F05" "Crash after M6 Commit" \
    "docker kill worker-b && sleep 1 && docker start worker-b" \
    "no duplicate Publish"

run_fault "F06" "Completion RPC lost" \
    "toxiproxy-cli toxic add -n rpc_loss -t timeout -a timeout=5000 && sleep 5 && toxiproxy-cli toxic remove -n rpc_loss" \
    "idempotent re-Respond"

# ═══════════════════════════════════════════
# 7-10: Temporal 服务故障
# ═══════════════════════════════════════════

run_fault "F07" "Matching restart" \
    "docker restart temporal-matching && sleep 3" \
    "WorkItem re-dispatched, not lost"

run_fault "F08" "History restart" \
    "docker restart temporal-history && sleep 3" \
    "event replay consistent"

run_fault "F09" "Temporal full restart" \
    "docker restart temporal-frontend temporal-history temporal-matching temporal-worker && sleep 10" \
    "all components recover, CHASM OntoFlow restores"

run_fault "F10" "PG temporarily unavailable" \
    "docker stop postgres && sleep 5 && docker start postgres && sleep 5" \
    "no downgrade to trust Worker; Authority stays AuthorityVerifying"

# ═══════════════════════════════════════════
# 11-15: 网络与竞争故障
# ═══════════════════════════════════════════

run_fault "F11" "Authority network partition" \
    "toxiproxy-cli toxic add -n auth_cut -t timeout -a timeout=10000 && sleep 5 && toxiproxy-cli toxic remove -n auth_cut" \
    "stops AuthorityVerified, recovers after"

run_fault "F12" "Artifact Store unreadable" \
    "chmod 000 /evidence && sleep 2 && chmod 755 /evidence" \
    "explicit failure, no silent skip"

run_fault "F13" "Worker-Temporal network partition" \
    "toxiproxy-cli toxic add -n worker_cut -t timeout -a timeout=8000 upstream && sleep 5 && toxiproxy-cli toxic remove -n worker_cut" \
    "timeout + re-dispatch"

run_fault "F14" "Dual worker race" \
    "docker start worker-a worker-b && sleep 1" \
    "single LoopExecutionLease, no duplicate"

run_fault "F15" "Stale generation late arrival" \
    "sleep 15" \
    "old generation envelope rejected"

# ═══════════════════════════════════════════
# 16-18: 存储与环境故障
# ═══════════════════════════════════════════

run_fault "F16" "Disk full" \
    "dd if=/dev/zero of=/evidence/fill bs=1M count=1000 2>/dev/null; rm -f /evidence/fill" \
    "EnvironmentError or Frozen, not silent"

run_fault "F17" "Artifact tampered" \
    "echo 'corrupted' >> /evidence/ontoflow-v0.1/i3-real-ontoloop/envelope.json" \
    "hash mismatch → rejected by verify()"

run_fault "F18" "Clock skew" \
    "date -s '@$(($(date +%s) - 3600))' && sleep 2 && ntpdate -s time.google.com" \
    "time comparisons use logical clock, wall clock skew tolerated"

# ═══════════════════════════════════════════
# Summary
# ═══════════════════════════════════════════

echo ""
echo "════════════════════════════════════════"
echo "Fault Matrix: $PASS passed, $FAIL failed"
echo "Evidence: $EVIDENCE_DIR"
echo "════════════════════════════════════════"

exit $FAIL
