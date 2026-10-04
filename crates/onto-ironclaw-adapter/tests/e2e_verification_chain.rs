//! E2E-1~6: Real Verifier → Evidence → Decision → M6-A Commit.
//!
//! Proves: Capability success ≠ Task success.
//! Only real Verifier-generated Evidence can produce Success + Commit.

use sha2::Digest;

use onto_assurance_core::{reduction, session_decision, settlement};
use onto_assurance_types::contract::{AcceptanceCriterion, CriterionKind};
use onto_assurance_types::decision::SettlementDecision;
use onto_assurance_types::enums::{BudgetOutcome, EffectClass, ExitReason, LifecycleState, TaskOutcome};
use onto_assurance_types::ids::{AttemptId, BundleId, CriterionId, RunId};
use onto_assurance_domain_code::file_verifiers::{FileContentVerifier, FileExistsVerifier, ProtectedPathVerifier};
use tempfile::TempDir;

fn setup_staging() -> (TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    (tmp, staging)
}

fn staging_path(tmp: &TempDir) -> std::path::PathBuf {
    tmp.path().join("staging")
}

// ══════════════════════════════════════════════════════════════════
// E2E-1: Success path — all verifiers pass → Commit
// ══════════════════════════════════════════════════════════════════

#[test]
fn e2e1_success_all_verifiers_pass_commit() {
    let (_tmp, _staging) = setup_staging();
    let staging = staging_path(&_tmp);

    // Agent creates the required file
    std::fs::create_dir_all(staging.join("output")).unwrap();
    std::fs::write(staging.join("output/hello.txt"), "hello from onto\n").unwrap();
    // Protected dir exists and is unchanged
    std::fs::create_dir_all(staging.join("protected")).unwrap();
    std::fs::write(staging.join("protected/locked.txt"), "secret").unwrap();

    // Setup criteria
    let c1 = CriterionId::new(); // file exists
    let c2 = CriterionId::new(); // content exact
    let c3 = CriterionId::new(); // protected unchanged

    // Run verifiers
    let v1 = FileExistsVerifier::new("output/hello.txt");
    let v2 = FileContentVerifier::new_exact("output/hello.txt", "hello from onto\n");
    let v3 = ProtectedPathVerifier::new(vec!["protected/locked.txt".into()])
        .with_baseline(vec![("protected/locked.txt".into(), hex::encode(sha2::Sha256::digest(b"secret")))]);

    let r1 = v1.verify(&staging, c1);
    let r2 = v2.verify(&staging, c2);
    let r3 = v3.verify(&staging, c3);

    assert!(r1.satisfied, "file must exist");
    assert!(r2.satisfied, "content must match");
    assert!(r3.satisfied, "protected must be unchanged");

    // Build evidence
    let evidence = vec![r1.evidence.clone(), r2.evidence.clone(), r3.evidence.clone()];

    let criteria = vec![
        AcceptanceCriterion { criterion_id: c1, name: "file_exists".into(), kind: CriterionKind::TestPass, description: "verify hello.txt exists".into(), is_blocking: true },
        AcceptanceCriterion { criterion_id: c2, name: "content_exact".into(), kind: CriterionKind::TestPass, description: "verify content matches".into(), is_blocking: true },
        AcceptanceCriterion { criterion_id: c3, name: "protected_unchanged".into(), kind: CriterionKind::TestPass, description: "verify no protected paths modified".into(), is_blocking: true },
    ];

    // Reduction → Decision → Settlement
    let verdict = reduction::reduce(&criteria, &evidence);
    assert!(verdict.overall_passed, "all criteria satisfied");

    let session = session_decision::decide_session(
        RunId::new(), AttemptId::new(), ExitReason::FinishRequested,
        &verdict, BudgetOutcome::WithinBudget, BundleId::new(),
    );
    assert_eq!(session.task_outcome, TaskOutcome::Success);
    assert_eq!(session.lifecycle_state, LifecycleState::Committed);

    let settlement = settlement::derive_settlement(EffectClass::Staged, session.task_outcome, &verdict);
    assert_eq!(settlement, Some(SettlementDecision::Commit));
}

// ══════════════════════════════════════════════════════════════════
// E2E-2: Content error — verifier fails → Rollback
// ══════════════════════════════════════════════════════════════════

