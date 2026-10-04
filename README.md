# Onto Assurance Kernel (`ontoos`)

**Cross-industry trust-transaction protocol for AI Agent operations.**

Extracted from [ontocode-core](https://github.com/nearai/ontocode) and designed to
integrate into [OntoRuntime Agent OS](https://github.com/nearai/ironclaw).

## What It Does

```
Agent proposes action
  ↓
OntoRuntime: Authorization → Trust → Approval → Resources
  ↓
Onto Kernel: ExecutionContract + EffectClass classification
  ↓
OntoRuntime: Runtime Lane executes (staged or direct)
  ↓
Onto Kernel: SideEffectManifest capture
  ↓
Pack Verifier: Industry-specific verification (code, ops, data, ...)
  ↓
Onto Kernel: Evidence reduction → SessionDecision → SettlementDecision
  ↓
OntoRuntime: Publish / Discard / Freeze side effects
```

Onto is **not** an Agent runtime. It does not handle auth, sandboxing, secrets,
or scheduling. It is the **trust-transaction layer** that guarantees:

- Every action has a contract with acceptance criteria
- Every result has cryptographically-linked evidence
- Every decision is deterministically reproducible
- Every side effect is gated by a settlement decision

## Crate Map

| Crate | Purpose | Deps |
|-------|---------|------|
| `onto-assurance-types` | Enums, IDs, contracts, evidence, decision models | serde, uuid, chrono |
| `onto-assurance-core` | Deterministic pure functions: hash, reduction, decision, replay | types only |
| `onto-assurance-runtime` | Port traits + TransactionCoordinator | core + port traits |
| `onto-ironclaw-adapter` | ShadowObserver, BuiltinGate, adapter stubs | runtime + OntoRuntime |
| `onto-pack-sdk` | PackVerifier trait + report types | types |
| `onto-code-pack` | Code verifiers: build, test, lint, profiler, artifacts | pack-sdk |

## Quick Start

```bash
cargo test --workspace
```

Expected: **98 tests, 0 failures.**

### Classify a capability

```rust
use onto_assurance_core::effect_classifier::{RulesBasedClassifier, EffectClassifier};
use onto_assurance_types::enums::RiskLevel;

let c = RulesBasedClassifier::new();
let result = c.classify("file_write", "", None, RiskLevel::Medium);
assert_eq!(result.effect_class, EffectClass::Staged);
```

### Build an evidence chain

```rust
use onto_assurance_core::evidence_chain::EvidenceChain;
use onto_assurance_types::evidence::{EvidenceRecord, VerifierBinding};

let mut chain = EvidenceChain::new(run_id, "genesis".into(), verifier);
chain.append(evidence_record)?;
let bundle = chain.seal();
EvidenceChain::verify(&bundle)?;
```

### Run the full pipeline

```rust
use onto_assurance_core::{reduction, session_decision, settlement};

let verdict = reduction::reduce(&contract.criteria, &evidence);
let session = session_decision::decide_session(run_id, attempt_id,
    ExitReason::FinishRequested, &verdict, BudgetOutcome::WithinBudget, bundle_id);
let settlement = settlement::derive_settlement(
    EffectClass::Staged, session.task_outcome, &verdict);
```

## Migration Status

| Phase | Description | Status |
|-------|-------------|--------|
| M0 | Freeze Python reference | ✅ `ontocode-python-assurance-reference-v1` |
| M1 | Schemas + golden fixtures | ✅ 10 scenarios |
| M2 | Rust pure-function kernel | ✅ canonical hash, evidence chain, reduction, decisions |
| M2.5 | Effect semantics | ✅ EffectClassifier, PreExecutionDecision, SettlementDecision |
| M3 | Runtime coordinator | ✅ Port traits (8), TransactionCoordinator, mock ports |
| M4 | OntoRuntime shadow integration | 🔲 Requires OntoRuntime hook system |
| M5 | Take over success determination | 🔲 Requires OntoRuntime loop_exit seam |
| M6 | Take over side-effect commit | 🔲 Requires EffectClass × Runtime Lane proof |
| M7 | Code pack migration | ✅ Profiler, build/test/lint verifiers, artifact collector |

## Design Invariants

- `onto-assurance-core` has **zero** dependencies on OntoRuntime, Python, Docker, or PostgreSQL
- `EffectClass` can only be **upgraded** (more restrictive), never downgraded by Agent
- `TaskOutcome` × `BudgetOutcome` × `LifecycleState` are **independent** dimensions
- `PreExecutionDecision` and `SettlementDecision` are **split** — irreversible actions are gated before execution
- Onto's Builtin hook can only **tighten** OntoRuntime decisions, never relax them
- Evidence chain uses canonical JSON with domain-separated SHA-256 hashes

## License

MIT OR Apache-2.0
