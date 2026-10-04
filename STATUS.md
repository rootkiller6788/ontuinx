# OntoOS — Runtime + Assurance Completion Status

**Date**: 2026-08-02  
**Branch**: `main`  
**Tags**: `p16-f3-closed`, `p16-f4-attempt-budget-closed`

---

## Overall Assessment

| Layer | Completion | Status |
|-------|-----------|--------|
| **OntoRuntime (L2)** — Agent Loop, Driver, Tools, Workspace | **85%** | Production-ready for single-workspace CLI |
| **OntoAssure (L1)** — Verification, Evidence, Decision, Transition | **80%** | Core loop closed; feedback delivery complete |
| **P16 Integration** — Runtime ↔ Assurance bridge | **75%** | Identity chain, feedback chain, soft budget done |
| **Experiment Infrastructure** — Supervisor, Grader, Budget | **60%** | Working but needs LLM-call-count limit (F4.1) |

---

## ✅ Completed

### P16 Core Loop
```
Agent Loop → Tool execution → Candidate → P16 verification
  → Verdict (Conformant/NonConformant/Inconclusive)
  → Decision (FinalizeCandidate/Continue/Escalate)
  → TransitionReceipt
  → Committed / Continuing
```
- Build/Test verifiers run on isolated workspace copy (cmake→make→ctest)
- Multi-step c-cmake pipeline registered at composition root
- Empty registry → BootstrapError (fail-closed)

### Identity Chain (F2)
- RunId/AttemptId preserved across IronClaw → Onto bridge
- `parse()` with fail-closed semantics (no ghost UUID generation)
- `attempt_ordinal` persisted in `TurnRunState` + `RunRecord`
- UUIDv5 deterministic AttemptId from (run_id, ordinal)
- ExitReason / Budget mapped from real LoopExit
- CI gate prevents `RunId::new()` / `AttemptId::new()` in bridge code

### Feedback Delivery (F3)
- `PendingContinuationFeedback` persisted in run state
- `target_attempt_id` binding prevents cross-attempt leakage
- `AgentEvidenceMode` (Structured / Generic) controls prompt projection
- Audit facts always preserved in Receipt regardless of projection mode
- Safety envelope: max 32 findings × 512 chars, control-char filtering
- Untrusted-content marking in system prompt

### Attempt Soft Budget (F4)
- `LoopExit::AttemptBudgetExhausted` variant
- P16 runs on budget-exhausted candidate (not SIGKILL)
- `ONTO_ATTEMPT_BUDGET_SECS` env var control
- Maps to `FinalizationExitReason::BudgetLimit`

### Experiment Infrastructure
- systemd-run cgroup isolation (MemoryMax=8G, KillMode=control-group)
- Circuit breaker: memory < 6GB or residual processes → abort
- Grader with structured JSON output + digest binding
- B2-A/B ceiling controls archived (agent autonomously fixes all bugs)
- B2-C transactional store benchmark (7 semantic defects, baseline 2/7)

### Tests
- onto-ironclaw-adapter: 185 tests pass
- 3 P16 fixture tests (correct project, build error, test failure)
- Identity conservation tests (bridge preserves identity, rejects invalid, deterministic)

---

## ⏸️ Remaining

### F4.1 — Authoritative Attempt Cutoff (P0)
**Current gap**: `tokio::time::timeout` wraps `invoke_driver` but agent loop can complete within budget without P16 intervention. Need:
- LLM call count limit per attempt (not just time)
- Step-boundary budget check before each LLM/tool call
- P16 must run after natural exit, not just after budget exhaustion
- Finalization reserved budget (agent cannot consume P16's time)

### F1F — VerificationContext from Manifest
**Current gap**: `changed_files: vec![]`, `language: "unknown"` hardcoded.
- Derive from candidate manifest diff vs checkpoint
- Language/profile from conformance config
- Needed for: ProtectedPath precision, incremental verification scope

### F1G — gVisor Enforcement
**Current gap**: `SandboxBackend::Direct` runs cmake/ctest on host.
- Production must use gVisor with read-only candidate mount
- No automatic fallback from gVisor to Direct

### Serve Multi-Workspace Binding
**Current gap**: `workspace_root` captured once at CLI startup.
- Serve mode: each run has independent workspace
- `RunRecord.workspace_binding` instead of executor-global `current_dir()`

### Checkpoint Protocol
**Current gap**: `checkpoint_ref: None` — IronClaw protocol doesn't carry it.
- Explicit `CheckpointBinding::NotProvidedByRuntime` (not ambiguous `None`)
- Incremental assurance after resume disabled until protocol supports it

### 515 `new()` → `generate()` Migration
- `new()` deprecated with warning; 515 call sites remain
- Needs mechanical batch replacement

### B2 Formal Experiments
- Structured/Generic paired experiments not yet run with valid P16 cycle
- B2-C needs LLM-call-count limit before experiments can begin

---

## Not Started

- **Multi-Pack**: Ops/Data/Workflow/Robotics/Industrial verifiers
- **PostgreSQL Production Stores**: Currently in-memory
- **Semantic Verifier**: LLM-based code review (stub)

---

### OntoLoop (L3) — ~65%

**What exists (code complete):**
- `onto_loop::decision::decide()` — verdict→decision mapping, **used in production** by `assured_bridge`
- `onto_loop::budget::LoopBudget` — attempt budget tracking with exhaustion detection
- `onto_loop::lifecycle` — full state machine (Continue/Committed/Escalated/Freeze)
- `onto_protocol::loop_protocol` — AttemptObservation, CandidateLoopDecision, AttemptDecision, NoCandidateLoopDecision types
- Progress decay detection (stall → Freeze after threshold)
- Cross-attempt finding fingerprint comparison
- `verdict_to_progress()` — ConformanceVerdict → ProgressSnapshot

**Not wired to production:**
- Attempt scheduler (relies on IronClaw's executor loop for Continue/retry)
- Checkpoint-based resume within attempts
- Standalone OntoLoop worker (no independent attempt orchestration outside P16)

### OntoFlow (L4) — ~70%

**What exists (code complete):**
- Go: `temporal/chasm/lib/ontoflow/` — 24 source files, **63 tests passing**
- Rust adapter: `crates/onto-temporal-adapter/` — **60 tests passing**
- Worker binary: `onto-worker`
- Protocol: `LoopInvocationRequest`, `LoopTerminalEnvelope`, `Heartbeat`, `ActivityTask`
- Types: Workflow, DAG, Batch, Barrier, Timer, Signal, Saga, Planning, Deliberation
- Authority projection: `resolve_loop_outcome()` (gRPC Rust→Go)
- Idempotency, lease, heartbeat, agent_task v1.0

**Not wired to production:**
- Not integrated into CLI/serve paths
- No end-to-end Go→Rust→P16 flow tested in production composition
- Worker not deployed in production

## Completion Percentages

```
OntoRuntime (L2)    █████████████████░░░  85%  (code 90%, integration 75%)
OntoAssure  (L1)    ████████████████░░░░  80%  (code 90%, integration 65%)
P16 Integration     ███████████████░░░░░  75%  (bridge code 85%, experiment 60%)
OntoFlow   (L4)     ██████████████░░░░░░  70%  (code 85%, integration 30%)
OntoLoop   (L3)     █████████████░░░░░░░  65%  (code 80%, integration 30%)

Overall (all 4 layers) ██████████████░░░░░░  75%
```
