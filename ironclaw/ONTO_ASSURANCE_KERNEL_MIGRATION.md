# Onto Assurance Kernel — Migration Plan

> **Approval status:**
> - M0–M4: **APPROVED** — executable immediately, no IronClaw changes needed
> - M5: **CONDITIONALLY APPROVED** — can proceed after M2.5, but only for success
>   determination, not side-effect gating
> - M6: **BLOCKED BY EFFECT SEMANTICS AND RUNTIME-LANE PROOF** — must complete
>   M2.5 (T1–T7 per-lane timeline + EffectClass protocol) before real commit authority
> - M7: **APPROVED AFTER KERNEL STABILIZATION**
>
> **Strategic direction**: 95% | **Module boundary**: 90% | **Rust split**: 90%
> **Integration-point accuracy**: 80% | **Transaction-safety closure**: 65%
>
> **Core thesis**: Extract the universal trust-transaction protocol from ontocode-core,
> discard everything IronClaw already provides, rebuild as a minimal deterministic Rust
> kernel, shadow-integrate into IronClaw, then sequentially take over success-decision
> and side-effect commit authority.

---

## 1. Target Architecture

```
IronClaw Agent OS                              Onto Assurance Kernel
══════════════════                              ══════════════════════

Identity / Tenant / Project              ┌─────────────────────────────┐
Agent Loop / LLM / Skill                 │ ExecutionContract           │
Capability Registry                      │ AcceptanceCriterion         │
Authorization / Trust / Approval         │ ExecutionTransaction        │
Secret / Network / Filesystem            │ SideEffectManifest          │
Resource Quota                           │ VerifierBinding             │
WASM / MCP / Docker Runtime              │ VerificationRun             │
EventStore / Projection / Memory         │ EvidenceBundle              │
RunState / Thread / Conversation         │ DeterministicReduction      │
                                         │ CommitDecision              │
        │                                │ CheckpointBinding           │
        │  CapabilityHost boundary       │ Replay                      │
        │                                │ FinalizationGateway         │
        ▼                                └──────────────┬──────────────┘
┌───────────────────┐                                   │
│ Onto Assurance    │◄──────────────────────────────────┘
│ Kernel            │
└───────────────────┘
```

**Critical call chain (branched by EffectClass):**

```
Agent proposes CapabilityRequest
        │
        ▼
Onto Kernel:  Construct ExecutionIntent + EffectClass classification
        │
        ▼
IronClaw:     Authorization → Trust → Approval → Resource
        │              (informed by EffectClass + Contract)
        ▼
Onto Kernel:  Create ExecutionTransaction (PREPARED)
        │
        ▼
        ├── Pure / ReadOnly
        │     → Execute directly
        │     → Capture results
        │     → Verify / Audit
        │
        ├── Staged
        │     → IronClaw creates staged environment
        │     → Execute in staging
        │     → Onto captures SideEffectManifest
        │     → Pack Verifier validates
        │     → Onto Evidence Reduction
        │     → PreExecutionDecision::Allow already granted
        │     → SettlementDecision → COMMIT (publish) / ROLLBACK (discard)
        │
        ├── Transactional
        │     → Begin real transaction
        │     → Execute
        │     → Onto captures SideEffectManifest
        │     → Pack Verifier validates
        │     → Onto Evidence Reduction
        │     → SettlementDecision → COMMIT / ROLLBACK
        │
        ├── Compensatable
        │     → PreExecutionDecision (Allow / RequireApproval)
        │     → Execute (real external effect)
        │     → Onto captures SideEffectManifest + compensation plan
        │     → Verify
        │     → SettlementDecision → Confirm / Compensate / Escalate
        │
        └── Irreversible
              → Strong pre-verification (all checkable criteria)
              → PreExecutionDecision (Allow only if all pre-checks pass)
              → Strong Approval
              → Exact Invocation Lease
              → Execute ONCE
              → Onto captures SideEffectManifest (post-hoc audit)
              → SettlementDecision → Confirm / Audit / Escalate
```

**Onto Kernel is responsible for the _transaction protocol and verdict_, not for
re-implementing Docker, WASM, MCP, authorization, or secrets.**

**Key principle: for Irreversible effects, the decision to proceed is made
BEFORE the real side effect occurs, not after.**

---

## 2. Boundary: What Goes Where

### 2.1 Enters `onto-assurance-kernel` (Rust)

These are cross-industry, deterministic, and have zero dependency on Python or
any specific coding language:

| Category | Types |
|----------|-------|
| **Base protocol** | `TaskOutcome`, `BudgetOutcome`, `LifecycleState`, `ExitReason`, `FailureKind`, `RetryOwner`, `ReasonCode`, typed IDs |
| **Contract** | `ExecutionIntent`, `ExecutionContract`, `Requirement`, `AcceptanceCriterion`, `Obligation`, `RiskLevel`, `EffectClass` |
| **Decision** | `PreExecutionDecision` (Allow/Deny/RequireApproval/RequireRestriction), `SettlementDecision` (Commit/Rollback/Confirm/Compensate/Freeze/Escalate) |
| **Transaction** | `ExecutionTransaction`, `TransactionState`, `SideEffectManifest`, `ArtifactManifest`, `CheckpointBinding` |
| **Classification** | `EffectClassifier` trait (trusted, fail-closed, Agent cannot downgrade) |
| **Verification** | `VerifierBinding`, `VerificationRun`, `EvidenceRecord`, `EvidenceBundle`, `CriterionVerdict`, `RequirementVerdict`, `EvidenceInvalidation` |
| **Deterministic decision** | `SessionDecisionService`, `AttemptDecisionService`, `Reduction`, `FinalizationGateway`, `RecoveryDecision` |
| **Integrity** | ContentHash, EnvelopeHash, ContextHash (three distinct hash domains), replay, provenance, integrity validation |

