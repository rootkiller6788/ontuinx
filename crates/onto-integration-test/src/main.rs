//! Onto Assurance Kernel — Integration Test Runner
//!
//! Run: cargo run --package onto-integration-test
//!
//! Tests the full Onto pipeline without OntoRuntime dependencies:
//!   ShadowObserver → BuiltinGate → Code Pack → Evidence → Decisions
//!
//! When --features ironclaw-integration is enabled (inside OntoRuntime workspace),
//! also registers as a Builtin hook and compares shadow decisions.

use onto_assurance_core::evidence_chain::EvidenceChain;
use onto_assurance_core::{reduction, session_decision, settlement};
use onto_assurance_types::contract::{
    AcceptanceCriterion, ApprovalMode, CriterionKind, ExecutionContract,
};
use onto_assurance_types::enums::{
    BudgetOutcome, EffectClass, ExitReason, LifecycleState, TaskOutcome,
};
use onto_assurance_types::evidence::{EvidenceRecord, EvidenceRecordKind, VerifierBinding};
use onto_assurance_types::ids::{
    AttemptId, ContractId, CriterionId, IntentId, RunId, TransactionId, VerifierId,
};
use onto_ironclaw_adapter::builtin_bridge::{BuiltinGate, ShadowObserver};
use onto_assurance_domain_code::project_profiler::Language;

