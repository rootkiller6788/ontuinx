//! Executor traits — L2's public interfaces to L1 and L3.

use async_trait::async_trait;
use crate::candidate::AgentRunOutcome;
use crate::loop_protocol::{LoopDirective, TransitionReceipt};
use crate::sandbox::{SandboxInvocationError, SandboxRunResult, SandboxValidationRequest};

// ── VerificationExecutor (L1 calls this) ──

/// L1's single entry point for external tool execution.
/// GVisorVerificationExecutor is the only production implementation.
#[async_trait]
pub trait VerificationExecutor: Send + Sync {
    /// Create a single gVisor Sandbox, batch-execute all checks,
    /// and return per-check raw results.
    async fn execute_plan(
        &self,
        request: &SandboxValidationRequest,
    ) -> Result<SandboxRunResult, SandboxInvocationError>;
}

// ── AttemptExecutor (L3 calls this) ──

/// L3's interface for managing Agent Runs and applying decisions.
#[async_trait]
pub trait AttemptExecutor: Send + Sync {
    async fn start_attempt(
        &self,
        attempt_id: &str,
        objective: &str,
        max_iterations: Option<u32>,
    ) -> Result<AgentRunHandle, AttemptError>;

    async fn await_completion(
        &self,
        handle: &AgentRunHandle,
    ) -> Result<AgentRunOutcome, AttemptError>;

    /// Apply a LoopDirective. decision_id is the idempotency key.
    /// Same decision_id + same directive_digest → return cached receipt.
    /// Same decision_id + different directive_digest → IdempotencyConflict.
    async fn apply_transition(
        &self,
        handle: &AgentRunHandle,
        directive: &LoopDirective,
    ) -> Result<TransitionReceipt, AttemptError>;
}

#[derive(Debug, Clone)]
pub struct AgentRunHandle {
    pub run_id: String,
    pub attempt_id: String,
    pub staging_root: String,
    pub started_at: String,
}

#[derive(Debug, Clone)]
pub struct StartAttemptRequest {
    pub attempt_id: String,
    pub objective: String,
    pub max_iterations: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum AttemptError {
    #[error("already executed: {0}")]
    AlreadyExecuted(String),
    #[error("run not found: {0}")]
    NotFound(String),
    #[error("idempotency conflict: {0}")]
    IdempotencyConflict(String),
    #[error("infrastructure: {0}")]
    Infrastructure(String),
    #[error("internal: {0}")]
    Internal(String),
}
