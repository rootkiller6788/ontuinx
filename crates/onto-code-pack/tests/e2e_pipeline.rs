//! End-to-end pipeline test — full Onto Assurance lifecycle with Code Pack.
//!
//! Simulates a C project (KVDB) going through:
//!   Profile → Build → Test → Evidence Chain → Reduction → Session Decision → Settlement

use onto_assurance_core::effect_classifier::{EffectClassifier, RulesBasedClassifier};
use onto_assurance_core::evidence_chain::EvidenceChain;
use onto_assurance_core::reduction;
use onto_assurance_core::session_decision;
use onto_assurance_core::settlement;
use onto_assurance_types::contract::{
    AcceptanceCriterion, ApprovalMode, CriterionKind, ExecutionContract,
};
use onto_assurance_types::decision::SettlementDecision;
use onto_assurance_types::enums::{
    BudgetOutcome, EffectClass, ExitReason, LifecycleState, TaskOutcome,
};
use onto_assurance_types::evidence::{EvidenceRecord, EvidenceRecordKind, VerifierBinding};
use onto_assurance_types::ids::{
    AttemptId, ContractId, CriterionId, IntentId, RunId, TransactionId, VerifierId,
};
use onto_code_pack::build_verifier::{CommandOutput, CommandRunner, CommandError, BuildVerifier};
use onto_code_pack::test_verifier::TestVerifier;
use onto_code_pack::project_profiler::profile_project;
use onto_pack_sdk::PackVerifier;

// ══════════════════════════════════════════════════════════════════
// Multi-command mock runner for E2E testing
// ══════════════════════════════════════════════════════════════════

struct E2EMockRunner {
    build_result: CommandOutput,
    test_result: CommandOutput,
}

