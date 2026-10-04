//! Test-support stores — gated behind #[cfg(any(test, feature = "test-support"))].
//!
//! Extracted from `finalizer.rs` during P16-6 cleanup.
//! These are NOT production types.

use std::collections::HashMap;
use std::sync::Mutex;

use onto_assurance_runtime::ports::{
    DecisionStoreError, DecisionStorePort, EventSinkError, EventSinkPort,
    EvidenceStoreError, EvidenceStorePort, RunFinalizationOutcome,
};
use onto_assurance_types::evidence::EvidenceBundle;
use onto_assurance_types::ids::{AttemptId, RunId};

// ══════════════════════════════════════════════════════════════════
// InMemoryEvidenceStore
// ══════════════════════════════════════════════════════════════════

pub struct InMemoryEvidenceStore {
    records: Mutex<HashMap<String, EvidenceBundle>>,
}

impl InMemoryEvidenceStore {
    pub fn new() -> Self {
        Self { records: Mutex::new(HashMap::new()) }
    }
    pub fn insert(&self, key: String, bundle: EvidenceBundle) {
        self.records.lock().unwrap().insert(key, bundle);
    }
}

#[async_trait::async_trait]
impl EvidenceStorePort for InMemoryEvidenceStore {
    async fn store(&self, bundle: &EvidenceBundle) -> Result<String, EvidenceStoreError> {
        let key = format!("evidence/{}", bundle.bundle_id);
        self.records.lock().unwrap().insert(key.clone(), bundle.clone());
        Ok(key)
    }
    async fn retrieve(&self, key: &str) -> Result<EvidenceBundle, EvidenceStoreError> {
        self.records.lock().unwrap().get(key).cloned()
            .ok_or(EvidenceStoreError::NotFound(key.into()))
    }
}

// ══════════════════════════════════════════════════════════════════
// InMemoryDecisionStore
// ══════════════════════════════════════════════════════════════════

pub struct InMemoryDecisionStore {
    decisions: Mutex<HashMap<String, RunFinalizationOutcome>>,
    pub fail_next_persist: Mutex<bool>,
}

impl InMemoryDecisionStore {
    pub fn new() -> Self {
        Self { decisions: Mutex::new(HashMap::new()), fail_next_persist: Mutex::new(false) }
    }
}

#[async_trait::async_trait]
impl DecisionStorePort for InMemoryDecisionStore {
    async fn persist_session(
        &self, run_id: RunId, _attempt_id: AttemptId, outcome: &RunFinalizationOutcome,
    ) -> Result<bool, DecisionStoreError> {
        let mut should_fail = self.fail_next_persist.lock().unwrap();
        if *should_fail { *should_fail = false; return Err(DecisionStoreError::Storage("injected persist failure".into())); }
        let already = self.decisions.lock().unwrap().contains_key(&run_id.to_string());
        self.decisions.lock().unwrap().insert(run_id.to_string(), outcome.clone());
        Ok(!already)
    }
    async fn load_session(&self, run_id: RunId) -> Result<Option<RunFinalizationOutcome>, DecisionStoreError> {
        Ok(self.decisions.lock().unwrap().get(&run_id.to_string()).cloned())
    }
}

// ══════════════════════════════════════════════════════════════════
// InMemoryEventSink
// ══════════════════════════════════════════════════════════════════

pub struct InMemoryEventSink {
    pub events: Mutex<Vec<(String, serde_json::Value)>>,
}

impl InMemoryEventSink {
    pub fn new() -> Self { Self { events: Mutex::new(Vec::new()) } }
}

#[async_trait::async_trait]
impl EventSinkPort for InMemoryEventSink {
    async fn emit(&self, event_type: &str, payload: &serde_json::Value) -> Result<(), EventSinkError> {
        self.events.lock().unwrap().push((event_type.into(), payload.clone()));
        Ok(())
    }
}
