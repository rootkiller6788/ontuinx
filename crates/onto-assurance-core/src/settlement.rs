//! SettlementDecision derivation — deterministic, per EffectClass.
//!
//! After execution/staging completes, determine how to settle the effects.
//! Pure function — no I/O.

use onto_assurance_types::decision::SettlementDecision;
use onto_assurance_types::enums::{EffectClass, ReasonCode, TaskOutcome};
use onto_assurance_types::evidence::RequirementVerdict;

// ══════════════════════════════════════════════════════════════════
// Settlement Decision
// ══════════════════════════════════════════════════════════════════

/// Derive the appropriate SettlementDecision for a completed transaction.
///
/// Inputs:
///   - effect_class: what kind of side effects were involved
///   - task_outcome: did the verification succeed?
///   - verdict: detailed requirement results
pub fn derive_settlement(
    effect_class: EffectClass,
    task_outcome: TaskOutcome,
    verdict: &RequirementVerdict,
) -> Option<SettlementDecision> {
    match task_outcome {
        TaskOutcome::Success => settlement_on_success(effect_class),
        TaskOutcome::Incomplete => settlement_on_incomplete(effect_class, verdict),
        TaskOutcome::Failed => settlement_on_failed(effect_class, verdict),
        TaskOutcome::EnvironmentError => Some(SettlementDecision::Freeze {
            reason: ReasonCode {
                domain: "infrastructure".into(),
                code: "environment_error".into(),
                detail: "infrastructure failure — freezing for manual review".into(),
            },
        }),
    }
}

fn settlement_on_success(effect_class: EffectClass) -> Option<SettlementDecision> {
    match effect_class {
        EffectClass::Pure | EffectClass::ReadOnly => {
            // Nothing to settle — no side effects to publish or confirm
            None
        }
        EffectClass::Staged | EffectClass::Transactional => {
            Some(SettlementDecision::Commit)
        }
        EffectClass::Compensatable | EffectClass::Irreversible => {
            Some(SettlementDecision::Confirm)
        }
    }
}

fn settlement_on_incomplete(
    effect_class: EffectClass,
    verdict: &RequirementVerdict,
) -> Option<SettlementDecision> {
    if verdict.blocking_unsatisfied.is_empty() {
        // Non-blocking criteria only — effects are valid, commit them
        settlement_on_success(effect_class)
    } else {
        // Blocking criteria unsatisfied — rollback or escalate
        match effect_class {
            EffectClass::Pure | EffectClass::ReadOnly => None,
            EffectClass::Staged => Some(SettlementDecision::Rollback {
                reason: ReasonCode {
                    domain: "verification".into(),
                    code: "blocking_unsatisfied".into(),
                    detail: format!(
                        "{} blocking criteria not met — rolling back staged effects",
                        verdict.blocking_unsatisfied.len()
                    ),
                },
            }),
            EffectClass::Transactional => Some(SettlementDecision::Rollback {
                reason: ReasonCode {
                    domain: "verification".into(),
                    code: "blocking_unsatisfied".into(),
                    detail: "blocking criteria not met — rolling back transaction".into(),
                },
            }),
            EffectClass::Compensatable => Some(SettlementDecision::Compensate {
                reason: ReasonCode {
                    domain: "verification".into(),
                    code: "blocking_unsatisfied".into(),
                    detail: format!(
                        "{} blocking criteria not met — running compensation",
                        verdict.blocking_unsatisfied.len()
                    ),
                },
                compensation_plan: onto_assurance_types::decision::CompensationPlan {
                    description: "Compensation for incomplete compensatable action".into(),
                    estimated_effect: "Reversal of primary effect".into(),
                    idempotency_key: None,
                },
            }),
            EffectClass::Irreversible => Some(SettlementDecision::Escalate {
                reason: ReasonCode {
                    domain: "verification".into(),
                    code: "irreversible_incomplete".into(),
                    detail: "irreversible action did not meet all criteria — escalate".into(),
                },
            }),
        }
    }
}

fn settlement_on_failed(
    effect_class: EffectClass,
    verdict: &RequirementVerdict,
) -> Option<SettlementDecision> {
    // Failed = blocking criteria unsatisfied
    settlement_on_incomplete(effect_class, verdict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::evidence::{CriterionVerdict, RequirementVerdict};
    use onto_assurance_types::ids::CriterionId;

    fn verdict(passed: bool, blocking_count: usize) -> RequirementVerdict {
        let mut blocking = Vec::new();
        for _ in 0..blocking_count {
            blocking.push(CriterionId::new());
        }
        RequirementVerdict {
            overall_passed: passed,
            blocking_unsatisfied: blocking,
            non_blocking_unsatisfied: vec![],
            criterion_verdicts: vec![],
        }
    }

    #[test]
    fn staged_success_commits() {
        let d = derive_settlement(EffectClass::Staged, TaskOutcome::Success, &verdict(true, 0));
        assert_eq!(d, Some(SettlementDecision::Commit));
    }

    #[test]
    fn pure_success_no_settlement() {
        let d = derive_settlement(EffectClass::Pure, TaskOutcome::Success, &verdict(true, 0));
        assert_eq!(d, None);
    }

    #[test]
    fn irreversible_success_confirms() {
        let d = derive_settlement(EffectClass::Irreversible, TaskOutcome::Success, &verdict(true, 0));
        assert_eq!(d, Some(SettlementDecision::Confirm));
    }

    #[test]
    fn staged_incomplete_with_blocking_rolls_back() {
        let d = derive_settlement(EffectClass::Staged, TaskOutcome::Incomplete, &verdict(false, 2));
        assert!(matches!(d, Some(SettlementDecision::Rollback { .. })));
    }

    #[test]
    fn compensatable_incomplete_compensates() {
        let d = derive_settlement(
            EffectClass::Compensatable,
            TaskOutcome::Incomplete,
            &verdict(false, 1),
        );
        assert!(matches!(d, Some(SettlementDecision::Compensate { .. })));
    }

    #[test]
    fn irreversible_failed_escalates() {
        let d = derive_settlement(EffectClass::Irreversible, TaskOutcome::Failed, &verdict(false, 1));
        assert!(matches!(d, Some(SettlementDecision::Escalate { .. })));
    }

    #[test]
    fn environment_error_freezes() {
        let d = derive_settlement(
            EffectClass::Staged,
            TaskOutcome::EnvironmentError,
            &verdict(true, 0),
        );
        assert!(matches!(d, Some(SettlementDecision::Freeze { .. })));
    }
}
