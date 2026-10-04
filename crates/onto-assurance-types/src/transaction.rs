//! Transaction types — one side-effect-bearing unit of work.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::enums::{EffectClass, TransactionState};
use crate::ids::{AttemptId, CheckpointId, ContractId, DecisionId, PublishReceiptId, RunId, TransactionId};

// ══════════════════════════════════════════════════════════════════
// ExecutionTransaction
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionTransaction {
    pub transaction_id: TransactionId,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub contract_id: ContractId,
    pub state: TransactionState,
    pub effect_class: EffectClass,
    pub checkpoint_binding: Option<CheckpointBinding>,
    pub side_effect_manifest: Option<SideEffectManifest>,
    pub artifact_manifest: Option<ArtifactManifest>,
    pub idempotency_key: Option<String>,
    pub created_at: DateTime<Utc>,
    pub state_entered_at: DateTime<Utc>,
}

// ══════════════════════════════════════════════════════════════════
// SideEffectManifest — what actually happened
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideEffectManifest {
    pub transaction_id: TransactionId,
    pub filesystem_changes: Vec<FilesystemChange>,
    pub network_requests: Vec<NetworkReceipt>,
    pub subprocess_invocations: Vec<SubprocessReceipt>,
    pub external_receipts: Vec<ExternalReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemChange {
    pub path: String,
    pub kind: FilesystemChangeKind,
    pub content_hash_after: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemChangeKind {
    Created,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkReceipt {
    pub url: String,
    pub method: String,
    pub status_code: Option<u16>,
    pub response_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubprocessReceipt {
    pub command: String,
    pub exit_code: Option<i32>,
    pub stdout_hash: String,
    pub stderr_hash: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalReceipt {
    pub system: String,
    pub action: String,
    pub receipt_id: Option<String>,
    pub status: String,
}

// ══════════════════════════════════════════════════════════════════
// ArtifactManifest — what was produced
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactManifest {
    pub transaction_id: TransactionId,
    pub artifacts: Vec<ArtifactEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactEntry {
    pub path: String,
    pub content_hash: String,
    pub size_bytes: u64,
}

// ══════════════════════════════════════════════════════════════════
// CheckpointBinding
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointBinding {
    pub checkpoint_id: CheckpointId,
    pub transaction_id: TransactionId,
    pub context_hash: String,
    pub created_at: DateTime<Utc>,
}

// ══════════════════════════════════════════════════════════════════
// Filesystem Staging Types (M6-A)
// ══════════════════════════════════════════════════════════════════

/// Content-addressed hash of filesystem state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash(pub String);

impl ContentHash {
    pub fn new(hash: impl Into<String>) -> Self {
        Self(hash.into())
    }
}

impl std::fmt::Display for ContentHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Handle to a staged filesystem environment. Agent operates inside this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemStageHandle {
    pub transaction_id: TransactionId,
    pub attempt_id: AttemptId,
    pub baseline_hash: ContentHash,
    pub staging_root: String,
}

/// Idempotency key for publish/discard operations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey(pub String);

impl IdempotencyKey {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }
}

/// Request to prepare a staging environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareStageRequest {
    pub transaction_id: TransactionId,
    pub attempt_id: AttemptId,
    pub source_path: String,
    pub staging_root: String,
}

/// Request to publish staged changes to the real workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishStageRequest {
    pub stage: FilesystemStageHandle,
    pub expected_baseline_hash: ContentHash,
    pub approved_manifest_hash: ContentHash,
    pub decision_id: DecisionId,
    pub idempotency_key: IdempotencyKey,
}

/// Receipt proving staged changes were published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishReceipt {
    pub transaction_id: TransactionId,
    pub before_hash: ContentHash,
    pub after_hash: ContentHash,
    pub manifest_hash: ContentHash,
    pub published_at: chrono::DateTime<chrono::Utc>,
}

/// Receipt proving staged changes were discarded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscardReceipt {
    pub transaction_id: TransactionId,
    pub discarded_at: chrono::DateTime<chrono::Utc>,
}

/// Current status of a staged environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageStatus {
    pub transaction_id: TransactionId,
    pub exists: bool,
    pub artifact_count: u32,
    pub baseline_hash: ContentHash,
}

// ══════════════════════════════════════════════════════════════════
// Execution Transaction State Machine (M6-A2c/A3)
// ══════════════════════════════════════════════════════════════════

/// State of a side-effect settlement transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTransactionState {
    /// Decision computed but settlement not yet started.
    Decided,
    /// Settlement in progress — publish has been requested.
    Publishing,
    /// Side effects successfully published and receipt persisted.
    Published,
    /// Settlement fully complete.
    Finalized,
    /// Settlement cannot proceed — manual intervention required.
    Frozen,
    /// Escalated to a higher authority or human operator.
    Escalated,
}

impl ExecutionTransactionState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Finalized | Self::Frozen | Self::Escalated)
    }
}

