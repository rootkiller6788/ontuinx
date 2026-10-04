//! RunFinalizationPort — bridges ironclaw_turns::RunFinalizationPort to Onto.
//!
//! The adapter maps OntoRuntime-native finalization types to Onto Assurance
//! Kernel types, calls the pure kernel functions, and maps the result back.
//! This is the ONLY production path that produces TaskOutcome.

use onto_assurance_core::{reduction, session_decision};
use onto_assurance_runtime::ports::{
    DecisionStorePort, EventSinkPort, EvidenceStorePort,
    RunFinalizationError, RunFinalizationOutcome, RunFinalizationPort,
    RunFinalizationRequest as OntoRequest,
};
use onto_assurance_types::enums::{EffectClass, ExitReason};
use onto_assurance_types::ids::DecisionId;
use std::sync::Arc;

// ══════════════════════════════════════════════════════════════════
// Stub
// ══════════════════════════════════════════════════════════════════

pub struct StubRunFinalizationPort;

#[async_trait::async_trait]
impl RunFinalizationPort for StubRunFinalizationPort {
    async fn finalize(&self, req: OntoRequest) -> Result<RunFinalizationOutcome, RunFinalizationError> {
        Ok(RunFinalizationOutcome {
            task_outcome: onto_assurance_types::enums::TaskOutcome::Success,
            budget_outcome: req.budget_outcome,
            lifecycle_state: onto_assurance_types::enums::LifecycleState::Committed,
            session_decision_id: DecisionId::new(),
            attempt_decision_id: DecisionId::new(),
            settlement_decision: None, effect_class: None, reason_codes: vec![],
        })
    }
}

// ══════════════════════════════════════════════════════════════════
// Production OntoRunFinalizationAdapter
// ══════════════════════════════════════════════════════════════════

/// Production implementation of Onto's `RunFinalizationPort`.
///
/// Loads evidence from stores, runs the Onto Assurance pure kernel, and
/// persists the authoritative decision.  Idempotent: same evidence → same
/// outcome, via `DecisionStorePort::persist_session()`.
pub struct OntoRunFinalizationAdapter {
    evidence_store: Arc<dyn EvidenceStorePort>,
    decision_store: Arc<dyn DecisionStorePort>,
    event_sink: Arc<dyn EventSinkPort>,
}

impl OntoRunFinalizationAdapter {
    pub fn new(
        evidence_store: Arc<dyn EvidenceStorePort>,
        decision_store: Arc<dyn DecisionStorePort>,
        event_sink: Arc<dyn EventSinkPort>,
    ) -> Self {
        Self { evidence_store, decision_store, event_sink }
    }
}

