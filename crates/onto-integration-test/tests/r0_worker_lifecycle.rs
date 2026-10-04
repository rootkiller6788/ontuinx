//! R0: Worker Lifecycle — 同 WorkItem 重派→同 loop_id, Lease 保护

use std::sync::Arc;
use onto_assurance_runtime::mocks::MockLoopRuntime;
use onto_assurance_runtime::ports::RunFinalizationOutcome;
use onto_assurance_types::enums::{BudgetOutcome, LifecycleState, TaskOutcome};
use onto_assurance_types::ids::DecisionId;
use onto_temporal_adapter::idempotency::{IdempotencyStore, IdempotencyResult, InMemoryIdempotencyStore};
use onto_temporal_adapter::lease::{InMemoryLoopLease, LeaseResult, LoopExecutionLease};
use onto_temporal_adapter::protocol::{LoopInvocationRequest, LoopTerminalState};
use onto_temporal_adapter::RuntimeLoopRunner;

fn mo(t: TaskOutcome, l: LifecycleState) -> RunFinalizationOutcome {
    RunFinalizationOutcome { task_outcome: t, budget_outcome: BudgetOutcome::WithinBudget, lifecycle_state: l, session_decision_id: DecisionId::new(), attempt_decision_id: DecisionId::new(), reason_codes: vec![], settlement_decision: None, effect_class: None }
}

fn rq(flow: &str, wi: &str, gen: u64) -> LoopInvocationRequest {
    let req = LoopInvocationRequest { schema_version: 1, flow_id: flow.into(), work_item_id: wi.into(), loop_id: format!("loop-{}-{}", flow, wi), task_spec_ref: format!("spec/{}", wi), contract_ref: "c".into(), policy_ref: "p".into(), input_artifact_refs: vec![], resource_class: "s".into(), risk_class: "l".into(), trust_requirement: "b".into(), budget_grant_ref: "g".into(), budget_grant_hash: "h".into(), execution_generation: gen, idempotency_key: format!("idem-{}-{}", flow, wi), deadline: None, request_binding_hash: String::new() };
    let h = req.compute_request_binding_hash();
    LoopInvocationRequest { request_binding_hash: h, ..req }
}

#[tokio::test] async fn r0_1_same_workitem_retry_same_loop_id() {
    let store = Arc::new(InMemoryIdempotencyStore::new());
    let req = rq("f", "w", 1);
    let loop_id = req.loop_id.clone();
    let rt = Arc::new(MockLoopRuntime::new()); rt.push_outcome(mo(TaskOutcome::Success, LifecycleState::Committed));
    let runner = RuntimeLoopRunner::with_empty_coordinator(store.clone(), rt, 5);
    runner.execute(req).await.unwrap();
    // Re-delivery: same flow+wi+gen must return cached terminal
    let r = store.check("f", "w", 1, &loop_id).unwrap();
    assert!(matches!(r, IdempotencyResult::Terminal(_)), "retry must return same loop_id");
}

#[tokio::test] async fn r0_2_lease_blocks_dual_workers() {
    let lease = Arc::new(InMemoryLoopLease::new());
    assert_eq!(lease.try_acquire("L1", "wA"), LeaseResult::Acquired);
    assert_eq!(lease.try_acquire("L1", "wB"), LeaseResult::AlreadyHeld{holder:"wA".into()});
    lease.release("L1", "wA");
    assert_eq!(lease.try_acquire("L1", "wB"), LeaseResult::Acquired);
}

#[tokio::test] async fn r0_3_request_binding_hash_preserved() {
    let store = Arc::new(InMemoryIdempotencyStore::new());
    let req = rq("f", "w", 1);
    let sent = req.request_binding_hash.clone();
    let rt = Arc::new(MockLoopRuntime::new()); rt.push_outcome(mo(TaskOutcome::Success, LifecycleState::Committed));
    let runner = RuntimeLoopRunner::with_empty_coordinator(store, rt, 5);
    let env = runner.execute(req).await.unwrap();
    assert_eq!(env.request_binding_hash, sent, "binding hash must survive pipeline");
}

#[tokio::test] async fn r0_4_completed_is_not_committed() {
    let store = Arc::new(InMemoryIdempotencyStore::new());
    let req = rq("f", "w", 1);
    let rt = Arc::new(MockLoopRuntime::new()); rt.push_outcome(mo(TaskOutcome::Success, LifecycleState::Committed));
    let runner = RuntimeLoopRunner::with_empty_coordinator(store, rt, 5);
    let env = runner.execute(req).await.unwrap();
    assert_eq!(env.reported_terminal_state, LoopTerminalState::Escalated);
    // ActivityTaskCompleted ≠ WorkItem Committed — Authority must verify separately
    assert!(env.decision_id.is_some());
}
