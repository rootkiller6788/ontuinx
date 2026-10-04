//! Context for the `AfterLoopExit` hook point — fired when an agent loop
//! exits for any reason (finish, budget, stuck, crash, cancel, stop).
//!
//! This is the hook point where Onto's FinalizationGateway takes over
//! success determination (M5).

use ironclaw_host_api::RunId;

/// Context provided to hooks when an agent loop exits.
///
/// Read-only snapshot of the run's final state. Hooks express changes
/// through `ObserverFact` and `BeforeCapabilityHookDecision` return types.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AfterLoopExitContext {
    /// The run that just exited.
    pub run_id: RunId,

    /// Why the loop stopped.
    pub exit_reason: LoopExitReason,

    /// Number of iterations before exit.
    pub iteration_count: u32,

    /// Wall-clock duration in milliseconds.
    pub wall_time_ms: u64,

    /// Number of LLM calls made in this run.
    pub llm_call_count: u32,

    /// If the loop produced a final answer, the summary text.
    pub final_answer_summary: Option<String>,

    /// Raw exit signal from the provider or runner.
    pub exit_signal: String,
}

/// Why an agent loop stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoopExitReason {
    /// Agent called finish() / requested completion.
    FinishRequested,
    /// LLM provider issued stop.
    ProviderStop,
    /// Budget limit reached (cost, iterations, or wall clock).
    BudgetLimit,
    /// Agent made no progress for N consecutive steps.
    Stuck,
    /// User or system cancelled.
    Cancelled,
    /// Infrastructure or runtime crash.
    Crashed,
}

impl std::fmt::Display for LoopExitReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FinishRequested => write!(f, "finish_requested"),
            Self::ProviderStop => write!(f, "provider_stop"),
            Self::BudgetLimit => write!(f, "budget_limit"),
            Self::Stuck => write!(f, "stuck"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::Crashed => write!(f, "crashed"),
        }
    }
}
