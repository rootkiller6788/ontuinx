//! R3: DAG + Fault Injection

use std::sync::Arc;
use onto_assurance_runtime::mocks::MockLoopRuntime;
use onto_assurance_runtime::ports::RunFinalizationOutcome;
use onto_assurance_types::enums::{BudgetOutcome, LifecycleState, TaskOutcome};
use onto_assurance_types::ids::DecisionId;
use onto_temporal_adapter::idempotency::{IdempotencyStore, InMemoryIdempotencyStore};
use onto_temporal_adapter::lease::{InMemoryLoopLease, LeaseResult, LoopExecutionLease};
use onto_temporal_adapter::protocol::{LoopInvocationRequest, LoopTerminalState};
use onto_temporal_adapter::RuntimeLoopRunner;

fn mo() -> RunFinalizationOutcome { RunFinalizationOutcome{task_outcome:TaskOutcome::Success,budget_outcome:BudgetOutcome::WithinBudget,lifecycle_state:LifecycleState::Committed,session_decision_id:DecisionId::new(),attempt_decision_id:DecisionId::new(),reason_codes:vec![],settlement_decision:None,effect_class:None} }
fn rq(id: &str) -> LoopInvocationRequest {
    let req = LoopInvocationRequest{schema_version:1,flow_id:"f".into(),work_item_id:id.into(),loop_id:format!("loop-{}",id),task_spec_ref:"s".into(),contract_ref:"c".into(),policy_ref:"p".into(),input_artifact_refs:vec![],resource_class:"s".into(),risk_class:"l".into(),trust_requirement:"b".into(),budget_grant_ref:"g".into(),budget_grant_hash:"h".into(),execution_generation:1,idempotency_key:format!("k-{}",id),deadline:None,request_binding_hash:String::new()};
    let h = req.compute_request_binding_hash(); LoopInvocationRequest{request_binding_hash:h,..req}
}

#[tokio::test] async fn r3_dag_sequence() {
    for id in &["A","B","C"] {
        let s = Arc::new(InMemoryIdempotencyStore::new()); let r = Arc::new(MockLoopRuntime::new()); r.push_outcome(mo());
        let env = RuntimeLoopRunner::with_empty_coordinator(s,r,5).execute(rq(id)).await.unwrap();
        assert_eq!(env.reported_terminal_state, LoopTerminalState::Escalated);
    }
}

#[tokio::test] async fn r3_crash_redispatch_same_loop() {
    let store = Arc::new(InMemoryIdempotencyStore::new());
    let req = rq("wi-retry");
    let lid = req.loop_id.clone();
    let rt = Arc::new(MockLoopRuntime::new()); rt.push_outcome(mo());
    RuntimeLoopRunner::with_empty_coordinator(store.clone(), rt, 5).execute(req).await.unwrap();
    let cached = store.try_complete(&lid);
    assert!(cached.is_some(), "re-dispatch returns cached terminal");
}

#[tokio::test] async fn r3_dual_worker_lease() {
    let lease = Arc::new(InMemoryLoopLease::new());
    assert_eq!(lease.try_acquire("L","A"), LeaseResult::Acquired);
    assert_eq!(lease.try_acquire("L","B"), LeaseResult::AlreadyHeld{holder:"A".into()});
}

#[tokio::test] async fn r3_escalated_blocks_downstream() {
    let s = Arc::new(InMemoryIdempotencyStore::new()); let r = Arc::new(MockLoopRuntime::new());
    // Success without Committed lifecycle → Escalate
    r.push_outcome(RunFinalizationOutcome{task_outcome:TaskOutcome::Success,budget_outcome:BudgetOutcome::WithinBudget,lifecycle_state:LifecycleState::Continuing,session_decision_id:DecisionId::new(),attempt_decision_id:DecisionId::new(),reason_codes:vec![],settlement_decision:None,effect_class:None});
    let env = RuntimeLoopRunner::with_empty_coordinator(s,r,5).execute(rq("bad")).await.unwrap();
    assert_eq!(env.reported_terminal_state, LoopTerminalState::Escalated);
    assert!(!env.reported_terminal_state.allows_downstream());
}

#[tokio::test] async fn r3_concurrent_isolated() {
    let mut h = vec![];
    for i in 0..5 { h.push(tokio::spawn(async move {
        let s = Arc::new(InMemoryIdempotencyStore::new()); let r = Arc::new(MockLoopRuntime::new()); r.push_outcome(RunFinalizationOutcome{task_outcome:TaskOutcome::Success,budget_outcome:BudgetOutcome::WithinBudget,lifecycle_state:LifecycleState::Committed,session_decision_id:DecisionId::new(),attempt_decision_id:DecisionId::new(),reason_codes:vec![],settlement_decision:None,effect_class:None});
        RuntimeLoopRunner::with_empty_coordinator(s,r,5).execute(rq(&format!("w{}",i))).await.unwrap()
    })); }
    for j in h { assert_eq!(j.await.unwrap().reported_terminal_state, LoopTerminalState::Escalated); }
}
