//! Host-agnostic `RunFinalizationPort` — authoritative finalization contract.
//!
//! The `TurnRunExecutor` calls this port on every loop exit INSTEAD of
//! directly mapping exits to `TurnStatus`.  The port implementation
//! (typically `onto-ironclaw-adapter::run_finalization_adapter`) loads
//! evidence, runs the assurance kernel, and returns the authoritative
//! outcome.  The executor then applies the outcome.

use async_trait::async_trait;
use std::fmt;

/// Reason the loop exited — input fact, NOT the final judgment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizationExitReason {
    FinishRequested,
    ProviderStop,
    BudgetLimit,
    Stuck,
    Cancelled,
    Crashed,
}

impl fmt::Display for FinalizationExitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FinishRequested => write!(f, "finish"),
            Self::ProviderStop => write!(f, "stop"),
            Self::BudgetLimit => write!(f, "budget"),
            Self::Stuck => write!(f, "stuck"),
            Self::Cancelled => write!(f, "cancel"),
            Self::Crashed => write!(f, "crash"),
        }
    }
}

/// Loop budget state — input fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizationBudget {
    WithinBudget,
    Depleted,
}

/// Task outcome — three independent dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizationTaskOutcome {
    Success,
    Incomplete,
    Failed,
    EnvironmentError,
}

/// Budget outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizationBudgetOutcome {
    WithinBudget,
    Depleted,
    HardLimitReached,
}

/// Lifecycle action taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizationLifecycle {
    Committed,
    Continuing,
    RolledBack,
    Escalated,
}

/// Input to the finalization port.
#[derive(Debug, Clone)]
pub struct RunFinalizationRequest {
    pub run_id: String,
    pub attempt_id: String,
    pub exit_reason: FinalizationExitReason,
    pub budget: FinalizationBudget,
    /// P16-F1: real workspace staging root. None = no candidate source
    /// available (sandbox verifiers will be Unavailable).
    pub staging_root: Option<std::path::PathBuf>,
}

/// Authoritative output from the finalization port.
#[derive(Debug, Clone)]
pub struct RunFinalizationOutcome {
    pub task_outcome: FinalizationTaskOutcome,
    pub budget_outcome: FinalizationBudgetOutcome,
    pub lifecycle_state: FinalizationLifecycle,
    pub session_decision_id: String,
    /// B2-A: structured feedback from P16 findings. Empty for Generic mode.
    pub reason_codes: Vec<String>,
}

/// Host-agnostic finalization port.  The ONLY path to produce a terminal
/// verdict for a run.  Never call `TurnStateStore::complete()` without
/// going through this port first.
#[async_trait]
pub trait RunFinalizationPort: Send + Sync {
    async fn finalize(
        &self,
        request: RunFinalizationRequest,
    ) -> Result<RunFinalizationOutcome, RunFinalizationError>;
}

#[derive(Debug)]
pub enum RunFinalizationError {
    Internal(String),
}

impl std::error::Error for RunFinalizationError {}

impl fmt::Display for RunFinalizationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Internal(msg) => write!(f, "finalization error: {msg}"),
        }
    }
}