### 2.2 Stays in IronClaw (not reimplemented)

| ontocode-core old module | IronClaw owner |
|--------------------------|----------------|
| `PermissionGate` | `ironclaw_authorization` |
| Standalone `PolicyEngine` | `ironclaw_authorization` + `ironclaw_trust` |
| `SandboxManager` | `ironclaw_process_sandbox` + `ironclaw_wasm` |
| Secret control | `ironclaw_secrets` |
| Network control | `ironclaw_network` |
| Filesystem policy | `ironclaw_filesystem` |
| Standalone `EventLog` | `ironclaw_events` + `ironclaw_reborn_event_store` |
| `Agent LoopController` | `ironclaw_agent_loop` + `ironclaw_runner` |
| Global task scheduler | `ironclaw_product` (mission/routine orchestration) |
| User / session / project | `ironclaw_product` layer |

Onto Kernel only keeps narrow adapter interfaces:

```
Onto Kernel
  → requests IronClaw authorization
  → requests IronClaw staged execution
  → requests IronClaw persistence
  → requests IronClaw publish or discard side effects
```

### 2.3 Retained as Industry Pack (not in universal kernel)

These are software-development-specific and live in `packs/code/`:

```
ProjectProfiler
Language identification
gcc / cargo / pytest / npm / mvn command parsing
Code compile verification
Code static analysis
Git Diff
PR / Merge
Code scope checking
Source file artifact collection
```

Future pack layout:

```
packs/
├── code/
├── ops/
├── data/
├── workflow/
├── robotics/
└── industrial/
```

---

## 3. Rust Crate Structure

Six crates with strict acyclic dependency:

```
ontoos/
├── Cargo.toml                          (workspace root)
│
├── crates/
│   ├── onto-assurance-types/           ── zero deps beyond serde, sha2, uuid
│   │   ├── IDs
│   │   ├── enums
│   │   ├── contracts
│   │   ├── evidence models
│   │   └── decision models
│   │
│   ├── onto-assurance-core/            ── depends on: types only
│   │   ├── reduction                  (deterministic, pure)
│   │   ├── invalidation               (deterministic, pure)
│   │   ├── decision                   (deterministic, pure)
│   │   ├── canonical hash             (locked across languages)
│   │   ├── state transition            (3 orthogonal FSMs)
│   │   └── replay calculation          (deterministic, pure)
│   │
│   ├── onto-assurance-runtime/         ── depends on: core + port traits
│   │   ├── transaction coordinator
│   │   ├── finalization gateway
│   │   ├── recovery
│   │   └── ports (traits only, no impls)
│   │
│   ├── onto-ironclaw-adapter/          ── depends on: runtime + ironclaw_*
│   │   ├── authorization adapter       (→ ironclaw_authorization)
│   │   ├── approval adapter            (→ ironclaw_approvals)
│   │   ├── runtime lane adapter        (→ ironclaw_host_runtime)
│   │   ├── event store adapter         (→ ironclaw_events)
│   │   └── persistence adapter         (→ ironclaw_run_state)
│   │
│   ├── onto-pack-sdk/                  ── depends on: types
│   │   ├── verifier traits
│   │   ├── requirement schemas
│   │   └── artifact adapters
│   │
│   └── onto-code-pack/                 ── depends on: pack-sdk
│       ├── project profiler
│       ├── build verifiers
│       ├── test verifiers
│       └── git/artifact adapters
│
├── schemas/v1/                         (language-agnostic JSON Schema)
├── fixtures/                           (golden input/output pairs)
├── conformance/                        (cross-language differential tests)
└── python-reference/                   (frozen reference oracle)
```

**Forbidden dependencies:**

```
onto-assurance-core → IronClaw       ❌
onto-assurance-core → Python         ❌
onto-assurance-core → Docker         ❌
onto-assurance-core → PostgreSQL     ❌
onto-assurance-core → any Verifier   ❌
```

`onto-assurance-core` must remain a pure deterministic library.

---

## 4. State Machines (Converged to Three)

Do not port the current 10+21-state dual-level FSM. Converge to three orthogonal
state machines. `FailureKind`, `RetryOwner`, `BudgetOutcome`, and `Verdict` are
**data**, not separate state machines.

The three orthogonal outcome dimensions are:

```rust
pub enum TaskOutcome {       // DID the task succeed?
    Success,
    Incomplete,
    Failed,
    EnvironmentError,
}

pub enum BudgetOutcome {     // DID we have enough resources?
    WithinBudget,
    Depleted,
    HardLimitReached,
}

pub enum LifecycleState {    // WHAT action was taken?
    Running,
    Finalizing,
    Committed,
    Continuing,
    RolledBack,
    Escalated,
}
```

These replace the error-prone single `SessionDecision` enum that previously
conflated `EXHAUSTED` (budget) with `CRASHED` (environment) with `SUCCESS`
(task result). The `FinalizationGateway` receives an `ExitReason`
(`FinishRequested | ProviderStop | BudgetLimit | Stuck | Cancelled | Crashed`)
but outputs the three independent dimensions. Example:

```
ExitReason = BudgetLimit
TaskOutcome = Success       ← task actually completed
BudgetOutcome = Depleted    ← but we ran out of budget
LifecycleState = Committed  ← evidence is valid, publish effects
```

### 4.1 RunLifecycle

Describes where the overall task is:

```
CREATED → RUNNING → FINALIZING → COMMITTED
                                 CONTINUING
                                 ROLLED_BACK
                                 ESCALATED
```

Terminal states are immutable once entered.

**Correlated with** (not mapped to) IronClaw `ironclaw_run_state::RunStatus`
via `run_id`. Onto state is the trust-transaction authority; IronClaw state
is the host runtime record. They are linked by ID, not semantically equivalent.

### 4.2 ExecutionTransaction

Describes one side-effect-bearing transaction:

