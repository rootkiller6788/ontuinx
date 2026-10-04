//! Port traits — the interface between the Onto runtime coordinator
//! and external systems (OntoRuntime or any other host).
//!
//! Core logic depends ONLY on these traits, never on concrete host types.
//! Each trait is a single-responsibility abstraction over one host capability.

use onto_assurance_types::contract::ExecutionIntent;
use onto_assurance_types::decision::SettlementDecision;
use onto_assurance_types::enums::{
    BudgetOutcome, EffectClass, ExitReason, LifecycleState, ReasonCode, TaskOutcome,
};
use onto_assurance_types::evidence::EvidenceBundle;
use onto_assurance_types::ids::{
    AttemptId, CheckpointId, DecisionId, RunId, TransactionId,
};
use onto_assurance_types::transaction::{
    ArtifactManifest, BeginDatabaseTransactionRequest, ContentHash, DatabaseCandidateSnapshot,
    DatabaseCommand, DatabaseCommandOutcome,
    DatabaseRollbackReceipt, DatabaseTransactionHandle, DatabaseTransactionReceipt, DiscardReceipt,
    ExecutionTransactionRecord, ExecutionTransactionState, FilesystemStageHandle, IdempotencyKey,
    PrepareStageRequest, PublishReceipt, PublishStageRequest, StageStatus,
};
use onto_assurance_types::transaction::{ExecutionTransaction, SideEffectManifest};

// ══════════════════════════════════════════════════════════════════
// AuthorizationPort
// ══════════════════════════════════════════════════════════════════

/// Request authorization for a capability invocation.
///
/// The host (OntoRuntime) evaluates grants, trust, and policy.
/// Onto only receives the receipt — it does not implement RBAC itself.
#[async_trait::async_trait]
pub trait AuthorizationPort: Send + Sync {
    /// Request authorization for an intent.
    async fn authorize(
        &self,
        intent: &ExecutionIntent,
    ) -> Result<AuthorizationReceipt, AuthorizationError>;

    /// Check whether a prior authorization is still valid.
    async fn check_valid(
        &self,
        receipt: &AuthorizationReceipt,
    ) -> Result<bool, AuthorizationError>;
}

/// A receipt proving that the host authorized a capability invocation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthorizationReceipt {
    pub run_id: RunId,
    pub authorized: bool,
    pub grant_id: String,
    pub lease_id: Option<String>,
    pub restrictions: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthorizationError {
    #[error("authorization denied: {0}")]
    Denied(String),
    #[error("host error: {0}")]
    Host(String),
}

// ══════════════════════════════════════════════════════════════════
// ApprovalPort
// ══════════════════════════════════════════════════════════════════

#[async_trait::async_trait]
pub trait ApprovalPort: Send + Sync {
    /// Request human approval.  Returns an approval ID for polling.
    async fn request_approval(
        &self,
        run_id: RunId,
        reason: &str,
    ) -> Result<String, ApprovalError>;

    /// Check the status of a pending approval.
    async fn check_status(
        &self,
        approval_id: &str,
    ) -> Result<ApprovalStatus, ApprovalError>;

    /// Wait for approval (or timeout).
    async fn wait_for_decision(
        &self,
        approval_id: &str,
        timeout_seconds: u64,
    ) -> Result<ApprovalDecision, ApprovalError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalStatus {
    Pending,
    Granted,
    Rejected,
    Expired,
}

#[derive(Debug, Clone)]
pub enum ApprovalDecision {
    Granted,
    Rejected { reason: String },
    TimedOut,
}

#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error("approval not found: {0}")]
    NotFound(String),
    #[error("host error: {0}")]
    Host(String),
}

// ══════════════════════════════════════════════════════════════════
// RuntimePort
// ══════════════════════════════════════════════════════════════════

#[async_trait::async_trait]
pub trait RuntimePort: Send + Sync {
    /// Request a staged execution environment.
    async fn stage(
        &self,
        transaction: &ExecutionTransaction,
    ) -> Result<StageHandle, RuntimeError>;

    /// Execute a capability in the staged environment.
    async fn execute(
        &self,
        handle: &StageHandle,
        command: &str,
    ) -> Result<ExecutionResult, RuntimeError>;

    /// Capture side effects from the staged environment.
    async fn capture_effects(
        &self,
        handle: &StageHandle,
    ) -> Result<SideEffectManifest, RuntimeError>;

