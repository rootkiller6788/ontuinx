//! AttemptExecutor — bridges onto-protocol::AttemptExecutor to IronClaw RuntimeRunPort.
//!
//! Wraps the existing RuntimeRunPort and adapts it to the new unified trait.

use async_trait::async_trait;
use onto_assurance_runtime::ports::RuntimeRunPort;
use onto_protocol::candidate::{AgentRunOutcome, RunCompletion, SealedCandidateRef, ArtifactManifest};
use onto_protocol::digest::{Digest, DigestAlgorithm};
use sha2::{Sha256, Digest as ShaDigest};
use onto_protocol::executor::{AttemptExecutor, AttemptError, AgentRunHandle};
use onto_protocol::loop_protocol::{LoopDirective, TransitionOutcome, TransitionReceipt};
use std::sync::{Arc, Mutex};

/// Bridges the existing RuntimeRunPort to the unified AttemptExecutor trait.
pub struct AttemptExecutorAdapter {
    runtime: Arc<dyn RuntimeRunPort>,
    applied: Mutex<Vec<(String, Digest, TransitionOutcome)>>,  // decision_id, directive_digest, outcome
    /// Cache: run_id → RunId for use in await_terminal
    run_id_cache: Mutex<Vec<(String, onto_assurance_types::ids::RunId)>>,
}

impl AttemptExecutorAdapter {
    pub fn new(runtime: Arc<dyn RuntimeRunPort>) -> Self {
        Self {
            runtime, applied: Mutex::new(vec![]), run_id_cache: Mutex::new(vec![]),
        }
    }

    fn compute_digest(data: &str) -> Digest {
        let hash = Sha256::digest(data.as_bytes());
        Digest::new(DigestAlgorithm::Sha256, hex::encode(hash))
    }
}