```
PREPARED → STAGED → EXECUTED → VERIFIED → DECIDED ─┬─ PUBLISHING → PUBLISHED
                                                   ├─ DISCARDING → DISCARDED
                                                   ├─ COMPENSATING → COMPENSATED
                                                   │                  COMPENSATION_FAILED
                                                   └─ FREEZING   → FROZEN
→ FINALIZED
```

SettlementDecision mapping:

```
Commit      → PUBLISHED
Rollback    → DISCARDED
Compensate  → COMPENSATED (or COMPENSATION_FAILED)
Freeze      → FROZEN
Escalate    → FROZEN (with escalation record)
```

**Compensation is not the same as Discard.** Creating a cloud resource and
later deleting it does not erase the fact that the resource existed. Evidence
must retain: original side effect, compensation request, compensation result,
residual side effects, and eventual consistency status.

**Critical crash windows:**

| Window | Risk | Recovery |
|--------|------|----------|
| PUBLISHING → PUBLISHED | Side effect published, state not persisted | Idempotency key + publish receipt + outbox reconciliation |
| COMPENSATING → COMPENSATED | Compensation issued, result unknown | Compensation idempotency key + re-query external state |
| FREEZING → FROZEN | Snapshot taken but not confirmed | Freeze receipt + retry freeze |

**Correlated with** (not mapped to) IronClaw `CapabilityLeaseStatus` via
`invocation_id` + `lease_id`. `Lease::Consumed` indicates the authorization
credential was used; it does not imply `Effect::Published`, `Evidence::Valid`,
or `Task::Successful`. These are distinct facts in different state machines.

### 4.3 ApprovalContinuation

Describes human-in-the-loop approval:

```
NOT_REQUIRED → WAITING → GRANTED
                         REJECTED
                         EXPIRED
```

**Invariant:** `APPROVAL_REQUIRED` must never proceed to execution.

Must: persist Continuation → release runtime resources → wait for approval →
acquire exact-invocation lease → resume the same transaction.

**Correlated with** IronClaw `ironclaw_approvals::ApprovalStatus`
(Pending / Approved / Denied / Expired / Discarded) via `approval_request_id`.

---

## 5. EffectClass + Dual Decision Model

Not all capabilities can be modeled as `execute → verify → commit`.
Some side effects are irreversible at execution time. A single
`CommitDecision` that means both "allow execution" and "publish effects"
creates semantic conflict on irreversible actions. Split into two decisions:

```rust
/// Produced BEFORE real external side effects.
pub enum PreExecutionDecision {
    Allow,
    Deny,
    RequireApproval,
    RequireRestriction,   // e.g., reduce network access, mount read-only
}

/// Produced AFTER execution or staging, determines how to settle.
pub enum SettlementDecision {
    Commit,               // publish staged effects
    Rollback,             // discard staged effects
    Confirm,              // acknowledge already-executed effect (audit trail)
    Compensate,           // run compensating action
    Freeze,               // snapshot and hold for human decision
    Escalate,             // freeze + notify human operator
}
```

### 5.1 EffectClass Classification

```rust
pub enum EffectClass {
    /// No side effects. Execute freely.
    Pure,

    /// Read-only observation. Execute freely, verify results.
    ReadOnly,

    /// Side effects confined to a staged environment.
    /// Execute → verify → SettlementDecision::Commit OR Rollback.
    Staged,

    /// Real transaction support available (e.g., SQL BEGIN/COMMIT/ROLLBACK).
    /// Begin → execute → verify → SettlementDecision::Commit OR Rollback.
    Transactional,

    /// Irreversible but with a compensating action.
    /// PreExecutionDecision → Execute → verify → SettlementDecision::Confirm OR Compensate.
    Compensatable,

    /// Irreversible external effect. Cannot be undone.
    /// Full pre-verification → PreExecutionDecision::Allow → Strong Approval
    /// → Exact Invocation Lease → Execute once → SettlementDecision::Confirm (post-audit).
    Irreversible,
}
```

### 5.2 Protocol Per EffectClass

| Class | Pre-Execution | Post-Execution Settlement |
|-------|--------------|--------------------------|
| Pure / ReadOnly | PreExecutionDecision::Allow (standard auth) | Result audit only |
| Staged | PreExecutionDecision::Allow | Commit / Rollback |
| Transactional | PreExecutionDecision::Allow | Commit / Rollback |
| Compensatable | Allow / RequireApproval | Confirm / Compensate / Escalate |
| Irreversible | Strong pre-verification → Allow / Deny | Confirm / Escalate (post-hoc audit) |

For Irreversible, do NOT say "produce CommitDecision before execution."
The correct statement: **PreExecutionDecision::Allow is produced before
execution; SettlementDecision::Confirm is produced after execution as
an audit record.** "Commit" implies publishable staged work — it is the
wrong term for irreversible external effects.

### 5.3 EffectClass × Runtime Lane Matrix

```
                    Pure   ReadOnly  Staged  Txnal  Compensatable  Irreversible
WASM sandbox         X       X         X
MCP tool             X       X                  X           X            X
Process (Docker)     X       X         X       X           X            X
HTTP / API           X       X                           X            X
Filesystem (worksp)  X       X         X
Filesystem (external)        X                  X           X            X
Database (SQL)       X       X         X       X
Email / Payment                                      X            X
Cloud resource                                       X            X
Robot / Actuator                                                 X            X
```

### 5.4 EffectClass MUST Come From Trusted Definitions (Not Agent/LLM)

**Invariant: Agent describes Intent; it does NOT declare EffectClass.**

A malicious or confused Agent could label `DROP DATABASE` as `ReadOnly`.
The classification source priority is:

```
1. CapabilityDescriptor built-in declaration   (shipped with the extension)
2. Industry Pack trusted definition            (packs/code, packs/ops, ...)
3. Enterprise Policy override                  (only UPGRADE risk, never downgrade)
4. Argument-sensitive analysis                 (e.g., SQL parser detects DROP → Irreversible)
5. Unknown → Irreversible or Deny              (fail-closed default)
```

**Invariant: EffectClass can only be UPGRADED (more restrictive), never
automatically DOWNGRADED.**

```rust
/// Trusted classifier — runs at the kernel boundary, not in the Agent loop.
pub trait EffectClassifier: Send + Sync {
    fn classify(
        &self,
        descriptor: &CapabilityDescriptor,
        arguments: &CanonicalArguments,
        policy: &EffectivePolicy,
    ) -> Result<EffectClassification, ClassificationError>;
}

/// The output of classification.  Agent cannot override these fields.
pub struct EffectClassification {
    pub effect_class: EffectClass,
    pub rationale: ReasonCode,
    pub upgraded_from: Option<EffectClass>,   // if policy escalated
    pub requires_approval: bool,
    pub pre_verification_required: bool,       // Irreversible → must be true
}
```

Unknown classification must fail-closed (Irreversible or Deny).

### 5.5 PreExecutionDecision Gate Timing

For each `EffectClass`, the PreExecutionDecision must be produced **before the
first real external side effect**:

| Class | Gate Placement |
|-------|---------------|
| Pure / ReadOnly | Standard IronClaw authorization; no additional gate |
| Staged | Authorization → PreExecutionDecision::Allow → execute in staging |
| Transactional | Authorization → PreExecutionDecision::Allow → begin txn → execute |
| Compensatable | PreExecutionDecision (Allow / RequireApproval) → execute |
| Irreversible | **PreExecutionDecision must be Allow BEFORE execution.** Full pre-verification, strong approval, exact lease, then execute ONCE. Post-execution is audit-only. |

General rule: **For Irreversible, the decision to proceed is made before the
real side effect occurs.** Do not execute first and then decide.

---

## 6. Integration Points (Exact IronClaw Traits)

### 6.1 Hook 1: Capability Request → Construct ExecutionIntent (BEFORE Authorization)

Authorization needs to know _what_ is being attempted and its risk profile.
Construct the Intent before IronClaw gates evaluate it:

```
Capability Request
  → Onto constructs ExecutionIntent (goal, EffectClass, resource scope, risk)
  → Onto EffectClassifier determines EffectClass from CapabilityDescriptor + arguments
  → ExecutionContract pre-bound to Intent
  → IronClaw Authorization / Trust / Approval (informed by EffectClass + Contract)
  → AuthorizationReceipt
  → Onto creates ExecutionTransaction
```

**Onto action:** Intercept the capability request, construct `ExecutionIntent`,
run `EffectClassifier`, and attach the classification to the authorization
context so IronClaw gates can consume it. Without this, the authorization
system sees only a raw tool call without task-level semantics.

### 6.2 Hook 2: After Authorization, Before Execution → Create ExecutionTransaction

**IronClaw seam:**
```rust
// ironclaw_capabilities::CapabilityObligationHandler
pub trait CapabilityObligationHandler {
    async fn prepare(&self, req: CapabilityObligationRequest)
        -> Result<CapabilityObligationOutcome, CapabilityObligationError>;
    async fn complete(&self, req: CapabilityObligationCompletionRequest)
        -> Result<..., ...>;
    async fn abort(&self, req: CapabilityObligationAbortRequest)
        -> Result<..., ...>;
}
```

**Onto action:** Wrap `prepare()` to create `ExecutionTransaction(STAGED)`.
Gate `complete()` on `SettlementDecision::Commit` (for Staged/Transactional)
or `SettlementDecision::Confirm` (for Compensatable/Irreversible).

### 6.3 Hook 3: After Runtime Returns → Capture SideEffectManifest

**IronClaw seam:**
```rust
// ironclaw_host_runtime::HostRuntime
pub trait HostRuntime: Send + Sync {
    async fn invoke_capability(&self, request: RuntimeInvocation)
        -> Result<RuntimeCapabilityOutcome, HostRuntimeError>;
}
// Outcome: RuntimeCapabilityOutcome::Completed { output, display_preview, usage }
```

**Onto action:** Construct `SideEffectManifest` from the completed outcome +
filesystem diff + network receipts.

### 6.4 Hook 4: On Mission/Loop Exit → FinalizationGateway

**IronClaw seam (needs new hook point):**
```rust
// Proposed addition to ironclaw_hooks::points
pub struct AfterLoopExitContext {
    pub run_id: RunId,
    pub exit_reason: ExitReason,       // finish, stop, budget, stuck, cancelled, crash
    pub session_summary: SessionSummary,
}
```

**Onto action:** Run `FinalizationGateway`.
Input: `ExitReason` (FinishRequested | ProviderStop | BudgetLimit | Stuck | Cancelled | Crashed).
Output: three orthogonal dimensions — `TaskOutcome` × `BudgetOutcome` × `LifecycleState`.
`FinalizationGateway` does NOT produce a flat `SessionDecision` enum that conflates
budget exhaustion with task failure or crashes with incomplete results.

### 6.5 Hook 5: Before Publishing Side Effects → CommitDecision Gate

**IronClaw seam:**
```rust
// ironclaw_authorization::CapabilityLeaseStorePort
pub trait CapabilityLeaseStorePort: Send + Sync {
    async fn consume(&self, scope: ResourceScope, lease_id: LeaseId)
        -> Result<CapabilityLease, CapabilityLeaseError>;
}
```

**⚠️ Verification required:** Confirm that `consume()` semantics mean
"publish side effects to the real world" and not just "mark the lease record
as consumed." If the side effect already occurred during `invoke_capability()`
and `consume()` is only a bookkeeping step, then gating on `consume()` does
NOT actually control the side effect. This must be verified per runtime lane.