    /// Publish staged effects to the real world.
    async fn publish(
        &self,
        handle: &StageHandle,
        decision: &SettlementDecision,
    ) -> Result<(), RuntimeError>;

    /// Discard staged effects without publishing.
    async fn discard(
        &self,
        handle: &StageHandle,
    ) -> Result<(), RuntimeError>;

    /// Freeze the staged environment for later inspection.
    async fn freeze(
        &self,
        handle: &StageHandle,
    ) -> Result<(), RuntimeError>;
}

#[derive(Debug, Clone)]
pub struct StageHandle {
    pub transaction_id: TransactionId,
    pub handle_id: String,
    pub environment_info: String,
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("stage creation failed: {0}")]
    StageFailed(String),
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    #[error("publish failed: {0}")]
    PublishFailed(String),
    #[error("host error: {0}")]
    Host(String),
}

// Old VerifierPort, VerificationRunResult, and VerifierError removed.
// Use onto_protocol::verifier::Verifier and onto_protocol::verifier::VerifierResult instead.

// ══════════════════════════════════════════════════════════════════
// EvidenceStorePort
// ══════════════════════════════════════════════════════════════════

#[async_trait::async_trait]
pub trait EvidenceStorePort: Send + Sync {
    /// Persist an evidence bundle.
    async fn store(
        &self,
        bundle: &EvidenceBundle,
    ) -> Result<String, EvidenceStoreError>;

    /// Retrieve an evidence bundle by its storage key.
    async fn retrieve(
        &self,
        key: &str,
    ) -> Result<EvidenceBundle, EvidenceStoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum EvidenceStoreError {
    #[error("evidence not found: {0}")]
    NotFound(String),
    #[error("storage error: {0}")]
    Storage(String),
}

// ══════════════════════════════════════════════════════════════════
// EventSinkPort
// ══════════════════════════════════════════════════════════════════

#[async_trait::async_trait]
pub trait EventSinkPort: Send + Sync {
    /// Emit a structured event to the host's event store.
    async fn emit(
        &self,
        event_type: &str,
        payload: &serde_json::Value,
    ) -> Result<(), EventSinkError>;
}

#[derive(Debug, thiserror::Error)]
pub enum EventSinkError {
    #[error("emit failed: {0}")]
    Failed(String),
}

// ══════════════════════════════════════════════════════════════════
// CheckpointPort
// ══════════════════════════════════════════════════════════════════

#[async_trait::async_trait]
pub trait CheckpointPort: Send + Sync {
    /// Save a checkpoint for recovery.
    async fn save(
        &self,
        run_id: RunId,
        state: &serde_json::Value,
    ) -> Result<String, CheckpointError>;

    /// Load the most recent checkpoint.
    async fn load(
        &self,
        run_id: RunId,
    ) -> Result<Option<serde_json::Value>, CheckpointError>;
}

#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("checkpoint error: {0}")]
    Failed(String),
}

// ══════════════════════════════════════════════════════════════════
// ClockPort
// ══════════════════════════════════════════════════════════════════

/// Abstract clock for deterministic replay.
pub trait ClockPort: Send + Sync {
    fn now(&self) -> chrono::DateTime<chrono::Utc>;
}

/// Real system clock.
pub struct SystemClock;

impl ClockPort for SystemClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}

/// Fixed clock for testing and replay.
pub struct FixedClock {
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl ClockPort for FixedClock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.timestamp
    }
}

// ══════════════════════════════════════════════════════════════════
// RunFinalizationPort (M5)
// ══════════════════════════════════════════════════════════════════

/// Input to the M5 finalization port — the facts of the loop exit.
#[derive(Debug, Clone)]
pub struct RunFinalizationRequest {
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub exit_reason: ExitReason,
    pub budget_outcome: BudgetOutcome,
    pub checkpoint_ref: Option<CheckpointId>,
    /// P16-F1: staging root from the agent's workspace. Must be a real,
    /// canonicalized directory. When None, sandbox-dependent verifiers
    /// (project.build, project.test) will be Unavailable — the finalizer
    /// fails-closed rather than synthesizing a fake path.
    pub staging_root: Option<std::path::PathBuf>,
}