#[async_trait::async_trait]
impl RunFinalizationPort for OntoRunFinalizationAdapter {
    async fn finalize(&self, req: OntoRequest) -> Result<RunFinalizationOutcome, RunFinalizationError> {
        // 1. Check idempotency — already finalized?
        if let Some(existing) = self.decision_store.load_session(req.run_id).await
            .map_err(|e| RunFinalizationError::Persistence(e.to_string()))?
        {
            return Ok(existing);
        }

        // 2. Emit start event
        let _ = self.event_sink.emit("assurance.finalization_started", &serde_json::json!({
            "run_id": req.run_id.to_string(),
            "attempt_id": req.attempt_id.to_string(),
            "exit_reason": format!("{:?}", req.exit_reason),
        })).await;

        // 3. Load evidence bundle
        let evidence_key = format!("evidence/{}", req.attempt_id);
        let bundle = self.evidence_store.retrieve(&evidence_key).await
            .map_err(|e| RunFinalizationError::EvidenceNotFound(e.to_string()))?;

        let evidence_records: Vec<_> = bundle.records.iter()
            .map(|r| r.record.clone()).collect();

        // 4. Build criteria from evidence (criterion_ids in bundle)
        let criteria: Vec<_> = evidence_records.iter().map(|r| {
            onto_assurance_types::contract::AcceptanceCriterion {
                criterion_id: r.criterion_id,
                name: "finalization_criterion".into(),
                kind: onto_assurance_types::contract::CriterionKind::TestPass,
                description: String::new(),
                is_blocking: true,
            }
        }).collect();

        // 5. Reduction (pure kernel)
        let verdict = reduction::reduce(&criteria, &evidence_records);

        // 6. Add exit_reason context (creates proper LifecycleState)
        let exit_reason = match req.exit_reason {
            ExitReason::Crashed => ExitReason::Crashed,
            ExitReason::Cancelled => ExitReason::Cancelled,
            _ => ExitReason::FinishRequested,
        };

        // 7. Session decision (pure kernel)
        let session = session_decision::decide_session(
            req.run_id, req.attempt_id, exit_reason, &verdict,
            req.budget_outcome, bundle.bundle_id,
        );

        // 8. Settlement (pure kernel, for logging/audit)
        let effect_class = EffectClass::Staged;
        let _settle = onto_assurance_core::settlement::derive_settlement(
            effect_class, session.task_outcome, &verdict,
        );

        // 9. Build outcome
        let outcome = RunFinalizationOutcome {
            task_outcome: session.task_outcome,
            budget_outcome: session.budget_outcome,
            lifecycle_state: session.lifecycle_state,
            session_decision_id: DecisionId::new(),
            attempt_decision_id: DecisionId::new(),
            settlement_decision: None,
            effect_class: None,
            reason_codes: verdict.blocking_unsatisfied.iter().map(|cid| {
                onto_assurance_types::enums::ReasonCode {
                    domain: "verification".into(),
                    code: "blocking_unsatisfied".into(),
                    detail: format!("criterion {:?} not satisfied", cid),
                }
            }).collect(),
        };

        // 10. Persist idempotently
        self.decision_store.persist_session(req.run_id, req.attempt_id, &outcome).await
            .map_err(|e| RunFinalizationError::Persistence(e.to_string()))?;

        // 11. Emit completed event
        let _ = self.event_sink.emit("assurance.finalization_completed", &serde_json::json!({
            "run_id": req.run_id.to_string(),
            "task_outcome": outcome.task_outcome,
            "lifecycle_state": outcome.lifecycle_state,
            "session_decision_id": outcome.session_decision_id.to_string(),
        })).await;

        Ok(outcome)
    }
}

// ══════════════════════════════════════════════════════════════════
// OntoRuntime bridge adapter (requires --features ironclaw-integration)
// ══════════════════════════════════════════════════════════════════

#[cfg(feature = "ironclaw-integration")]
pub mod ironclaw {
    use super::*;
    use onto_assurance_types::enums::{BudgetOutcome, ExitReason};
    use onto_assurance_types::ids::{AttemptId, RunId};
    use std::sync::Arc;

    /// Bridges `ironclaw_turns::run_finalization::RunFinalizationPort` to Onto's
    /// `RunFinalizationPort`.  The runner calls the OntoRuntime-native trait,
    /// this adapter translates to Onto's own port which runs the kernel.
    pub struct OntoRuntimeToOntoFinalizationBridge {
        onto_port: Arc<dyn RunFinalizationPort>,
    }

    impl OntoRuntimeToOntoFinalizationBridge {
        pub fn new(onto_port: Arc<dyn RunFinalizationPort>) -> Self {
            Self { onto_port }
        }
    }

    #[async_trait::async_trait]
    impl ironclaw_turns::run_finalization::RunFinalizationPort for OntoRuntimeToOntoFinalizationBridge {
        async fn finalize(
            &self,
            request: ironclaw_turns::run_finalization::RunFinalizationRequest,
        ) -> Result<ironclaw_turns::run_finalization::RunFinalizationOutcome, ironclaw_turns::run_finalization::RunFinalizationError> {
            let onto_req = OntoRequest {
            run_id: RunId::parse(&request.run_id)
                .map_err(|e| ironclaw_turns::run_finalization::RunFinalizationError::Internal(
                    format!("invalid run_id: {}", e)))?,
            attempt_id: AttemptId::parse(&request.attempt_id)
                .map_err(|e| ironclaw_turns::run_finalization::RunFinalizationError::Internal(
                    format!("invalid attempt_id: {}", e)))?,
            exit_reason: map_exit_reason(request.exit_reason),
            budget_outcome: map_budget(request.budget),
            checkpoint_ref: None, // IronClaw protocol does not yet carry checkpoint_ref
            staging_root: request.staging_root.clone(),
        };
        let result = self.onto_port.finalize(onto_req).await.map_err(|e| {
            ironclaw_turns::run_finalization::RunFinalizationError::Internal(e.to_string())
        })?;
        Ok(ironclaw_turns::run_finalization::RunFinalizationOutcome {
            task_outcome: map_task_outcome(result.task_outcome),
            budget_outcome: map_budget_outcome(result.budget_outcome),
            lifecycle_state: map_lifecycle(result.lifecycle_state),
            session_decision_id: result.session_decision_id.to_string(),
        })
    }
}