**For each lane, trace the exact timeline:**
```
T1: Authorization
T2: Lease claim
T3: Runtime start
T4: First external side effect ← CommitDecision MUST be before this
T5: Runtime return
T6: Lease consume
T7: Event persistence
```

---

## 7. Event Strategy

### 7.1 First-Class Event Types (post-shadow-mode)

Do not bury evidence in `ObserverFact` metadata long-term. Add:

```
AssuranceTransactionPrepared
SideEffectManifestRecorded
VerificationRunCompleted
EvidenceBundleSealed
SessionDecisionProduced
AttemptDecisionProduced
EffectPublishRequested
EffectPublished
EffectDiscarded
EffectFrozen
ReplayCompleted
```

### 7.2 Storage Split

| Store | Content |
|-------|---------|
| **EventStore** | ID, hash, URI/ref, summary, version, causation/correlation IDs |
| **EvidenceStore** (separate) | Full EvidenceBundle, stdout/stderr, artifacts, replay inputs |

Keeps EventStore from bloating on large verification payloads.

---

## 8. Trust Tier: Builtin Bridge (Thin TCB) + Monotonic Tightening

Onto must register as `Builtin` hook tier (not `Installed`) because it needs
authority to block capability dispatch.

But only a **thin bridge** enters the IronClaw TCB:

```
Within TCB (onto-ironclaw-adapter):
  ├── Verify decision signature
  ├── Check transaction state
  ├── Check SettlementDecision
  ├── Emit gate result (Allow/Deny/RequireApproval)
  └── Write immutable events

Outside TCB (runs in separate process/WASM if needed):
  ├── Industry verifiers
  ├── LLM requirement interpreter
  ├── Report generation
  ├── Code Pack
  ├── Semantic scorer
  └── External data queries
```

Ideal structure:

```
Untrusted Verifier Worker
        ↓ produces facts
Onto Pure Reduction Core     ← deterministic, can be formally verified
        ↓ produces signed decision
Builtin Bridge               ← thin, auditable, enforces the decision
```

### 8.1 Onto Can Only Tighten, Never Relax IronClaw Decisions

Even though IronClaw allows Builtin hooks to return `Allow`, Onto must not
override an IronClaw `Deny` into an `Allow`. The final authorization is a
monotonic intersection:

```
FinalAllow = IronClawAllow ∧ OntoContractAllow ∧ OntoEffectSafetyAllow
```

Rules:

```
IronClaw DENY             → Onto MUST NOT change to ALLOW
IronClaw REQUIRE_APPROVAL → Onto MUST NOT bypass approval
IronClaw ALLOW            → Onto MAY change to DENY
IronClaw ALLOW            → Onto MAY change to REQUIRE_APPROVAL
IronClaw ALLOW            → Onto MAY change to RESTRICT (scope down)
```

This must be enforced via property tests and architecture-boundary tests
in `onto-ironclaw-adapter`.

---

## 9. Cross-Language Protocol (Freeze Before Rust)

Before any Rust code is written for M2, freeze these schemas as
language-agnostic JSON Schema:

```
ExecutionIntent
ExecutionContract
CapabilityRequestRef
AuthorizationReceipt
ExecutionTransaction
SideEffectManifest
VerificationRun
EvidenceRecord
EvidenceBundle
SessionDecision
AttemptDecision
CheckpointBinding
ReplayInput
ReplayResult
```

Every object must carry:

```
schema_version
kernel_version
run_id
attempt_id
transaction_id
correlation_id
causation_id
created_at
context_hash
```

### Canonical Hash Rules (Three Distinct Hash Domains)

A single `context_hash` that covers both content and envelope creates
circular dependency risks and non-determinism. Split into three:

#### ContentHash

Hashes only semantic content — what was decided, not when or by whom:

```
Requirement fields
Criterion fields
Evidence payload (inputs, outputs, artifacts)
Decision inputs
```

Excludes: `created_at`, database IDs, storage URIs, processing node,
`context_hash` itself (avoids cycles).

#### EnvelopeHash

Binds the event shell for tamper-evident storage:

```
content_hash
run_id
transaction_id
causation_id
correlation_id
created_at
schema_version
kernel_version
hash_algorithm
```

#### ContextHash

Binds the verification context:

```
Artifact tree (ContentHash of each)
Requirement set (ContentHash of each)
Verifier version + toolchain
Environment fingerprint
Policy version
Attempt number
Checkpoint reference
```

Each hash uses a domain separator to prevent cross-domain collision:

```
ONTO:CONTENT:EVIDENCE:V1
ONTO:ENVELOPE:EVIDENCE:V1
ONTO:CONTEXT:DECISION:V1
ONTO:CONTENT:CHECKPOINT:V1
...
```

General hash rules:

```
Explicit field order
UTF-8 encoding
Numeric canonical representation (no trailing zeros, no `1.0` vs `1`)
UTC timestamps only (ISO 8601, no timezone abbreviations)
NaN / Infinity rejected at serialization boundary
Map keys sorted lexicographically
Non-semantic fields excluded
domain_separator + schema_version included in hash input
hash_algorithm versioned (allow future migration to SHA-384 etc.)
```

Lock this down with fixed fixtures: Python and Rust must produce the **exact
same hash bytes** for the same input, across all three hash domains.

---

## 10. Migration Phases

### M0: Freeze Python Reference

```
Tag: ontocode-python-assurance-reference-v1

Freeze a reference_manifest.json containing:
  {
    "git_commit": "<sha>",
    "test_inventory": [...],          // complete list + per-test hash
    "test_inventory_hash": "<sha>",
    "fixture_inventory": [...],
    "schema_versions": {...},
    "dependency_lock_hash": "<sha>",
    "e2e_sample_ids": [...]
  }

Freeze:
  - P0–P5 data models (all enums, structs, invariants)
  - All current tests (deterministic + property + integration + finalization
    + budget + idempotent-finalize + auto-complete-without-finish)
  - Evidence property tests
  - Finalization tests
  - Real KVDB E2E samples (input + output + evidence chain)
  - Historical success/failure replay samples

Python version is now: Reference Oracle.
The manifest makes the reference itself verifiable — you can hash the
test inventory and detect drift.
No semantic changes allowed during migration (only explicit bug fixes).
```

