//! Mock implementations of all Port traits for testing.
//!
//! Gated behind `#[cfg(any(test, feature = "test-support"))]`.
//! Each mock records calls and returns pre-configured responses.

use std::collections::HashMap;
use std::sync::Mutex;

use onto_assurance_types::contract::ExecutionIntent;
use onto_assurance_types::evidence::EvidenceBundle;
use onto_assurance_types::ids::{AttemptId, RunId, TransactionId};
use onto_assurance_types::transaction::{ExecutionTransaction, SideEffectManifest};

use crate::ports::{
    ApprovalDecision, ApprovalError, ApprovalPort, ApprovalStatus, AuthorizationError,
    AuthorizationPort, AuthorizationReceipt, CheckpointError, CheckpointPort, ClockPort,
    DecisionStoreError, DecisionStorePort, EventSinkError, EventSinkPort,
    EvidenceStoreError, EvidenceStorePort, ExecutionResult, RunFinalizationOutcome,
    RuntimeError, RuntimePort, StageHandle,
};

// ══════════════════════════════════════════════════════════════════
// MockAuthorizationPort
// ══════════════════════════════════════════════════════════════════

pub struct MockAuthorizationPort {
    pub allow: bool,
    pub call_log: Mutex<Vec<String>>,
}

impl MockAuthorizationPort {
    pub fn new_allow() -> Self {
        Self { allow: true, call_log: Mutex::new(Vec::new()) }
    }

    pub fn new_deny() -> Self {
        Self { allow: false, call_log: Mutex::new(Vec::new()) }
    }
}

#[async_trait::async_trait]
impl AuthorizationPort for MockAuthorizationPort {
    async fn authorize(&self, _intent: &ExecutionIntent) -> Result<AuthorizationReceipt, AuthorizationError> {
        self.call_log.lock().unwrap().push("authorize".into());
        Ok(AuthorizationReceipt {
            run_id: RunId::new(),
            authorized: self.allow,
            grant_id: "mock-grant".into(),
            lease_id: Some("mock-lease".into()),
            restrictions: vec![],
        })
    }

    async fn check_valid(&self, _receipt: &AuthorizationReceipt) -> Result<bool, AuthorizationError> {
        self.call_log.lock().unwrap().push("check_valid".into());
        Ok(self.allow)
    }
}

// ══════════════════════════════════════════════════════════════════
// MockApprovalPort
// ══════════════════════════════════════════════════════════════════

pub struct MockApprovalPort {
    pub decision: ApprovalDecision,
    pub call_log: Mutex<Vec<String>>,
}

impl MockApprovalPort {
    pub fn new_granted() -> Self {
        Self { decision: ApprovalDecision::Granted, call_log: Mutex::new(Vec::new()) }
    }

