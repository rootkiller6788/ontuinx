//! OntoLoop types — matching ralph-loop-agent semantics in Rust.
//!
//! Outer loop: while (not done && not stopped) { run → evaluate → feedback }
//! Inner loop: OntoRuntime Agent Loop (tool calls, LLM)
//! Evaluator: OntoAssure trusted verdict (not fuzzy verifyCompletion)

use serde::{Deserialize, Serialize};

use crate::ids::{AttemptId, RunId};

// ══════════════════════════════════════════════════════════════════
// Loop Settings (matches ralph-loop-agent stopWhen)
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopSettings {
    /// Maximum number of outer loop iterations.
    pub max_iterations: u32,

    /// Optional token budget limit.
    pub max_total_tokens: Option<u64>,

    /// Optional cost limit in cents.
    pub max_cost_cents: Option<u64>,

    /// Timeout per iteration in seconds.
    pub timeout_per_iteration_secs: u64,
}

impl Default for LoopSettings {
    fn default() -> Self {
        Self { max_iterations: 5, max_total_tokens: None, max_cost_cents: None, timeout_per_iteration_secs: 300 }
    }
}

impl LoopSettings {
    pub fn iteration_count_is(max: u32) -> Self {
        Self { max_iterations: max, ..Default::default() }
    }
    pub fn with_cost_limit(mut self, max_cents: u64) -> Self {
        self.max_cost_cents = Some(max_cents); self
    }
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_per_iteration_secs = secs; self
    }
}

// ══════════════════════════════════════════════════════════════════
// Loop State
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopState {
    pub iteration: u32,
    pub total_tokens_used: u64,
    pub total_cost_cents: u64,
    pub settings: LoopSettings,
    pub status: LoopStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopStatus {
    Running,
    Completed,
    Failed,
    BudgetExhausted,
    Escalated,
}

// ══════════════════════════════════════════════════════════════════
// Verification Result (replaces fuzzy verifyCompletion)
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Is the task complete?
    pub complete: bool,

    /// Reason for the verdict.
    pub reason: String,

    /// If not complete, what specific criteria are unsatisfied.
    pub unsatisfied_criteria: Vec<String>,

    /// If not complete, structured guidance for the next attempt
    /// (NOT a fuzzy "please continue" prompt).
    pub feedback: Option<LoopFeedback>,

    /// The OntoAssure decision that produced this result.
    pub source_decision: LoopDecision,
}

// ══════════════════════════════════════════════════════════════════
// Loop Decision (OntoAssure tells OntoLoop what to do next)
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopDecision {
    /// Task complete — exit loop with success.
    Commit,
    /// Continue with structured gaps.
    Continue,
    /// Rollback to a checkpoint.
    Rollback { checkpoint_id: Option<String> },
    /// Escalate to human or upstream.
    Escalate { reason: String },
    /// Environment or infrastructure error.
    EnvironmentError,
}

impl LoopDecision {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Commit | Self::Escalate { .. } | Self::EnvironmentError)
    }
}

// ══════════════════════════════════════════════════════════════════
// Structured Feedback (NOT "please fix it")
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopFeedback {
    /// What specific criteria were not met.
    pub missing_criteria: Vec<String>,

    /// Concrete repair suggestion (e.g. "add test_hello.py", not "fix tests").
    pub suggested_repair: Option<String>,

    /// Reference to failed evidence.
    pub evidence_refs: Vec<String>,

    /// Previous attempt ID for traceability.
    pub previous_attempt_id: Option<AttemptId>,
}

// ══════════════════════════════════════════════════════════════════
// Attempt Record
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub attempt_id: AttemptId,
    pub run_id: Option<RunId>,
    pub iteration: u32,
    pub task_outcome: Option<String>,
    pub lifecycle_state: Option<String>,
    pub feedback: Option<LoopFeedback>,
}

// ══════════════════════════════════════════════════════════════════
// Task Input
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntoLoopTask {
    pub objective: String,
    pub criteria: Vec<TaskCriterion>,
    pub settings: LoopSettings,
}

/// Request to start a continuation attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuationRequest {
    pub previous_attempt_id: AttemptId,
    pub feedback: Option<LoopFeedback>,
    pub iteration: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCriterion {
    pub name: String,
    pub description: String,
    pub kind: String, // "test_pass", "lint_pass", "build_pass", etc.
}