### M1: Establish Schemas and Golden Fixtures

```
schemas/v1/                    (JSON Schema for all 14 core objects)
fixtures/
├── success/
├── incomplete/
├── verification_failed/
├── environment_error/
├── budget_depleted_success/
├── checkpoint_mismatch/
├── evidence_tampered/
├── cross_attempt_pollution/
├── commit_failed/
└── replay_mismatch/

Each fixture contains:
  input.json
  expected_evidence_hash.txt
  expected_session_decision.json
  expected_attempt_decision.json
  expected_replay_result.json

Acceptance: Python Reference reads fixture → produces fixed results.
No Rust code yet — this step validates the spec is unambiguous.
```

### M2: Rust Pure-Function Kernel (First Pass)

Implement ONLY (no network, no DB, no Docker, no IronClaw):

```
Canonical hash
Evidence invalidation
Criterion reduction
Requirement reduction
SessionDecisionService
AttemptDecisionService
CheckpointBinding
Replay hash calculation
State transition validation
```

Acceptance:

```
All Golden Fixtures: Python result == Rust result  (100%, not "mostly")
Context Hash:        Python == Rust
Decision:            Python == Rust
Replay:              Python == Rust
```

### M2.5: Effect Semantics

**Blocker scope (corrected):**

```
M2.5 is a HARD BLOCKER for M6 (side-effect commit authority).
M2.5 is NOT a blocker for M5 (success determination).

Rationale: M5 only decides "is the task done?" — it does not control
whether side effects are published or discarded. M5 can proceed as long
as it does not claim authority over real-world effects.
```

Must complete before M6:

```
1. EffectClass model (Pure / ReadOnly / Staged / Transactional /
   Compensatable / Irreversible)
2. EffectClassifier trait + trusted-source invariants (Agent cannot downgrade)
3. PreExecutionDecision + SettlementDecision protocol per EffectClass
4. Per-Runtime-Lane side-effect timeline (T1–T7 for each lane: WASM, MCP,
   Docker, HTTP, Filesystem, Database)
5. Identification of T4 (first external side effect) for each lane
6. Confirmation that IronClaw's consume() semantics match publish semantics,
   or documentation of where the protocol needs extension for each lane
7. Staged / transactional / compensatable / irreversible protocol
8. Publish / discard / freeze / compensate semantics
9. Idempotency key + publish receipt model
10. Crash-window recovery rules per transaction state
```

What M1 must already freeze for EffectClass:

```
EffectClass enum (schema)
EffectClassification struct (schema)
PreExecutionDecision enum (schema)
SettlementDecision enum (schema)
```

What M2 must already implement (pure functions, no IronClaw):

```
EffectClassification logic (rules-based, no LLM)
PreExecutionDecision derivation from EffectClass + Contract + RiskLevel
SettlementDecision derivation from Evidence + CriterionVerdict
```

Without M2.5 completion, do not proceed to M6.

### M3: Rust Runtime Coordinator

Add Port traits on top of the pure kernel:

```
AuthorizationPort
ApprovalPort
RuntimePort
VerifierPort
ArtifactStorePort
EvidenceStorePort
EventSinkPort
CheckpointPort
ClockPort
```

Core logic depends ONLY on these traits, never on IronClaw concrete types.

Conceptual flow:

```
prepare(contract)
  → authorize(capability)
  → stage(runtime)
  → execute(runtime)
  → capture_effects(runtime)
  → verify(verifier ports)
  → reduce(core)
  → publish / discard / freeze(runtime)
  → persist(event/evidence stores)
```

### M4: Shadow Mode — IronClaw Integration

Register Onto as observer only — compute decisions but do not block:

```
1. Register Builtin Observer hooks (before_capability, after_capability)
2. Read events from IronClaw EventStore
3. Re-compute Evidence + Decision in Onto kernel
4. Record ShadowDecision (comparison log, never blocks real tasks)
5. Run differential comparison:
   - IronClaw current task state
   - Python Reference result
   - Rust Kernel result
```

Acceptance: zero divergence across all fixtures and real KVDB E2E runs.

### M5: Take Over Success Determination

The first authority transfer — the safest layer:

```
Agent stops
  → IronClaw triggers FinalizationGateway (new loop_exit seam)
  → Rust Kernel runs FinalizationGateway
  → Input: ExitReason (FinishRequested | ProviderStop | BudgetLimit |
            Stuck | Cancelled | Crashed)
  → Output: TaskOutcome × BudgetOutcome × LifecycleState
  → Example: BudgetLimit + all criteria met +
              valid evidence → (Success, Depleted, Committed)

Rules:
  - Agent cannot directly produce SUCCESS
  - IronClaw Mission cannot self-mark as complete
  - Side-effect publication may still use the original path (temporarily)
  - M5 controls "is the task done?" — NOT "should effects be published?"
```

### M6: Take Over ExecutionTransaction and Commit

The highest-risk transfer. Proceed by EffectClass, from safest to most
dangerous:

```
Batch 1: Pure / ReadOnly           (no side effects)
Batch 2: Staged filesystem/code    (isolated, discardable)
Batch 3: Transactional database    (real rollback available)
Batch 4: Compensatable API         (compensation possible)
Batch 5: Irreversible external     (pre-execution gate, strong approval)
```

For each batch:

```
IronClaw Authorization
  ↓
Rust Onto Transaction PREPARED
  ↓
IronClaw Runtime Lane creates staged environment (Staged) or
begins real transaction (Transactional) or prepares pre-flight (Compensatable)
  ↓
Execute
  ↓
Rust captures SideEffectManifest
  ↓
Verify in same environment
  ↓
Evidence + SettlementDecision
  ├── Commit    → IronClaw publish staged effects
  ├── Rollback  → IronClaw discard staged effects / rollback txn
  ├── Compensate→ IronClaw run compensation action
  ├── Confirm   → IronClaw persist audit record (Irreversible/Compensatable)
  └── Escalate  → IronClaw freeze snapshot + notify human
```

Invariant: **Verify before publish, destroy sandbox after.**

For Irreversible effects specifically: **PreExecutionDecision::Allow is
produced BEFORE execution. SettlementDecision::Confirm is produced AFTER
execution as an audit record — it does not control whether the effect
happened (it already did), only whether the evidence package is complete.**

### M7: Migrate Code Pack

After the universal kernel is stable:

```
Phase 1: Artifact collection + hash + command result parsing + Git diff
Phase 2: Project identification + build plan parsing + basic language verifiers
Phase 3: Complex static analysis + LLM semantic verifiers
```

Initial implementation can bridge:

```
Rust Onto Kernel
  ↓ VerifierPort
Python Code Verifier Worker (existing code)
  ↓ VerificationRun JSON
Rust Evidence / Decision
```

Gradually Rust-ify the verifiers without blocking the AI OS integration.

---

## 11. Test Strategy

### 11.1 Differential Tests

```
Same fixture
  → Python Reference
  → Rust Kernel
  → Field-level comparison
```

Must cover:

```
Success
Incomplete
Verification failed
Environment error
Evidence tampered
Cross-attempt pollution
Budget depleted but success achieved
Success without explicit finish() call
Commit failure
Replay mismatch
```

### 11.2 Property Tests (Rust)

```
No valid Evidence → must not produce SUCCESS
Any blocking UNSATISFIED → must not produce SUCCESS
EnvironmentError → must not produce COMMIT
Code changed → old Evidence invalidated
Requirements changed → old Evidence invalidated
Verifier changed → old Evidence invalidated
Evidence reordered → decision unchanged
Duplicate Evidence → no increase in proof weight
Cross-attempt Evidence → rejected
COMMITTED terminal state → immutable
```

### 11.3 Fault Injection (post IronClaw integration)

```
Runtime execution interrupted mid-flight
Evidence write fails
EventStore write fails
Crash before side-effect publish
Crash after publish, before state persisted
Duplicate Commit
Concurrent Commit
Approval lease expires
Checkpoint tampered
```

Must guarantee (with explicit scope limits):

```
Staged / Transactional:
  → No double publish of staged effects
  → No half-commits (either fully PUBLISHED or fully DISCARDED)

External systems with idempotency key support:
  → No duplicate effects (provider guarantees idempotency)

Irreversible without idempotency support:
  → At-most-once dispatch OR human reconciliation
  → NEVER blind auto-retry on UNKNOWN_EXTERNAL_OUTCOME
  → UNKNOWN_EXTERNAL_OUTCOME → ESCALATE (not silent retry, not fake success)

Universal:
  → No SUCCESS without evidence
  → Convergence after recovery (same input → same final decision)
  → Do NOT claim exactly-once for systems that cannot prove receipt
```

### 11.4 Real E2E

KVDB alone proves the Code Pack. A universal assurance kernel requires at
least three additional EffectClass scenarios:

```
E2E-1 (Code Pack — Staged):
  C KVDB implementation
  → WASM/Docker sandbox
  → gcc compile
  → 9/9 tests pass
  → EvidenceBundle
  → SettlementDecision::Commit
  → Replay

E2E-2 (Data Pack — Transactional):
  SQLite schema migration + data transformation
  → BEGIN TRANSACTION
  → Execute DDL + DML
  → Verify schema + data integrity
  → SettlementDecision::Commit (or Rollback on failure)
  → EvidenceBundle with pre/post snapshots

E2E-3 (Workflow Pack — Irreversible):
  Simulated email sending API
  → PreExecutionDecision checks (recipient, content, rate limit)
  → Strong approval (human-in-the-loop for Irreversible)
  → Exact invocation lease
  → Execute once
  → SettlementDecision::Confirm (post-audit)
  → Replay verification

Optional E2E-4 (Ops Pack — Compensatable):
  Simulated cloud resource create + delete
  → PreExecutionDecision::Allow
  → Execute (create resource)
  → Verify fails → SettlementDecision::Compensate
  → Compensation executes (delete resource)
  → EvidenceBundle: original effect + compensation + residual state
```

These do not need real production systems — local deterministic simulators
(mock HTTP server, in-memory SQLite, fake email sink) are sufficient to
validate the protocol. But without exercising Staged, Transactional,
Compensatable, and Irreversible paths, the kernel remains only proven
for code-generation workflows.

---

## 12. Python Deletion Conditions

Do NOT delete the Python implementation when Rust "appears to work."
All conditions must be met simultaneously:

```
Reference manifest verified        ✅ (M0 manifest hash matches current state)
Golden fixture parity              100% (all fixtures, all three hash domains)
Hash parity (Content/Envelope/
  Context)                         100%
Decision parity                    100%
Replay parity                      100%
P0–P5 invariant tests              all passing
E2E-1 (Code/Staged)                passing
E2E-2 (Data/Transactional)         passing
E2E-3 (Workflow/Irreversible)      passing
E2E-4 (Ops/Compensatable)          passing (optional but recommended)
Fault injection                    passing (all crash windows)
IronClaw shadow-mode divergence    0
Rust is sole TaskOutcome producer  ✅
Rust is sole SettlementDecision
  producer (Commit/Rollback/
  Compensate/Confirm/Escalate)     ✅
```

Then:

