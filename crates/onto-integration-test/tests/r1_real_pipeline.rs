//! R1: Real OntoAssure pipeline — reduction→decision→settlement

use onto_assurance_core::{reduction, session_decision, settlement};
use onto_assurance_core::evidence_chain::EvidenceChain;
use onto_assurance_types::contract::{AcceptanceCriterion, CriterionKind};
use onto_assurance_types::enums::{BudgetOutcome, EffectClass, ExitReason, LifecycleState, TaskOutcome};
use onto_assurance_types::evidence::{EvidenceRecord, EvidenceRecordKind, VerifierBinding};
use onto_assurance_types::ids::{AttemptId, CriterionId, EvidenceId, RunId, TransactionId, VerifierId};
use onto_assurance_runtime::mocks::MockLoopRuntime;
use onto_assurance_runtime::ports::RunFinalizationOutcome;
use onto_assurance_types::ids::DecisionId;
use onto_ironclaw_adapter::loop_adapter::LoopAdapter;
use std::sync::Arc;

fn mo(t: TaskOutcome, l: LifecycleState) -> RunFinalizationOutcome {
    RunFinalizationOutcome { task_outcome: t, budget_outcome: BudgetOutcome::WithinBudget, lifecycle_state: l, session_decision_id: DecisionId::new(), attempt_decision_id: DecisionId::new(), reason_codes: vec![], settlement_decision: None, effect_class: None }
}

#[test] fn r1_1_reduction_pipeline() {
    let run_id = RunId::new();
    let mut chain = EvidenceChain::new(run_id, "genesis".into(), VerifierBinding{verifier_id:VerifierId::new(),verifier_version:"1.0".into(),toolchain:None,environment_hash:None});
    chain.append(EvidenceRecord{evidence_id:EvidenceId::new(),transaction_id:TransactionId::new(),criterion_id:CriterionId::new(),kind:EvidenceRecordKind::VerifierReport,payload:serde_json::json!({"passed":true}),recorded_at:chrono::Utc::now()}).unwrap();
    let bundle = chain.seal();
    let c = vec![AcceptanceCriterion{criterion_id:CriterionId::new(),name:"t".into(),kind:CriterionKind::TestPass,description:"".into(),is_blocking:true}];
    let e = vec![EvidenceRecord{evidence_id:EvidenceId::new(),transaction_id:TransactionId::new(),criterion_id:c[0].criterion_id,kind:EvidenceRecordKind::VerifierReport,payload:serde_json::json!({"passed":true}),recorded_at:chrono::Utc::now()}];
    let v = reduction::reduce(&c, &e);
    assert!(v.overall_passed);
    let s = session_decision::decide_session(run_id, AttemptId::new(), ExitReason::FinishRequested, &v, BudgetOutcome::WithinBudget, bundle.bundle_id);
    assert_eq!(s.task_outcome, TaskOutcome::Success);
    assert_eq!(s.lifecycle_state, LifecycleState::Committed);
    let st = settlement::derive_settlement(EffectClass::Staged, s.task_outcome, &v);
    assert_eq!(st, Some(onto_assurance_types::decision::SettlementDecision::Commit));
}

#[test] fn r1_2_failed_verdict_no_commit() {
    let c = vec![AcceptanceCriterion{criterion_id:CriterionId::new(),name:"t".into(),kind:CriterionKind::TestPass,description:"".into(),is_blocking:true}];
    let e = vec![EvidenceRecord{evidence_id:EvidenceId::new(),transaction_id:TransactionId::new(),criterion_id:c[0].criterion_id,kind:EvidenceRecordKind::VerifierReport,payload:serde_json::json!({"passed":false}),recorded_at:chrono::Utc::now()}];
    let v = reduction::reduce(&c, &e);
    assert!(!v.overall_passed);
}

#[test] fn r1_3_empty_evidence_no_success() {
    let c = vec![AcceptanceCriterion{criterion_id:CriterionId::new(),name:"t".into(),kind:CriterionKind::TestPass,description:"".into(),is_blocking:true}];
    let v = reduction::reduce(&c, &[]);
    assert!(!v.overall_passed, "empty evidence must not pass");
}

#[tokio::test] async fn r1_4_two_attempt_cycle() {
    let rt = Arc::new(MockLoopRuntime::new());
    rt.push_outcome(mo(TaskOutcome::Failed, LifecycleState::Continuing));
    rt.push_outcome(mo(TaskOutcome::Success, LifecycleState::Committed));
    let adapter = LoopAdapter::new(5);
    let o1 = adapter.execute_attempt(rt.as_ref(), AttemptId::new(), 1, "task").await.unwrap();
    assert_eq!(o1.task_outcome, TaskOutcome::Failed);
    let o2 = adapter.execute_attempt(rt.as_ref(), AttemptId::new(), 2, "task").await.unwrap();
    assert_eq!(o2.task_outcome, TaskOutcome::Success);
}
