#!/usr/bin/env bash
# P0: Bridge/adapter must not generate RunId or AttemptId.
set -euo pipefail
cd "$(dirname "$0")/.."
FAIL=0

echo "=== P0: bridges must not generate RunId / AttemptId ==="
BRIDGES=(
  "ironclaw/crates/ironclaw_reborn_composition/src/onto_assurance.rs"
  "crates/onto-ironclaw-adapter/src/assured_bridge.rs"
  "crates/onto-ironclaw-adapter/src/run_finalization_adapter.rs"
)
for f in "${BRIDGES[@]}"; do
  [ -f "$f" ] || continue
  in_test=0; line_num=0
  while IFS= read -r line; do
    line_num=$((line_num + 1))
    [[ "$line" == *"#[cfg(test)]"* ]] && in_test=1
    [[ "$line" == "}" && $in_test -eq 1 ]] && { in_test=0; continue; }
    if [[ $in_test -eq 0 ]] && echo "$line" | grep -qE '\b(RunId|AttemptId)::new\(\)'; then
      echo "FAIL:$f:$line_num: must not generate RunId/AttemptId — $line"
      FAIL=1
    fi
  done < "$f"
done

# Verify bridge uses parse()
BRIDGE="ironclaw/crates/ironclaw_reborn_composition/src/onto_assurance.rs"
if [ -f "$BRIDGE" ] && ! grep -q 'RunId::parse\|AttemptId::parse' "$BRIDGE"; then
  echo "FAIL:$BRIDGE: must use RunId::parse() for identity recovery"
  FAIL=1
fi

[ $FAIL -eq 0 ] && echo "P0 identity: PASS" || echo "P0 identity: FAILED"
exit $FAIL
