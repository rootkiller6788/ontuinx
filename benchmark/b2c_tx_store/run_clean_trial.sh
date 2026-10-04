#!/usr/bin/env bash
# B2-C clean trial: unique workdir, pre-flight digest check, isolated build.
set -euo pipefail
LABEL="${1:?}" MODE="${2:?}" RDIR="${3:?}"
B2C=/home/admin1/OntoOS/benchmark/b2c_tx_store
# Only hash the actual task files (not reference/store_ref.c)
BASELINE_FILES=($B2C/include/store.h $B2C/src/store.c $B2C/tests/test_store.c $B2C/CMakeLists.txt)
BASELINE_DIGEST=$(cat "${BASELINE_FILES[@]}" | sha256sum | cut -c1-16)
TS=$(date +%s)
W=/tmp/b2c_${LABEL}_${TS}
mkdir -p $W/include $W/src $W/tests
cp $B2C/include/store.h $W/include/
cp $B2C/src/store.c $W/src/
cp $B2C/tests/test_store.c $W/tests/
cp $B2C/CMakeLists.txt $W/

# Pre-flight: verify task files match baseline
WD_FILES=($W/include/store.h $W/src/store.c $W/tests/test_store.c $W/CMakeLists.txt)
WD_DIGEST=$(cat "${WD_FILES[@]}" 2>/dev/null | sha256sum | cut -c1-16)
if [ "$WD_DIGEST" != "$BASELINE_DIGEST" ]; then
  echo "PREFLIGHT_FAIL: workdir digest $WD_DIGEST != baseline $BASELINE_DIGEST" >&2
  exit 1
fi

# Run
cd $W
FEEDBACK=""; [ "$MODE" = "generic" ] && FEEDBACK="ONTO_FEEDBACK_MODE=generic"
t0=$(date +%s)
env $FEEDBACK ONTO_MAX_LLM_TURNS=3 LLM_BACKEND=deepseek DEEPSEEK_API_KEY=${DEEPSEEK_API_KEY:?} DEEPSEEK_MODEL=deepseek-chat \
  timeout 900 /home/admin1/OntoOS/ironclaw/target/debug/ironclaw run --profile reborn-planned-default \
  --message "Fix src/store.c bugs. All tests pass without memory errors." > $W/out 2>&1; echo "EXIT=$?" >> $W/out
ELAPSED=$(( $(date +%s) - t0 ))

# Results
O=$(sed 's/\x1b\[[0-9;]*m//g' $W/out)
FINAL_FILES=($W/include/store.h $W/src/store.c $W/tests/test_store.c $W/CMakeLists.txt)
FINAL_DIGEST=$(cat "${FINAL_FILES[@]}" 2>/dev/null | sha256sum | cut -c1-16)
BUDGET=$(echo "$O" | grep -c 'budget exhausted' || echo 0)
REASON=$(echo "$O" | grep -oP 'reason_count=\K[0-9]+' | tail -1 || echo 0)
COMMITTED=$(echo "$O" | grep -c 'Committed' || echo 0)
P16=$(echo "$O" | grep -c 'task_outcome' || echo 0)
DIFF=$(diff $B2C/src/store.c $W/src/store.c 2>/dev/null | wc -l)
GRA=$(python3 $B2C/grader/grader.py $W 2>&1 | python3 -c "import sys,json;d=json.load(sys.stdin);print(f'{d[\"tests_passed\"]}/{d[\"tests_total\"]} b={d[\"build_success\"]} a={d[\"asan_clean\"]}')" 2>/dev/null || echo "?/?")
DIGEST_OK="no"; [ "$FINAL_DIGEST" != "$BASELINE_DIGEST" ] && [ "$DIFF" -gt 0 ] && DIGEST_OK="yes"

echo "$LABEL: elapsed=${ELAPSED}s budget=$BUDGET reason=$REASON committed=$COMMITTED p16=$P16 diff=$DIFF grade=$GRA digest_ok=$DIGEST_OK baseline=$BASELINE_DIGEST final=$FINAL_DIGEST"

# Save
mkdir -p $RDIR
cat > $RDIR/$LABEL.json << EOF
{"label":"$LABEL","mode":"$MODE","elapsed_s":$ELAPSED,"budget_hit":$BUDGET,"reason_count":"$REASON","committed":$COMMITTED,"p16_events":$P16,"diff_lines":$DIFF,"grade":"$GRA","digest_ok":"$DIGEST_OK","baseline_digest":"$BASELINE_DIGEST","final_digest":"$FINAL_DIGEST"}
EOF
cp $W/out $RDIR/${LABEL}_stdout.log 2>/dev/null
cp $W/src/store.c $RDIR/${LABEL}_store.c 2>/dev/null
rm -rf $W
