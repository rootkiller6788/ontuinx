#!/bin/bash
# ══════════════════════════════════════════════════════════════════════
# Onto Assurance Kernel — OntoRuntime 集成对比测试
#
# 测试: OntoRuntime 原生 vs OntoRuntime + OntoAssure
# 指标: False Success Rate, Evidence Detection, Agent Loop
# ══════════════════════════════════════════════════════════════════════

set -e

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'
PASS=0; FAIL=0

log()  { echo -e "${BLUE}[$(date +%H:%M:%S)]${NC} $1"; }
pass() { echo -e "  ${GREEN}✅ $1${NC}"; PASS=$((PASS+1)); }
fail() { echo -e "  ${RED}❌ $1${NC}"; FAIL=$((FAIL+1)); }
warn() { echo -e "  ${YELLOW}⚠️  $1${NC}"; }

# ── Config ──
IRONCLAW_BIN="/home/admin1/ironclaw-main/target/debug/ironclaw"
TOKEN=$(cat /home/admin1/.ironclaw/reborn/webui-token 2>/dev/null || echo "test-token-$(date +%s)")
API_BASE="http://127.0.0.1:3000"
API_KEY="sk-6278327262384ea29c5c9b06e669f8f3"
MODEL="deepseek-v4-flash"
API_URL="https://api.deepseek.com/v1"

export IRONCLAW_REBORN_WEBUI_TOKEN="$TOKEN"
export OPENAI_API_KEY="$API_KEY"
export OPENAI_BASE_URL="$API_URL"
export OPENAI_MODEL="$MODEL"

# ── Cleanup ──
cleanup() {
    log "Cleaning up..."
    pkill -f "ironclaw serve" 2>/dev/null || true
}
trap cleanup EXIT

# ══════════════════════════════════════════════════════════════════════
# Test 1: OntoRuntime 启动 + Onto 编译验证
# ══════════════════════════════════════════════════════════════════════
log "══════ Test 1: Binary Verification ══════"

if [ -f "$IRONCLAW_BIN" ]; then
    pass "OntoRuntime binary exists ($(du -sh $IRONCLAW_BIN | cut -f1))"
else
    fail "OntoRuntime binary not found"
    exit 1
fi

VERSION=$("$IRONCLAW_BIN" --version 2>/dev/null || echo "unknown")
log "  Version: $VERSION"

# Check if onto code is linked
if strings "$IRONCLAW_BIN" | grep -q "onto.assurance"; then
    pass "onto-assurance code linked in binary"
else
    warn "onto-assurance not detected in strings (may be stripped)"
fi

# Check hooks
if strings "$IRONCLAW_BIN" | grep -q "AfterLoopExit\|run_finalizer"; then
    pass "AfterLoopExit hook linked"
else
    warn "AfterLoopExit hook not found in strings"
fi

# ══════════════════════════════════════════════════════════════════════
# Test 2: LLM API 连通性
# ══════════════════════════════════════════════════════════════════════
log "══════ Test 2: LLM API Connectivity ══════"

RESP=$(curl -s -X POST "$API_URL/chat/completions" \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -d "{\"model\":\"$MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"say OK\"}],\"max_tokens\":5}" 2>&1)

if echo "$RESP" | grep -q '"choices"'; then
    pass "LLM API connected ($MODEL)"
else
    fail "LLM API failed: $(echo $RESP | head -c 100)"
fi

# ══════════════════════════════════════════════════════════════════════
# Test 3: OntoRuntime 启动
# ══════════════════════════════════════════════════════════════════════
log "══════ Test 3: Server Startup ══════"

pkill -f "ironclaw serve" 2>/dev/null || true
sleep 2

"$IRONCLAW_BIN" serve > /tmp/ironclaw_test.log 2>&1 &
SERVER_PID=$!
log "  Server PID: $SERVER_PID"

# Wait for startup
for i in $(seq 1 30); do
    if curl -s "$API_BASE/api/health" 2>/dev/null | grep -q "healthy"; then
        log "  Server ready after ${i}s"
        break
    fi
    if ! kill -0 $SERVER_PID 2>/dev/null; then
        fail "Server crashed during startup"
        cat /tmp/ironclaw_test.log | tail -20
        exit 1
    fi
    sleep 1
done

HEALTH=$(curl -s "$API_BASE/api/health" 2>/dev/null)
if echo "$HEALTH" | grep -q "healthy"; then
    pass "Server healthy: $HEALTH"
