//! SessionDecisionService — the FinalizationGateway core.
//!
//! Input:  ExitReason + RequirementVerdict + BudgetOutcome
//! Output: TaskOutcome × BudgetOutcome × LifecycleState (three orthogonal dimensions)
//!
//! Pure function — deterministic, no I/O, no OntoRuntime.

use onto_assurance_types::decision::SessionResult;
use onto_assurance_types::enums::{
    BudgetOutcome, ExitReason, LifecycleState, TaskOutcome,
};
use onto_assurance_types::evidence::RequirementVerdict;
use onto_assurance_types::ids::{AttemptId, BundleId, RunId};

// ══════════════════════════════════════════════════════════════════
// SessionDecisionService
// ══════════════════════════════════════════════════════════════════

/// Produce the final three-dimensional session result.
///
/// This is the ONLY path to produce a final `TaskOutcome`.
/// The Agent cannot set its own SUCCESS; OntoRuntime Mission cannot
/// self-mark as complete.
pub fn decide_session_at(
    run_id: RunId,
    attempt_id: AttemptId,
    exit_reason: ExitReason,
    verdict: &RequirementVerdict,
    budget: BudgetOutcome,
    evidence_bundle_id: BundleId,
    decided_at: chrono::DateTime<chrono::Utc>,
) -> SessionResult {
    let task_outcome = determine_task_outcome(exit_reason, verdict);
    let lifecycle_state = determine_lifecycle(exit_reason, &task_outcome, verdict);

    SessionResult {
        run_id,
        attempt_id,
        task_outcome,
        budget_outcome: budget,
        lifecycle_state,
        evidence_bundle_id,
        criterion_results: verdict
            .criterion_verdicts
            .iter()
            .map(|v| onto_assurance_types::decision::CriterionResult {
                criterion_id: v.criterion_id,
                status: if v.satisfied {
                    onto_assurance_types::enums::CriterionStatus::Satisfied
                } else {
                    onto_assurance_types::enums::CriterionStatus::Unsatisfied
                },
                detail: v.detail.clone(),
            })
            .collect(),
        decided_at,
    }
}

/// Convenience wrapper — uses current wall-clock time.  Not deterministic;
/// use `decide_session_at` for replay.
pub fn decide_session(
    run_id: RunId,
    attempt_id: AttemptId,
    exit_reason: ExitReason,
    verdict: &RequirementVerdict,
    budget: BudgetOutcome,
    evidence_bundle_id: BundleId,
) -> SessionResult {
    decide_session_at(
        run_id,
        attempt_id,
        exit_reason,
        verdict,
        budget,
        evidence_bundle_id,
        chrono::Utc::now(),
    )
}

/// Determine TaskOutcome from ExitReason + RequirementVerdict.
///
/// Key principle: ExitReason tells us WHY the agent stopped.
/// RequirementVerdict tells us WHAT was achieved.
/// These are orthogonal — budget exhaustion + all criteria met = SUCCESS.
fn determine_task_outcome(exit_reason: ExitReason, verdict: &RequirementVerdict) -> TaskOutcome {
    match exit_reason {
        ExitReason::FinishRequested | ExitReason::BudgetLimit | ExitReason::ProviderStop => {
            if verdict.overall_passed {
                TaskOutcome::Success
            } else if verdict.blocking_unsatisfied.is_empty() {
                TaskOutcome::Incomplete
            } else {
                TaskOutcome::Failed
            }
        }
        ExitReason::Stuck => TaskOutcome::Incomplete,
        ExitReason::Cancelled => TaskOutcome::Incomplete,
        ExitReason::Crashed => TaskOutcome::EnvironmentError,
    }
}

/// Determine LifecycleState.
///
/// COMMITTED: task is done, evidence is valid, no more work needed.
/// CONTINUING: task needs another attempt (not all criteria met, budget remains).
/// ROLLED_BACK: non-blocking failures, staged effects should be discarded.
/// ESCALATED: irrecoverable situation, needs human intervention.
fn determine_lifecycle(
    exit_reason: ExitReason,
    task_outcome: &TaskOutcome,
    verdict: &RequirementVerdict,
) -> LifecycleState {
    match exit_reason {
        ExitReason::Crashed => LifecycleState::Escalated,
        ExitReason::Cancelled => LifecycleState::RolledBack,
        ExitReason::Stuck if !verdict.overall_passed => LifecycleState::Escalated,
        _ => {
            match task_outcome {
                TaskOutcome::Success | TaskOutcome::Incomplete
                    if verdict.blocking_unsatisfied.is_empty() =>
                {
                    LifecycleState::Committed
                }
                TaskOutcome::Failed => LifecycleState::Continuing,
                TaskOutcome::EnvironmentError => LifecycleState::Escalated,
                _ => LifecycleState::Committed,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::evidence::{CriterionVerdict, RequirementVerdict};
    use onto_assurance_types::ids::CriterionId;

    fn make_verdict(passed: bool) -> RequirementVerdict {
        let cid = CriterionId::new();
        RequirementVerdict {
            overall_passed: passed,
            blocking_unsatisfied: if passed { vec![] } else { vec![cid] },
            non_blocking_unsatisfied: vec![],
            criterion_verdicts: vec![CriterionVerdict {
                criterion_id: cid,
                satisfied: passed,
                evidence_ids: vec![],
                detail: "".into(),
            }],
        }
    }

    #[test]
    fn finish_with_all_criteria_met_is_success() {
        let result = decide_session(
            RunId::new(),
            AttemptId::new(),
            ExitReason::FinishRequested,
            &make_verdict(true),
            BudgetOutcome::WithinBudget,
            BundleId::new(),
        );
        assert_eq!(result.task_outcome, TaskOutcome::Success);
        assert_eq!(result.lifecycle_state, LifecycleState::Committed);
    }

    #[test]
    fn budget_depleted_but_success_is_still_success() {
        // Critical: BudgetOutcome and TaskOutcome are independent dimensions
        let result = decide_session(
            RunId::new(),
            AttemptId::new(),
            ExitReason::BudgetLimit,
            &make_verdict(true),
            BudgetOutcome::Depleted,
            BundleId::new(),
        );
        assert_eq!(result.task_outcome, TaskOutcome::Success);
        assert_eq!(result.budget_outcome, BudgetOutcome::Depleted);
        assert_eq!(result.lifecycle_state, LifecycleState::Committed);
    }

    #[test]
    fn crash_is_environment_error_not_failed() {
        let result = decide_session(
            RunId::new(),
            AttemptId::new(),
            ExitReason::Crashed,
            &make_verdict(true),
            BudgetOutcome::WithinBudget,
            BundleId::new(),
        );
        assert_eq!(result.task_outcome, TaskOutcome::EnvironmentError);
        assert_eq!(result.lifecycle_state, LifecycleState::Escalated);
    }

    #[test]
    fn stuck_with_failure_is_escalated() {
        let result = decide_session(
            RunId::new(),
            AttemptId::new(),
            ExitReason::Stuck,
            &make_verdict(false),
            BudgetOutcome::WithinBudget,
            BundleId::new(),
        );
        assert_eq!(result.task_outcome, TaskOutcome::Incomplete);
        assert_eq!(result.lifecycle_state, LifecycleState::Escalated);
    }
}