/// A recoverable settlement transaction record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionTransactionRecord {
    pub transaction_id: TransactionId,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub decision_id: DecisionId,
    pub state: ExecutionTransactionState,
    pub revision: u64,
    pub baseline_hash: ContentHash,
    pub manifest_hash: ContentHash,
    pub evidence_bundle_hash: ContentHash,
    pub checkpoint_binding_hash: ContentHash,
    pub idempotency_key: IdempotencyKey,
    pub publish_receipt_id: Option<PublishReceiptId>,
    pub last_error: Option<String>,
}

/// Structured failure reason for a transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionFailure {
    pub domain: String,
    pub code: String,
    pub detail: String,
}

// ══════════════════════════════════════════════════════════════════
// Database Effect Types (M6-B)
// ══════════════════════════════════════════════════════════════════

/// Handle to an active database transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseTransactionHandle {
    pub transaction_id: TransactionId,
    pub attempt_id: AttemptId,
    pub database_resource: String,
    pub begun_at: DateTime<Utc>,
}

/// Request to begin a database transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeginDatabaseTransactionRequest {
    pub transaction_id: TransactionId,
    pub attempt_id: AttemptId,
    pub database_resource: String,
}

/// A structured database command (not raw SQL).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseCommand {
    Insert {
        table: String,
        columns: Vec<String>,
        values: Vec<serde_json::Value>,
    },
    Update {
        table: String,
        set: Vec<(String, serde_json::Value)>,
        predicate: String,
    },
    Delete {
        table: String,
        predicate: String,
    },
    Prepared {
        statement_id: String,
        parameters: Vec<serde_json::Value>,
    },
    RawSql {
        sql: String,
        max_rows_affected: Option<u64>,
    },
}

/// Outcome of executing a database command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseCommandOutcome {
    pub rows_affected: u64,
    pub command: String,
}

/// Snapshot of candidate database state for verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseCandidateSnapshot {
    pub transaction_id: TransactionId,
    pub row_counts: Vec<(String, u64)>,
    pub checksum: Option<String>,
}

/// Receipt proving a database transaction was committed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseTransactionReceipt {
    pub transaction_id: TransactionId,
    pub before_snapshot_hash: ContentHash,
    pub after_mutation_hash: ContentHash,
    pub committed_at: DateTime<Utc>,
}

/// Receipt proving a database transaction was rolled back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseRollbackReceipt {
    pub transaction_id: TransactionId,
    pub rolled_back_at: DateTime<Utc>,
}

/// Manifest describing what mutations a transaction made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseMutationManifest {
    pub transaction_id: TransactionId,
    pub tables_affected: Vec<String>,
    pub total_rows_modified: u64,
    pub command_count: u32,
}

// ══════════════════════════════════════════════════════════════════
// Compensatable External Effect Types (M6-C)
// ══════════════════════════════════════════════════════════════════

/// Receipt from an external operation that can potentially be compensated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalOperationReceipt {
    pub transaction_id: TransactionId,
    pub external_system: String,
    pub operation_id: String,
    pub operation_type: String,
    pub idempotency_key: IdempotencyKey,
    pub executed_at: DateTime<Utc>,
}

/// Request to compensate (undo) a previously executed external operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationRequest {
    pub original_receipt: ExternalOperationReceipt,
    pub compensation_capability: String,
    pub reason: String,
}

/// Receipt proving compensation was executed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationReceipt {
    pub transaction_id: TransactionId,
    pub original_operation_id: String,
    pub compensation_status: CompensationStatus,
    pub executed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompensationStatus {
    Compensated,
    CompensationFailed,
    Confirmed, // verified OK, no compensation needed
}

/// Describes how to compensate an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationDescriptor {
    pub capability_name: String,
    pub description: String,
    pub estimated_effect: String,
}

// ══════════════════════════════════════════════════════════════════
// Irreversible External Effect Types (M6-D)
// ══════════════════════════════════════════════════════════════════

/// One-time-use lease authorizing exactly one irreversible dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExactInvocationLease {
    pub lease_id: String,
    pub capability: String,
    pub actor: String,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub params_hash: ContentHash,
    pub max_uses: u32, // must be 1
    pub used_count: u32,
    pub expires_at: DateTime<Utc>,
    pub decision_id: DecisionId,
}

impl ExactInvocationLease {
    pub fn is_valid(&self) -> bool {
        self.used_count < self.max_uses && chrono::Utc::now() < self.expires_at
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

/// Intent recorded BEFORE dispatch — enables crash recovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchIntent {
    pub transaction_id: TransactionId,
    pub capability: String,
    pub params_hash: ContentHash,
    pub lease_id: String,
    pub idempotency_key: IdempotencyKey,
    pub recorded_at: DateTime<Utc>,
}

/// Receipt from an external dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalDispatchReceipt {
    pub transaction_id: TransactionId,
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

/// Case requiring manual reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualReconciliationCase {
    pub transaction_id: TransactionId,
    pub dispatch_id: String,
    pub reason: String,
    pub recommended_action: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_effect_manifest_serde() {
        let manifest = SideEffectManifest {
            transaction_id: TransactionId::new(),
            filesystem_changes: vec![],
            network_requests: vec![],
            subprocess_invocations: vec![],
            external_receipts: vec![],
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let back: SideEffectManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest.transaction_id, back.transaction_id);
    }
}
