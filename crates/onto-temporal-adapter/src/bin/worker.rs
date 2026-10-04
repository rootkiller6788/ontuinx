//! OntoLoop Worker — Real OntoFlow Activity Worker for execute_onto_loop tasks.
//!
//! I3: Accepts LoopInvocationRequest (JSON via stdin or file),
//! runs the full OntoLoop pipeline with real OntoRuntime/OntoAssure mocks,
//! and outputs LoopTerminalEnvelope.
//!
//! Usage:
//!   cargo run --bin onto-worker -- --input task.json
//!   cargo run --bin onto-worker -- --scenario two-attempt

use std::fs;
use std::sync::Arc;

use onto_assurance_runtime::mocks::MockLoopRuntime;
use onto_assurance_runtime::ports::RunFinalizationOutcome;
use onto_assurance_types::enums::{BudgetOutcome, LifecycleState, TaskOutcome};
use onto_assurance_types::ids::DecisionId;
use onto_temporal_adapter::idempotency::InMemoryIdempotencyStore;
use onto_temporal_adapter::protocol::{LoopInvocationRequest, LoopTerminalState};
use onto_temporal_adapter::RuntimeLoopRunner;
use onto_assurance_runtime::pipeline::PipelineManager;
use onto_temporal_adapter::assured_coordinator::AssuredAttemptCoordinator;

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

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scenario = if args.len() > 1 { &args[1] } else { "two-attempt" };

    println!("OntoLoop Worker v0.1 — I3 Real Multi-Attempt Execution");
    println!("=========================================================");

    match scenario {
        "two-attempt" => run_two_attempt_scenario().await,
        "success" => run_single_attempt_scenario().await,
        "input" => {
            let path = args.get(2).expect("usage: onto-worker input <file.json>");
            let json = fs::read_to_string(path).expect("read input file");
            let req: LoopInvocationRequest = serde_json::from_str(&json).expect("parse request");
            run_from_request(req).await;
        }
        _ => {
            eprintln!("Unknown scenario: {}", scenario);
            eprintln!("Usage: onto-worker [two-attempt|success|input <file>]");
        }
    }
}

/// I3 core scenario: Attempt 1 fails, Attempt 2 succeeds, OntoAssure commits.
async fn run_two_attempt_scenario() {
    println!("\n--- I3: Two-Attempt Convergence ---");

    let store = Arc::new(InMemoryIdempotencyStore::new());
    let mock_rt = Arc::new(MockLoopRuntime::new());

    // Attempt 1: Agent fails
    mock_rt.push_outcome(make_outcome(TaskOutcome::Failed, LifecycleState::Continuing));
    // Attempt 2: Agent succeeds + OntoAssure commits
    mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

    let req = build_request("i3-flow", "wi-code-task", 1);
    let req_hash = req.request_binding_hash.clone();
    let loop_id = req.loop_id.clone();

    println!("[WORKER] Received task: flow={} wi={} loop={}", req.flow_id, req.work_item_id, loop_id);
    println!("[WORKER] request_binding_hash={}", req_hash);

    // P16-1: Coordinator is mandatory. Use new() with explicit coordinator.
    let coordinator = Arc::new(AssuredAttemptCoordinator::new(PipelineManager::new()));
    let runner = RuntimeLoopRunner::new(store, mock_rt, coordinator, 5);

    let envelope = runner.execute(req).await.unwrap();

    println!("\n[WORKER] Loop complete:");
    println!("  reported_terminal_state: {:?}", envelope.reported_terminal_state);
    println!("  total_attempts: {}", envelope.total_attempts);
    println!("  decision_id: {:?}", envelope.decision_id);
    println!("  decision_hash: {:?}", envelope.decision_hash);
    println!("  output_checkpoint_hash: {:?}", envelope.output_checkpoint_hash);
    println!("  outcome_binding_hash: {}", envelope.outcome_binding_hash);
    println!("  request_binding_hash: {} (matches original: {})",
             envelope.request_binding_hash,
             envelope.request_binding_hash == req_hash);

    // P15: Fail-Closed — with PipelineManager mounted, Success→Escalate (no real Verifier registered)
    assert_eq!(envelope.total_attempts, 2, "I3-2: Must have 2 attempts");
    assert_eq!(envelope.reported_terminal_state, LoopTerminalState::Escalated,
               "P15: Fail-Closed without registered Verifiers");
    assert!(envelope.decision_id.is_some(), "I3-6: Decision ID must exist");
    assert!(envelope.output_checkpoint_hash.is_some(), "I3-4: Checkpoint hash must exist");
    assert_eq!(envelope.request_binding_hash, req_hash, "I3-8: Binding hash preserved");

    println!("\n✅ P15 ACCEPTANCE: Coordinator wired, Fail-Closed escalates on empty registry");

    // Output envelope as JSON for temporal server
    let json = serde_json::to_string_pretty(&envelope).unwrap();
    println!("\n=== LOOP_TERMINAL_ENVELOPE ===");
    println!("{}", json);
}

/// Single attempt success (fast path).
async fn run_single_attempt_scenario() {
    let store = Arc::new(InMemoryIdempotencyStore::new());
    let mock_rt = Arc::new(MockLoopRuntime::new());
    mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

    let req = build_request("i3-flow", "wi-fast", 1);
    let runner = RuntimeLoopRunner::with_empty_coordinator(store, mock_rt, 5);
    let envelope = runner.execute(req).await.unwrap();

    println!("  total_attempts: {}", envelope.total_attempts);
    println!("  reported_terminal_state: {:?}", envelope.reported_terminal_state);
    println!("  decision_id: {:?}", envelope.decision_id);
}

/// Run from a JSON request file.
async fn run_from_request(req: LoopInvocationRequest) {
    let store = Arc::new(InMemoryIdempotencyStore::new());
    let mock_rt = Arc::new(MockLoopRuntime::new());
    mock_rt.push_outcome(make_outcome(TaskOutcome::Failed, LifecycleState::Continuing));
    mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

    let runner = RuntimeLoopRunner::with_empty_coordinator(store, mock_rt, 5);
    let envelope = runner.execute(req).await.unwrap();
    println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
}

fn build_request(flow: &str, wi: &str, gen: u64) -> LoopInvocationRequest {
    let req = LoopInvocationRequest {
        schema_version: 1,
        flow_id: flow.to_string(),
        work_item_id: wi.to_string(),
        loop_id: format!("loop-{}-{}", flow, wi),
        task_spec_ref: "spec/write-hello-py".into(),
        contract_ref: "contract/default".into(),
        policy_ref: "policy/default".into(),
        input_artifact_refs: vec![],
        resource_class: "standard".into(),
        risk_class: "low".into(),
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
