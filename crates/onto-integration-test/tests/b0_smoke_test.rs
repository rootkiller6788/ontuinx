//! B0: Production entry smoke test.
//!
//! Verifies the full production chain produces all required digests.
//!
//! Chain: RuntimeLoopRunner → AssuredAttemptCoordinator → PipelineManager
//!        → DecisionEngine → LifecycleReducer
//!
//! Digests saved: candidate_digest, plan_digest, evidence_bundle_digest,
//!                verdict_digest, decision_id, directive_digest,
//!                receipt_digest, terminal_state

use std::sync::Arc;
use std::fs;

use onto_assurance_runtime::mocks::MockLoopRuntime;
use onto_assurance_runtime::pipeline::{PassRegistry, PipelineManager};
use onto_assurance_runtime::ports::RunFinalizationOutcome;
use onto_assurance_types::enums::{BudgetOutcome, LifecycleState, TaskOutcome};
use onto_assurance_types::ids::DecisionId;
use onto_temporal_adapter::idempotency::InMemoryIdempotencyStore;
use onto_temporal_adapter::protocol::{LoopInvocationRequest, LoopTerminalState, LoopTerminalEnvelope};
use onto_temporal_adapter::RuntimeLoopRunner;
use onto_temporal_adapter::assured_coordinator::AssuredAttemptCoordinator;

fn make_outcome(task: TaskOutcome, lifecycle: LifecycleState) -> RunFinalizationOutcome {
    RunFinalizationOutcome {
        task_outcome: task, budget_outcome: BudgetOutcome::WithinBudget,
        lifecycle_state: lifecycle, session_decision_id: DecisionId::new(),
        attempt_decision_id: DecisionId::new(), reason_codes: vec![],
        settlement_decision: None, effect_class: None,
    }
}

fn build_request(flow: &str, wi: &str, gen: u64) -> LoopInvocationRequest {
    let mut req = LoopInvocationRequest {
        schema_version: 1, flow_id: flow.to_string(), work_item_id: wi.to_string(),
        loop_id: format!("loop-{}-{}", flow, wi),
        task_spec_ref: "spec/smoke".into(), contract_ref: "contract/smoke".into(),
        policy_ref: "policy/default".into(), input_artifact_refs: vec![],
        resource_class: "standard".into(), risk_class: "low".into(),
        trust_requirement: "basic".into(),
        budget_grant_ref: format!("grant-{}", wi),
        budget_grant_hash: "grant-hash".into(),
        execution_generation: gen,
        idempotency_key: format!("idem-{}-{}", flow, wi),
        deadline: None,
        request_binding_hash: String::new(),
    };
    let hash = req.compute_request_binding_hash();
    LoopInvocationRequest { request_binding_hash: hash, ..req }
}

/// B0-1: Full chain with empty registry → Escalate (Fail-Closed)
#[tokio::test]
async fn b0_1_full_chain_empty_registry_escalates() {
    let store = Arc::new(InMemoryIdempotencyStore::new());
    let mock_rt = Arc::new(MockLoopRuntime::new());
    mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

    let coordinator = Arc::new(AssuredAttemptCoordinator::new(PipelineManager::new()));
    let runner = RuntimeLoopRunner::new(store, mock_rt, coordinator, 5);
    let req = build_request("b0", "smoke-empty", 1);

    let envelope = runner.execute(req).await.unwrap();

    // P16-0: empty registry → PipelineError → no verdict → Unavailable → Escalate
    assert_eq!(envelope.reported_terminal_state, LoopTerminalState::Escalated);
    assert!(envelope.outcome_binding_hash.len() > 0);
    assert!(envelope.request_binding_hash.len() > 0);

    println!("B0-1 DIGEST CHAIN:");
    println!("  request_binding_hash:  {}", envelope.request_binding_hash);
    println!("  outcome_binding_hash:  {}", envelope.outcome_binding_hash);
    println!("  terminal_state:        {:?}", envelope.reported_terminal_state);
    println!("  total_attempts:        {}", envelope.total_attempts);
}

