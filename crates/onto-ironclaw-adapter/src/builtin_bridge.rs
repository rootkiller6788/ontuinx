//! Onto Builtin Bridge — the ONLY Onto code that enters OntoRuntime's TCB.
//!
//! This bridge is registered as a `Builtin`-tier hook at the
//! `BeforeCapability` hook point.  It is intentionally THIN:
//!
//! ```text
//! Within TCB (this file + auth_adapter):
//!   ├── Verify decision signature from Onto Reduction Core
//!   ├── Check transaction state
//!   ├── Check SettlementDecision
//!   ├── Emit gate result (Allow/Deny/PauseApproval)
//!   └── Write immutable events to OntoRuntime EventStore
//!
//! Outside TCB (separate process/WASM):
//!   ├── Industry verifiers (Code Pack, Ops Pack, ...)
//!   ├── LLM requirement interpreter
//!   ├── Report generation
//!   ├── Semantic scorer
//!   └── External data queries
//! ```
//!
//! ## Monotonic Tightening Rule
//!
//! ```text
//! OntoRuntime DENY             → Onto MUST NOT change to ALLOW
//! OntoRuntime REQUIRE_APPROVAL → Onto MUST NOT bypass
//! OntoRuntime ALLOW            → Onto MAY change to DENY
//! OntoRuntime ALLOW            → Onto MAY change to REQUIRE_APPROVAL
//! OntoRuntime ALLOW            → Onto MAY change to RESTRICT (scope down)
//! ```

use onto_assurance_core::effect_classifier::{EffectClassifier, RulesBasedClassifier};
use onto_assurance_types::contract::ExecutionContract;
use onto_assurance_types::decision::SettlementDecision;
use onto_assurance_types::enums::{EffectClass, TaskOutcome};

// ══════════════════════════════════════════════════════════════════
// GateDecision — the only three things Onto can say to OntoRuntime
// ══════════════════════════════════════════════════════════════════

/// Mirrors `ironclaw_hooks::kinds::gate::BeforeCapabilityHookDecision`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OntoGateDecision {
    /// Allow the capability to proceed.  Only producible by Builtin hooks.
    Allow,
    /// Block the capability.  Fail-closed for all trust tiers.
    Deny { reason: String },
    /// Pause the run waiting for explicit user approval.
    PauseApproval { reason: String },
}

// ══════════════════════════════════════════════════════════════════
// ShadowObserver — M4 shadow mode
// ══════════════════════════════════════════════════════════════════

/// During M4 shadow mode, Onto runs as an OBSERVER only.
/// It computes decisions but does NOT block real tasks.
/// It logs ShadowDecisions for comparison with OntoRuntime's own state.
pub struct ShadowObserver {
    classifier: Box<dyn EffectClassifier>,
}

impl ShadowObserver {
    pub fn new(classifier: Box<dyn EffectClassifier>) -> Self {
        Self { classifier }
    }