#[async_trait]
impl AttemptExecutor for AttemptExecutorAdapter {
    async fn start_attempt(
        &self,
        attempt_id: &str,
        objective: &str,
        max_iterations: Option<u32>,
    ) -> Result<AgentRunHandle, AttemptError> {
        let rid = self.runtime
            .start_run(max_iterations.unwrap_or(5), objective)
            .await
            .map_err(|e| AttemptError::Internal(e))?;

        let handle = AgentRunHandle {
            run_id: rid.to_string(),
            attempt_id: attempt_id.to_string(),
            staging_root: format!("/tmp/onto-staging-{}", attempt_id),
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        self.run_id_cache.lock().unwrap().push((handle.run_id.clone(), rid));
        Ok(handle)
    }

    async fn await_completion(
        &self,
        handle: &AgentRunHandle,
    ) -> Result<AgentRunOutcome, AttemptError> {
        let run_id = {
            let cache = self.run_id_cache.lock().unwrap();
            cache.iter()
                .find(|(h, _)| h == &handle.run_id)
                .map(|(_, rid)| *rid)
        };

        let outcome = match run_id {
            Some(rid) => self.runtime.await_terminal(rid).await,
            None => return Err(AttemptError::NotFound(handle.run_id.clone())),
        }
        .map_err(|e| AttemptError::Internal(e))?;

        let candidate_digest = Self::compute_digest(&handle.attempt_id);
        let candidate = SealedCandidateRef::new(
            &handle.attempt_id,
            candidate_digest.clone(),
            Self::compute_digest("manifest"),
            &handle.staging_root,
        );

        Ok(AgentRunOutcome::CandidateProduced {
            completion: RunCompletion {
                staging_root: handle.staging_root.clone(),
                artifact_manifest: Some(ArtifactManifest { entries: vec![] }),
                observations: outcome.reason_codes.iter().map(|r| format!("{}:{}", r.domain, r.code)).collect(),
                duration_ms: 0,
            },
            candidate,
        })
    }

    async fn apply_transition(
        &self,
        handle: &AgentRunHandle,
        directive: &LoopDirective,
    ) -> Result<TransitionReceipt, AttemptError> {
        let (decision_id, outcome) = match directive {
            LoopDirective::CandidateBound { decision_id, decision, .. } => {
                let outcome = match decision {
                    onto_protocol::loop_protocol::CandidateLoopDecision::FinalizeCandidate => {
                        TransitionOutcome::Finalized {
                            settlement: onto_protocol::loop_protocol::SettlementState::Committed,
                            receipt_ref: format!("finalize-{}", handle.attempt_id),
                        }
                    }
                    onto_protocol::loop_protocol::CandidateLoopDecision::DiscardCandidate { .. } => {
                        TransitionOutcome::CandidateDiscarded
                    }
                    onto_protocol::loop_protocol::CandidateLoopDecision::Freeze { .. } => {
                        TransitionOutcome::Frozen
                    }
                    onto_protocol::loop_protocol::CandidateLoopDecision::Escalate { .. } => {
                        TransitionOutcome::Escalated
                    }
                    _ => TransitionOutcome::Continued,
                };
                (decision_id.clone(), outcome)
            }
            LoopDirective::AttemptBound { decision_id, attempt_id: _, decision } => {
                let outcome = match decision {
                    onto_protocol::loop_protocol::NoCandidateLoopDecision::CloseFailed => {
                        TransitionOutcome::AttemptClosed
                    }
                    onto_protocol::loop_protocol::NoCandidateLoopDecision::Freeze { .. } => {
                        TransitionOutcome::Frozen
                    }
                    onto_protocol::loop_protocol::NoCandidateLoopDecision::Escalate { .. } => {
                        TransitionOutcome::Escalated
                    }
                };
                (decision_id.clone(), outcome)
            }
        };

        // Idempotency: same decision_id + same directive_digest → cached receipt
        //              same decision_id + different directive_digest → IdempotencyConflict
        let directive_digest = match directive {
            LoopDirective::CandidateBound { verdict_digest, .. } => verdict_digest.clone(),
            LoopDirective::AttemptBound { decision_id, .. } => Digest::new(DigestAlgorithm::Sha256, decision_id),
        };
        {
            let applied = self.applied.lock().unwrap();
            if let Some((_, cached_digest, cached_outcome)) = applied.iter().find(|(id, _, _)| id == &decision_id) {
                if cached_digest == &directive_digest {
                    return Ok(TransitionReceipt {
                    receipt_id: format!("receipt-{}", decision_id),
                    receipt_digest: Digest::new(DigestAlgorithm::Sha256, "cached"),
                    decision_id,
                    directive_digest: Digest::new(DigestAlgorithm::Sha256, "cached"),
                    attempt_id: handle.attempt_id.clone(),
                    candidate_id: Some(handle.attempt_id.clone()),
                    candidate_digest: Some(Digest::new(DigestAlgorithm::Sha256, "cached")),
                    verdict_id: None,
                    verdict_digest: None,
                    outcome: cached_outcome.clone(),
                });
                } else {
                    return Err(AttemptError::IdempotencyConflict(format!(
                        "decision_id={} already applied with different directive_digest", decision_id)));
                }
            }
        }

        // Record and return
        self.applied.lock().unwrap().push((decision_id.clone(), directive_digest, outcome.clone()));
        let receipt_id = format!("receipt-{}", decision_id);

        Ok(TransitionReceipt {
            receipt_id: receipt_id.clone(),
            receipt_digest: Digest::new(DigestAlgorithm::Sha256, &receipt_id),
            decision_id,
            directive_digest: Digest::new(DigestAlgorithm::Sha256, "applied"),
            attempt_id: handle.attempt_id.clone(),
            candidate_id: Some(handle.attempt_id.clone()),
            candidate_digest: Some(Digest::new(DigestAlgorithm::Sha256, "applied")),
            verdict_id: None,
            verdict_digest: None,
            outcome,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_runtime::mocks::MockLoopRuntime;
    use onto_assurance_runtime::ports::RunFinalizationOutcome;
    use onto_assurance_types::enums::{BudgetOutcome, LifecycleState, TaskOutcome};
    use onto_assurance_types::ids::DecisionId;

    #[tokio::test]
    async fn start_and_complete_attempt() {
        let mock = Arc::new(MockLoopRuntime::new());
        mock.push_outcome(RunFinalizationOutcome {
            task_outcome: TaskOutcome::Success,
            budget_outcome: BudgetOutcome::WithinBudget,
            lifecycle_state: LifecycleState::Committed,
            session_decision_id: DecisionId::new(),
            attempt_decision_id: DecisionId::new(),
            reason_codes: vec![],
            settlement_decision: None,
            effect_class: None,
        });

        let executor = AttemptExecutorAdapter::new(mock);
        let handle = executor.start_attempt("att-1", "write hello()", Some(5)).await.unwrap();
        assert_eq!(handle.attempt_id, "att-1");

        let result = executor.await_completion(&handle).await.unwrap();
        assert!(matches!(result, AgentRunOutcome::CandidateProduced { .. }));
    }

    #[tokio::test]
    async fn apply_finalize_produces_receipt() {
        let mock = Arc::new(MockLoopRuntime::new());
        let executor = AttemptExecutorAdapter::new(mock);
        let handle = executor.start_attempt("att-2", "task", Some(3)).await.unwrap();

        let directive = LoopDirective::CandidateBound {
            decision_id: "dec-1".to_string(),
            attempt_id: "att-2".to_string(),
            candidate_id: "c1".to_string(),
            candidate_digest: Digest::new(DigestAlgorithm::Sha256, "c1"),
            verdict_id: "v1".to_string(),
            verdict_digest: Digest::new(DigestAlgorithm::Sha256, "v1"),
            decision: onto_protocol::loop_protocol::CandidateLoopDecision::FinalizeCandidate,
        };

        let receipt = executor.apply_transition(&handle, &directive).await.unwrap();
        assert!(matches!(receipt.outcome, TransitionOutcome::Finalized { .. }));
    }

    #[tokio::test]
    async fn apply_twice_is_idempotent() {
        let mock = Arc::new(MockLoopRuntime::new());
        let executor = AttemptExecutorAdapter::new(mock);
        let handle = executor.start_attempt("att-3", "task", Some(3)).await.unwrap();

        let directive = LoopDirective::CandidateBound {
            decision_id: "dec-2".to_string(),
            attempt_id: "att-3".to_string(),
            candidate_id: "c2".to_string(),
            candidate_digest: Digest::new(DigestAlgorithm::Sha256, "c2"),
            verdict_id: "v2".to_string(),
            verdict_digest: Digest::new(DigestAlgorithm::Sha256, "v2"),
            decision: onto_protocol::loop_protocol::CandidateLoopDecision::FinalizeCandidate,
        };

        let r1 = executor.apply_transition(&handle, &directive).await.unwrap();
        let r2 = executor.apply_transition(&handle, &directive).await.unwrap();
        assert_eq!(r1.decision_id, r2.decision_id);
        assert!(matches!(r1.outcome, TransitionOutcome::Finalized { .. }));
        // Second call returns cached receipt (outcome is the same Finalized)
    }

    #[tokio::test]
    async fn same_decision_different_directive_is_conflict() {
        let mock = Arc::new(MockLoopRuntime::new());
        let executor = AttemptExecutorAdapter::new(mock);
        let handle = executor.start_attempt("att-4", "task", Some(3)).await.unwrap();

        let d1 = LoopDirective::CandidateBound {
            decision_id: "dec-conflict".to_string(), attempt_id: "att-4".to_string(),
            candidate_id: "c1".to_string(), candidate_digest: Digest::new(DigestAlgorithm::Sha256, "c1"),
            verdict_id: "v1".to_string(), verdict_digest: Digest::new(DigestAlgorithm::Sha256, "v1"),
            decision: onto_protocol::loop_protocol::CandidateLoopDecision::FinalizeCandidate,
        };
        let d2 = LoopDirective::CandidateBound {
            decision_id: "dec-conflict".to_string(), attempt_id: "att-4".to_string(),
            candidate_id: "c1".to_string(), candidate_digest: Digest::new(DigestAlgorithm::Sha256, "c1"),
            verdict_id: "v1".to_string(), verdict_digest: Digest::new(DigestAlgorithm::Sha256, "v-DIFFERENT"),
            decision: onto_protocol::loop_protocol::CandidateLoopDecision::FinalizeCandidate,
        };

        let _ = executor.apply_transition(&handle, &d1).await.unwrap();
        let r2 = executor.apply_transition(&handle, &d2).await;
        assert!(r2.is_err(), "same decision_id + different digest → IdempotencyConflict");
    }
}
