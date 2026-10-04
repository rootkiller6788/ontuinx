# OntoRuntime Integration Guide

Steps to integrate `ontoos` into OntoRuntime, from shadow mode (M4) through
full side-effect gating (M6). Each step specifies exact files and code.

## Prerequisites

```bash
# Add ontoos as a workspace member in ironclaw-main/Cargo.toml
# Under [workspace.members], add:
"../ontoos/crates/onto-assurance-types",
"../ontoos/crates/onto-assurance-core",
"../ontoos/crates/onto-assurance-runtime",
"../ontoos/crates/onto-ironclaw-adapter",

# Or symlink into crates/:
ln -s ../../ontoos/crates/onto-assurance-types crates/
ln -s ../../ontoos/crates/onto-assurance-core crates/
ln -s ../../ontoos/crates/onto-assurance-runtime crates/
ln -s ../../ontoos/crates/onto-ironclaw-adapter crates/
```

## M4: Shadow Mode

### Step 1: Register Builtin Hook

**File:** `crates/ironclaw_reborn_composition/src/lib.rs` (or wherever `HookRegistry` is assembled)

```rust
use onto_ironclaw_adapter::builtin_bridge::ShadowObserver;

// In the composition function that builds HookRegistry:
let onto_shadow = ShadowObserver::default();
let onto_hook = HookBinding::new("onto.assurance.shadow")
    .with_trust_class(HookTrustClass::Builtin)
    .at_point(HookPoint::BeforeCapability)  // also AfterCapability
    .with_handler(move |ctx| {
        let obs = onto_shadow.observe_capability(
            &ctx.capability_name,
            &ctx.arguments_summary,
        );
        // Log shadow observation — NEVER block
        tracing::info!(?obs, "onto shadow observation");
        ObserverFact::recorded()
    })
    .build();
hook_registry.register(onto_hook);
```

### Step 2: Compare Shadow Decisions

**File:** New test `tests/integration/onto_shadow_compare.rs`

```rust
// After each test run, compare:
//   OntoRuntime's actual outcome vs Onto's shadow decision
// Expected: zero divergence for all golden fixture scenarios
```

### Step 3: Verify

```bash
cargo test -p onto-ironclaw-adapter  # 8 tests, must pass
cargo test --test onto_shadow_compare  # zero divergence
```

## M5: Take Over Success Determination

### Step 1: Add LoopExit Hook Point

**File:** `crates/ironclaw_hooks/src/points/mod.rs`

Add after existing hook points:

```rust
/// Context provided to hooks when an agent loop exits (any exit reason).
pub struct AfterLoopExitContext {
    pub run_id: RunId,
    pub exit_reason: ExitReason,       // FinishRequested | ProviderStop |
                                       // BudgetLimit | Stuck | Cancelled | Crashed
    pub session_summary: SessionSummary,
    pub criteria_results: Vec<CriterionResult>,
}
```

**File:** `crates/ironclaw_hooks/src/points/mod.rs` — add to `HookPoint` enum:

```rust
pub enum HookPoint {
    BeforeCapability,
    AfterCapability,
    BeforePrompt,
    AfterLoopExit,  // ← NEW
    // ...
}
```

### Step 2: Wire FinalizationGateway

**File:** `crates/ironclaw_agent_loop/src/exit.rs` (or wherever loop exit is handled)

```rust
use onto_assurance_core::session_decision;
use onto_assurance_types::enums::{ExitReason, TaskOutcome, BudgetOutcome, LifecycleState};

// In the loop exit handler, BEFORE OntoRuntime marks the run as complete:
let onto_session = session_decision::decide_session(
    run_id, attempt_id,
    map_exit_reason(ironclaw_exit),
    &requirement_verdict,
    map_budget(ironclaw_budget),
    evidence_bundle_id,
);

// onto_session.task_outcome is now the AUTHORITATIVE success determination.
// OntoRuntime's own status is informational only.
```

### Step 3: Enforce Single SUCCESS Producer

```rust
// Invariant check (add to ironclaw_architecture tests):
// No code path other than session_decision::decide_session() may produce
// a final TaskOutcome::Success.
```

## M6: Take Over Side-Effect Commit

### Step 1: Per-Lane T1–T7 Timeline

Before M6, trace the exact side-effect timeline for each Runtime Lane:

