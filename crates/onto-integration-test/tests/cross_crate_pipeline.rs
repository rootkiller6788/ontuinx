//! Cross-crate integration tests — full pipeline E2E.
//!
//! Exercises multiple crates together:
//!   onto-assurance-types → onto-assurance-core → onto-assurance-runtime
//!   → onto-loop → onto-ironclaw-adapter → onto-temporal-adapter

use std::sync::Arc;

use onto_assurance_core::evidence_chain::EvidenceChain;
use onto_assurance_core::{reduction, session_decision, settlement};
use onto_assurance_runtime::mocks::MockLoopRuntime;
use onto_assurance_runtime::ports::RunFinalizationOutcome;
use onto_assurance_types::contract::{
    AcceptanceCriterion, ApprovalMode, CriterionKind, ExecutionContract,
};
use onto_assurance_types::enums::{BudgetOutcome, EffectClass, LifecycleState, TaskOutcome};
use onto_assurance_types::evidence::{EvidenceRecord, EvidenceRecordKind, VerifierBinding};
use onto_assurance_types::ids::{
    AttemptId, CriterionId, DecisionId, RunId, TransactionId, VerifierId,
};
use onto_assurance_types::ingress::{InputEnvelope, RuntimeActorRef, SessionSource, StartRunRequest};
use onto_ironclaw_adapter::builtin_bridge::BuiltinGate;
use onto_ironclaw_adapter::direct_run::DirectRunAdapter;
use onto_ironclaw_adapter::run_ingress::StubRunIngress;
use onto_loop::budget::LoopBudget;
use onto_loop::checkpoint::AttemptCheckpoint;
use onto_loop::progress::{compare_progress, ProgressComparison, ProgressSnapshot};
use onto_temporal_adapter::idempotency::{IdempotencyStore, InMemoryIdempotencyStore};
use onto_temporal_adapter::protocol::{LoopInvocationRequest, LoopTerminalEnvelope, LoopTerminalState};
use onto_temporal_adapter::RuntimeLoopRunner;

fn make_outcome(task: TaskOutcome, lifecycle: LifecycleState) -> RunFinalizationOutcome {
    RunFinalizationOutcome {
        task_outcome: task,
        budget_outcome: BudgetOutcome::WithinBudget,
        lifecycle_state: lifecycle,
        session_decision_id: DecisionId::new(),
        attempt_decision_id: DecisionId::new(),
        reason_codes: vec![],
        settlement_decision: None,
        effect_class: None,
    }
}

fn make_temporal_request(flow: &str, wi: &str, gen: u64) -> LoopInvocationRequest {
    let req = LoopInvocationRequest {
        schema_version: 1,
        flow_id: flow.to_string(),
        work_item_id: wi.to_string(),
        loop_id: format!("loop-{}-{}", flow, wi),
        task_spec_ref: format!("spec/{}", wi),
        contract_ref: "contract/default".into(),
        policy_ref: "policy/default".into(),
        input_artifact_refs: vec![],
        resource_class: "standard".into(),
        risk_class: "low".into(),
        trust_requirement: "basic".into(),
        budget_grant_ref: format!("grant-{}", wi),
        budget_grant_hash: "grant-hash".into(),
        execution_generation: gen,
        idempotency_key: format!("idem-{}", wi),
        deadline: None,
        request_binding_hash: String::new(),
    };
    let hash = req.compute_request_binding_hash();
    LoopInvocationRequest { request_binding_hash: hash, ..req }
}

// ══════════════════════════════════════════════════════════════════
// Pipeline 1: DirectRun → Runtime → Finalization
// ══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn pipeline_direct_run_to_finalization() {
    let ingress = Box::new(StubRunIngress::new());
    let adapter = DirectRunAdapter::new(ingress);

    let mock_rt = MockLoopRuntime::new();
    mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

    let result = adapter.execute(
        StartRunRequest {
            actor: RuntimeActorRef::agent("test-bot"),
            source: SessionSource::Direct,
            input: InputEnvelope::new("write hello()"),
            project_id: None,
            parent_work_item: None,
        },
        &mock_rt,
    ).await;

    assert!(result.is_committed());
    assert_eq!(result.task_outcome, TaskOutcome::Success);
}

