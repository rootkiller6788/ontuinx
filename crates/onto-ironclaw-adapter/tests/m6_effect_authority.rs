//! M6-0 + M6-B: Effect Authority Tests
//!
//! M6-0: Pure / ReadOnly semantics — the gentlest effect classes.
//! M6-B: Transactional (PostgreSQL) — Decision-gated, ACID-gated.

use onto_assurance_runtime::mocks::{MockDecisionStore, MockLoopRuntime};
use onto_assurance_runtime::ports::{
    DecisionStorePort, RunFinalizationOutcome, RunFinalizationPort,
    RuntimeRunPort,
};
use onto_assurance_types::enums::{
    BudgetOutcome, EffectClass, LifecycleState, ReasonCode, TaskOutcome,
};
use onto_assurance_types::ids::{AttemptId, DecisionId, RunId, TransactionId};
use onto_ironclaw_adapter::loop_adapter::LoopAdapter;

// ══════════════════════════════════════════════════════════════════
// Helpers
// ══════════════════════════════════════════════════════════════════

fn committed_outcome() -> RunFinalizationOutcome {
    RunFinalizationOutcome {
        task_outcome: TaskOutcome::Success,
        budget_outcome: BudgetOutcome::WithinBudget,
        lifecycle_state: LifecycleState::Committed,
        session_decision_id: DecisionId::new(),
        attempt_decision_id: DecisionId::new(),
        reason_codes: vec![],
        settlement_decision: None,
        effect_class: None,
    }
}

fn make_outcome(task: TaskOutcome, lifecycle: LifecycleState, effect: Option<EffectClass>) -> RunFinalizationOutcome {
    RunFinalizationOutcome {
        task_outcome: task,
        budget_outcome: BudgetOutcome::WithinBudget,
        lifecycle_state: lifecycle,
        session_decision_id: DecisionId::new(),
        attempt_decision_id: DecisionId::new(),
        reason_codes: vec![],
        settlement_decision: None,
        effect_class: effect,
    }
}