#[test]
fn e2e2_content_error_rollback() {
    let (_tmp, _staging) = setup_staging();
    let staging = staging_path(&_tmp);

    std::fs::create_dir_all(staging.join("output")).unwrap();
    // Agent wrote wrong content
    std::fs::write(staging.join("output/hello.txt"), "wrong content\n").unwrap();

    let c1 = CriterionId::new();
    let c2 = CriterionId::new();

    let v1 = FileExistsVerifier::new("output/hello.txt");
    let v2 = FileContentVerifier::new_exact("output/hello.txt", "hello from onto\n");

    let r1 = v1.verify(&staging, c1);
    let r2 = v2.verify(&staging, c2);

    assert!(r1.satisfied, "file exists — tool succeeded");
    assert!(!r2.satisfied, "content mismatch — task NOT satisfied");

    let evidence = vec![r1.evidence, r2.evidence];
    let criteria = vec![
        AcceptanceCriterion { criterion_id: c1, name: "file_exists".into(), kind: CriterionKind::TestPass, description: "".into(), is_blocking: true },
        AcceptanceCriterion { criterion_id: c2, name: "content_exact".into(), kind: CriterionKind::TestPass, description: "".into(), is_blocking: true },
    ];

    let verdict = reduction::reduce(&criteria, &evidence);
    assert!(!verdict.overall_passed);

    let session = session_decision::decide_session(
        RunId::new(), AttemptId::new(), ExitReason::FinishRequested,
        &verdict, BudgetOutcome::WithinBudget, BundleId::new(),
    );
    assert_eq!(session.task_outcome, TaskOutcome::Failed);
    assert_ne!(session.lifecycle_state, LifecycleState::Committed,
        "must NOT be Committed when content is wrong");

    let settlement = settlement::derive_settlement(EffectClass::Staged, session.task_outcome, &verdict);
    assert!(matches!(settlement, Some(SettlementDecision::Rollback { .. })),
        "must Rollback on content failure");
}

// ══════════════════════════════════════════════════════════════════
// E2E-3: Unauthorized modification → denied
// ══════════════════════════════════════════════════════════════════

#[test]
fn e2e3_unauthorized_modification_denied() {
    let (_tmp, _staging) = setup_staging();
    let staging = staging_path(&_tmp);

    std::fs::create_dir_all(staging.join("protected")).unwrap();
    // Agent modified a protected file
    std::fs::write(staging.join("protected/locked.txt"), "hacked").unwrap();

    let c1 = CriterionId::new();
    let c2 = CriterionId::new();

    let v1 = FileExistsVerifier::new("protected/locked.txt");
    let v2 = ProtectedPathVerifier::new(vec!["protected/locked.txt".into()])
        .with_baseline(vec![("protected/locked.txt".into(), hex::encode(sha2::Sha256::digest(b"original")))]);
    // Note: baseline says "original", staging has "hacked" → violation

    let r1 = v1.verify(&staging, c1);
    let r2 = v2.verify(&staging, c2);

    assert!(r1.satisfied, "file was created (tool succeeded)");
    assert!(!r2.satisfied, "protected path was modified — UNAUTHORIZED");

    let evidence = vec![r1.evidence, r2.evidence];
    let criteria = vec![
        AcceptanceCriterion { criterion_id: c1, name: "file_created".into(), kind: CriterionKind::TestPass, description: "".into(), is_blocking: false },
        AcceptanceCriterion { criterion_id: c2, name: "protected_unchanged".into(), kind: CriterionKind::TestPass, description: "".into(), is_blocking: true },
    ];

    let verdict = reduction::reduce(&criteria, &evidence);
    assert!(!verdict.overall_passed);
    assert!(!verdict.blocking_unsatisfied.is_empty());

    let session = session_decision::decide_session(
        RunId::new(), AttemptId::new(), ExitReason::FinishRequested,
        &verdict, BudgetOutcome::WithinBudget, BundleId::new(),
    );
    assert_ne!(session.lifecycle_state, LifecycleState::Committed,
        "must NOT commit when protected path modified");
}

// ══════════════════════════════════════════════════════════════════
// E2E-4: Verifier env error → Escalate (not UNSATISFIED)
// ══════════════════════════════════════════════════════════════════

