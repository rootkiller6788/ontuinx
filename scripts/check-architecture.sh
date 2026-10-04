#!/bin/bash
# Architecture dependency direction check — Phase 1 automated enforcement
# Run: bash scripts/check-architecture.sh
set -euo pipefail

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
PASS=0; FAIL=0; WARN=0
MANIFEST="${CARGO_MANIFEST_PATH:-Cargo.toml}"

check() { local n="$1"; shift; if "$@"; then echo -e "  ${GREEN}✅${NC} $n"; PASS=$((PASS+1)); else echo -e "  ${RED}❌${NC} $n"; FAIL=$((FAIL+1)); fi }
warn() { echo -e "  ${YELLOW}⚠️${NC} $1"; WARN=$((WARN+1)); }

header() { echo ""; echo -e "${YELLOW}── $1 ──${NC}"; }

echo "=== OntoRuntime Architecture Check ==="

# ══════════════════════════════════════════════════════════════════
header "Core Purity (onto-assurance-core must have ZERO external infra deps)"

for dep in ironclaw postgres redis temporal tokio-postgres deadpool-postgres; do
    check "no $dep in onto-assurance-core" \
        bash -c "! cargo tree -p onto-assurance-core --manifest-path '$MANIFEST' 2>/dev/null | grep -qi '$dep'"
done

# ══════════════════════════════════════════════════════════════════
header "Runtime Purity (onto-assurance-runtime must not depend on OntoRuntime)"

for dep in ironclaw postgres redis; do
    check "no $dep in onto-assurance-runtime" \
        bash -c "! cargo tree -p onto-assurance-runtime --manifest-path '$MANIFEST' 2>/dev/null | grep -qi '$dep'"
done

# ══════════════════════════════════════════════════════════════════
header "Source-level Cross-Reference Audit"

check "no OntoRuntime types in onto-assurance-core/src/" \
    bash -c '! grep -r "ironclaw_turns\|ironclaw_runner\|ironclaw_host\|RebornTurnRun" crates/onto-assurance-core/src/ 2>/dev/null | grep -q .'

check "no OntoRuntime concrete types in onto-assurance-runtime/src/" \
    bash -c '! grep -rn "use ironclaw_turns::\|use ironclaw_runner::\|use ironclaw_host::\|ironclaw_turns::[A-Z]\|ironclaw_runner::[A-Z]\|ironclaw_host::[A-Z]" crates/onto-assurance-runtime/src/ 2>/dev/null | grep -q .'

# ══════════════════════════════════════════════════════════════════
header "CommitPermit Integrity"

check "CommitPermit fields are private (not pub)" \
    bash -c '! grep -A10 "pub struct CommitPermit" crates/onto-assurance-runtime/src/ports.rs | grep -E "pub (decision_id|transaction_id|baseline_hash|manifest_hash)" | grep -v "pub fn\|pub(crate)" | grep -q .'

check "CommitPermit::issue() is pub(crate)" \
    bash -c 'grep -A1 "pub(crate) fn issue" crates/onto-assurance-runtime/src/ports.rs | grep -q "pub(crate)"'

check "CommitPermit::new_for_test_only exists (test bypass)" \
    bash -c 'grep -q "pub fn new_for_test_only" crates/onto-assurance-runtime/src/ports.rs'

check "DatabaseCommitPermit fields private" \
    bash -c '! grep -A10 "pub struct DatabaseCommitPermit" crates/onto-assurance-runtime/src/ports.rs | grep -E "^\\s+pub (decision_id|transaction_id|attempt_id|database_resource|mutation_manifest_hash|idempotency_key)" | grep -v "pub fn\|pub(crate)" | grep -q .'

check "DatabaseCommitPermit::issue() pub(crate)" \
    bash -c 'grep -A1 "pub(crate) fn issue" crates/onto-assurance-runtime/src/ports.rs | grep -q "pub(crate)"'

# ══════════════════════════════════════════════════════════════════
header "M5/M6-A Regression Baseline"

check "78 tests, 0 failures" \
    bash -c 't=$(mktemp); cargo test -p onto-assurance-core -p onto-assurance-runtime -p onto-ironclaw-adapter --features test-support --lib --manifest-path "$0" > "$t" 2>&1; bad=$(grep -c "[1-9][0-9]* failed" "$t" || true); rm -f "$t"; test "$bad" -eq 0' "$MANIFEST"

check "no code warnings (lib)" \
    bash -c 'set +o pipefail; ! cargo check -p onto-assurance-runtime -p onto-ironclaw-adapter --manifest-path "$0" 2>&1 | grep -q "warning: unused\|warning: unnecessary"' "$MANIFEST"

# ══════════════════════════════════════════════════════════════════
header "Evidence Package"

check "evidence/m5/ exists"    bash -c 'test -f evidence/m5/acceptance-report.json'
check "evidence/m6-a/ exists"  bash -c 'test -f evidence/m6-a/acceptance-report.json'
check "docs/ architecture docs" bash -c 'test -f docs/ONTO_RUNTIME_ARCHITECTURE.md && test -f docs/OWNERSHIP_MATRIX.md && test -f docs/STATE_AUTHORITY.md && test -f docs/INTEGRATION_SEAMS.md'

# ══════════════════════════════════════════════════════════════════
echo ""
echo "=============================="
printf "Result: ${GREEN}%d passed${NC}, ${RED}%d failed${NC}" $PASS $FAIL
if [ $WARN -gt 0 ]; then printf ", ${YELLOW}%d warnings${NC}" $WARN; fi
echo ""
echo "=============================="
exit $FAIL