fn main() {
    let mut passed = 0u32;
    let mut failed = 0u32;

    macro_rules! check {
        ($name:expr, $cond:expr) => {
            if $cond {
                passed += 1;
                println!("  ✅ {}", $name);
            } else {
                failed += 1;
                println!("  ❌ {}", $name);
            }
        };
    }

    println!("Onto Assurance Kernel — Integration Tests");
    println!("===========================================\n");

    // ── 1. ShadowObserver tests ──
    println!("--- ShadowObserver ---");
    let observer = ShadowObserver::default();
    let obs = observer.observe_capability("file_write", "");
    check!("file_write is Staged", obs.effect_class == EffectClass::Staged);
    check!("file_write no approval needed", !obs.requires_approval);

    let obs = observer.observe_capability("deploy", "kubectl apply");
    check!("deploy is Irreversible", obs.effect_class == EffectClass::Irreversible);
    check!("deploy requires approval", obs.requires_approval);

    let obs = observer.observe_capability("file_read", "/etc/passwd");
    check!("file_read is ReadOnly", obs.effect_class == EffectClass::ReadOnly);

    let obs = observer.observe_capability("send_email", "user@example.com");
    check!("send_email is Irreversible", obs.effect_class == EffectClass::Irreversible);

    let obs = observer.observe_capability("unknown_tool", "--dangerous");
    check!("unknown is Irreversible (fail-closed)", obs.effect_class == EffectClass::Irreversible);

    // ── 2. BuiltinGate tests ──
    println!("\n--- BuiltinGate ---");
    let contract = ExecutionContract {
        contract_id: ContractId::new(), intent_id: IntentId::new(),
        criteria: vec![AcceptanceCriterion {
            criterion_id: CriterionId::new(), name: "build".into(),
            kind: CriterionKind::TestPass, description: "must compile".into(),
            is_blocking: true,
        }],
        evidence_required: vec![], approval_mode: ApprovalMode::OnHighRisk,
        max_attempts: 3, created_at: chrono::Utc::now(),
    };
    let gate = BuiltinGate::default();

    let dec = gate.evaluate_before_capability("deploy", "", &contract);
    check!("deploy requires PauseApproval",
        matches!(dec, onto_ironclaw_adapter::builtin_bridge::OntoGateDecision::PauseApproval { .. }));

    let dec = gate.evaluate_before_capability("file_write", "", &contract);
    check!("file_write is Allow",
        matches!(dec, onto_ironclaw_adapter::builtin_bridge::OntoGateDecision::Allow));

    let dec = gate.evaluate_settlement(
        TaskOutcome::Success, EffectClass::Staged,
        &onto_assurance_types::decision::SettlementDecision::Commit,
    );
    check!("monotonic: Commit on Success → Commit",
        dec == onto_assurance_types::decision::SettlementDecision::Commit);

    let dec = gate.evaluate_settlement(
        TaskOutcome::Failed, EffectClass::Staged,
        &onto_assurance_types::decision::SettlementDecision::Commit,
    );
    check!("monotonic: Commit on Failed → Escalate",
        matches!(dec, onto_assurance_types::decision::SettlementDecision::Escalate { .. }));

    // ── 3. Code Pack project profiler ──
    println!("\n--- Code Pack ---");
    let files = vec!["main.c".into(), "kvdb.c".into(), "kvdb.h".into(), "Makefile".into()];
    let profile = onto_assurance_domain_code::project_profiler::profile_project(&files);
    check!("C project detected", profile.language == Language::C);
    check!("Make build system", matches!(profile.build_system, onto_assurance_domain_code::project_profiler::BuildSystem::Make));
    check!("C projects use CTest", matches!(profile.test_framework, onto_assurance_domain_code::project_profiler::TestFramework::CTest));

    // ── 4. Evidence chain ──
    println!("\n--- Evidence Chain ---");
    let run_id = RunId::new();
    let mut chain = EvidenceChain::new(
        run_id, "genesis".into(),
        VerifierBinding {
            verifier_id: VerifierId::new(), verifier_version: "1.0.0".into(),
            toolchain: Some("gcc".into()), environment_hash: None,
        },
    );
    let rec1 = EvidenceRecord {
        evidence_id: onto_assurance_types::ids::EvidenceId::new(),
        transaction_id: TransactionId::new(),
        criterion_id: CriterionId::new(),
        kind: EvidenceRecordKind::TestOutput,
        payload: serde_json::json!({"passed": true, "tests": 9}),
        recorded_at: chrono::Utc::now(),
    };
    chain.append(rec1).expect("append 1 OK");
    let rec2 = EvidenceRecord {
        evidence_id: onto_assurance_types::ids::EvidenceId::new(),
        transaction_id: TransactionId::new(),
        criterion_id: CriterionId::new(),
        kind: EvidenceRecordKind::VerifierReport,
        payload: serde_json::json!({"passed": true}),
        recorded_at: chrono::Utc::now(),
    };
    chain.append(rec2).expect("append 2 OK");
    let bundle = chain.seal();
    check!("evidence chain has 2 records", bundle.record_count == 2);
    check!("chain indexes start at 1", bundle.records[0].chain_index == 1);
    check!("chain indexes increment", bundle.records[1].chain_index == 2);
    check!("chain integrity passes",
        onto_assurance_core::evidence_chain::EvidenceChain::verify(&bundle).is_ok());

    // ── 5. Full pipeline ──
    println!("\n--- Pipeline ---");
    let criteria = vec![
        AcceptanceCriterion { criterion_id: CriterionId::new(), name: "build".into(),
            kind: CriterionKind::TestPass, description: "".into(), is_blocking: true },
        AcceptanceCriterion { criterion_id: CriterionId::new(), name: "tests".into(),
            kind: CriterionKind::TestPass, description: "".into(), is_blocking: true },
    ];
    let evidence = criteria.iter().map(|c| EvidenceRecord {
        evidence_id: onto_assurance_types::ids::EvidenceId::new(),
        transaction_id: TransactionId::new(),
        criterion_id: c.criterion_id,
        kind: EvidenceRecordKind::VerifierReport,
        payload: serde_json::json!({"passed": true}),
        recorded_at: chrono::Utc::now(),
    }).collect::<Vec<_>>();

    let verdict = reduction::reduce(&criteria, &evidence);
    check!("reduction: all criteria satisfied", verdict.overall_passed);

    let session = session_decision::decide_session(
        run_id, AttemptId::new(), ExitReason::FinishRequested,
        &verdict, BudgetOutcome::WithinBudget, bundle.bundle_id,
    );
    check!("session: SUCCESS", session.task_outcome == TaskOutcome::Success);
    check!("session: COMMITTED", session.lifecycle_state == LifecycleState::Committed);

    let settlement = settlement::derive_settlement(
        EffectClass::Staged, session.task_outcome, &verdict,
    );
    check!("settlement: Commit", settlement == Some(onto_assurance_types::decision::SettlementDecision::Commit));

    // ── Summary ──
    println!("\n=========================================");
    println!("Result: {}/{} passed, {}/{} failed",
        passed, passed + failed, failed, passed + failed);

    if failed > 0 { std::process::exit(1); }
}