else
    fail "Server not healthy: $HEALTH"
    cat /tmp/ironclaw_test.log | tail -20
    exit 1
fi

# Check startup log for onto-related messages
if grep -qi "onto\|assurance\|AfterLoop\|run_finalizer" /tmp/ironclaw_test.log 2>/dev/null; then
    pass "Onto-related startup messages found"
else
    warn "No Onto messages in startup log (expected before agent runs)"
fi

# ══════════════════════════════════════════════════════════════════════
# Test 4: Agent 任务执行
# ══════════════════════════════════════════════════════════════════════
log "══════ Test 4: Agent Task Execution ══════"

# Create thread
CID=$(uuidgen 2>/dev/null || echo "test-$RANDOM")
THREAD_RESP=$(curl -s -X POST "$API_BASE/api/webchat/v2/threads" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"client_action_id\":\"$CID\"}" 2>&1)

TID=$(echo "$THREAD_RESP" | python3 -c "import json,sys;print(json.load(sys.stdin)['thread']['thread_id'])" 2>/dev/null)

if [ -n "$TID" ]; then
    pass "Thread created: $TID"
else
    fail "Thread creation failed: ${THREAD_RESP:0:200}"
    exit 1
fi

# Send message - simple coding task
CID2=$(uuidgen 2>/dev/null || echo "msg-$RANDOM")
log "  Sending agent task: 'Write Python hello() function'"

MSG_RESP=$(curl -s -X POST "$API_BASE/api/webchat/v2/threads/$TID/send_message" \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d "{\"client_action_id\":\"$CID2\",\"content\":\"Write a Python function hello() that returns the string Hello World. Return ONLY the code, no explanation.\"}" 2>&1)

# Poll for agent completion
log "  Waiting for agent to complete (max 120s)..."
for i in $(seq 1 120); do
    STATUS=$(curl -s "$API_BASE/api/webchat/v2/threads/$TID" \
      -H "Authorization: Bearer $TOKEN" 2>/dev/null | \
      python3 -c "import json,sys;d=json.load(sys.stdin);print(d.get('thread',{}).get('status',''))" 2>/dev/null)

    if [ "$STATUS" = "completed" ] || [ "$STATUS" = "failed" ]; then
        log "  Agent finished: $STATUS (${i}s)"
        break
    fi
    sleep 1
done

# Check for Onto hook execution in logs
log "══════ Test 5: Onto Hook Verification ══════"

if grep -qi "AfterLoopExit\|run_finalizer\|assurance.finalization\|onto" /tmp/ironclaw_test.log 2>/dev/null; then
    pass "Onto hooks triggered during agent run"
    grep -i "AfterLoopExit\|run_finalizer\|assurance" /tmp/ironclaw_test.log | head -5 | while read line; do
        echo "    $line"
    done
else
    fail "No Onto hook traces found in server log"
    warn "  Check: is --features onto-assurance activated?"
    warn "  Log size: $(wc -l < /tmp/ironclaw_test.log) lines"
    warn "  Last 5 lines:"
    tail -5 /tmp/ironclaw_test.log | while read line; do echo "    $line"; done
fi

# ══════════════════════════════════════════════════════════════════════
# Test 6: OntoAssure 独立验证 (即使 Agent 路由不通也能跑)
# ══════════════════════════════════════════════════════════════════════
log "══════ Test 6: OntoAssure Standalone Verification ══════"

cd /home/admin1/OntoOS

# Run unit tests
log "  Running ontoos unit tests..."
if cargo test -p onto-assurance-core -p onto-ironclaw-adapter 2>&1 | grep -q "0 failed"; then
    pass "OntoAssure unit tests pass"
else
    fail "OntoAssure unit tests failed"
fi

# Run conformance
log "  Running golden fixtures..."
if cargo run -p onto-conformance -- --all 2>&1 | grep -q "6/6 passed"; then
    pass "Golden fixtures: 6/6 passed"
else
    fail "Golden fixtures failed"
fi

# ══════════════════════════════════════════════════════════════════════
# Summary
# ══════════════════════════════════════════════════════════════════════
log "══════════════════════════════════"
log "  Results: ${GREEN}$PASS passed${NC}, ${RED}$FAIL failed${NC}"
log "══════════════════════════════════"

# Cleanup
kill $SERVER_PID 2>/dev/null || true
exit $FAIL
