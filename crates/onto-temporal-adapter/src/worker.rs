//! OntoLoopWorker trait — the single entry point Go calls.
//!
//! Go OntoFlow dispatches a WorkItem via Temporal ActivityTask.
//! The Activity payload is LoopInvocationRequest (JSON).
//! The Activity result is LoopTerminalEnvelope (JSON).
//!
//! This trait wraps the full OntoLoop lifecycle behind one async method.

use async_trait::async_trait;

use crate::protocol::LoopInvocationRequest;
use crate::protocol::LoopTerminalEnvelope;

/// Errors that can occur during OntoLoop execution.
#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    /// The request references a loop_id that conflicts with an existing
    /// loop (different generation or contract_ref).
    #[error("idempotency violation: {0}")]
    IdempotencyViolation(String),

    /// The task specification or contract could not be resolved.
    #[error("specification not found: {0}")]
    SpecNotFound(String),

    /// The OntoLoop runtime encountered an internal error.
    #[error("runtime error: {0}")]
    Runtime(String),

    /// The loop was cancelled by an external signal.
    #[error("cancelled")]
    Cancelled,

    /// The configured budget was exhausted.
    #[error("budget exhausted: {0}")]
    BudgetExhausted(String),
}

/// The single interface Go calls to execute an OntoLoop.
///
/// # Contract
///
/// - **Idempotent**: calling with the same `loop_id` + `execution_generation`
///   must return the same terminal envelope (or resume an in-flight loop).
/// - **At-most-once per generation**: a new generation should only be created
///   by Go after a previous generation has reached a terminal state.
/// - **Go only gets references**: the returned envelope contains references
///   to decisions and artifacts, not the full content.
#[async_trait]
pub trait OntoLoopWorker: Send + Sync {
    /// Execute an OntoLoop to terminal state (or resume a running one).
    ///
    /// This is the ONLY method Go calls. All Attempt management, checkpoint
    /// selection, progress tracking, and budget enforcement happens inside
    /// this method.
    async fn run_or_resume_to_terminal(
        &self,
        request: LoopInvocationRequest,
    ) -> Result<LoopTerminalEnvelope, WorkerError>;
}