```
Lane: WASM
T1: Authorization (ironclaw_authorization)
T2: Lease claim (CapabilityLeaseStorePort::claim)
T3: WASM module instantiation
T4: First WASM function call with side effects
T5: WASM return value
T6: Lease consume (CapabilityLeaseStorePort::consume)
T7: Event persistence
→ For WASM: T4 is inside the sandbox — effects are staged.
  Commit gate can be placed between T5 and T6.

Lane: MCP (HTTP/stdio)
T1: Authorization
T2: Lease claim
T3: MCP connection established
T4: JSON-RPC call sent to MCP server  ← external effect
T5: Response received
T6: Lease consume
T7: Event persistence
→ For MCP: T4 may already have external effects.
  PreExecutionDecision must be before T4 for Irreversible MCP calls.

Lane: Docker Process
T1: Authorization
T2: Lease claim
T3: Docker container created
T4: Command executed ← filesystem effects inside container (staged);
                      network effects may be external
T5: Container returns
T6: Lease consume
T7: Event persistence
→ Depends on EffectClass of the specific command.
```

### Step 2: Wire Settlement Gate

**File:** `crates/ironclaw_capabilities/src/obligations.rs` (or `CapabilityHost`)

```rust
use onto_assurance_core::settlement;
use onto_ironclaw_adapter::builtin_bridge::BuiltinGate;

// In CapabilityHost, after verification completes, before obligation complete:
let onto_settlement = settlement::derive_settlement(
    effect_class, task_outcome, &verdict,
);

let gate = BuiltinGate::new(Box::new(RulesBasedClassifier::new()));
let final_decision = gate.evaluate_settlement(
    task_outcome,
    effect_class,
    &ironclaw_settlement,  // what OntoRuntime would do by default
);

// Monotonic tightening: final_decision is at least as restrictive as
// ironclaw_settlement.
match final_decision {
    SettlementDecision::Commit => obligation_handler.complete(req).await?,
    SettlementDecision::Rollback { .. } => obligation_handler.abort(req).await?,
    SettlementDecision::Freeze { .. } => obligation_handler.freeze(req).await?,
    _ => obligation_handler.escalate(req).await?,
}
```

### Step 3: Add `verify()` to ObligationHandler

**File:** `crates/ironclaw_capabilities/src/obligations.rs`

```rust
pub trait CapabilityObligationHandler {
    async fn prepare(...) -> ...;   // existing
    async fn verify(...) -> ...;    // ← NEW: run verification before complete/abort
    async fn complete(...) -> ...;  // existing
    async fn abort(...) -> ...;     // existing
}
```

### Step 4: Enable Adapter Imports

**File:** `crates/onto-ironclaw-adapter/Cargo.toml`

Uncomment OntoRuntime dependencies:

```toml
[dependencies]
# Uncomment these when building inside OntoRuntime workspace:
ironclaw_capabilities = { path = "../ironclaw_capabilities" }
ironclaw_authorization = { path = "../ironclaw_authorization" }
ironclaw_hooks = { path = "../ironclaw_hooks" }
ironclaw_events = { path = "../ironclaw_events" }
ironclaw_run_state = { path = "../ironclaw_run_state" }
ironclaw_host_runtime = { path = "../ironclaw_host_runtime" }
ironclaw_approvals = { path = "../ironclaw_approvals" }
ironclaw_host_api = { path = "../ironclaw_host_api" }
```

Then implement the adapter stubs:

```rust
// onto-ironclaw-adapter/src/auth_adapter.rs
impl AuthorizationPort for OntoRuntimeAuthorizationAdapter {
    async fn authorize(&self, intent: &ExecutionIntent) -> Result<AuthorizationReceipt, AuthorizationError> {
        let ironclaw_decision = self.authorizer.authorize_dispatch(
            &self.execution_context(intent),
            &self.capability_descriptor(intent),
            &ResourceEstimate::default(),
        ).await;
        // Map OntoRuntime Decision → Onto AuthorizationReceipt
    }
}
```

## Architecture Boundary Tests

**File:** `crates/ironclaw_architecture/src/reborn_dependency_boundaries.rs`

Add:

```rust
#[test]
fn onto_core_has_zero_ironclaw_deps() {
    // onto-assurance-core must not depend on any ironclaw_* crate
}

#[test]
fn onto_builtin_only_tightens() {
    // OntoGateDecision can only be Allow→Deny, Allow→PauseApproval
    // Never Deny→Allow or PauseApproval→Allow
}
```

## E2E Verification

After integration, re-run the golden fixtures through OntoRuntime:

```bash
# KVDB scenario (Code Pack, Staged)
cargo test --test onto_e2e kvdb_c_project_full_success_pipeline

# Deploy scenario (Irreversible)
cargo test --test onto_e2e deploy_irreversible_success_confirms

# All 10 golden fixtures
python conformance/diff_runner.py --fixture-dir fixtures/ --rust-bin target/debug/ontoctl
```

Expected: 10/10 fixture parity, zero shadow divergence, 98 Rust tests pass.

## Rollback Plan

If shadow mode shows non-zero divergence:
1. Disable Onto Builtin hook (set to observer-only)
2. Fix divergence in onto-assurance-core
3. Re-run shadow comparison
4. Re-enable when divergence reaches zero