    pub fn new_rejected(reason: &str) -> Self {
        Self {
            decision: ApprovalDecision::Rejected { reason: reason.into() },
            call_log: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl ApprovalPort for MockApprovalPort {
    async fn request_approval(&self, _run_id: RunId, _reason: &str) -> Result<String, ApprovalError> {
        self.call_log.lock().unwrap().push("request_approval".into());
        Ok("mock-approval-id".into())
    }

    async fn check_status(&self, _approval_id: &str) -> Result<ApprovalStatus, ApprovalError> {
        self.call_log.lock().unwrap().push("check_status".into());
        match &self.decision {
            ApprovalDecision::Granted => Ok(ApprovalStatus::Granted),
            ApprovalDecision::Rejected { .. } => Ok(ApprovalStatus::Rejected),
            ApprovalDecision::TimedOut => Ok(ApprovalStatus::Expired),
        }
    }

    async fn wait_for_decision(&self, _approval_id: &str, _timeout_seconds: u64) -> Result<ApprovalDecision, ApprovalError> {
        self.call_log.lock().unwrap().push("wait_for_decision".into());
        Ok(self.decision.clone())
    }
}

// ══════════════════════════════════════════════════════════════════
// MockRuntimePort
// ══════════════════════════════════════════════════════════════════

pub struct MockRuntimePort {
    pub exit_code: i32,
    pub stdout: String,
    pub call_log: Mutex<Vec<String>>,
}

impl MockRuntimePort {
    pub fn new_success() -> Self {
        Self {
            exit_code: 0,
            stdout: "OK".into(),
            call_log: Mutex::new(Vec::new()),
        }
    }

    pub fn new_failure(code: i32, stderr: &str) -> Self {
        Self {
            exit_code: code,
            stdout: stderr.into(),
            call_log: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl RuntimePort for MockRuntimePort {
    async fn stage(&self, txn: &ExecutionTransaction) -> Result<StageHandle, RuntimeError> {
        self.call_log.lock().unwrap().push("stage".into());
        Ok(StageHandle {
            transaction_id: txn.transaction_id,
            handle_id: "mock-stage-id".into(),
            environment_info: "mock sandbox".into(),
        })
    }

    async fn execute(&self, _handle: &StageHandle, _command: &str) -> Result<ExecutionResult, RuntimeError> {
        self.call_log.lock().unwrap().push("execute".into());
        Ok(ExecutionResult {
            exit_code: self.exit_code,
            stdout: self.stdout.clone(),
            stderr: String::new(),
            duration_ms: 42,
        })
    }

    async fn capture_effects(&self, _handle: &StageHandle) -> Result<SideEffectManifest, RuntimeError> {
        self.call_log.lock().unwrap().push("capture_effects".into());
        Ok(SideEffectManifest {
            transaction_id: TransactionId::new(),
            filesystem_changes: vec![],
            network_requests: vec![],
            subprocess_invocations: vec![],
            external_receipts: vec![],
        })
    }

    async fn publish(&self, _handle: &StageHandle, _decision: &onto_assurance_types::decision::SettlementDecision) -> Result<(), RuntimeError> {
        self.call_log.lock().unwrap().push("publish".into());
        Ok(())
    }

    async fn discard(&self, _handle: &StageHandle) -> Result<(), RuntimeError> {
        self.call_log.lock().unwrap().push("discard".into());
        Ok(())
    }

    async fn freeze(&self, _handle: &StageHandle) -> Result<(), RuntimeError> {
        self.call_log.lock().unwrap().push("freeze".into());
        Ok(())
    }
}

// Old MockVerifierPort removed — use onto_protocol's MockVerificationExecutor instead.

// ══════════════════════════════════════════════════════════════════
// MockEvidenceStorePort
// ══════════════════════════════════════════════════════════════════

pub struct MockEvidenceStorePort {
    pub stored: Mutex<HashMap<String, EvidenceBundle>>,
}

impl MockEvidenceStorePort {
    pub fn new() -> Self {
        Self { stored: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl EvidenceStorePort for MockEvidenceStorePort {
    async fn store(&self, bundle: &EvidenceBundle) -> Result<String, EvidenceStoreError> {
        let key = format!("evidence/{}", bundle.bundle_id);
        self.stored.lock().unwrap().insert(key.clone(), bundle.clone());
        Ok(key)
    }

    async fn retrieve(&self, key: &str) -> Result<EvidenceBundle, EvidenceStoreError> {
        self.stored
            .lock()
            .unwrap()
            .get(key)
            .cloned()
            .ok_or(EvidenceStoreError::NotFound(key.into()))
    }
}

// ══════════════════════════════════════════════════════════════════
// MockEventSinkPort
// ══════════════════════════════════════════════════════════════════

pub struct MockEventSinkPort {
    pub events: Mutex<Vec<(String, serde_json::Value)>>,
}

impl MockEventSinkPort {
    pub fn new() -> Self {
        Self { events: Mutex::new(Vec::new()) }
    }
}

#[async_trait::async_trait]
impl EventSinkPort for MockEventSinkPort {
    async fn emit(&self, event_type: &str, payload: &serde_json::Value) -> Result<(), EventSinkError> {
        self.events.lock().unwrap().push((event_type.into(), payload.clone()));
        Ok(())
    }
}

// ══════════════════════════════════════════════════════════════════
// MockCheckpointPort
// ══════════════════════════════════════════════════════════════════

pub struct MockCheckpointPort {
    pub checkpoints: Mutex<HashMap<RunId, serde_json::Value>>,
}

impl MockCheckpointPort {
    pub fn new() -> Self {
        Self { checkpoints: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl CheckpointPort for MockCheckpointPort {
    async fn save(&self, run_id: RunId, state: &serde_json::Value) -> Result<String, CheckpointError> {
        self.checkpoints.lock().unwrap().insert(run_id, state.clone());
        Ok(format!("checkpoint/{}", run_id))
    }

    async fn load(&self, run_id: RunId) -> Result<Option<serde_json::Value>, CheckpointError> {
        Ok(self.checkpoints.lock().unwrap().get(&run_id).cloned())
    }
}

// ══════════════════════════════════════════════════════════════════
// MockClockPort
// ══════════════════════════════════════════════════════════════════

pub struct MockClockPort {
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl MockClockPort {
    pub fn new() -> Self {
        Self { timestamp: chrono::Utc::now() }
    }
}

impl ClockPort for MockClockPort {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.timestamp
    }
}

// ══════════════════════════════════════════════════════════════════
// MockDecisionStore (M5)
// ══════════════════════════════════════════════════════════════════

/// In-memory decision store for testing M5 finalization.
/// Stores `RunFinalizationOutcome` keyed by `RunId`.
pub struct MockDecisionStore {
    pub store: Mutex<HashMap<String, RunFinalizationOutcome>>,
}

impl MockDecisionStore {
    pub fn new() -> Self {
        Self { store: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl DecisionStorePort for MockDecisionStore {
    async fn persist_session(
        &self,
        run_id: RunId,
        _attempt_id: AttemptId,
        outcome: &RunFinalizationOutcome,
    ) -> Result<bool, DecisionStoreError> {
        let already_exists = self.store.lock().unwrap().contains_key(&run_id.to_string());
        self.store.lock().unwrap().insert(run_id.to_string(), outcome.clone());
        Ok(!already_exists) // true = first write
    }

    async fn load_session(
        &self,
        run_id: RunId,
    ) -> Result<Option<RunFinalizationOutcome>, DecisionStoreError> {
        Ok(self.store.lock().unwrap().get(&run_id.to_string()).cloned())
    }
}

// ══════════════════════════════════════════════════════════════════
// MockRuntimeRunPort — recording stub for OntoLoop tests (Phase 6)
// ══════════════════════════════════════════════════════════════════

use crate::ports::{AssuranceResultPort, ContinuationIngressPort, RuntimeRunPort};
use onto_assurance_types::ontoloop::ContinuationRequest;

pub struct MockLoopRuntime {
    pub runs: Mutex<Vec<(u32, String)>>,
    pub outcomes: Mutex<Vec<RunFinalizationOutcome>>,
    pub cancelled: Mutex<Vec<RunId>>,
}

impl MockLoopRuntime {
    pub fn new() -> Self {
        Self { runs: Mutex::new(vec![]), outcomes: Mutex::new(vec![]), cancelled: Mutex::new(vec![]) }
    }
    pub fn push_outcome(&self, o: RunFinalizationOutcome) { self.outcomes.lock().unwrap().push(o); }
    pub fn run_count(&self) -> usize { self.runs.lock().unwrap().len() }
}

#[async_trait::async_trait]
impl RuntimeRunPort for MockLoopRuntime {
    async fn start_run(&self, n: u32, obj: &str) -> Result<RunId, String> {
        self.runs.lock().unwrap().push((n, obj.into()));
        Ok(RunId::new())
    }
    async fn await_terminal(&self, _rid: RunId) -> Result<RunFinalizationOutcome, String> {
        let mut outcomes = self.outcomes.lock().unwrap();
        if outcomes.is_empty() { Err("no outcomes".into()) }
        else { Ok(outcomes.remove(0)) } // FIFO
    }
    async fn cancel_run(&self, rid: RunId) -> Result<(), String> {
        self.cancelled.lock().unwrap().push(rid); Ok(())
    }
}

#[async_trait::async_trait]
impl AssuranceResultPort for MockLoopRuntime {
    async fn load_finalization(&self, _rid: RunId) -> Result<RunFinalizationOutcome, String> {
        self.outcomes.lock().unwrap().last().cloned().ok_or_else(|| "none".into())
    }
}

#[async_trait::async_trait]
impl ContinuationIngressPort for MockLoopRuntime {
    async fn start_continuation(&self, _req: ContinuationRequest) -> Result<RunId, String> {
        Ok(RunId::new())
    }
}
