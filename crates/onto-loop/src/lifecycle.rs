//! LifecycleReducer — the single point of state reduction.
//!
//! Every Directive + TransitionReceipt goes through this reducer.
//! No other code may map directive outcomes to terminal states.
//!
//! # Reduction table
//!
//! | Directive | Transition Outcome | Lifecycle |
//! |-----------|-------------------|-----------|
//! | FinalizeCandidate | Finalized(Committed) | Committed |
//! | FinalizeCandidate | Finalized(RolledBack) | Failed |
//! | Continue | Continued | Continue |
//! | Freeze | Frozen | Frozen |
//! | Escalate | Escalated | Escalated |
//! | Continue (stalled) | — | Continue (p16-4: Freeze after threshold) |

use onto_protocol::loop_protocol::{
    CandidateLoopDecision, NoCandidateLoopDecision, LoopDirective,
    SettlementState, TransitionReceipt, TransitionOutcome,
};

/// The reduced lifecycle state after a directive has been executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lifecycle {
    /// Continue to next Attempt.
    Continue,
    /// Attempt fully committed.
    Committed { receipt_ref: String },
    /// Attempt failed (rolled back, attempt closed).
    Failed { reason: String },
    /// Attempt frozen for inspection.
    Frozen { reason: String },
    /// Attempt escalated for manual resolution.
    Escalated { reason: String },
}

impl Lifecycle {
    /// True if this lifecycle state ends the loop.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Lifecycle::Continue)
    }
}

/// The single authoritative state reducer.
pub struct LifecycleReducer;

impl LifecycleReducer {
    pub fn new() -> Self { Self }