/// Authoritative outcome from the finalization port.
/// This is the ONLY path to produce a terminal verdict for a run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunFinalizationOutcome {
    pub task_outcome: TaskOutcome,
    pub budget_outcome: BudgetOutcome,
    pub lifecycle_state: LifecycleState,
    pub session_decision_id: DecisionId,
    pub attempt_decision_id: DecisionId,
    pub reason_codes: Vec<ReasonCode>,
    /// M6-A2c: the settlement decision computed by the kernel.
    pub settlement_decision: Option<SettlementDecision>,
    /// M6-A2c: the effect class used in settlement computation.
    pub effect_class: Option<EffectClass>,
}

#[derive(Debug, thiserror::Error)]
pub enum RunFinalizationError {
    #[error("evidence not found: {0}")]
    EvidenceNotFound(String),
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("internal error: {0}")]
    Internal(String),
}

/// M5 authoritative finalization port.
/// Called on every loop exit BEFORE OntoRuntime commits the exit state.
#[async_trait::async_trait]
pub trait RunFinalizationPort: Send + Sync {
    async fn finalize(
        &self,
        req: RunFinalizationRequest,
    ) -> Result<RunFinalizationOutcome, RunFinalizationError>;
}

// ══════════════════════════════════════════════════════════════════
// DecisionStorePort (M5)
// ══════════════════════════════════════════════════════════════════

#[async_trait::async_trait]
pub trait DecisionStorePort: Send + Sync {
    /// Persist a session decision idempotently.
    /// Returns `true` if this is the first write, `false` if already present.
    async fn persist_session(
        &self,
        run_id: RunId,
        attempt_id: AttemptId,
        outcome: &RunFinalizationOutcome,
    ) -> Result<bool, DecisionStoreError>;

    /// Load a previously-persisted session decision.
    /// Returns `None` if no decision exists for this run.
    async fn load_session(
        &self,
        run_id: RunId,
    ) -> Result<Option<RunFinalizationOutcome>, DecisionStoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum DecisionStoreError {
    #[error("storage error: {0}")]
    Storage(String),
}

// ══════════════════════════════════════════════════════════════════
// StagedFilesystemPort (M6-A)
// ══════════════════════════════════════════════════════════════════

/// Port for isolated filesystem staging. Agent modifies a staging directory;
/// publish() copies to the real workspace only after Onto COMMIT.
///
/// # Invariants
///
/// - Agent writes ONLY to the staging directory.
/// - The real workspace is unchanged until publish() succeeds.
/// - publish() requires: SUCCESS decision, COMMIT settlement, valid evidence,
///   matching artifact manifest, unmodified baseline hash.
#[async_trait::async_trait]
pub trait StagedFilesystemPort: Send + Sync {
    /// Prepare a staging environment from the source workspace.
    async fn prepare(
        &self,
        request: PrepareStageRequest,
    ) -> Result<FilesystemStageHandle, StageError>;

    /// Compute the artifact manifest (list of changed files with hashes).
    async fn manifest(
        &self,
        stage: &FilesystemStageHandle,
    ) -> Result<ArtifactManifest, StageError>;

    /// Publish staged changes to the real workspace. Requires a valid
    /// [`CommitPermit`] issued by the [`StagedSettlementCoordinator`].
    /// Must be idempotent.
    async fn publish(
        &self,
        request: PublishStageRequest,
        permit: &CommitPermit,
    ) -> Result<PublishReceipt, StageError>;

    /// Discard staged changes without affecting the real workspace.
    async fn discard(
        &self,
        stage: &FilesystemStageHandle,
    ) -> Result<DiscardReceipt, StageError>;

