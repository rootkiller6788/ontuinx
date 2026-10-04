//! AttemptDecisionService — per-attempt settlement logic.
//!
//! After each attempt, decide: retry, commit, rollback, compensate, or escalate.
//! Pure function — deterministic, no I/O.

use onto_assurance_types::decision::SettlementDecision;
use onto_assurance_types::enums::{
    BudgetOutcome, EffectClass, FailureKind, ReasonCode, RetryOwner, TaskOutcome,
};
use onto_assurance_types::evidence::RequirementVerdict;

// ══════════════════════════════════════════════════════════════════
// AttemptDecision
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptDecision {
    pub retry: bool,
    pub retry_owner: RetryOwner,
    pub settlement: Option<SettlementDecision>,
    pub failure_kind: FailureKind,
}

/// Decide what to do after one attempt completes.
pub fn decide_attempt(
    task_outcome: TaskOutcome,
    budget: BudgetOutcome,
    verdict: &RequirementVerdict,
    effect_class: EffectClass,
    attempt_number: u32,
    max_attempts: u32,
) -> AttemptDecision {
    match task_outcome {
        TaskOutcome::Success => AttemptDecision {
            retry: false,
            retry_owner: RetryOwner::None,
            settlement: resolve_settlement_on_success(effect_class),
            failure_kind: FailureKind::Unknown, // not a failure
        },
        TaskOutcome::Incomplete => {
            if attempt_number < max_attempts && budget == BudgetOutcome::WithinBudget {
                AttemptDecision {
                    retry: true,
                    retry_owner: RetryOwner::Ontocode,
                    settlement: None,
                    failure_kind: FailureKind::ExecutionFailed,
                }
            } else {
                AttemptDecision {
                    retry: false,
                    retry_owner: RetryOwner::None,
                    settlement: Some(SettlementDecision::Escalate {
                        reason: ReasonCode {
                            domain: "attempt".into(),
                            code: "max_attempts_exhausted".into(),
                            detail: format!("{} attempts, budget: {:?}", attempt_number, budget),
                        },
                    }),
                    failure_kind: FailureKind::Unknown,
                }
            }
        }
        TaskOutcome::Failed => {
            if verdict.blocking_unsatisfied.is_empty()
                && attempt_number < max_attempts
                && budget == BudgetOutcome::WithinBudget
            {
                AttemptDecision {
                    retry: true,
                    retry_owner: RetryOwner::Ontocode,
                    settlement: Some(SettlementDecision::Rollback {
                        reason: ReasonCode {
                            domain: "attempt".into(),
                            code: "retry_with_rollback".into(),
                            detail: "non-blocking failures, retrying".into(),
                        },
                    }),
                    failure_kind: FailureKind::VerificationFailed,
                }
            } else {
                AttemptDecision {
                    retry: false,
                    retry_owner: RetryOwner::None,
                    settlement: Some(SettlementDecision::Escalate {
                        reason: ReasonCode {
                            domain: "attempt".into(),
                            code: "blocking_failure".into(),
                            detail: format!(
                                "{} blocking criteria unsatisfied",
                                verdict.blocking_unsatisfied.len()
                            ),
                        },
                    }),
                    failure_kind: FailureKind::VerificationFailed,
                }
            }
        }
        TaskOutcome::EnvironmentError => AttemptDecision {
            retry: false,
            retry_owner: RetryOwner::None,
            settlement: Some(SettlementDecision::Freeze {
                reason: ReasonCode {
                    domain: "infrastructure".into(),
                    code: "environment_error".into(),
                    detail: "infrastructure failure — freezing for manual review".into(),
                },
            }),
            failure_kind: FailureKind::SandboxFatal,
        },
    }
}

/// Resolve the correct SettlementDecision when the task succeeded.
fn resolve_settlement_on_success(effect_class: EffectClass) -> Option<SettlementDecision> {
    match effect_class {
        EffectClass::Pure | EffectClass::ReadOnly => None, // nothing to settle
        EffectClass::Staged | EffectClass::Transactional => Some(SettlementDecision::Commit),
        EffectClass::Compensatable | EffectClass::Irreversible => {
            Some(SettlementDecision::Confirm)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_verdict() -> RequirementVerdict {
        RequirementVerdict {
            overall_passed: true,
            blocking_unsatisfied: vec![],
            non_blocking_unsatisfied: vec![],
            criterion_verdicts: vec![],
        }
    }

    #[test]
    fn success_with_staged_commits() {
        let d = decide_attempt(
            TaskOutcome::Success,
            BudgetOutcome::WithinBudget,
            &empty_verdict(),
            EffectClass::Staged,
            1,
            3,
        );
        assert!(!d.retry);
        assert_eq!(d.settlement, Some(SettlementDecision::Commit));
    }

    #[test]
    fn success_with_irreversible_confirms() {
        let d = decide_attempt(
            TaskOutcome::Success,
            BudgetOutcome::WithinBudget,
            &empty_verdict(),
            EffectClass::Irreversible,
            1,
            3,
        );
        assert!(!d.retry);
        assert_eq!(d.settlement, Some(SettlementDecision::Confirm));
    }

    #[test]
    fn incomplete_within_budget_retries() {
        let d = decide_attempt(
            TaskOutcome::Incomplete,
            BudgetOutcome::WithinBudget,
            &empty_verdict(),
            EffectClass::Staged,
            1,
            3,
        );
        assert!(d.retry);
        assert_eq!(d.retry_owner, RetryOwner::Ontocode);
    }

    #[test]
    fn incomplete_budget_depleted_escalates() {
        let d = decide_attempt(
            TaskOutcome::Incomplete,
            BudgetOutcome::Depleted,
            &empty_verdict(),
            EffectClass::Staged,
            3,
            3,
        );
        assert!(!d.retry);
        assert!(matches!(d.settlement, Some(SettlementDecision::Escalate { .. })));
    }

    #[test]
    fn environment_error_freezes() {
        let d = decide_attempt(
            TaskOutcome::EnvironmentError,
            BudgetOutcome::WithinBudget,
            &empty_verdict(),
            EffectClass::Irreversible,
            1,
            3,
        );
        assert!(!d.retry);
        assert!(matches!(d.settlement, Some(SettlementDecision::Freeze { .. })));
    }
}