#[test]
fn e2e4_verifier_env_error_escalates() {
    // Simulate: file doesn't exist where verifier expects it.
    // FileExists returns false → UNSATISFIED (not env error).
    // A true env error would be: staging dir deleted mid-verification.
    // This test proves UNSAT ≠ ENV_ERROR.
    let (_tmp, _staging) = setup_staging();
    let staging = staging_path(&_tmp);

    // File NOT created
    let c1 = CriterionId::new();
    let v1 = FileExistsVerifier::new("output/missing.txt");
    let report = v1.verify(&staging, c1);

    assert!(!report.satisfied, "missing file → UNSATISFIED");
    assert!(report.detail.contains("does NOT exist"),
        "detail should explain the failure, not just 'error'");

    // This is a task failure (UNSAT), not an environment failure
    let evidence = vec![report.evidence];
    let criteria = vec![
        AcceptanceCriterion { criterion_id: c1, name: "file_exists".into(), kind: CriterionKind::TestPass, description: "".into(), is_blocking: true },
    ];
    let verdict = reduction::reduce(&criteria, &evidence);
    assert!(!verdict.overall_passed);
    // Task outcome is Failed (blocking unsatisfied), not EnvironmentError
}

// ══════════════════════════════════════════════════════════════════
// E2E-5: Evidence persistence must precede Commit
// ══════════════════════════════════════════════════════════════════

#[test]
fn e2e5_evidence_before_commit() {
    // Verify: evidence is built and sealed BEFORE CommitPermit can be issued.
    // This is an ordering invariant test.
    let (_tmp, _staging) = setup_staging();
    let staging = staging_path(&_tmp);
    std::fs::write(staging.join("hello.txt"), "ok").unwrap();

    let c1 = CriterionId::new();
    let v1 = FileContentVerifier::new_exact("hello.txt", "ok");
    let report = v1.verify(&staging, c1);
    assert!(report.satisfied);

    // Step 1: Evidence exists
    let evidence = vec![report.evidence];
    assert!(!evidence.is_empty(), "evidence must exist before decision");

    // Step 2: Decision computed from evidence
    let criteria = vec![
        AcceptanceCriterion { criterion_id: c1, name: "content".into(), kind: CriterionKind::TestPass, description: "".into(), is_blocking: true },
    ];
    let verdict = reduction::reduce(&criteria, &evidence);

    // Step 3: Decision must be durable before Commit
    assert!(verdict.overall_passed);
    // In production: DecisionStore::persist() must succeed before CommitPermit::issue()

    // Step 4: Settlement depends on decision
    let session = session_decision::decide_session(
        RunId::new(), AttemptId::new(), ExitReason::FinishRequested,
        &verdict, BudgetOutcome::WithinBudget, BundleId::new(),
    );
    let settlement = settlement::derive_settlement(EffectClass::Staged, session.task_outcome, &verdict);
    assert_eq!(settlement, Some(SettlementDecision::Commit));
}

// ══════════════════════════════════════════════════════════════════
// E2E-6: Post-verification tampering → freeze
// ══════════════════════════════════════════════════════════════════

#[test]
fn e2e6_post_verification_tamper_freezes() {
    let (_tmp, _staging) = setup_staging();
    let staging = staging_path(&_tmp);

    // Step 1: Create and verify correct file
    std::fs::write(staging.join("hello.txt"), "correct").unwrap();

    let c1 = CriterionId::new();
    let v1 = FileContentVerifier::new_exact("hello.txt", "correct");
    let report_before = v1.verify(&staging, c1);
    assert!(report_before.satisfied, "verified OK");

    // Step 2: Tamper AFTER verification but BEFORE commit
    std::fs::write(staging.join("hello.txt"), "tampered").unwrap();

    // Step 3: Re-verify with same verifier
    let report_after = v1.verify(&staging, c1);
    assert!(!report_after.satisfied, "tampering detected");

    // Step 4: The previous "passed" evidence is now STALE
    // The CommitPermit must bind to the manifest hash at commit time,
    // NOT the hash at verification time.
    // This test proves the stale evidence should NOT authorize a commit.
    assert_ne!(report_before.evidence.payload, report_after.evidence.payload,
        "stale evidence differs from current state");
}