    /// Inspect the current status of a stage.
    async fn inspect(
        &self,
        stage: &FilesystemStageHandle,
    ) -> Result<StageStatus, StageError>;
}

#[derive(Debug, thiserror::Error)]
pub enum StageError {
    #[error("stage not found: {0}")]
    NotFound(String),
    #[error("baseline hash mismatch: expected {expected}, actual {actual}")]
    BaselineMismatch { expected: String, actual: String },
    #[error("manifest hash mismatch: expected {expected}, actual {actual}")]
    ManifestMismatch { expected: String, actual: String },
    #[error("stage preparation failed: {0}")]
    PrepareFailed(String),
    #[error("publish failed: {0}")]
    PublishFailed(String),
    #[error("discard failed: {0}")]
    DiscardFailed(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("not authorized: {0}")]
    NotAuthorized(String),
}

// ══════════════════════════════════════════════════════════════════
// CommitPermit (M6-A2) — opaque publish authorization
// ══════════════════════════════════════════════════════════════════

/// Opaque proof that a publish is authorized by a durable Onto decision.
///
/// Can only be constructed by [`StagedSettlementCoordinator`]. Adapters may
/// read its fields but MUST NOT construct one themselves.
#[derive(Debug, Clone)]
/// Opaque publish capability token. Fields are private — only
/// [`StagedSettlementCoordinator`] can construct one. Adapters may
/// read fields via accessors but MUST NOT construct.
///
/// Does NOT implement `Deserialize`. Cannot be created from CLI/HTTP/Agent input.
pub struct CommitPermit {
    decision_id: DecisionId,
    transaction_id: TransactionId,
    attempt_id: AttemptId,
    baseline_hash: ContentHash,
    manifest_hash: ContentHash,
    idempotency_key: IdempotencyKey,
}

impl CommitPermit {
    // ── Read-only accessors ──
    pub fn decision_id(&self) -> DecisionId { self.decision_id }
    pub fn transaction_id(&self) -> TransactionId { self.transaction_id }
    pub fn attempt_id(&self) -> AttemptId { self.attempt_id }
    pub fn baseline_hash(&self) -> &ContentHash { &self.baseline_hash }
    pub fn manifest_hash(&self) -> &ContentHash { &self.manifest_hash }
    pub fn idempotency_key(&self) -> &IdempotencyKey { &self.idempotency_key }

    // ── Construction (restricted) ──

    /// ⚠️  Test-only. Bypasses all authorization. Never use in production.
    pub fn new_for_test_only(
        decision_id: DecisionId,
        transaction_id: TransactionId,
        attempt_id: AttemptId,
        baseline_hash: ContentHash,
        manifest_hash: ContentHash,
    ) -> Self {
        Self {
            decision_id, transaction_id, attempt_id,
            baseline_hash, manifest_hash,
            idempotency_key: IdempotencyKey::new(format!("test-{}", transaction_id)),
        }
    }