// ══════════════════════════════════════════════════════════════════
// Pipeline 2: OntoFlow Adapter → LoopRunner → Full Attempt Cycle
// ══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn pipeline_temporal_runner_two_attempts() {
    let store = Arc::new(InMemoryIdempotencyStore::new());
    let mock_rt = Arc::new(MockLoopRuntime::new());
    mock_rt.push_outcome(make_outcome(TaskOutcome::Failed, LifecycleState::Continuing));
    mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

    let runner = RuntimeLoopRunner::with_empty_coordinator(store, mock_rt, 5);
    let envelope = runner.execute(make_temporal_request("flow-1", "wi-A", 1)).await.unwrap();

    // P16: Fail-Closed — empty registry → PipelineError → Escalate on attempt 1
    assert_eq!(envelope.reported_terminal_state, LoopTerminalState::Escalated);
    assert_eq!(envelope.total_attempts, 1);
    assert!(envelope.decision_id.is_some());
    assert!(envelope.outcome_binding_hash.len() > 0);
}

// ══════════════════════════════════════════════════════════════════
// Pipeline 3: Evidence Chain → Reduction → Session → Settlement
// ══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_evidence_to_settlement() {
    let run_id = RunId::new();
    let mut chain = EvidenceChain::new(
        run_id, "genesis".into(),
        VerifierBinding {
            verifier_id: VerifierId::new(), verifier_version: "1.0.0".into(),
            toolchain: Some("gcc".into()), environment_hash: None,
        },
    );

    for _i in 0..2 {
        chain.append(EvidenceRecord {
            evidence_id: onto_assurance_types::ids::EvidenceId::new(),
            transaction_id: TransactionId::new(),
            criterion_id: CriterionId::new(),
            kind: EvidenceRecordKind::VerifierReport,
            payload: serde_json::json!({"passed": true}),
            recorded_at: chrono::Utc::now(),
        }).unwrap();
    }
    let bundle = chain.seal();

    let criteria = vec![AcceptanceCriterion {
        criterion_id: CriterionId::new(), name: "build".into(),
        kind: CriterionKind::TestPass, description: "".into(), is_blocking: true,
    }];
    let evidence: Vec<_> = criteria.iter().map(|c| EvidenceRecord {
        evidence_id: onto_assurance_types::ids::EvidenceId::new(),
        transaction_id: TransactionId::new(),
        criterion_id: c.criterion_id,
        kind: EvidenceRecordKind::VerifierReport,
        payload: serde_json::json!({"passed": true}),
        recorded_at: chrono::Utc::now(),
    }).collect();

    let verdict = reduction::reduce(&criteria, &evidence);
    assert!(verdict.overall_passed);

    let session = session_decision::decide_session(
        run_id, AttemptId::new(),
        onto_assurance_types::enums::ExitReason::FinishRequested,
        &verdict, BudgetOutcome::WithinBudget, bundle.bundle_id,
    );
    assert_eq!(session.task_outcome, TaskOutcome::Success);
    assert_eq!(session.lifecycle_state, LifecycleState::Committed);

    let settlement = settlement::derive_settlement(
        EffectClass::Staged, session.task_outcome, &verdict,
    );
    assert_eq!(settlement, Some(onto_assurance_types::decision::SettlementDecision::Commit));
}

// ══════════════════════════════════════════════════════════════════
// Pipeline 4: BuiltinGate monotonic tightening
// ══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_builtin_gate_monotonic() {
    let gate = BuiltinGate::default();

    // Onto can only TIGHTEN OntoRuntime decisions
    let ironclaw_says_commit = onto_assurance_types::decision::SettlementDecision::Commit;

    let tightened = gate.evaluate_settlement(
        TaskOutcome::Failed,
        EffectClass::Staged,
        &ironclaw_says_commit,
    );
    // Agent failed → Onto overrides Commit → Escalate
    assert!(matches!(tightened, onto_assurance_types::decision::SettlementDecision::Escalate { .. }));

    // Agent succeeded → Onto allows Commit through
    let allowed = gate.evaluate_settlement(
        TaskOutcome::Success,
        EffectClass::Staged,
        &ironclaw_says_commit,
    );
    assert_eq!(allowed, onto_assurance_types::decision::SettlementDecision::Commit);
}

