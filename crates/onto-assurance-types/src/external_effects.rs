//! External Effect types — M6-C Compensatable External Effect Authority.
//!
//! Compensation ≠ Rollback. External operations leave real-world traces
//! (billing, notifications, audit records) even if the resource is deleted.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{AttemptId, DecisionId, RunId};

// ══════════════════════════════════════════════════════════════════
// Core IDs
// ══════════════════════════════════════════════════════════════════

/// Identifies an external effect operation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExternalOperationId(pub String);

/// Identifies an external resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExternalResourceId(pub String);

// ══════════════════════════════════════════════════════════════════
// ExternalOperationIntent — recorded BEFORE any HTTP call
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalOperationIntent {
    pub operation_id: ExternalOperationId,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub decision_id: DecisionId,
    pub capability_id: String,
    pub external_resource: String,
    pub operation_kind: ExternalOperationKind,
    pub request_payload_hash: String,
    pub idempotency_key: String,
    pub expected_effect_class: crate::enums::EffectClass,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalOperationKind {
    Create,
    Read,
    Update,
    Delete,
    Custom,
}

// ══════════════════════════════════════════════════════════════════
// ExternalOperationReceipt — proof the external call happened
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalOperationReceipt {
    pub operation_id: ExternalOperationId,
    pub provider_operation_id: String,
    pub external_resource_id: Option<ExternalResourceId>,
    pub request_hash: String,
    pub response_hash: String,
    pub external_version: u64,
    pub http_status: u16,
    pub observed_state: ExternalOperationStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalOperationStatus {
    Created,
    NotFound,
    Conflict,
    Unknown,
}

// ══════════════════════════════════════════════════════════════════
// Compensation types
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationDescriptor {
    pub original_capability_id: String,
    pub compensation_capability_id: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationRequest {
    pub original_operation_id: ExternalOperationId,
    pub original_receipt_id: String,
    pub external_resource_id: ExternalResourceId,
    pub expected_external_version: u64,
    pub compensation_arguments_hash: String,
    pub idempotency_key: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationReceipt {
    pub compensation_id: ExternalOperationId,
    pub original_operation_id: ExternalOperationId,
    pub external_resource_id: ExternalResourceId,
    pub status: CompensationStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompensationStatus {
    Compensated,
    CompensationFailed,
    Confirmed,
}

// ══════════════════════════════════════════════════════════════════
// External Effect State Machine
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalEffectState {
    Prepared,
    Authorized,
    Executing,
    Executed,
    Verifying,
    Confirmed,
    CompensationAuthorized,
    Compensating,
    Compensated,
    Unknown,
    Frozen,
    Escalated,
}

impl ExternalEffectState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Confirmed | Self::Compensated | Self::Frozen | Self::Escalated)
    }
}

// ══════════════════════════════════════════════════════════════════
// M6-D: Irreversible External Effect Types
// ══════════════════════════════════════════════════════════════════

/// One-time-use lease authorizing exactly one irreversible dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExactInvocationLease {
    pub lease_id: String,
    pub capability: String,
    pub actor: String,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub params_hash: String,
    pub max_uses: u32,
    pub used_count: u32,
    pub expires_at: DateTime<Utc>,
    pub decision_id: DecisionId,
}

impl ExactInvocationLease {
    pub fn is_valid(&self) -> bool {
        self.used_count < self.max_uses && Utc::now() < self.expires_at
    }
    pub fn consume(&mut self) -> bool {
        if self.is_valid() { self.used_count += 1; true } else { false }
    }
}

/// Pre-execution decision for irreversible effects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreExecutionDecision {
    pub decision_id: DecisionId,
    pub authorized: bool,
    pub required_approvals: Vec<String>,
    pub granted_approvals: Vec<String>,
    pub lease: Option<ExactInvocationLease>,
}

/// Intent recorded BEFORE dispatch — crash recovery point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchIntent {
    pub transaction_id: String,
    pub capability: String,
    pub params_hash: String,
    pub lease_id: String,
    pub idempotency_key: String,
    pub recorded_at: DateTime<Utc>,
}

/// Receipt from an external dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalDispatchReceipt {
    pub transaction_id: String,
    pub dispatch_id: String,
    pub external_system: String,
    pub status: DispatchStatus,
    pub responded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchStatus {
    Confirmed,
    UnknownExternalOutcome,
    Rejected,
}