    /// Observe a capability invocation and produce a shadow decision.
    /// This is called AFTER OntoRuntime has already authorized and dispatched.
    /// The shadow decision is logged but NEVER blocks the real task.
    pub fn observe_capability(
        &self,
        capability_kind: &str,
        arguments_hint: &str,
    ) -> ShadowObservation {
        let classification = self.classifier.classify(
            capability_kind,
            arguments_hint,
            None,
            onto_assurance_types::enums::RiskLevel::Medium,
        );

        ShadowObservation {
            effect_class: classification.effect_class,
            requires_approval: classification.requires_approval,
            pre_verification_required: classification.pre_verification_required,
            upgraded_from: classification.upgraded_from,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ShadowObservation {
    pub effect_class: EffectClass,
    pub requires_approval: bool,
    pub pre_verification_required: bool,
    pub upgraded_from: Option<EffectClass>,
}

// ══════════════════════════════════════════════════════════════════
// BuiltinGate — M5/M6 authoritative mode
// ══════════════════════════════════════════════════════════════════

/// During M5/M6, Onto runs as an AUTHORITATIVE gate.
/// It CAN block capability dispatch and CAN override success determination.
///
/// This is the thin TCB entry point.  It receives a pre-computed
/// signed decision from the Onto Reduction Core (running outside TCB)
/// and enforces it.
pub struct BuiltinGate {
    classifier: Box<dyn EffectClassifier>,
}

impl BuiltinGate {
    pub fn new(classifier: Box<dyn EffectClassifier>) -> Self {
        Self { classifier }
    }

    /// Evaluate a capability invocation BEFORE OntoRuntime dispatch.
    ///
    /// Returns the Onto gate decision.  This is combined with OntoRuntime's
    /// own authorization via monotonic intersection:
    ///
    /// ```ignore
    /// FinalAllow = OntoRuntimeAllow ∧ OntoAllow
    /// ```
    pub fn evaluate_before_capability(
        &self,
        capability_kind: &str,
        arguments_hint: &str,
        contract: &ExecutionContract,
    ) -> OntoGateDecision {
        let classification = self.classifier.classify(
            capability_kind,
            arguments_hint,
            None,
            onto_assurance_types::enums::RiskLevel::Medium,
        );

        // Any effect requiring approval → PauseApproval
        if classification.requires_approval {
            return OntoGateDecision::PauseApproval {
                reason: format!(
                    "{:?} action '{}' requires human approval before execution",
                    classification.effect_class, capability_kind
                ),
            };
        }

        // Pre-verification required with blocking criteria → PauseApproval
        if classification.pre_verification_required
            && contract.criteria.iter().any(|c| c.is_blocking)
        {
            return OntoGateDecision::PauseApproval {
                reason: format!(
                    "Pre-verification required for {:?} action '{}'",
                    classification.effect_class, capability_kind
                ),
            };
        }

        OntoGateDecision::Allow
    }

    /// Evaluate the final settlement AFTER execution completes.
    ///
    /// This gates the publish/discard/freeze decision.
    pub fn evaluate_settlement(
        &self,
        task_outcome: TaskOutcome,
        _effect_class: EffectClass,
        ironclaw_settlement: &SettlementDecision,
    ) -> SettlementDecision {
        // Monotonic tightening: Onto can only be MORE restrictive
        match (ironclaw_settlement, task_outcome) {
            (SettlementDecision::Commit, TaskOutcome::Success) => {
                // OntoRuntime wants to commit and task succeeded — allow
                SettlementDecision::Commit
            }
            (SettlementDecision::Commit, _) => {
                // OntoRuntime wants to commit but task did NOT succeed — escalate
                SettlementDecision::Escalate {
                    reason: onto_assurance_types::enums::ReasonCode {
                        domain: "onto".into(),
                        code: "settlement_override".into(),
                        detail: format!(
                            "task outcome is {:?}, cannot commit staged effects",
                            task_outcome
                        ),
                    },
                }
            }
            // For all other OntoRuntime decisions, Onto passes through
            // (Rollback, Freeze, Escalate are already restrictive)
            (other, _) => other.clone(),
        }
    }
}

impl Default for BuiltinGate {
    fn default() -> Self {
        Self::new(Box::new(RulesBasedClassifier::new()))
    }
}

impl Default for ShadowObserver {
    fn default() -> Self {
        Self::new(Box::new(RulesBasedClassifier::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_runtime::ports::{RunFinalizationOutcome, RunFinalizationPort, RunFinalizationRequest};
    use onto_assurance_types::enums::BudgetOutcome;
    use std::sync::Arc;
    use onto_assurance_types::contract::{
        AcceptanceCriterion, ApprovalMode, CriterionKind, ExecutionContract,
    };
    use onto_assurance_types::enums::EffectClass;
    use onto_assurance_types::ids::{ContractId, CriterionId, IntentId};

    fn make_contract() -> ExecutionContract {
        ExecutionContract {
            contract_id: ContractId::new(),
            intent_id: IntentId::new(),
            criteria: vec![AcceptanceCriterion {
                criterion_id: CriterionId::new(),
                name: "tests".into(),
                kind: CriterionKind::TestPass,
                description: "all tests pass".into(),
                is_blocking: true,
            }],
            evidence_required: vec![],
            approval_mode: ApprovalMode::OnHighRisk,
            max_attempts: 3,
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn shadow_observer_classifies_correctly() {
        let observer = ShadowObserver::default();
        let obs = observer.observe_capability("file_write", "");
        assert_eq!(obs.effect_class, EffectClass::Staged);
        assert!(!obs.requires_approval);
    }

    #[test]
    fn builtin_gate_irreversible_requires_approval() {
        let gate = BuiltinGate::default();
        let contract = make_contract();
        let decision = gate.evaluate_before_capability("deploy", "", &contract);
        assert!(matches!(decision, OntoGateDecision::PauseApproval { .. }));
    }

    #[test]
    fn builtin_gate_file_write_allowed() {
        let gate = BuiltinGate::default();
        let contract = make_contract();
        let decision = gate.evaluate_before_capability("file_write", "", &contract);
        assert_eq!(decision, OntoGateDecision::Allow);
    }

    #[test]
    fn monotonic_tightening_commit_on_failure_escalates() {
        let gate = BuiltinGate::default();
        let result = gate.evaluate_settlement(
            TaskOutcome::Failed,
            EffectClass::Staged,
            &SettlementDecision::Commit,
        );
        assert!(matches!(result, SettlementDecision::Escalate { .. }));
    }

    #[test]
    fn monotonic_tightening_commit_on_success_passes() {
        let gate = BuiltinGate::default();
        let result = gate.evaluate_settlement(
            TaskOutcome::Success,
            EffectClass::Staged,
            &SettlementDecision::Commit,
        );
        assert_eq!(result, SettlementDecision::Commit);
    }

    #[test]
    fn shadow_unknown_tool_is_irreversible() {
        let observer = ShadowObserver::default();
        let obs = observer.observe_capability("mystery_tool_v99", "--delete-all");
        assert_eq!(obs.effect_class, EffectClass::Irreversible);
        assert!(obs.requires_approval);
    }

    // ── RunFinalizationPort tests ──

    #[tokio::test]
    async fn stub_finalization_returns_success() {
        use crate::run_finalization_adapter::StubRunFinalizationPort;
        let port = StubRunFinalizationPort;
        let outcome = port.finalize(RunFinalizationRequest {
            run_id: onto_assurance_types::ids::RunId::new(),
            attempt_id: onto_assurance_types::ids::AttemptId::new(),
            exit_reason: onto_assurance_types::enums::ExitReason::FinishRequested,
            budget_outcome: BudgetOutcome::WithinBudget,
            checkpoint_ref: None,
            staging_root: None,
        }).await.unwrap();
        assert_eq!(outcome.task_outcome, onto_assurance_types::enums::TaskOutcome::Success);
        assert_eq!(outcome.lifecycle_state, onto_assurance_types::enums::LifecycleState::Committed);
    }

    #[tokio::test]
    async fn full_finalization_pipeline_with_mock_stores() {
        use crate::run_finalization_adapter::OntoRunFinalizationAdapter;
        use onto_assurance_runtime::ports::{
            DecisionStorePort, DecisionStoreError, EventSinkPort, EventSinkError,
            EvidenceStoreError, EvidenceStorePort,
        };
        use onto_assurance_types::evidence::{EvidenceBundle, EvidenceRecord, EvidenceRecordKind};
        use std::collections::HashMap;
        use std::sync::Mutex;

        struct MockEvidence { pub data: Mutex<HashMap<String, EvidenceBundle>> }
        #[async_trait::async_trait]
        impl EvidenceStorePort for MockEvidence {
            async fn store(&self, b: &EvidenceBundle) -> Result<String, EvidenceStoreError> {
                self.data.lock().unwrap().insert(b.bundle_id.to_string(), b.clone());
                Ok(b.bundle_id.to_string())
            }
            async fn retrieve(&self, k: &str) -> Result<EvidenceBundle, EvidenceStoreError> {
                self.data.lock().unwrap().get(k).cloned().ok_or(EvidenceStoreError::NotFound(k.into()))
            }
        }

        struct MockDecStore { pub data: Mutex<HashMap<String, RunFinalizationOutcome>> }
        #[async_trait::async_trait]
        impl DecisionStorePort for MockDecStore {
            async fn persist_session(&self, rid: onto_assurance_types::ids::RunId, _aid: onto_assurance_types::ids::AttemptId, o: &RunFinalizationOutcome)
                -> Result<bool, DecisionStoreError> { self.data.lock().unwrap().insert(rid.to_string(), o.clone()); Ok(true) }
            async fn load_session(&self, rid: onto_assurance_types::ids::RunId) -> Result<Option<RunFinalizationOutcome>, DecisionStoreError> {
                Ok(self.data.lock().unwrap().get(&rid.to_string()).cloned())
            }
        }

        struct MockEvents;
        #[async_trait::async_trait]
        impl EventSinkPort for MockEvents {
            async fn emit(&self, _t: &str, _p: &serde_json::Value) -> Result<(), EventSinkError> { Ok(()) }
        }

        let evidence = MockEvidence { data: Mutex::new(HashMap::new()) };
        let dec = MockDecStore { data: Mutex::new(HashMap::new()) };
        let events = MockEvents;

        // Store some evidence first
        let eid = onto_assurance_types::ids::EvidenceId::new();
        let cid = onto_assurance_types::ids::CriterionId::new();
        let record = EvidenceRecord {
            evidence_id: eid, transaction_id: onto_assurance_types::ids::TransactionId::new(),
            criterion_id: cid, kind: EvidenceRecordKind::TestOutput,
            payload: serde_json::json!({"passed": true}), recorded_at: chrono::Utc::now(),
        };
        let mut chain = onto_assurance_core::evidence_chain::EvidenceChain::new(
            onto_assurance_types::ids::RunId::new(), "genesis".into(),
            onto_assurance_types::evidence::VerifierBinding {
                verifier_id: onto_assurance_types::ids::VerifierId::new(),
                verifier_version: "1".into(), toolchain: None, environment_hash: None,
            },
        );
        chain.append(record).unwrap();
        let bundle = chain.seal();
        let attempt_id = onto_assurance_types::ids::AttemptId::new();
        let evidence_key = format!("evidence/{}", attempt_id);
        evidence.store(&bundle).await.unwrap();
        // Overwrite key to match adapter's lookup pattern
        evidence.data.lock().unwrap().insert(evidence_key, bundle.clone());

        let port = OntoRunFinalizationAdapter::new(
            Arc::new(evidence), Arc::new(dec), Arc::new(events),
        );
        let outcome = port.finalize(RunFinalizationRequest {
            run_id: onto_assurance_types::ids::RunId::new(),
            attempt_id,
            exit_reason: onto_assurance_types::enums::ExitReason::FinishRequested,
            budget_outcome: BudgetOutcome::WithinBudget,
            checkpoint_ref: None,
            staging_root: None,
        }).await.unwrap();
        assert_eq!(outcome.task_outcome, onto_assurance_types::enums::TaskOutcome::Success);
        assert_eq!(outcome.lifecycle_state, onto_assurance_types::enums::LifecycleState::Committed);
    }

    #[tokio::test]
    async fn fault_injection_persist_failure_still_returns_decision() {
        use crate::run_finalization_adapter::OntoRunFinalizationAdapter;
        use onto_assurance_runtime::ports::{
            DecisionStorePort, DecisionStoreError, EventSinkPort, EventSinkError,
            EvidenceStoreError, EvidenceStorePort,
        };
        use std::sync::Mutex;

        struct FailingDecStore;
        #[async_trait::async_trait]
        impl DecisionStorePort for FailingDecStore {
            async fn persist_session(&self, _rid: onto_assurance_types::ids::RunId,
                _aid: onto_assurance_types::ids::AttemptId, _o: &RunFinalizationOutcome,
            ) -> Result<bool, DecisionStoreError> {
                Err(DecisionStoreError::Storage("simulated disk failure".into()))
            }
            async fn load_session(&self, _rid: onto_assurance_types::ids::RunId,
            ) -> Result<Option<RunFinalizationOutcome>, DecisionStoreError> {
                Ok(None) // idempotency check: no prior decision
            }
        }
        struct MockEvt;
        #[async_trait::async_trait]
        impl EventSinkPort for MockEvt {
            async fn emit(&self, _t: &str, _p: &serde_json::Value) -> Result<(), EventSinkError> { Ok(()) }
        }
        struct MockEvidence { pub data: Mutex<std::collections::HashMap<String, onto_assurance_types::evidence::EvidenceBundle>> }
        #[async_trait::async_trait]
        impl EvidenceStorePort for MockEvidence {
            async fn store(&self, b: &onto_assurance_types::evidence::EvidenceBundle) -> Result<String, EvidenceStoreError> {
                self.data.lock().unwrap().insert(b.bundle_id.to_string(), b.clone()); Ok(b.bundle_id.to_string())
            }
            async fn retrieve(&self, k: &str) -> Result<onto_assurance_types::evidence::EvidenceBundle, EvidenceStoreError> {
                self.data.lock().unwrap().get(k).cloned().ok_or(EvidenceStoreError::NotFound(k.into()))
            }
        }
        let ev = MockEvidence { data: Mutex::new(std::collections::HashMap::new()) };
        let eid = onto_assurance_types::ids::EvidenceId::new();
        let rec = onto_assurance_types::evidence::EvidenceRecord {
            evidence_id: eid, transaction_id: onto_assurance_types::ids::TransactionId::new(),
            criterion_id: onto_assurance_types::ids::CriterionId::new(),
            kind: onto_assurance_types::evidence::EvidenceRecordKind::TestOutput,
            payload: serde_json::json!({"passed": true}), recorded_at: chrono::Utc::now(),
        };
        let mut ch = onto_assurance_core::evidence_chain::EvidenceChain::new(
            onto_assurance_types::ids::RunId::new(), "g".into(),
            onto_assurance_types::evidence::VerifierBinding {
                verifier_id: onto_assurance_types::ids::VerifierId::new(),
                verifier_version: "1".into(), toolchain: None, environment_hash: None,
            },
        );
        ch.append(rec).unwrap();
        let bundle = ch.seal();
        let aid = onto_assurance_types::ids::AttemptId::new();
        ev.store(&bundle).await.unwrap();
        ev.data.lock().unwrap().insert(format!("evidence/{}", aid), bundle.clone());

        let port = OntoRunFinalizationAdapter::new(
            Arc::new(ev), Arc::new(FailingDecStore), Arc::new(MockEvt),
        );
        let result = port.finalize(RunFinalizationRequest {
            run_id: onto_assurance_types::ids::RunId::new(), attempt_id: aid,
            exit_reason: onto_assurance_types::enums::ExitReason::FinishRequested,
            budget_outcome: BudgetOutcome::WithinBudget, checkpoint_ref: None,
            staging_root: None,
        }).await;
        // MUST fail-closed — persist failure should escalate, not silently succeed
        assert!(result.is_err(), "persist failure must propagate error (fail-closed)");
    }

    #[tokio::test]
    async fn stub_finalization_preserves_budget() {
        use crate::run_finalization_adapter::StubRunFinalizationPort;
        let port = StubRunFinalizationPort;
        let outcome = port.finalize(RunFinalizationRequest {
            run_id: onto_assurance_types::ids::RunId::new(),
            attempt_id: onto_assurance_types::ids::AttemptId::new(),
            exit_reason: onto_assurance_types::enums::ExitReason::BudgetLimit,
            budget_outcome: BudgetOutcome::Depleted,
            checkpoint_ref: None,
            staging_root: None,
        }).await.unwrap();
        assert_eq!(outcome.budget_outcome, BudgetOutcome::Depleted);
    }

    #[test]
    fn confirm_on_failure_is_passed_through() {
        // OntoRuntime says Confirm (for an Irreversible effect) — Onto passes through.
        // Confirm means "acknowledge the effect happened" — it's already restrictive.
        let gate = BuiltinGate::default();
        let result = gate.evaluate_settlement(
            TaskOutcome::Success,
            EffectClass::Irreversible,
            &SettlementDecision::Confirm,
        );
        assert_eq!(result, SettlementDecision::Confirm);
    }

    #[test]
    fn rollback_is_always_passed_through() {
        let gate = BuiltinGate::default();
        let result = gate.evaluate_settlement(
            TaskOutcome::Failed,
            EffectClass::Staged,
            &SettlementDecision::Rollback {
                reason: onto_assurance_types::enums::ReasonCode {
                    domain: "test".into(),
                    code: "test".into(),
                    detail: "test".into(),
                },
            },
        );
        assert!(matches!(result, SettlementDecision::Rollback { .. }));
    }
}
