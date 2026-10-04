//! RuntimeObservationPort adapter — Phase 4 stub.
//!
//! Records observations in memory. Production: writes to OntoRuntime EventStore
//! with references that OntoAssure's Verifier can later resolve.

use std::sync::Mutex;

use onto_assurance_runtime::ports::{ObservationError, RuntimeObservationPort};
use onto_assurance_types::observation::RuntimeObservation;

/// In-memory observation store for testing.
pub struct InMemoryObservationStore {
    pub observations: Mutex<Vec<RuntimeObservation>>,
}

impl InMemoryObservationStore {
    pub fn new() -> Self {
        Self { observations: Mutex::new(Vec::new()) }
    }

    pub fn count(&self) -> usize {
        self.observations.lock().unwrap().len()
    }

    pub fn last(&self) -> Option<RuntimeObservation> {
        self.observations.lock().unwrap().last().cloned()
    }
}

impl Default for InMemoryObservationStore {
    fn default() -> Self { Self::new() }
}

#[async_trait::async_trait]
impl RuntimeObservationPort for InMemoryObservationStore {
    async fn observe(
        &self,
        observation: RuntimeObservation,
    ) -> Result<(), ObservationError> {
        self.observations.lock().unwrap().push(observation);
        Ok(())
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::ids::InvocationId;
    use onto_assurance_types::observation::{OutcomeRef, OutcomeType, RuntimeErrorKind};

    #[tokio::test]
    async fn records_observation() {
        let store = InMemoryObservationStore::new();
        let obs = RuntimeObservation::new(
            InvocationId::new(),
            OutcomeRef { outcome_id: "o1".into(), outcome_type: OutcomeType::Completed },
        ).with_exit_code(0);

        store.observe(obs).await.unwrap();
        assert_eq!(store.count(), 1);
    }

    #[tokio::test]
    async fn observation_separates_facts_from_judgment() {
        let store = InMemoryObservationStore::new();

        // A failed execution produces an observation with an error
        let obs = RuntimeObservation::new(
            InvocationId::new(),
            OutcomeRef { outcome_id: "o2".into(), outcome_type: OutcomeType::Failed },
        ).with_exit_code(1).with_error(RuntimeErrorKind::Timeout);

        store.observe(obs).await.unwrap();

        let recorded = store.last().unwrap();
        assert!(!recorded.execution_ok());
        assert_eq!(recorded.exit_code, Some(1));

        // But the store does NOT produce Evidence or Verdict — those are
        // separate OntoAssure concerns.
    }
}