    /// Reduce a directive + receipt into a lifecycle state.
    ///
    /// Every production code path that receives a TransitionReceipt MUST
    /// call this function.  No bypass.
    pub fn reduce(
        &self,
        directive: &LoopDirective,
        receipt: &TransitionReceipt,
    ) -> Lifecycle {
        match &receipt.outcome {
            TransitionOutcome::Finalized { settlement, receipt_ref } => {
                match settlement {
                    SettlementState::Committed => {
                        Lifecycle::Committed { receipt_ref: receipt_ref.clone() }
                    }
                    SettlementState::RolledBack => {
                        Lifecycle::Failed {
                            reason: format!("RolledBack: receipt={}", receipt_ref),
                        }
                    }
                    SettlementState::Frozen => {
                        Lifecycle::Frozen {
                            reason: format!("Frozen by TransitionExecutor: receipt={}", receipt_ref),
                        }
                    }
                    SettlementState::Unknown => {
                        Lifecycle::Escalated {
                            reason: format!("Unknown settlement: receipt={}", receipt_ref),
                        }
                    }
                }
            }
            TransitionOutcome::Continued => Lifecycle::Continue,
            TransitionOutcome::Frozen => {
                Lifecycle::Frozen { reason: "Frozen by TransitionExecutor".into() }
            }
            TransitionOutcome::Escalated => {
                Lifecycle::Escalated { reason: "Escalated by TransitionExecutor".into() }
            }
            TransitionOutcome::AttemptClosed => {
                Lifecycle::Failed { reason: "AttemptClosed".into() }
            }
            TransitionOutcome::CandidateDiscarded => {
                Lifecycle::Failed { reason: "CandidateDiscarded".into() }
            }
            TransitionOutcome::CheckpointRestored { .. } => {
                Lifecycle::Continue
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_protocol::digest::{Digest, DigestAlgorithm};

    fn d(s: &str) -> Digest { Digest::new(DigestAlgorithm::Sha256, s.to_string()) }

    #[test]
    fn p16_4_1_finalize_committed_is_committed() {
        let reducer = LifecycleReducer::new();
        let directive = LoopDirective::CandidateBound {
            decision_id: "d1".into(),
            attempt_id: "a1".into(),
            candidate_id: "c1".into(),
            candidate_digest: d("c1"),
            verdict_id: "v1".into(),
            verdict_digest: d("v1"),
            decision: CandidateLoopDecision::FinalizeCandidate,
        };
        let receipt = TransitionReceipt {
            receipt_id: "r1".into(),
            receipt_digest: d("r1"),
            decision_id: "d1".into(),
            directive_digest: d("dir1"),
            attempt_id: "a1".into(),
            candidate_id: Some("c1".into()),
            candidate_digest: Some(d("c1")),
            verdict_id: Some("v1".into()),
            verdict_digest: Some(d("v1")),
            outcome: TransitionOutcome::Finalized {
                settlement: SettlementState::Committed,
                receipt_ref: "r1".into(),
            },
        };
        let lifecycle = reducer.reduce(&directive, &receipt);
        assert_eq!(lifecycle, Lifecycle::Committed { receipt_ref: "r1".into() });
        assert!(lifecycle.is_terminal());
    }

    #[test]
    fn p16_4_2_continued_is_not_terminal() {
        let reducer = LifecycleReducer::new();
        let directive = LoopDirective::CandidateBound {
            decision_id: "d1".into(),
            attempt_id: "a1".into(),
            candidate_id: "c1".into(),
            candidate_digest: d("c1"),
            verdict_id: "v1".into(),
            verdict_digest: d("v1"),
            decision: CandidateLoopDecision::Continue { feedback: vec![] },
        };
        let receipt = TransitionReceipt {
            receipt_id: "r1".into(),
            receipt_digest: d("r1"),
            decision_id: "d1".into(),
            directive_digest: d("dir1"),
            attempt_id: "a1".into(),
            candidate_id: Some("c1".into()),
            candidate_digest: Some(d("c1")),
            verdict_id: Some("v1".into()),
            verdict_digest: Some(d("v1")),
            outcome: TransitionOutcome::Continued,
        };
        let lifecycle = reducer.reduce(&directive, &receipt);
        assert_eq!(lifecycle, Lifecycle::Continue);
        assert!(!lifecycle.is_terminal());
    }

    #[test]
    fn p16_4_3_rolled_back_is_failed() {
        let reducer = LifecycleReducer::new();
        let directive = LoopDirective::CandidateBound {
            decision_id: "d1".into(),
            attempt_id: "a1".into(),
            candidate_id: "c1".into(),
            candidate_digest: d("c1"),
            verdict_id: "v1".into(),
            verdict_digest: d("v1"),
            decision: CandidateLoopDecision::FinalizeCandidate,
        };
        let receipt = TransitionReceipt {
            receipt_id: "r1".into(),
            receipt_digest: d("r1"),
            decision_id: "d1".into(),
            directive_digest: d("dir1"),
            attempt_id: "a1".into(),
            candidate_id: Some("c1".into()),
            candidate_digest: Some(d("c1")),
            verdict_id: Some("v1".into()),
            verdict_digest: Some(d("v1")),
            outcome: TransitionOutcome::Finalized {
                settlement: SettlementState::RolledBack,
                receipt_ref: "r1".into(),
            },
        };
        let lifecycle = reducer.reduce(&directive, &receipt);
        assert!(matches!(lifecycle, Lifecycle::Failed { .. }));
    }

    #[test]
    fn p16_4_4_frozen_is_frozen() {
        let reducer = LifecycleReducer::new();
        let directive = LoopDirective::CandidateBound {
            decision_id: "d1".into(),
            attempt_id: "a1".into(),
            candidate_id: "c1".into(),
            candidate_digest: d("c1"),
            verdict_id: "v1".into(),
            verdict_digest: d("v1"),
            decision: CandidateLoopDecision::Freeze { reason: "test".into() },
        };
        let receipt = TransitionReceipt {
            receipt_id: "r1".into(),
            receipt_digest: d("r1"),
            decision_id: "d1".into(),
            directive_digest: d("dir1"),
            attempt_id: "a1".into(),
            candidate_id: Some("c1".into()),
            candidate_digest: Some(d("c1")),
            verdict_id: Some("v1".into()),
            verdict_digest: Some(d("v1")),
            outcome: TransitionOutcome::Frozen,
        };
        let lifecycle = reducer.reduce(&directive, &receipt);
        assert!(matches!(lifecycle, Lifecycle::Frozen { .. }));
    }
}