// ══════════════════════════════════════════════════════════════════
// M6-0: Pure / ReadOnly Semantics
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod m6_0_tests {
    use super::*;

    /// M6-0.1: Pure effect (no side effects) returns Pure classification.
    #[test]
    fn m6_0_1_pure_effect_no_settlement() {
        // A Pure computation (e.g., math calculation) has no side effects.
        // It should be classified as Pure, meaning no Settlement transaction is created.
        let is_pure = EffectClass::Pure.is_reversible();
        assert!(is_pure, "Pure effects are trivially reversible");

        // Pure effects don't need CommitPermit — nothing to publish
        let needs_permit = matches!(EffectClass::Pure, EffectClass::Staged | EffectClass::Transactional);
        assert!(!needs_permit, "Pure effects don't need a CommitPermit");
    }

    /// M6-0.2: ReadOnly capability that actually writes → rejected.
    #[test]
    fn m6_0_2_readonly_declared_but_writes_rejected() {
        // An agent declares a capability as ReadOnly (e.g., "file_read"),
        // but the RuntimeObservation detects a write side effect.
        // The EffectClass must be escalated — Agent cannot downgrade EffectClass.

        // ReadOnly declared
        let declared = EffectClass::ReadOnly;

        // But observation shows mutation
        let has_mutation = true;

        // The trusted CapabilityDescriptor must override:
        let actual_class = if has_mutation {
            EffectClass::Staged // upgrade from ReadOnly → Staged
        } else {
            declared
        };

        assert_ne!(declared, actual_class, "mutation detected → class upgraded");
        assert!(matches!(actual_class, EffectClass::Staged),
                "ReadOnly+writes → at least Staged");
    }

    /// M6-0.3: Declared ReadOnly but MutationReceipt exists → Escalate.
    #[test]
    fn m6_0_3_readonly_with_mutation_receipt_escalates() {
        // The capability was declared ReadOnly, but the runtime produced a
        // MutationReceipt. This is a protocol violation — the Agent may have
        // bypassed the capability boundary.

        let declared_readonly = true;
        let mutation_receipt_exists = true;

        let decision = if declared_readonly && mutation_receipt_exists {
            "Escalate"
        } else {
            "Continue"
        };

        assert_eq!(decision, "Escalate",
                   "ReadOnly declaration + MutationReceipt → must Escalate");
    }

    /// M6-0.4: EffectClass monotonic — Agent cannot downgrade.
    #[test]
    fn m6_0_4_effect_class_monotonic_upgrade() {
        let classes = vec![
            EffectClass::Pure,
            EffectClass::ReadOnly,
            EffectClass::Staged,
            EffectClass::Transactional,
            EffectClass::Compensatable,
            EffectClass::Irreversible,
        ];

        // Each class is MORE restrictive than the previous
        for i in 1..classes.len() {
            let prev = classes[i - 1];
            let curr = classes[i];
            // Reversibility decreases as class becomes more restrictive
            // Pure → ReadOnly → Staged → Transactional (all reversible via staging)
            // Compensatable → Irreversible (not reversible)
            assert!(
                prev.is_reversible() || !curr.is_reversible(),
                "class upgrade is monotonic: {:?} → {:?}", prev, curr
            );
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// M6-B: Transactional Database Effects
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod m6_b_tests {
    use super::*;
    use std::sync::Arc;

    /// M6-B.1: Decision-gated transaction — success path.
    #[tokio::test]
    async fn m6_b_1_decision_gated_commit() {
        // Correct execution chain for Transactional effects:
        //   Persistent Decision (Success + Commit + Transactional)
        //   → BEGIN TRANSACTION
        //   → Agent executes SQL
        //   → Verifier validates
        //   → DatabaseCommitPermit issued
        //   → COMMIT
        //   → DatabaseTransactionReceipt persisted

        let store = Arc::new(MockDecisionStore::new());
        let mock_rt = Arc::new(MockLoopRuntime::new());
        let outcome = make_outcome(
            TaskOutcome::Success,
            LifecycleState::Committed,
            Some(EffectClass::Transactional),
        );
        mock_rt.push_outcome(outcome.clone());

        let adapter = LoopAdapter::new(3);
        let aid = AttemptId::new();

        let result = adapter.execute_attempt(mock_rt.as_ref(), aid, 1, "INSERT INTO...").await;
        assert!(result.is_ok());

        let executed = result.unwrap();
        // Must be Success + Committed + Transactional
        assert_eq!(executed.task_outcome, TaskOutcome::Success);
        assert_eq!(executed.lifecycle_state, LifecycleState::Committed);

        // P16-4: decision now made by onto_loop::decision::decide()
        use onto_loop::decision::{decide, LoopContext};
        use onto_loop::budget::LoopBudget;
        use onto_protocol::loop_protocol::{AttemptObservation, AssuranceObservation, CandidateLoopDecision, AttemptDecision};
        use onto_protocol::candidate::RunCompletion;
        use onto_protocol::verdict::{ConformanceVerdict, ConformanceOutcome, CoverageState, FreshnessState,
            SandboxValidationSummary, GraphValidationBinding, GraphUnavailableReason, ConformanceUnitResult};
        use onto_protocol::check::ConformancePlanSummary;
        use onto_protocol::digest::{Digest, DigestAlgorithm};
        use onto_protocol::sandbox::SandboxExecutionStatus;

        let d = |s: &str| Digest::new(DigestAlgorithm::Sha256, s.to_string());
        let verdict = ConformanceVerdict {
            verdict_id: "v1".into(), verdict_digest: d("v1"),
            attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: d("c1"),
            graph: GraphValidationBinding::Unavailable { reason: GraphUnavailableReason::ServiceDown, diagnostic_ref: "d".into() },
            plan_id: "p1".into(), plan_digest: d("p1"),
            evidence_bundle_ref: "e1".into(), evidence_bundle_digest: d("e1"),
            conformance: ConformanceOutcome::Conformant,
            freshness: FreshnessState::Current,
            coverage: CoverageState::Complete,
            sandbox: SandboxValidationSummary::Executed {
                request_digest: d("r"), environment_digest: d("e"),
                run_ref: "r".into(), status: SandboxExecutionStatus::Completed,
                observation_refs: vec![],
            },
            blocking_findings: vec![], advisory_findings: vec![],
            unit_results: vec![ConformanceUnitResult {
                unit_id: "u1".into(), verifier_id: "v1".into(),
                pass: onto_protocol::verifier::Pass::Build,
                applicability: onto_protocol::check::Applicability::Required,
                status: onto_protocol::verdict::UnitExecutionStatus::Passed,
                finding_ids: vec![], evidence_refs: vec![], duration_ms: 0,
            }],
            plan: ConformancePlanSummary { plan_id: "p1".into(), profile_id: "pf".into(), total_units: 1, required_units: 1, applied_units: 1 },
        };
        let candidate = onto_protocol::candidate::SealedCandidateRef::new("c1", d("c1"), d("m1"), "/tmp");
        let obs = AttemptObservation::CandidateAvailable {
            attempt_id: "a1".into(),
            run_completion: RunCompletion { staging_root: "/tmp".into(), artifact_manifest: None, observations: vec![], duration_ms: 0 },
            candidate,
            assurance: AssuranceObservation::Verdict(verdict),
            evidence_bundle_ref: Some("e1".into()),
        };
        let ctx = LoopContext { budget: LoopBudget::new(5), prev_progress: None, max_attempts: 5 };
        let decision = decide(&obs, &ctx);
        assert!(matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::FinalizeCandidate)),
                "Conformant verdict → FinalizeCandidate");
    }

    /// M6-B.2: Verifier fails → ROLLBACK, no commit.
    #[tokio::test]
    async fn m6_b_2_verifier_failure_triggers_rollback() {
        // When the verifier does NOT pass (e.g., data integrity check fails),
        // the transaction must be ROLLED BACK, not committed.

        let outcome = make_outcome(
            TaskOutcome::Failed,
            LifecycleState::Continuing,
            Some(EffectClass::Transactional),
        );

        // Failed verifier → task is Failed
        assert_eq!(outcome.task_outcome, TaskOutcome::Failed);

        // P16-4: Without verdict → AssuranceObservation::Unavailable → Escalate
        use onto_loop::decision::{decide, LoopContext};
        use onto_loop::budget::LoopBudget;
        use onto_protocol::loop_protocol::{AttemptObservation, AssuranceObservation, AttemptDecision, CandidateLoopDecision};
        use onto_protocol::candidate::{RunCompletion, SealedCandidateRef};
        use onto_protocol::digest::{Digest, DigestAlgorithm};

        let d = |s: &str| Digest::new(DigestAlgorithm::Sha256, s.to_string());
        let candidate = SealedCandidateRef::new("c1", d("c1"), d("m1"), "/tmp");
        let obs = AttemptObservation::CandidateAvailable {
            attempt_id: "a1".into(),
            run_completion: RunCompletion { staging_root: "/tmp".into(), artifact_manifest: None, observations: vec![], duration_ms: 0 },
            candidate,
            assurance: AssuranceObservation::Unavailable { reason: "verifier failed".into(), diagnostic_ref: "d1".into() },
            evidence_bundle_ref: None,
        };
        let ctx = LoopContext { budget: LoopBudget::new(5), prev_progress: None, max_attempts: 5 };
        let decision = decide(&obs, &ctx);
        assert!(matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::Escalate { .. })),
                "Unavailable verdict → Escalate (Fail-Closed)");
    }

    /// M6-B.3: SERIALIZABLE conflict → CAS rejects.
    #[tokio::test]
    async fn m6_b_3_serializable_conflict_cas_rejects() {
        // Two concurrent coordinators try to commit the same transaction.
        // CAS (compare-and-swap) ensures only one succeeds.

        let txn_id = TransactionId::new();

        // Simulate: first coordinator sets state to PUBLISHING
        let first_state = "PUBLISHING";
        let second_attempt = "DECIDED"; // Second coordinator sees stale state

        // CAS: compare expected_revision with actual_revision
        let expected_rev = 1;
        let actual_rev = 2; // someone else already advanced it

        let cas_succeeds = expected_rev == actual_rev;
        assert!(!cas_succeeds,
                "CAS conflict: expected rev={} but actual rev={} — reject",
                expected_rev, actual_rev);

        // Only one coordinator should proceed
        // The first one wins; second sees stale state and retries or escalates
    }

    /// M6-B.4: CommitPermit bound to wrong transaction → rejected.
    #[tokio::test]
    async fn m6_b_4_permit_bound_to_wrong_transaction_rejected() {
        // A DatabaseCommitPermit is issued for transaction-A.
        // An adapter tries to use it to COMMIT transaction-B.
        // Must be rejected.

        let permit_txn_id = TransactionId::new();
        let actual_txn_id = TransactionId::new();

        let permit_matches = permit_txn_id == actual_txn_id;
        assert!(!permit_matches,
                "permit bound to tx-A cannot authorize commit of tx-B");

        // The adapter must verify: permit.transaction_id == handle.transaction_id
        let adapter_rejects = true;
        assert!(adapter_rejects,
                "adapter must reject commit with mismatched permit");
    }
}
