//! OntoFlow ↔ OntoLoop wire protocol types.
//!
//! These types are the serialization contract between Go OntoFlow and Rust
//! OntoLoop Worker. They are designed for JSON serialization (serde).
//!
//! ## Design invariants
//!
//! - Go sends references (artifact refs), NOT content
//! - Go does NOT send: AttemptNum, RepairPrompt, Checkpoint selection, ProgressSnapshot
//! - Rust returns references (decision_id, evidence_bundle_ref), NOT full evidence
//! - LoopTerminalEnvelope is a worker REPORT — Go must verify via AuthorityProjectionPort

use serde::{Deserialize, Serialize};

// ══════════════════════════════════════════════════════════════════
// LoopInvocationRequest — Go OntoFlow → Rust Worker
// ══════════════════════════════════════════════════════════════════

/// Request from Go OntoFlow to execute a single OntoLoop WorkItem.
///
/// Go sends what the task IS and WHERE its inputs live.
/// Go does NOT send how to execute (that's the Loop's job).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopInvocationRequest {
    pub schema_version: u32,

    // ── Identity ──
    pub flow_id: String,
    pub work_item_id: String,
    pub loop_id: String,

    // ── Task definition (references, not content) ──
    pub task_spec_ref: String,
    pub contract_ref: String,
    pub policy_ref: String,
    pub input_artifact_refs: Vec<String>,

    // ── Resource & risk ──
    pub resource_class: String,
    pub risk_class: String,
    pub trust_requirement: String,

    // ── Flow-assigned budget grant ──
    pub budget_grant_ref: String,
    pub budget_grant_hash: String,

    // ── Version & idempotency ──
    pub execution_generation: u64,
    pub idempotency_key: String,

    // ── Optional constraints ──
    pub deadline: Option<String>,

    // ── Input binding hash ──
    /// Hash over all input fields. Rust stores this at receipt; Go verifies
    /// it is returned unchanged in the envelope.
    pub request_binding_hash: String,
}

impl LoopInvocationRequest {
    /// The canonical identity key for this invocation.
    /// flow_id + work_item_id + generation → unique loop_id
    pub fn identity_key(&self) -> String {
        format!(
            "{}:{}:gen{}",
            self.flow_id, self.work_item_id, self.execution_generation
        )
    }

    /// Compute the request binding hash over all immutable input fields.
    /// Uses SHA-256 (same as Go side) for cross-language consistency.
    pub fn compute_request_binding_hash(&self) -> String {
        use sha2::{Sha256, Digest};
        let payload = format!(
            "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            self.flow_id,
            self.work_item_id,
            self.loop_id,
            self.task_spec_ref,
            self.contract_ref,
            self.policy_ref,
            self.input_artifact_refs.join(","),
            self.budget_grant_ref,
            self.budget_grant_hash,
            self.execution_generation
        );
        let hash = Sha256::digest(payload.as_bytes());
        hex::encode(&hash[..8]) // first 8 bytes = 16 hex chars, matching Go
    }
}

// ══════════════════════════════════════════════════════════════════
// LoopTerminalState
// ══════════════════════════════════════════════════════════════════

/// The terminal state reported by the Worker.
///
/// Naming: these are Worker-reported states. Go accepts only after
/// verifying via AuthorityProjectionPort.
///
/// `LoopBudgetExhausted` refers ONLY to Rust OntoLoop internal budget
/// (Attempts, NoProgress, Regressions). Flow-level budget is Go's domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopTerminalState {
    /// OntoAssure verified success + Settlement committed.
    Committed,
    /// Needs human intervention.
    Escalated,
    /// External environment unavailable (not the agent's fault).
    EnvironmentBlocked,
    /// Rust OntoLoop internal budget exhausted (Attempts, NoProgress, Regressions).
    LoopBudgetExhausted,
    /// Cancelled by external signal.
    Cancelled,
    /// Protocol error (binding mismatch, version mismatch, corrupted idempotency).
    ProtocolFailed,
}

impl LoopTerminalState {
    /// Can downstream WorkItems be unlocked?
    pub fn allows_downstream(&self) -> bool {
        matches!(self, Self::Committed)
    }

    /// Should Go retry this WorkItem?
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::EnvironmentBlocked)
    }

    /// Is this a definitive end (no further automated action)?
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Committed | Self::Escalated | Self::Cancelled | Self::ProtocolFailed
        )
    }

    /// Does this state require Go to check Flow budget?
    pub fn is_budget_related(&self) -> bool {
        matches!(self, Self::LoopBudgetExhausted)
    }
}