    fn map_exit_reason(r: ironclaw_turns::run_finalization::FinalizationExitReason) -> ExitReason {
    match r {
        ironclaw_turns::run_finalization::FinalizationExitReason::FinishRequested => ExitReason::FinishRequested,
        ironclaw_turns::run_finalization::FinalizationExitReason::ProviderStop => ExitReason::ProviderStop,
        ironclaw_turns::run_finalization::FinalizationExitReason::BudgetLimit => ExitReason::BudgetLimit,
        ironclaw_turns::run_finalization::FinalizationExitReason::Stuck => ExitReason::Stuck,
        ironclaw_turns::run_finalization::FinalizationExitReason::Cancelled => ExitReason::Cancelled,
        ironclaw_turns::run_finalization::FinalizationExitReason::Crashed => ExitReason::Crashed,
    }
}

fn map_budget(b: ironclaw_turns::run_finalization::FinalizationBudget) -> BudgetOutcome {
    match b {
        ironclaw_turns::run_finalization::FinalizationBudget::WithinBudget => BudgetOutcome::WithinBudget,
        ironclaw_turns::run_finalization::FinalizationBudget::Depleted => BudgetOutcome::Depleted,
    }
}

fn map_task_outcome(o: onto_assurance_types::enums::TaskOutcome) -> ironclaw_turns::run_finalization::FinalizationTaskOutcome {
    match o {
        onto_assurance_types::enums::TaskOutcome::Success => ironclaw_turns::run_finalization::FinalizationTaskOutcome::Success,
        onto_assurance_types::enums::TaskOutcome::Incomplete => ironclaw_turns::run_finalization::FinalizationTaskOutcome::Incomplete,
        onto_assurance_types::enums::TaskOutcome::Failed => ironclaw_turns::run_finalization::FinalizationTaskOutcome::Failed,
        onto_assurance_types::enums::TaskOutcome::EnvironmentError => ironclaw_turns::run_finalization::FinalizationTaskOutcome::EnvironmentError,
    }
}

    fn map_budget_outcome(o: BudgetOutcome) -> ironclaw_turns::run_finalization::FinalizationBudgetOutcome {
        match o {
            BudgetOutcome::WithinBudget => ironclaw_turns::run_finalization::FinalizationBudgetOutcome::WithinBudget,
            BudgetOutcome::Depleted => ironclaw_turns::run_finalization::FinalizationBudgetOutcome::Depleted,
            BudgetOutcome::HardLimitReached => ironclaw_turns::run_finalization::FinalizationBudgetOutcome::HardLimitReached,
        }
    }

    fn map_lifecycle(l: onto_assurance_types::enums::LifecycleState) -> ironclaw_turns::run_finalization::FinalizationLifecycle {
        match l {
            onto_assurance_types::enums::LifecycleState::Committed => ironclaw_turns::run_finalization::FinalizationLifecycle::Committed,
            onto_assurance_types::enums::LifecycleState::Continuing => ironclaw_turns::run_finalization::FinalizationLifecycle::Continuing,
            onto_assurance_types::enums::LifecycleState::RolledBack => ironclaw_turns::run_finalization::FinalizationLifecycle::RolledBack,
            _ => ironclaw_turns::run_finalization::FinalizationLifecycle::Escalated,
        }
    }
}