```
Python ontocode-core
  → retain as compatibility Facade only
  → internally delegates to Rust
  → eventually remove Reference implementation
```

---

## 13. Code Size Estimates (Ranges, Not Targets)

```
Component                        Low        High
─────────────────────────────────────────────────
onto-assurance-types             1,500      2,500
onto-assurance-core              3,500      5,500
onto-assurance-runtime           2,500      5,000
onto-ironclaw-adapter            3,000      7,000
onto-pack-sdk                      400        800
onto-code-pack                   1,500      4,000
Tests, fixtures, fault-injection 5,000     10,000
─────────────────────────────────────────────────
Total (source + tests)          17,400     34,800
```

Current ontocode-core Python: 18,379 lines. The Rust kernel is likely similar
magnitude, but with zero redundant architecture.

**Line count is not an acceptance criterion.** Golden fixture parity is.

---

## 14. Immediate Execution Order

These steps can start now, with no IronClaw changes required:

```
1. Tag: ontocode-python-assurance-reference-v1

2. Create docs/assurance-kernel-boundary.md
   Document:
   - IronClaw ownership (what stays)
   - Onto ownership (what moves to Rust)
   - Deleted/adapted modules
   - Three converged state machines
   - EffectClass taxonomy

3. Create schemas/v1/
   Freeze all 14 core objects as JSON Schema

4. Convert P0–P5 tests into Golden Fixtures
   One directory per scenario, fixed input/output files

5. Create Rust workspace + six crates (stubs first)

6. Implement in order:
   a. Canonical hash
   b. Evidence invalidation
   c. Criterion reduction
   d. SessionDecisionService
   e. AttemptDecisionService
   f. Checkpoint binding
   g. Replay

7. Establish Python/Rust differential test runner

8. Proceed to onto-assurance-runtime ONLY after 100% fixture parity

9. Shadow-integrate with IronClaw LAST
```

---

## 15. Risk Register

| Risk | Severity | Mitigation |
|------|----------|------------|
| Agent/LLM declares EffectClass to bypass restrictions | **Critical** | EffectClassifier is trusted kernel code; Agent cannot override; unknown→Irreversible fail-closed |
| `consume()` ≠ side-effect publish boundary | **Critical** | M2.5: trace T1–T7 per lane; identify T4 for each lane before M6 |
| Irreversible effect executed before PreExecutionDecision | **Critical** | PreExecutionDecision::Allow must be produced BEFORE T4; gate in CapabilityHost, not post-hoc |
| Canonical hash divergence Python↔Rust | **High** | Three-hash-domain spec in M1; cross-language fixtures in M2 |
| Crash between PUBLISHING and PUBLISHED | **High** | Idempotency key + publish receipt + outbox reconciliation |
| Crash between COMPENSATING and COMPENSATED | **High** | Compensation idempotency key + external state re-query |
| Onto Builtin bridge too large, expands TCB | **Medium** | Thin bridge; verifiers outside TCB; architecture-boundary tests |
| IronClaw changes during migration | **Medium** | Shadow mode (M4) is non-blocking; adapters isolate churn |
| Onto SUCCESS conflated with EXHAUSTED/CRASHED | **Medium** | Three-dimensional outcome model enforced in FinalizationGateway property tests |
| Line-count estimate used as deadline | **Low** | Treat as rough range only; golden fixture parity is the real metric |
| Code Pack migration blocks AI OS integration | **Low** | Bridge via VerifierPort to Python worker as interim step |

---

## 16. Decision Log

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | Converge to 3 FSMs, not port 10+21 | Current dual-level FSM encodes SDK integration, not universal semantics |
| 2 | `onto-assurance-core` is pure, zero deps | Enables formal verification, WASM compilation, multi-host portability |
| 3 | EffectClass Schema frozen at M1, logic at M2, lane timeline at M2.5 | Early phases need the enum; pure classification logic is deterministic; M6 needs per-lane timeline proof |
| 4 | Builtin Bridge is thin; verifiers are external | Keeps IronClaw TCB small; verifier bugs don't compromise the gate |
| 5 | Python Reference frozen at M0 with manifest | Manifest makes the reference itself verifiable; prevents semantic drift |
| 6 | Shadow mode before authority transfer | M4 proves correctness non-disruptively before M5–M6 block real tasks |
| 7 | Commit authority transferred by EffectClass | Irreversible effects gated last; Pure/Staged effects gated first |
| 8 | Three-dimensional outcome (TaskOutcome × BudgetOutcome × LifecycleState) | EXHAUSTED/CRASHED must not be conflated with SUCCESS/FAILED |
| 9 | PreExecutionDecision + SettlementDecision split | A single CommitDecision that means both "allow execution" and "publish effects" creates semantic conflict on Irreversible actions |
| 10 | EffectClass from trusted CapabilityDescriptor, not Agent | Agent can only describe Intent; malicious Agent could label destructive actions as ReadOnly |
| 11 | Onto state ↔ IronClaw state correlated by ID, not mapped 1:1 | Lease::Consumed ≠ Effect::Published ≠ Task::Successful; distinct state machines, distinct facts |
| 12 | Onto monotonic tightening only | IronClaw DENY → Onto cannot ALLOW; prevents Builtin hook from becoming a privilege escalation path |
| 13 | Intent constructed BEFORE authorization | Authorization gates need EffectClass + Contract context to make informed decisions |
| 14 | Three-hash-domain model (Content/Envelope/Context) | Single context_hash creates cycles and non-determinism; domain separators prevent cross-domain collision |
| 15 | At-most-once for irreversible without idempotency; UNKNOWN→ESCALATE | Cannot claim exactly-once for systems that cannot prove receipt; blind retry on unknown outcome is dangerous |
| 16 | M2.5 hard-blocks M6, not M5 | M5 only decides task completion; M6 controls real-world side effects |
