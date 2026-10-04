#!/usr/bin/env bash
# P16-P0: Production anti-regression gates.
set -euo pipefail
cd "$(dirname "$0")/.."
FAIL=0

# Helper: check a file for forbidden patterns, excluding #[cfg(test)] blocks.
# Takes: pattern, file, description
check_no_pattern_outside_tests() {
  local pattern="$1" file="$2" desc="$3"
  local in_test=0 line_num=0
  while IFS= read -r line; do
    line_num=$((line_num + 1))
    [[ "$line" == *"#[cfg(test)]"* ]] && in_test=1
    [[ "$line" == "#[cfg(test)]"* ]] && in_test=1  
    [[ "$line" == "}" && $in_test -eq 1 ]] && { in_test=0; continue; }
    if [[ $in_test -eq 0 ]] && echo "$line" | grep -q "$pattern"; then
      echo "FAIL:$file:$line_num: $desc — found: $line"
      FAIL=1
    fi
  done < "$file"
}

echo "=== P16-P0: checking production bridges for manufactured identities ==="
for f in \
  "crates/onto-ironclaw-adapter/src/assured_bridge.rs" \
  "ironclaw/crates/ironclaw_reborn_composition/src/onto_assurance.rs"; do
  [ -f "$f" ] || continue
  check_no_pattern_outside_tests 'RunId::new()' "$f" "production must not manufacture RunId"
  check_no_pattern_outside_tests 'AttemptId::new()' "$f" "production must not manufacture AttemptId"
done

echo "=== P16-P0: checking for hardcoded exit/budget in onto_assurance.rs ==="
BRIDGE="ironclaw/crates/ironclaw_reborn_composition/src/onto_assurance.rs"
if [ -f "$BRIDGE" ]; then
  check_no_pattern_outside_tests 'exit_reason:.*ExitReason::FinishRequested' "$BRIDGE" "exit_reason must not be hardcoded"
  check_no_pattern_outside_tests 'budget_outcome:.*BudgetOutcome::WithinBudget' "$BRIDGE" "budget_outcome must not be hardcoded"
fi

echo "=== P16-P0: checking production composition for test adapters ==="
for f in \
  "ironclaw/crates/ironclaw_reborn_composition/src/runtime.rs" \
  "ironclaw/crates/ironclaw_reborn_composition/src/onto_assurance.rs"; do
  [ -f "$f" ] || continue
  if grep -n 'StubRunFinalizationPort\|with_in_memory_stores\|run_finalization_adapter' "$f" 2>/dev/null; then
    echo "FAIL: $f imports test-only adapter"
    FAIL=1
  fi
done

if [ $FAIL -eq 0 ]; then
  echo "P16-P0: all production gates PASS"
else
  echo "P16-P0: some gates FAILED"
fi
exit $FAIL