impl CommandRunner for E2EMockRunner {
    fn run(&self, command: &str, _args: &[&str], _dir: &str) -> Result<CommandOutput, CommandError> {
        // gcc/make → build result; cargo/pytest/ctest → test result
        if command.contains("cargo") || command.contains("pytest") || command.contains("ctest") || command.contains("test") {
            Ok(self.test_result.clone())
        } else {
            Ok(self.build_result.clone())
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// KVDB E2E: C project, 9 tests, all pass → Commit
// ══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn kvdb_c_project_full_success_pipeline() {
    // ── 1. Profile the project ──
    let files = vec![
        "main.c".to_string(), "kvdb.c".to_string(), "kvdb.h".to_string(),
        "test_kvdb.c".to_string(), "Makefile".to_string(),
    ];
    let profile = profile_project(&files);
    assert_eq!(profile.language, onto_code_pack::project_profiler::Language::C);
    assert_eq!(profile.build_system, onto_code_pack::project_profiler::BuildSystem::Make);
    assert_eq!(profile.source_files.len(), 4); // main.c + kvdb.c + test_kvdb.c + kvdb.h

    // ── 2. Classify effect ──
    let classifier = RulesBasedClassifier::new();
    let classification = classifier.classify(
        "file_write", "",
        None,
        onto_assurance_types::enums::RiskLevel::Medium,
    );
    assert_eq!(classification.effect_class, EffectClass::Staged);

    // ── 3. Build contract ──
    let contract = ExecutionContract {
        contract_id: ContractId::new(),
        intent_id: IntentId::new(),
        criteria: vec![
            AcceptanceCriterion {
                criterion_id: CriterionId::new(),
                name: "build".into(),
                kind: CriterionKind::TestPass,
                description: "project must compile without errors".into(),
                is_blocking: true,
            },
            AcceptanceCriterion {
                criterion_id: CriterionId::new(),
                name: "tests".into(),
                kind: CriterionKind::TestPass,
                description: "all 9 KVDB tests must pass".into(),
                is_blocking: true,
            },
        ],
        evidence_required: vec![],
        approval_mode: ApprovalMode::OnHighRisk,
        max_attempts: 3,
        created_at: chrono::Utc::now(),
    };

    // ── 4. Run build verifier ──
    let mock_runner = Box::new(E2EMockRunner {
        build_result: CommandOutput {
            exit_code: 0, stdout: "gcc -o kvdb main.c kvdb.c\nBuild OK".into(),
            stderr: String::new(), duration_ms: 1500,
        },
        test_result: CommandOutput {
            exit_code: 0,
            stdout: "test result: ok. 9 passed; 0 failed; 0 ignored; finished in 0.05s".into(),
            stderr: String::new(), duration_ms: 800,
        },
    });

    let build_verifier = BuildVerifier::new("code-build", "gcc", "gcc", mock_runner);
    let build_report = build_verifier.verify(
        TransactionId::new(), "/workspace/kvdb",
        &contract.criteria[..1],
    ).await.unwrap();
    assert!(build_report.passed);

    // ── 5. Run test verifier ──
    let test_runner = E2EMockRunner {
        build_result: CommandOutput { exit_code: 0, stdout: String::new(), stderr: String::new(), duration_ms: 0 },
        test_result: CommandOutput {
            exit_code: 0,
            stdout: "test result: ok. 9 passed; 0 failed; 0 ignored; finished in 0.05s".into(),
            stderr: String::new(), duration_ms: 800,
        },
    };
    let test_verifier = TestVerifier::cargo_test(Box::new(test_runner));
    let test_report = test_verifier.verify(
        TransactionId::new(), "/workspace/kvdb",
        &contract.criteria[1..],
    ).await.unwrap();
    assert!(test_report.passed);
    assert_eq!(test_report.passed_checks, 9);

    // ── 6. Build evidence records ──
    let run_id = RunId::new();
    let txn_id = TransactionId::new();
    let mut evidence_records: Vec<EvidenceRecord> = Vec::new();

    for criterion in &contract.criteria {
        let record = EvidenceRecord {
            evidence_id: onto_assurance_types::ids::EvidenceId::new(),
            transaction_id: txn_id,
            criterion_id: criterion.criterion_id,
            kind: EvidenceRecordKind::VerifierReport,
            payload: serde_json::json!({"passed": true}),
            recorded_at: chrono::Utc::now(),
        };
        evidence_records.push(record);
    }

    // ── 7. Build evidence chain ──
    let mut chain = EvidenceChain::new(
        run_id,
        contract.contract_id.to_string(),
        VerifierBinding {
            verifier_id: VerifierId::new(),
            verifier_version: "1.0.0".into(),
            toolchain: Some("gcc".into()),
            environment_hash: None,
        },
    );
    for record in &evidence_records {
        chain.append(record.clone()).expect("append should succeed for unsealed chain");
    }
    let bundle = chain.seal();
    assert_eq!(bundle.record_count, 2);

    // Verify chain integrity
    onto_assurance_core::evidence_chain::EvidenceChain::verify(&bundle).unwrap();

    // ── 8. Reduce evidence ──
    let verdict = reduction::reduce(&contract.criteria, &evidence_records);
    assert!(verdict.overall_passed);
    assert!(verdict.blocking_unsatisfied.is_empty());

    // ── 9. Session decision ──
    let session = session_decision::decide_session(
        run_id, AttemptId::new(), ExitReason::FinishRequested,
        &verdict, BudgetOutcome::WithinBudget, bundle.bundle_id,
    );
    assert_eq!(session.task_outcome, TaskOutcome::Success);
    assert_eq!(session.budget_outcome, BudgetOutcome::WithinBudget);
    assert_eq!(session.lifecycle_state, LifecycleState::Committed);

    // ── 10. Settlement ──
    let settlement = settlement::derive_settlement(
        EffectClass::Staged,
        session.task_outcome,
        &verdict,
    );
    assert_eq!(settlement, Some(SettlementDecision::Commit));

    // ── 11. Verify replay hash ──
    let replay_input = onto_assurance_core::replay::ReplayInput {
        run_id: run_id.to_string(),
        contract: serde_json::to_value(&contract).unwrap(),
        transactions: vec![],
        checkpoint_bindings: vec![],
    };
    let replay_hash = onto_assurance_core::replay::compute_replay_hash(&replay_input).unwrap();
    assert!(!replay_hash.is_empty());
}

// ══════════════════════════════════════════════════════════════════
// Failure pipeline: build fails → Rollback
// ══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn build_failure_pipeline_rollback() {
    let contract = ExecutionContract {
        contract_id: ContractId::new(),
        intent_id: IntentId::new(),
        criteria: vec![AcceptanceCriterion {
            criterion_id: CriterionId::new(),
            name: "build".into(),
            kind: CriterionKind::TestPass,
            description: "must compile".into(),
            is_blocking: true,
        }],
        evidence_required: vec![],
        approval_mode: ApprovalMode::Never,
        max_attempts: 3,
        created_at: chrono::Utc::now(),
    };

    let mock = Box::new(E2EMockRunner {
        build_result: CommandOutput {
            exit_code: 1, stdout: String::new(),
            stderr: "error: undefined reference to 'kvdb_get'".into(),
            duration_ms: 1200,
        },
        test_result: CommandOutput { exit_code: 0, stdout: String::new(), stderr: String::new(), duration_ms: 0 },
    });

    let verifier = BuildVerifier::gcc(mock);
    let report = verifier.verify(
        TransactionId::new(), "/ws/kvdb",
        &contract.criteria,
    ).await.unwrap();
    assert!(!report.passed);

    // With build failure, blocking criteria unsatisfied → Rollback
    let verdict = reduction::reduce(&contract.criteria, &[]);
    assert!(!verdict.overall_passed);

    let settlement = settlement::derive_settlement(
        EffectClass::Staged,
        TaskOutcome::Failed,
        &verdict,
    );
    assert!(matches!(settlement, Some(SettlementDecision::Rollback { .. })));
}

// ══════════════════════════════════════════════════════════════════
// Irreversible E2E: deploy → Confirm (post-audit)
// ══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn deploy_irreversible_success_confirms() {
    let classifier = RulesBasedClassifier::new();
    let classification = classifier.classify(
        "deploy", "kubectl apply -f deployment.yaml",
        None,
        onto_assurance_types::enums::RiskLevel::Medium,
    );
    assert_eq!(classification.effect_class, EffectClass::Irreversible);
    assert!(classification.requires_approval);
    assert!(classification.pre_verification_required);

