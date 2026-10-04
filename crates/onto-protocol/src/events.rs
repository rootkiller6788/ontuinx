//! Event protocol — cross-layer events consumed by OntoGraph.
//!
//! Each layer publishes its own events. OntoGraph consumes all,
//! deduplicates by event_id, and detects gaps by attempt sequence_number.

use serde::{Deserialize, Serialize};

/// Generic event envelope — all events carry this header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    pub event_id: String,
    pub schema_version: u32,
    pub attempt_id: String,
    pub sequence_number: u64,
    pub correlation_id: String,
    pub causation_id: Option<String>,
    pub occurred_at: String,
    pub payload: T,
}

// ── Runtime Events ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateProduced {
    pub candidate_id: String,
    pub artifact_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateSealed {
    pub candidate_id: String,
    pub digest: String,
    pub manifest_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxExecutionCompleted {
    pub request_id: String,
    pub status: String,
    pub check_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeActionExecuted {
    pub decision_id: String,
    pub outcome: String,
}

// ── Assure Events ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanResolved {
    pub plan_id: String,
    pub unit_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingProduced {
    pub finding_id: String,
    pub verifier_id: String,
    pub pass: String,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerdictIssued {
    pub verdict_id: String,
    pub outcome: String,
    pub blocking_count: u32,
    pub advisory_count: u32,
}

// ── Loop Events ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionMade {
    pub decision_id: String,
    pub decision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptStateTransitioned {
    pub attempt_id: String,
    pub from: String,
    pub to: String,
}

// ── Flow Events ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItemScheduled {
    pub work_item_id: String,
    pub loop_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarrierSatisfied {
    pub barrier_id: String,
    pub participant_count: u32,
}

// ── Graph Event payloads ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateGraphSnapshotSealed {
    pub snapshot_id: String,
    pub candidate_id: String,
    pub source_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxValidationRequested {
    pub request_id: String,
    pub candidate_id: String,
    pub check_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxValidationStarted {
    pub request_id: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeTransitionStarted {
    pub decision_id: String,
    pub attempt_id: String,
    pub transition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionReceiptIssued {
    pub receipt_id: String,
    pub decision_id: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptOutcomeReduced {
    pub attempt_id: String,
    pub task_outcome: String,
    pub lifecycle_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptClosed {
    pub attempt_id: String,
    pub final_state: String,
}
