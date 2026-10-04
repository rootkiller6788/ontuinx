//! Phase 3+4 Final Acceptance Tests — 3 golden scenarios.
//!
//! Proves complete closed loop:
//!   Tool execution → Evidence → Reduction → Decision → Commit/Escalate

use onto_assurance_core::evidence_chain::EvidenceChain;
use onto_assurance_core::{reduction, session_decision, settlement};
use onto_assurance_types::contract::{AcceptanceCriterion, CriterionKind, ExecutionContract};
use onto_assurance_types::decision::SettlementDecision;
use onto_assurance_types::enums::{
    BudgetOutcome, EffectClass, ExitReason, LifecycleState, TaskOutcome,
};
use onto_assurance_types::evidence::{
    EvidenceRecord, EvidenceRecordKind, RequirementVerdict, VerifierBinding,
};
use onto_assurance_types::ids::{
    AttemptId, BundleId, ContractId, CriterionId, EvidenceId, IntentId, RunId,
    TransactionId, VerifierId,
};

fn make_criterion(id: CriterionId, name: &str, blocking: bool) -> AcceptanceCriterion {
    AcceptanceCriterion {
        criterion_id: id, name: name.into(), kind: CriterionKind::TestPass,
        description: format!("verify {}", name), is_blocking: blocking,
    }
}

fn make_evidence(criterion_id: CriterionId, passed: bool, detail: &str) -> EvidenceRecord {
    EvidenceRecord {
        evidence_id: EvidenceId::new(),
        transaction_id: TransactionId::new(),
        criterion_id,
        kind: EvidenceRecordKind::VerifierReport,
        payload: serde_json::json!({"passed": passed, "detail": detail}),
        recorded_at: chrono::Utc::now(),
    }
}

// ══════════════════════════════════════════════════════════════════
// Scenario 1: Normal — all criteria satisfied → COMMIT
// ══════════════════════════════════════════════════════════════════

#[test]
fn golden_normal_commit() {
    let file_exists = CriterionId::new();
    let content_correct = CriterionId::new();

    let criteria = vec![
        make_criterion(file_exists, "hello.py exists", true),
        make_criterion(content_correct, "hello() returns 'Hello World'", true),
    ];

    // Agent created the file correctly
    let evidence = vec![
        make_evidence(file_exists, true, "file hello.py found at expected path"),
        make_evidence(content_correct, true, "function returns 'Hello World'"),
    ];

    let verdict = reduction::reduce(&criteria, &evidence);
    assert!(verdict.overall_passed);
    assert!(verdict.blocking_unsatisfied.is_empty());

    let session = session_decision::decide_session(
        RunId::new(), AttemptId::new(), ExitReason::FinishRequested,
        &verdict, BudgetOutcome::WithinBudget, BundleId::new(),
    );
    assert_eq!(session.task_outcome, TaskOutcome::Success);
    assert_eq!(session.lifecycle_state, LifecycleState::Committed);

    let settle = settlement::derive_settlement(EffectClass::Staged, session.task_outcome, &verdict);
    assert_eq!(settle, Some(SettlementDecision::Commit));
}

// ══════════════════════════════════════════════════════════════════
// Scenario 2: Code error — content wrong → ESCALATE/FAILED
// ══════════════════════════════════════════════════════════════════

#[test]
fn golden_code_error_escalate() {
    let file_exists = CriterionId::new();
    let content_correct = CriterionId::new();

    let criteria = vec![
        make_criterion(file_exists, "hello.py exists", true),
        make_criterion(content_correct, "hello() returns 'Hello World'", true),
    ];

    // Agent created file but with wrong content
    let evidence = vec![
        make_evidence(file_exists, true, "file hello.py found"),
        make_evidence(content_correct, false, "function returns 'Goodbye' instead of 'Hello World'"),
    ];

    let verdict = reduction::reduce(&criteria, &evidence);
    assert!(!verdict.overall_passed);
    assert!(!verdict.blocking_unsatisfied.is_empty());

    let session = session_decision::decide_session(
        RunId::new(), AttemptId::new(), ExitReason::FinishRequested,
        &verdict, BudgetOutcome::WithinBudget, BundleId::new(),
    );
    assert_eq!(session.task_outcome, TaskOutcome::Failed);
    // Failed → Continuing (needs retry) or Escalated
    assert_ne!(session.lifecycle_state, LifecycleState::Committed);

    let settle = settlement::derive_settlement(EffectClass::Staged, session.task_outcome, &verdict);
    assert!(matches!(settle, Some(SettlementDecision::Rollback { .. })));
}

// ══════════════════════════════════════════════════════════════════
// Scenario 3: Unauthorized modification → DENIED/ESCALATE
// ══════════════════════════════════════════════════════════════════

#[test]
fn golden_unauthorized_denied() {
    let protected_file = CriterionId::new();
    let no_protected_modified = CriterionId::new();

    let criteria = vec![
        make_criterion(protected_file, "protected file untouched", true),
        make_criterion(no_protected_modified, "only hello.py modified", true),
    ];

    // Agent tried to modify protected file
    let evidence = vec![
        make_evidence(protected_file, false, "protected file was modified — UNAUTHORIZED"),
        make_evidence(no_protected_modified, true, "only hello.py changed"),
    ];

    let verdict = reduction::reduce(&criteria, &evidence);
    assert!(!verdict.overall_passed);
    assert_eq!(verdict.blocking_unsatisfied.len(), 1);
    assert_eq!(verdict.blocking_unsatisfied[0], protected_file);

    let session = session_decision::decide_session(
        RunId::new(), AttemptId::new(), ExitReason::FinishRequested,
        &verdict, BudgetOutcome::WithinBudget, BundleId::new(),
    );
    assert_eq!(session.task_outcome, TaskOutcome::Failed);
    assert_ne!(session.lifecycle_state, LifecycleState::Committed);
}

// ══════════════════════════════════════════════════════════════════
// Evidence Chain Integrity (regression from M5)
// ══════════════════════════════════════════════════════════════════

#[test]
fn evidence_chain_verifies_correctly() {
    let run_id = RunId::new();
    let mut chain = EvidenceChain::new(
        run_id, "genesis".into(),
        VerifierBinding {
            verifier_id: VerifierId::new(), verifier_version: "1.0".into(),
            toolchain: None, environment_hash: None,
        },
    );
    let cid = CriterionId::new();
    chain.append(make_evidence(cid, true, "ok")).unwrap();
    let bundle = chain.seal();
    EvidenceChain::verify(&bundle).unwrap();
}

#[test]
fn tampered_evidence_detected() {
    let run_id = RunId::new();
    let mut chain = EvidenceChain::new(
        run_id, "genesis".into(),
        VerifierBinding {
            verifier_id: VerifierId::new(), verifier_version: "1.0".into(),
            toolchain: None, environment_hash: None,
        },
    );
    let cid = CriterionId::new();
    chain.append(make_evidence(cid, true, "ok")).unwrap();
    let mut bundle = chain.seal();
    // Tamper
    bundle.records[0].record.payload = serde_json::json!({"passed": false});
    assert!(EvidenceChain::verify(&bundle).is_err());
}
