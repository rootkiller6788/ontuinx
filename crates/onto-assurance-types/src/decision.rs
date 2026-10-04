//! Decision types — the Onto kernel's verdict on execution and settlement.
//!
//! Split into two phases:
//!   1. PreExecutionDecision — produced BEFORE real external side effects.
//!   2. SettlementDecision  — produced AFTER execution/staging.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::enums::{BudgetOutcome, CriterionStatus, LifecycleState, ReasonCode, TaskOutcome};
use crate::ids::{AttemptId, BundleId, RunId};

// ══════════════════════════════════════════════════════════════════
// PreExecutionDecision — before the first real external side effect
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreExecutionDecision {
    /// Proceed with execution.
    Allow,
    /// Do not execute.  Return the reason to the Agent.
    Deny { reason: ReasonCode },
    /// Execution requires human approval first.
    RequireApproval { reason: ReasonCode },
    /// Execute with restricted capabilities (e.g. read-only mount, no network).
    RequireRestriction { reason: ReasonCode, restrictions: Vec<Restriction> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Restriction {
    ReadOnlyFilesystem,
    NoNetwork,
    NoSubprocess,
    TimeoutSeconds(u64),
}

// ══════════════════════════════════════════════════════════════════
// SettlementDecision — after execution/staging
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettlementDecision {
    /// Publish staged effects to the real world.
    Commit,
    /// Discard staged effects (or rollback transaction).
    Rollback { reason: ReasonCode },
    /// Acknowledge the effect happened (for Irreversible/Compensatable).
    Confirm,
    /// Run the compensating action.
    Compensate { reason: ReasonCode, compensation_plan: CompensationPlan },
    /// Snapshot and hold for human decision.
    Freeze { reason: ReasonCode },
    /// Freeze + notify a human operator.
    Escalate { reason: ReasonCode },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationPlan {
    pub description: String,
    pub estimated_effect: String,
    pub idempotency_key: Option<String>,
}

// ══════════════════════════════════════════════════════════════════
// Session-level result — three orthogonal dimensions
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionResult {
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub task_outcome: TaskOutcome,
    pub budget_outcome: BudgetOutcome,
    pub lifecycle_state: LifecycleState,
    pub evidence_bundle_id: BundleId,
    pub criterion_results: Vec<CriterionResult>,
    pub decided_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriterionResult {
    pub criterion_id: crate::ids::CriterionId,
    pub status: CriterionStatus,
    pub detail: String,
}

// ══════════════════════════════════════════════════════════════════
// Attempt-level result
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptResult {
    pub attempt_id: AttemptId,
    pub attempt_number: u32,
    pub task_outcome: TaskOutcome,
    pub budget_outcome: BudgetOutcome,
    pub settlement: Option<SettlementDecision>,
    pub wall_time_ms: u64,
    pub llm_calls: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{AttemptId, BundleId, RunId};

    #[test]
    fn pre_execution_decision_serde() {
        let d = PreExecutionDecision::Allow;
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, r#""allow""#);
    }

    #[test]
    fn settlement_commit_serde() {
        let d = SettlementDecision::Commit;
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, r#""commit""#);
    }

    #[test]
    fn session_result_dimensions_independent() {
        // Budget exceeded but task succeeded — should be expressible
        let result = SessionResult {
            run_id: RunId::new(),
            attempt_id: AttemptId::new(),
            task_outcome: TaskOutcome::Success, // completed all criteria
            budget_outcome: BudgetOutcome::Depleted, // but ran out of money
            lifecycle_state: LifecycleState::Committed,
            evidence_bundle_id: BundleId::new(),
            criterion_results: vec![],
            decided_at: Utc::now(),
        };
        assert_eq!(result.task_outcome, TaskOutcome::Success);
        assert_eq!(result.budget_outcome, BudgetOutcome::Depleted);
        // Previously these would have been conflated into "EXHAUSTED"
    }
}