    let contract = ExecutionContract {
        contract_id: ContractId::new(),
        intent_id: IntentId::new(),
        criteria: vec![AcceptanceCriterion {
            criterion_id: CriterionId::new(),
            name: "deploy_check".into(),
            kind: CriterionKind::TestPass,
            description: "deployment manifest is valid".into(),
            is_blocking: true,
        }],
        evidence_required: vec![],
        approval_mode: ApprovalMode::Always,
        max_attempts: 1,
        created_at: chrono::Utc::now(),
    };

    // Evidence from pre-deployment check
    let evidence = vec![EvidenceRecord {
        evidence_id: onto_assurance_types::ids::EvidenceId::new(),
        transaction_id: TransactionId::new(),
        criterion_id: contract.criteria[0].criterion_id,
        kind: EvidenceRecordKind::VerifierReport,
        payload: serde_json::json!({"passed": true, "manifest_valid": true}),
        recorded_at: chrono::Utc::now(),
    }];

    let verdict = reduction::reduce(&contract.criteria, &evidence);
    assert!(verdict.overall_passed);

    let settlement = settlement::derive_settlement(
        EffectClass::Irreversible,
        TaskOutcome::Success,
        &verdict,
    );
    // Irreversible + Success → Confirm (audit trail, NOT Commit)
    assert_eq!(settlement, Some(SettlementDecision::Confirm));
}