// ══════════════════════════════════════════════════════════════════
// LoopTerminalEnvelope — Rust Worker → Go OntoFlow
// ══════════════════════════════════════════════════════════════════

/// Terminal report from Rust Worker to Go OntoFlow.
///
/// This is a WORKER REPORT, not an authoritative fact. Go MUST verify
/// the outcome via AuthorityProjectionPort before accepting it.
///
/// Contains references to authoritative decisions and artifacts, NOT the
/// full evidence or workspace contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopTerminalEnvelope {
    pub schema_version: u32,

    // ── Identity ──
    pub flow_id: String,
    pub work_item_id: String,
    pub loop_id: String,
    pub execution_generation: u64,

    // ── Input binding ──
    /// Hash of the original LoopInvocationRequest. Rust stores this at
    /// receipt and returns it unchanged. Go verifies it matches what was sent.
    pub request_binding_hash: String,

    // ── Worker-reported terminal state (NOT yet accepted by Go) ──
    /// Named `reported_*` to emphasize: this is the Worker's claim.
    /// Go transitions: OutcomeReported → AuthorityVerifying → Committed.
    pub reported_terminal_state: LoopTerminalState,

    // ── Authority references (not content) ──
    pub decision_id: Option<String>,
    pub decision_hash: Option<String>,
    pub evidence_bundle_ref: Option<String>,
    pub settlement_receipt_ref: Option<String>,

    // ── Artifact references ──
    pub output_artifact_refs: Vec<String>,
    pub output_checkpoint_hash: Option<String>,

    // ── Output binding ──
    /// Hash binding request_binding_hash + all outcome fields.
    pub outcome_binding_hash: String,

    // ── Attempt summary (informational, not authoritative) ──
    pub total_attempts: u32,
    pub terminal_reason: String,
}

impl LoopTerminalEnvelope {
    /// Compute the outcome binding hash over request_binding_hash + all outcome fields.
    /// Uses SHA-256 (same as Go side) for cross-language consistency.
    pub fn compute_outcome_binding_hash(&self) -> String {
        use sha2::{Sha256, Digest};
        let payload = format!(
            "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            self.request_binding_hash,
            serde_enum_str(self.reported_terminal_state),
            self.decision_id.as_deref().unwrap_or(""),
            self.decision_hash.as_deref().unwrap_or(""),
            self.output_checkpoint_hash.as_deref().unwrap_or(""),
            self.settlement_receipt_ref.as_deref().unwrap_or(""),
            self.output_artifact_refs.join(","),
            self.execution_generation,
            self.total_attempts,
            self.schema_version
        );
        let hash = Sha256::digest(payload.as_bytes());
        hex::encode(&hash[..8]) // first 8 bytes = 16 hex chars, matching Go
    }
}

fn serde_enum_str(s: LoopTerminalState) -> &'static str {
    match s {
        LoopTerminalState::Committed => "committed",
        LoopTerminalState::Escalated => "escalated",
        LoopTerminalState::EnvironmentBlocked => "environment_blocked",
        LoopTerminalState::LoopBudgetExhausted => "loop_budget_exhausted",
        LoopTerminalState::Cancelled => "cancelled",
        LoopTerminalState::ProtocolFailed => "protocol_failed",
    }
}

// ══════════════════════════════════════════════════════════════════
// OntoLoopHeartbeat — Rust Worker → OntoFlow
// ══════════════════════════════════════════════════════════════════

/// Periodic heartbeat from Rust Worker to OntoFlow during long-running loops.
///
/// Heartbeats inform liveness, cancellation propagation, timeout detection,
/// and progress display. They do NOT participate in success determination.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OntoLoopHeartbeat {
    // ── Identity ──
    pub flow_id: String,
    pub work_item_id: String,
    pub loop_id: String,

    // ── Current execution state ──
    pub current_attempt_id: Option<String>,
    pub current_run_id: Option<String>,

    /// High-level lifecycle state of the loop.
    pub lifecycle_state: LoopLifecycle,

    /// Hash of the current progress snapshot (for integrity check).
    pub progress_snapshot_hash: Option<String>,

    // ── Authority references ──
    pub last_decision_id: Option<String>,
    pub current_checkpoint_id: Option<String>,
}

/// Coarse lifecycle state exposed via heartbeat (not authoritative).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoopLifecycle {
    /// Loop is actively executing an attempt.
    Executing,
    /// Waiting for external resource or human input.
    Waiting,
    /// Loop has reached a terminal state.
    Terminal,
    /// Loop is in an unknown state (recovery needed).
    Unknown,
}