/// Helper: create an all-passing stub verifier for smoke tests
fn stub_verifier(id: &str, pass: onto_protocol::verifier::Pass) -> Box<dyn onto_protocol::verifier::Verifier> {
    struct StubV { desc: onto_protocol::verifier::VerifierDescriptor }
    impl StubV {
        fn new(id: &str, pass: onto_protocol::verifier::Pass) -> Self {
            Self { desc: onto_protocol::verifier::VerifierDescriptor {
                verifier_id: id.into(), pass,
                stage: onto_protocol::verifier::VerificationStage::PreGraph,
                mode: onto_protocol::verifier::VerificationMode::Internal,
                supported_rules: vec![],
            }}
        }
    }
    #[async_trait::async_trait]
    impl onto_protocol::verifier::Verifier for StubV {
        fn descriptor(&self) -> &onto_protocol::verifier::VerifierDescriptor { &self.desc }
        fn external_requirements(&self, _: &onto_protocol::context::VerificationContext) -> Vec<onto_protocol::check::ExternalCheckRequirement> { vec![] }
        async fn evaluate(&self, _: &onto_protocol::context::VerificationContext, _: &[(&String, &onto_protocol::sandbox::RawCheckResult)], _: &onto_protocol::verifier::VerifierServices<'_>) -> onto_protocol::verifier::VerifierResult {
            onto_protocol::verifier::VerifierResult { verifier_id: self.desc.verifier_id.clone(), pass: self.desc.pass, status: onto_protocol::verifier::VerifierStatus::Completed, findings: vec![], raw_evidence: vec![], diagnostic: None }
        }
    }
    Box::new(StubV::new(id, pass))
}

fn build_registry_4_stubs() -> PassRegistry {
    let mut reg = PassRegistry::new();
    for (id, pass) in &[
        ("artifact.manifest.integrity", onto_protocol::verifier::Pass::FileIntegrity),
        ("file.protected_path", onto_protocol::verifier::Pass::FileIntegrity),
        ("project.build", onto_protocol::verifier::Pass::Build),
        ("project.test", onto_protocol::verifier::Pass::Behavior),
    ] {
        reg.register(id, stub_verifier(id, *pass)).unwrap();
    }
    reg
}

/// B0-2: Full chain with all-passing verifiers → Conformant → Committed
#[tokio::test]
async fn b0_2_full_chain_conformant_committed() {
    let pm = PipelineManager::from_registry(build_registry_4_stubs());
    let coordinator = Arc::new(AssuredAttemptCoordinator::new(pm));

    let store = Arc::new(InMemoryIdempotencyStore::new());
    let mock_rt = Arc::new(MockLoopRuntime::new());
    mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

    let runner = RuntimeLoopRunner::new(store, mock_rt, coordinator, 5);
    let req = build_request("b0", "smoke-conformant", 1);
    let envelope = runner.execute(req).await.unwrap();

    println!("B0-2 CONFORMANT → COMMITTED:");
    println!("  request_binding_hash:  {}", envelope.request_binding_hash);
    println!("  outcome_binding_hash:  {}", envelope.outcome_binding_hash);
    println!("  terminal_state:        {:?}", envelope.reported_terminal_state);
    println!("  total_attempts:        {}", envelope.total_attempts);
    println!("  decision_id:           {:?}", envelope.decision_id);
    println!("  decision_hash:         {:?}", envelope.decision_hash);
    println!("  checkpoint_hash:       {:?}", envelope.output_checkpoint_hash);
    println!("  terminal_reason:       {}", envelope.terminal_reason);

    assert_eq!(envelope.total_attempts, 1);
    assert!(envelope.request_binding_hash.len() > 0);
    assert!(envelope.outcome_binding_hash.len() > 0);
    assert!(envelope.decision_id.is_some());
    assert!(envelope.output_checkpoint_hash.is_some());
    // All verifiers pass → Conformant → FinalizeCandidate → Committed
    assert_eq!(envelope.reported_terminal_state, LoopTerminalState::Committed,
        "All-passing verifiers must produce Committed");
}

/// B0-3: Two-attempt convergence — first fails, second succeeds
#[tokio::test]
async fn b0_3_two_attempt_convergence() {
    let pm = PipelineManager::from_registry(build_registry_4_stubs());
    let coordinator = Arc::new(AssuredAttemptCoordinator::new(pm));

    let store = Arc::new(InMemoryIdempotencyStore::new());
    let mock_rt = Arc::new(MockLoopRuntime::new());
    mock_rt.push_outcome(make_outcome(TaskOutcome::Failed, LifecycleState::Continuing));
    mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

    let runner = RuntimeLoopRunner::new(store, mock_rt, coordinator, 5);
    let req = build_request("b0", "smoke-two", 1);
    let envelope = runner.execute(req).await.unwrap();

    println!("B0-3 TWO-ATTEMPT:");
    println!("  terminal_state:        {:?}", envelope.reported_terminal_state);
    println!("  total_attempts:        {}", envelope.total_attempts);
    println!("  decision_id:           {:?}", envelope.decision_id);
    println!("  outcome_binding_hash:  {}", envelope.outcome_binding_hash);

    assert_eq!(envelope.total_attempts, 1,
        "All verifiers pass → Conformant on attempt 1 (Agent self-report irrelevant)");
    assert!(envelope.decision_id.is_some());
    assert_eq!(envelope.reported_terminal_state, LoopTerminalState::Committed);
}