// ══════════════════════════════════════════════════════════════════
// Pipeline 5: Budget + Progress + Checkpoint across crates
// ══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_budget_progress_checkpoint() {
    // Budget (onto-loop)
    let mut budget = LoopBudget::new(5);
    budget.record_attempt();
    budget.record_attempt();
    assert!(!budget.attempt_exhausted());

    // Checkpoint (onto-loop)
    let cp = AttemptCheckpoint {
        checkpoint_id: onto_assurance_types::ids::CheckpointId::new(),
        attempt_id: AttemptId::new(),
        input_state_hash: onto_assurance_types::transaction::ContentHash::new("in"),
        output_state_hash: onto_assurance_types::transaction::ContentHash::new("out"),
        parent_checkpoint_id: None,
    };
    assert!(cp.parent_checkpoint_id.is_none());

    // Progress (onto-loop)
    let c1 = onto_assurance_types::ids::CriterionId::new();
    let c2 = onto_assurance_types::ids::CriterionId::new();
    let prev = ProgressSnapshot {
        satisfied_criteria: vec![c1],
        unsatisfied_criteria: vec![c2],
        protected_scope_violations: 0, failed_verifiers: 0,
        environment_errors: 0,
        checkpoint_hash: onto_assurance_types::transaction::ContentHash::new("h1"),
    };
    let curr = ProgressSnapshot {
        satisfied_criteria: vec![c1, c2],
        unsatisfied_criteria: vec![],
        protected_scope_violations: 0, failed_verifiers: 0,
        environment_errors: 0,
        checkpoint_hash: onto_assurance_types::transaction::ContentHash::new("h2"),
    };
    assert_eq!(compare_progress(&prev, &curr), ProgressComparison::Improved);
}

// ══════════════════════════════════════════════════════════════════
// Pipeline 6: Idempotency across temporal adapter
// ══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn pipeline_idempotency_cross_crate() {
    let store = Arc::new(InMemoryIdempotencyStore::new());
    let mock_rt = Arc::new(MockLoopRuntime::new());
    mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

    let req = make_temporal_request("flow-x", "wi-idem", 1);
    let loop_id = req.loop_id.clone();

    let runner = RuntimeLoopRunner::with_empty_coordinator(store.clone(), mock_rt.clone(), 5);
    let envelope1 = runner.execute(req).await.unwrap();
    // P14: Fail-Closed — without Verdict, Success→Escalate
    assert_eq!(envelope1.reported_terminal_state, LoopTerminalState::Escalated);

    let result = store.check("flow-x", "wi-idem", 1, &loop_id);
    match result {
        Ok(onto_temporal_adapter::idempotency::IdempotencyResult::Terminal(cached)) => {
            assert_eq!(cached.loop_id, loop_id);
            assert_eq!(cached.reported_terminal_state, LoopTerminalState::Escalated);
        }
        other => panic!("expected Terminal, got {:?}", other),
    }
}

// ══════════════════════════════════════════════════════════════════
// Pipeline 7: Profile + Pack selection
// ══════════════════════════════════════════════════════════════════

#[test]
fn pipeline_profile_pack_selection() {
    use onto_assurance_types::profile::{ProfileKind, ProfileRegistry, RiskTolerance};
    use onto_pack_sdk::packs::{PackDomain, PackRegistry};

    let profiles = ProfileRegistry {
        profiles: onto_assurance_types::profile::RuntimeProfile::all_builtins(),
        default_profile: "coding-agent".into(),
    };

    let packs = PackRegistry::available_packs();

    // Coding agent uses code-pack
    let coding = profiles.find("coding-agent").unwrap();
    assert_eq!(coding.kind, ProfileKind::CodingAgent);
    assert_eq!(coding.assurance_pack, "code-pack");

    let code_pack = packs.find_by_domain(PackDomain::Code).unwrap();
    assert!(code_pack.criteria.contains(&"build_pass".to_string()));

    // Ops agent uses ops-pack
    let ops = profiles.find("ops-agent").unwrap();
    assert_eq!(ops.assurance_pack, "ops-pack");
    assert_eq!(ops.max_risk_level, RiskTolerance::High);

    let ops_pack = packs.find_by_domain(PackDomain::Ops).unwrap();
    assert!(ops_pack.criteria.contains(&"rollback_plan_exists".to_string()));
}