    /// Authorized construction. Only callable from within
    /// `onto-assurance-runtime` (the coordinator).
    pub(crate) fn issue(
        decision_id: DecisionId,
        transaction_id: TransactionId,
        attempt_id: AttemptId,
        baseline_hash: ContentHash,
        manifest_hash: ContentHash,
    ) -> Self {
        Self {
            decision_id, transaction_id, attempt_id,
            baseline_hash, manifest_hash,
            idempotency_key: IdempotencyKey::new(format!("publish-{}", transaction_id)),
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// PublishReceiptStorePort (M6-A2)
// ══════════════════════════════════════════════════════════════════

#[async_trait::async_trait]
pub trait PublishReceiptStorePort: Send + Sync {
    /// Persist a publish receipt. Must be idempotent.
    async fn store_receipt(
        &self,
        receipt: &PublishReceipt,
    ) -> Result<bool, ReceiptStoreError>;

    /// Look up a previously-persisted receipt by idempotency key.
    async fn lookup_receipt(
        &self,
        idempotency_key: &IdempotencyKey,
    ) -> Result<Option<PublishReceipt>, ReceiptStoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ReceiptStoreError {
    #[error("receipt not found: {0}")]
    NotFound(String),
    #[error("storage error: {0}")]
    Storage(String),
}

// ══════════════════════════════════════════════════════════════════
// PublishAuthorizationError (M6-A2)
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, thiserror::Error)]
pub enum PublishAuthorizationError {
    #[error("decision not found: {0}")]
    DecisionNotFound(String),
    #[error("decision not durable — not yet persisted")]
    DecisionNotDurable,
    #[error("task not successful: {0:?}")]
    TaskNotSuccessful(onto_assurance_types::enums::TaskOutcome),
    #[error("settlement not commit: {0:?}")]
    SettlementNotCommit(onto_assurance_types::decision::SettlementDecision),
    #[error("effect class mismatch")]
    EffectClassMismatch,
    #[error("run mismatch")]
    RunMismatch,
    #[error("attempt mismatch")]
    AttemptMismatch,
    #[error("transaction mismatch")]
    TransactionMismatch,
    #[error("baseline mismatch")]
    BaselineMismatch,
    #[error("manifest mismatch")]
    ManifestMismatch,
    #[error("evidence invalidated")]
    EvidenceInvalidated,
    #[error("checkpoint mismatch")]
    CheckpointMismatch,
    #[error("invalid transaction state: {0:?}")]
    InvalidTransactionState(onto_assurance_types::enums::TransactionState),
    #[error("already settled differently")]
    AlreadySettledDifferently,
    #[error("internal error: {0}")]
    Internal(String),
    #[error("CAS conflict: expected revision {expected}, actual {actual}")]
    CasConflict { expected: u64, actual: u64 },
}

// ══════════════════════════════════════════════════════════════════
// TransactionStorePort (M6-A2c/A3)
// ══════════════════════════════════════════════════════════════════

#[async_trait::async_trait]
pub trait TransactionStorePort: Send + Sync {
    async fn create(
        &self,
        record: ExecutionTransactionRecord,
    ) -> Result<(), TransactionStoreError>;

    async fn load(
        &self,
        transaction_id: &TransactionId,
    ) -> Result<Option<ExecutionTransactionRecord>, TransactionStoreError>;

    async fn compare_and_set(
        &self,
        transaction_id: &TransactionId,
        expected_revision: u64,
        expected_state: ExecutionTransactionState,
        new_state: ExecutionTransactionState,
    ) -> Result<ExecutionTransactionRecord, TransactionStoreError>;

    async fn list_recoverable(
        &self,
    ) -> Result<Vec<ExecutionTransactionRecord>, TransactionStoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum TransactionStoreError {
    #[error("transaction not found: {0}")]
    NotFound(String),
    #[error("CAS conflict: expected rev {expected}, actual {actual}")]
    CasConflict { expected: u64, actual: u64 },
    #[error("storage error: {0}")]
    Storage(String),
}

// ══════════════════════════════════════════════════════════════════
// RunStateStorePort (M6-A3)
// ══════════════════════════════════════════════════════════════════

#[async_trait::async_trait]
pub trait RunStateStorePort: Send + Sync {
    /// Project the lifecycle state for a run. Idempotent.
    async fn set_lifecycle(
        &self,
        run_id: RunId,
        state: LifecycleState,
    ) -> Result<(), RunStateStoreError>;
}

#[derive(Debug, thiserror::Error)]
pub enum RunStateStoreError {
    #[error("run not found: {0}")]
    NotFound(String),
    #[error("storage error: {0}")]
    Storage(String),
}

// ══════════════════════════════════════════════════════════════════
// RunIngressPort (Phase 2)
// ══════════════════════════════════════════════════════════════════

use onto_assurance_types::ingress::{RunIngressError, StartRunRequest, StartRunResult};

/// Unified entry point for starting agent runs.
///
/// Conversation, CLI, HTTP, DirectRun, OntoLoop, and Temporal all enter
/// through this port. Each run records its actor and source.
#[async_trait::async_trait]
pub trait RunIngressPort: Send + Sync {
    /// Start a new agent run.
    async fn start_run(
        &self,
        request: StartRunRequest,
    ) -> Result<StartRunResult, RunIngressError>;
}

// ══════════════════════════════════════════════════════════════════
// CapabilityDescriptorPort (Phase 3)
// ══════════════════════════════════════════════════════════════════

use onto_assurance_types::capability::{CapabilityDescriptor, CapabilityInvocationEnvelope};

/// Resolves capability metadata from the trusted registry.
///
/// The Agent must never be able to set or downgrade EffectClass.
/// This port is the single source of truth for capability classification.
#[async_trait::async_trait]
pub trait CapabilityDescriptorPort: Send + Sync {
    /// Look up the trusted descriptor for a capability.
    async fn resolve(
        &self,
        capability_name: &str,
    ) -> Result<CapabilityDescriptor, CapabilityDescriptorError>;

    /// Create an invocation envelope with the correct EffectClass
    /// from the trusted descriptor. Agent-supplied effect_class is ignored.
    async fn create_envelope(
        &self,
        capability_name: &str,
        run_id: onto_assurance_types::ids::RunId,
        actor: String,
        arguments_hash: String,
        resource_refs: Vec<onto_assurance_types::capability::ResourceRef>,
    ) -> Result<CapabilityInvocationEnvelope, CapabilityDescriptorError> {
        let descriptor = self.resolve(capability_name).await?;
        Ok(CapabilityInvocationEnvelope::new(
            &descriptor, run_id, actor, arguments_hash, resource_refs,
        ))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CapabilityDescriptorError {
    #[error("capability not found: {0}")]
    NotFound(String),
    #[error("capability not authorized for this actor: {0}")]
    Unauthorized(String),
    #[error("internal error: {0}")]
    Internal(String),
}

// ══════════════════════════════════════════════════════════════════
// RuntimeObservationPort (Phase 4)
// ══════════════════════════════════════════════════════════════════

use onto_assurance_types::observation::RuntimeObservation;

/// Receives raw execution facts from OntoRuntime's capability execution.
///
/// OntoRuntime calls this after every capability invocation completes.
/// OntoAssure's Verifier reads these observations to build Evidence.
#[async_trait::async_trait]
pub trait RuntimeObservationPort: Send + Sync {
    /// Record a runtime observation.
    async fn observe(
        &self,
        observation: RuntimeObservation,
    ) -> Result<(), ObservationError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ObservationError {
    #[error("observation store error: {0}")]
    Store(String),
}

// ══════════════════════════════════════════════════════════════════
// DatabaseCommitPermit (M6-B)
// ══════════════════════════════════════════════════════════════════

/// Opaque permit authorizing a database COMMIT.
/// Only constructible by the DatabaseSettlementCoordinator.
#[derive(Debug, Clone)]
pub struct DatabaseCommitPermit {
    decision_id: DecisionId,
    transaction_id: TransactionId,
    attempt_id: AttemptId,
    database_resource: String,
    mutation_manifest_hash: ContentHash,
    idempotency_key: IdempotencyKey,
}

impl DatabaseCommitPermit {
    pub fn decision_id(&self) -> DecisionId { self.decision_id }
    pub fn transaction_id(&self) -> TransactionId { self.transaction_id }
    pub fn attempt_id(&self) -> AttemptId { self.attempt_id }
    pub fn database_resource(&self) -> &str { &self.database_resource }
    pub fn mutation_manifest_hash(&self) -> &ContentHash { &self.mutation_manifest_hash }
    pub fn idempotency_key(&self) -> &IdempotencyKey { &self.idempotency_key }

    pub fn new_for_test_only(
        decision_id: DecisionId, transaction_id: TransactionId,
        attempt_id: AttemptId, database_resource: String,
        manifest_hash: ContentHash,
    ) -> Self {
        Self { decision_id, transaction_id, attempt_id, database_resource,
               mutation_manifest_hash: manifest_hash,
               idempotency_key: IdempotencyKey::new(format!("db-{}", transaction_id)) }
    }

    pub(crate) fn issue(
        decision_id: DecisionId, transaction_id: TransactionId,
        attempt_id: AttemptId, database_resource: String,
        manifest_hash: ContentHash,
    ) -> Self {
        Self { decision_id, transaction_id, attempt_id, database_resource,
               mutation_manifest_hash: manifest_hash,
               idempotency_key: IdempotencyKey::new(format!("db-{}", transaction_id)) }
    }
}

// ══════════════════════════════════════════════════════════════════
// TransactionalDatabasePort (M6-B)
// ══════════════════════════════════════════════════════════════════

#[async_trait::async_trait]
pub trait TransactionalDatabasePort: Send + Sync {
    async fn begin(
        &self,
        request: BeginDatabaseTransactionRequest,
    ) -> Result<DatabaseTransactionHandle, DatabaseError>;

    async fn execute(
        &self,
        tx: &DatabaseTransactionHandle,
        command: DatabaseCommand,
    ) -> Result<DatabaseCommandOutcome, DatabaseError>;

    async fn inspect_candidate(
        &self,
        tx: &DatabaseTransactionHandle,
    ) -> Result<DatabaseCandidateSnapshot, DatabaseError>;

    async fn commit(
        &self,
        tx: DatabaseTransactionHandle,
        permit: &DatabaseCommitPermit,
    ) -> Result<DatabaseTransactionReceipt, DatabaseError>;

    async fn rollback(
        &self,
        tx: DatabaseTransactionHandle,
    ) -> Result<DatabaseRollbackReceipt, DatabaseError>;
}

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    #[error("transaction error: {0}")]
    TransactionError(String),
    #[error("not authorized: {0}")]
    NotAuthorized(String),
    #[error("permit mismatch")]
    PermitMismatch,
    #[error("schema violation: {0}")]
    SchemaViolation(String),
}

// ══════════════════════════════════════════════════════════════════
// CompensatableExternalPort (M6-C)
// ══════════════════════════════════════════════════════════════════

use onto_assurance_types::transaction::{
    CompensationReceipt, CompensationRequest, DispatchIntent, ExactInvocationLease,
    ExternalDispatchReceipt, ExternalOperationReceipt,
};

/// Port for compensatable external operations.
/// Execute → verify → confirm / compensate / freeze.
#[async_trait::async_trait]
pub trait CompensatableExternalPort: Send + Sync {
    /// Execute an external operation.
    async fn execute(
        &self,
        transaction_id: TransactionId,
        capability: &str,
        params: &serde_json::Value,
    ) -> Result<ExternalOperationReceipt, ExternalEffectError>;

    /// Query the current state of an external operation.
    async fn query_status(
        &self,
        operation_id: &str,
        external_system: &str,
    ) -> Result<ExternalOperationReceipt, ExternalEffectError>;

    /// Execute a compensation.
    async fn compensate(
        &self,
        request: CompensationRequest,
    ) -> Result<CompensationReceipt, ExternalEffectError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ExternalEffectError {
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("compensation failed: {0}")]
    CompensationFailed(String),
    #[error("not authorized: {0}")]
    NotAuthorized(String),
    #[error("unknown outcome: {0}")]
    UnknownOutcome(String),
}

// ══════════════════════════════════════════════════════════════════
// IrreversibleDispatchPort (M6-D)
// ══════════════════════════════════════════════════════════════════

/// Port for at-most-once dispatch of irreversible external effects.
#[async_trait::async_trait]
pub trait IrreversibleDispatchPort: Send + Sync {
    /// Record dispatch intent BEFORE any external call.
    async fn record_intent(
        &self,
        intent: DispatchIntent,
    ) -> Result<(), IrreversibleEffectError>;

    /// Dispatch with a valid lease. Must be at-most-once.
    async fn dispatch(
        &self,
        intent: &DispatchIntent,
        lease: &ExactInvocationLease,
        params: &serde_json::Value,
    ) -> Result<ExternalDispatchReceipt, IrreversibleEffectError>;

    /// Query external system for actual outcome.
    async fn query_external(
        &self,
        dispatch_id: &str,
        external_system: &str,
    ) -> Result<ExternalDispatchReceipt, IrreversibleEffectError>;
}

#[derive(Debug, thiserror::Error)]
pub enum IrreversibleEffectError {
    #[error("lease expired or consumed")]
    LeaseInvalid,
    #[error("dispatch failed: {0}")]
    DispatchFailed(String),
    #[error("unknown outcome: {0}")]
    UnknownOutcome(String),
    #[error("not authorized: {0}")]
    NotAuthorized(String),
}

// ══════════════════════════════════════════════════════════════════
// OntoLoop Ports (Phase 6)
// ══════════════════════════════════════════════════════════════════

use onto_assurance_types::ontoloop::ContinuationRequest;

/// Start and control OntoRuntime runs from OntoLoop.
#[async_trait::async_trait]
pub trait RuntimeRunPort: Send + Sync {
    async fn start_run(&self, attempt_number: u32, objective: &str) -> Result<RunId, String>;
    async fn await_terminal(&self, run_id: RunId) -> Result<RunFinalizationOutcome, String>;
    async fn cancel_run(&self, run_id: RunId) -> Result<(), String>;
}

/// Load OntoAssure finalization for an attempt.
#[async_trait::async_trait]
pub trait AssuranceResultPort: Send + Sync {
    async fn load_finalization(&self, run_id: RunId) -> Result<RunFinalizationOutcome, String>;
}

/// Select a checkpoint for rollback.
#[async_trait::async_trait]
pub trait CheckpointReferencePort: Send + Sync {
    async fn select_checkpoint(&self, run_id: RunId) -> Result<Option<CheckpointId>, String>;
}

/// Start a continuation attempt.
#[async_trait::async_trait]
pub trait ContinuationIngressPort: Send + Sync {
    async fn start_continuation(&self, request: ContinuationRequest) -> Result<RunId, String>;
}
