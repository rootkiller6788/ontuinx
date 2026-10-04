#!/usr/bin/env bash
# B2 trial supervisor — runs one trial inside a systemd-run transient
# cgroup with hard memory/task limits, so a runaway cannot kill WSL.
# Usage: run_trial_supervised.sh <label> <structured|generic> <out_dir>
set -Eeuo pipefail

LABEL="${1:?label required}"
MODE="${2:?mode required}"
OUT_DIR="${3:?out dir required}"
B2="/home/admin1/OntoOS/benchmark/b2_bug_fix"
TRIAL_BIN="$B2/run_single_trial.sh"

mkdir -p "$OUT_DIR"

# --- Pre-flight health checks (circuit breaker) ---
AVAIL_KB="$(awk '/MemAvailable:/ {print $2}' /proc/meminfo)"
MIN_KB=$((6 * 1024 * 1024))
if (( AVAIL_KB < MIN_KB )); then
  echo "CIRCUIT-BREAKER: available ${AVAIL_KB} KiB < ${MIN_KB} KiB; not starting $LABEL"
  exit 75
fi

RESIDUAL="$(pgrep -af 'ironclaw run|run_single_trial|cmake|make|ctest' | grep -v grep || true)"
if [[ -n "$RESIDUAL" ]]; then
  echo "CIRCUIT-BREAKER: residual processes present; aborting $LABEL:"
  echo "$RESIDUAL"
  exit 75
fi

# --- Run inside transient cgroup ---
# KillMode=control-group + KillSignal + MemoryMax + TasksMax contain any fan-out.
systemd-run --user \
  --wait \
  --collect \
  --unit="b2-${LABEL}-$(date +%s)" \
  --property=KillMode=control-group \
  --property=MemoryMax=8G \
  --property=MemorySwapMax=2G \
  --property=TasksMax=256 \
  --property=CPUQuota=400% \
  --property=TimeoutStopSec=60 \
  --setenv=DEEPSEEK_API_KEY="${DEEPSEEK_API_KEY}" \
  bash -c "ONTO_FEEDBACK_MODE='${MODE}' '$TRIAL_BIN' '$LABEL' '$MODE' '$OUT_DIR'"
STATUS=$?

if [[ $STATUS -eq 0 ]] && [[ -f "$OUT_DIR/status.json" ]]; then
  echo "[supervisor] $LABEL done: $(cat "$OUT_DIR/status.json")"
else
  echo "[supervisor] $LABEL FAILED (systemd-run exit=$STATUS)"
fi

# --- Post-run residual check ---
sleep 2
REMAIN="$(pgrep -af 'ironclaw run|cmake|make|ctest' | grep -v grep || true)"
if [[ -n "$REMAIN" ]]; then
  echo "WARNING: residuals after $LABEL:"
  echo "$REMAIN"
  exit 76
fi

exit $STATUS
