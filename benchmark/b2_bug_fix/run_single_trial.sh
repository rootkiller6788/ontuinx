#!/usr/bin/env bash
# B2 single trial runner — one trial, one isolated process group.
# Usage: run_single_trial.sh <label> <structured|generic> <out_dir>
# Must be called inside a systemd-run cgroup (see run_trial_supervised.sh).
set -Eeuo pipefail

LABEL="${1:?label required}"
MODE="${2:?mode required (structured|generic)}"
OUT_DIR="${3:?out dir required}"

B2="/home/admin1/OntoOS/benchmark/b2_bug_fix"
IRONCLAW="/home/admin1/OntoOS/ironclaw/target/debug/ironclaw"
mkdir -p "$OUT_DIR"
echo "[single_trial] starting LABEL=$LABEL MODE=$MODE OUT=$OUT_DIR at $(date +%T)" >>"$OUT_DIR/stdout.log"

# --- Workdir ---
WORKDIR="$(mktemp -d /tmp/b2-${LABEL}-XXXXXX)"
cp "$B2"/template/*.c "$B2"/template/*.h "$B2"/template/CMakeLists.txt "$WORKDIR"/

mkdir -p "$OUT_DIR"
START_EPOCH="$(date +%s)"

# --- Feedback mode ---
export ONTO_FEEDBACK_MODE="$MODE"
export LLM_BACKEND=deepseek
export DEEPSEEK_API_KEY="${DEEPSEEK_API_KEY:?DEEPSEEK_API_KEY required}"
export DEEPSEEK_MODEL=deepseek-chat
# Bound build parallelism — the cmake/make/ASan fan-out is what OOM'd WSL.
export CMAKE_BUILD_PARALLEL_LEVEL=2
export MAKEFLAGS="-j2"
export TOKIO_WORKER_THREADS=4
export RAYON_NUM_THREADS=2

# --- Run (bounded, group-killable, in project workdir) ---
# 900s agent budget + 180s finalization grace = 1080s total.
# kill-after=120s gives cmake/ctest/ASan time to flush before SIGKILL.
set +e
(
  cd "$WORKDIR" &&
  timeout --signal=TERM --kill-after=120s 1080s \
    "$IRONCLAW" run --profile reborn-planned-default \
      --message "$(cat "$B2/prompt.txt")" \
    >"$OUT_DIR/stdout.log" 2>&1
)
RUN_STATUS=$?
set -e

END_EPOCH="$(date +%s)"
ELAPSED=$(( END_EPOCH - START_EPOCH ))

# --- Grade against a CLEAN snapshot (copy agent output first) ---
GRADE_DIR="$(mktemp -d /tmp/b2-grade-XXXXXX)"
cp "$B2"/template/kv_config.h "$B2"/template/test_kv_config.c "$B2"/template/CMakeLists.txt "$GRADE_DIR"/
cp "$WORKDIR/kv_config.c" "$GRADE_DIR/kv_config.c" 2>/dev/null || cp "$B2/template/kv_config.c" "$GRADE_DIR/kv_config.c"
# grader always prints ONE line of JSON on stdout
python3 "$B2/grader/grader.py" "$GRADE_DIR" >"$OUT_DIR/grader.json" 2>"$OUT_DIR/grader.stderr"
GRADER_RC=$?
rm -rf "$GRADE_DIR"

# Validate JSON schema with jq
if ! jq -e '(.build_success|type=="boolean") and (.asan_clean|type=="boolean") and (.tests_passed|type=="number") and (.tests_total|type=="number")' "$OUT_DIR/grader.json" >/dev/null 2>&1; then
  echo "GRADER_INVALID: rc=$GRADER_RC stderr=$(cat "$OUT_DIR/grader.stderr")" >>"$OUT_DIR/stdout.log"
  TESTS="invalid"
  BUILD="invalid"
  ASAN="invalid"
  GRADE_STATUS="GRADER_ERROR"
else
  TESTS="$(jq -r '"\(.tests_passed)/\(.tests_total)"' "$OUT_DIR/grader.json")"
  BUILD="$(jq -r '.build_success' "$OUT_DIR/grader.json")"
  ASAN="$(jq -r '.asan_clean' "$OUT_DIR/grader.json")"
  GRADE_STATUS="$(jq -r '.failure_stage // ("PASS")' "$OUT_DIR/grader.json")"
  [ "$GRADE_STATUS" = "null" ] && GRADE_STATUS="PASS"
fi
echo "[grade] rc=$GRADER_RC status=$GRADE_STATUS tests=$TESTS build=$BUILD asan=$ASAN" >>"$OUT_DIR/stdout.log"

OUT="$(sed 's/\x1b\[[0-9;]*m//g' "$OUT_DIR/stdout.log")"
P16_EVENTS="$(echo "$OUT" | grep -c 'task_outcome' || echo 0)"
COMMITTED="$(echo "$OUT" | grep -c 'Committed' || echo 0)"

# --- Atomic result write ---
cat >"$OUT_DIR/status.tmp" <<EOF
{
  "label": "$LABEL",
  "mode": "$MODE",
  "run_status": $RUN_STATUS,
  "elapsed_s": $ELAPSED,
  "tests": "$TESTS",
  "build": "$BUILD",
  "asan": "$ASAN",
  "grade_status": "$GRADE_STATUS",
  "p16_events": "$P16_EVENTS",
  "committed": "$COMMITTED",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF
mv "$OUT_DIR/status.tmp" "$OUT_DIR/status.json"

# --- Save artifacts ---
cp "$WORKDIR/kv_config.c" "$OUT_DIR/attempt-final-kv_config.c" 2>/dev/null || true
cp "$OUT_DIR/stdout.log" "$OUT_DIR/p16.log" 2>/dev/null || true

# --- Cleanup ---
rm -rf "$WORKDIR"

echo "[$LABEL] mode=$MODE status=$RUN_STATUS elapsed=${ELAPSED}s tests=$TESTS build=$BUILD asan=$ASAN p16=$P16_EVENTS committed=$COMMITTED"
