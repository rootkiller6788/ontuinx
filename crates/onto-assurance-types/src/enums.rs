//! Core enumerations for the Onto Assurance Kernel.
//!
//! These are the language-agnostic protocol types — frozen at schema v1.
//! No behaviour lives here; behaviour belongs in `onto-assurance-core`.

use serde::{Deserialize, Serialize};

// ══════════════════════════════════════════════════════════════════
// §1 — Three-Dimensional Outcome Model
// ══════════════════════════════════════════════════════════════════

/// DID the task succeed?  Evidence-driven, immutable after finalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskOutcome {
    Success,
    Incomplete,
    Failed,
    EnvironmentError,
}

impl TaskOutcome {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Success | Self::Failed | Self::EnvironmentError)
    }
}

/// DID we have enough resources?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetOutcome {
    WithinBudget,
    Depleted,
    HardLimitReached,
}

/// WHAT action was taken?  Monotonic — never goes backward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Running,
    Finalizing,
    Committed,
    Continuing,
    RolledBack,
    Escalated,
}

impl LifecycleState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Committed | Self::Escalated)
    }
}

// ══════════════════════════════════════════════════════════════════
// §2 — Exit Reason (input to FinalizationGateway)
// ══════════════════════════════════════════════════════════════════

/// WHY did the agent loop stop?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    /// Agent called finish() explicitly.
    FinishRequested,
    /// LLM provider issued a stop signal.
    ProviderStop,
    /// Budget (cost, iterations, or wall-clock) exhausted.
    BudgetLimit,
    /// Agent made no progress for N consecutive steps.
    Stuck,
    /// User or system cancelled the run.
    Cancelled,
    /// Unrecoverable infrastructure or runtime crash.
    Crashed,
}

// ══════════════════════════════════════════════════════════════════
// §3 — EffectClass
// ══════════════════════════════════════════════════════════════════

/// Side-effect classification.  Determined by trusted `EffectClassifier`,
/// NOT by the Agent or LLM.  Can only be upgraded (more restrictive), never
/// automatically downgraded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectClass {
    /// No side effects.
    Pure,
    /// Read-only observation.
    ReadOnly,
    /// Side effects confined to a staged environment — discardable.
    Staged,
    /// Real transaction support (e.g. SQL BEGIN/COMMIT/ROLLBACK).
    Transactional,
    /// Irreversible but with a compensating action available.
    Compensatable,
    /// Irreversible external effect — cannot be undone.
    Irreversible,
}

impl EffectClass {
    /// Does this class require a PreExecutionDecision before execution?
    pub fn requires_pre_decision(self) -> bool {
        matches!(self, Self::Compensatable | Self::Irreversible)
    }

    /// Does this class support staging (execute → verify → publish/discard)?
    pub fn supports_staging(self) -> bool {
        matches!(self, Self::Staged | Self::Transactional)
    }

    /// Is the side effect reversible after execution?
    pub fn is_reversible(self) -> bool {
        matches!(self, Self::Pure | Self::ReadOnly | Self::Staged | Self::Transactional)
    }
}

// ══════════════════════════════════════════════════════════════════
// §4 — Risk Level
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

// ══════════════════════════════════════════════════════════════════
// §5 — Transaction State
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionState {
    Prepared,
    Staged,
    Executing,
    Executed,
    Verifying,
    Verified,
    Decided,
    Publishing,
    Published,
    Discarding,
    Discarded,
    Compensating,
    Compensated,
    CompensationFailed,
    Freezing,
    Frozen,
    Finalized,
}

impl TransactionState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Published | Self::Discarded | Self::Compensated
                | Self::CompensationFailed | Self::Frozen | Self::Finalized
        )
    }

    pub fn is_settling(self) -> bool {
        matches!(
            self,
            Self::Publishing | Self::Discarding | Self::Compensating | Self::Freezing
        )
    }
}

// ══════════════════════════════════════════════════════════════════
// §6 — Failure & Recovery
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    ContractDenied,
    PolicyDenied,
    PermissionDenied,
    VerificationFailed,
    ExecutionFailed,
    SandboxTransient,
    SandboxFatal,
    PipelineInternal,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryOwner {
    None,
    Runtime,
    Ontocode,
}

// ══════════════════════════════════════════════════════════════════
// §7 — ReasonCode (structured, not free-text)
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReasonCode {
    pub domain: String,       // e.g. "verification", "policy", "evidence"
    pub code: String,         // e.g. "syntax_error", "hash_mismatch"
    pub detail: String,       // human-readable, bounded to 256 chars
}

// ══════════════════════════════════════════════════════════════════
// §8 — RunLifecycle state
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunLifecycle {
    Created,
    Running,
    Finalizing,
    Committed,
    Continuing,
    RolledBack,
    Escalated,
}

impl RunLifecycle {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Committed | Self::Escalated)
    }
}

// ══════════════════════════════════════════════════════════════════
// §9 — Approval state
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    NotRequired,
    Waiting,
    Granted,
    Rejected,
    Expired,
}

// ══════════════════════════════════════════════════════════════════
// §10 — Verdict per criterion
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionStatus {
    Satisfied,
    Unsatisfied,
    Blocking,
    Skipped,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementStatus {
    Satisfied,
    Unsatisfied,
    PartiallySatisfied,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_class_reversibility() {
        assert!(EffectClass::Pure.is_reversible());
        assert!(EffectClass::ReadOnly.is_reversible());
        assert!(EffectClass::Staged.is_reversible());
        assert!(EffectClass::Transactional.is_reversible());
        assert!(!EffectClass::Compensatable.is_reversible());
        assert!(!EffectClass::Irreversible.is_reversible());
    }

    #[test]
    fn transaction_state_terminal() {
        assert!(TransactionState::Published.is_terminal());
        assert!(TransactionState::Discarded.is_terminal());
        assert!(TransactionState::Compensated.is_terminal());
        assert!(!TransactionState::Prepared.is_terminal());
        assert!(!TransactionState::Executing.is_terminal());
    }

    #[test]
    fn outcome_serde_roundtrip() {
        let outcome = TaskOutcome::Success;
        let json = serde_json::to_string(&outcome).unwrap();
        assert_eq!(json, r#""success""#);
        let back: TaskOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome, back);
    }
}
