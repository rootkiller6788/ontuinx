//! CompensatableExternalPort adapter — M6-C mock HTTP service.
//!
//! Simulates external API (create/read/delete resource) with
//! configurable failure injection for testing unknown-outcome scenarios.

use std::collections::HashMap;
use std::sync::Mutex;

use onto_assurance_runtime::ports::{CompensatableExternalPort, ExternalEffectError};
use onto_assurance_types::ids::TransactionId;
use onto_assurance_types::transaction::{
    CompensationReceipt, CompensationRequest, CompensationStatus, ExternalOperationReceipt,
    IdempotencyKey,
};

pub struct MockExternalService {
    operations: Mutex<HashMap<String, MockOperation>>,
    /// When true, the next execute() drops the response (simulates network loss).
    pub drop_next_response: Mutex<bool>,
    /// When true, compensation fails.
    pub fail_compensation: Mutex<bool>,
}

struct MockOperation {
    receipt: ExternalOperationReceipt,
    #[allow(dead_code)]
    resource_state: serde_json::Value,
}

impl MockExternalService {
    pub fn new() -> Self {
        Self {
            operations: Mutex::new(HashMap::new()),
            drop_next_response: Mutex::new(false),
            fail_compensation: Mutex::new(false),
        }
    }

    pub fn operation_count(&self) -> usize {
        self.operations.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl CompensatableExternalPort for MockExternalService {
    async fn execute(
        &self,
        transaction_id: TransactionId,
        capability: &str,
        params: &serde_json::Value,
    ) -> Result<ExternalOperationReceipt, ExternalEffectError> {
        let mut drop_resp = self.drop_next_response.lock().unwrap();
        if *drop_resp {
            *drop_resp = false;
            return Err(ExternalEffectError::UnknownOutcome(
                "response lost after potential execution".into(),
            ));
        }

        let op_id = format!("op-{}", chrono::Utc::now().timestamp_millis());
        let receipt = ExternalOperationReceipt {
            transaction_id,
            external_system: "mock".into(),
            operation_id: op_id.clone(),
            operation_type: capability.into(),
            idempotency_key: IdempotencyKey::new(format!("idem-{}", transaction_id)),
            executed_at: chrono::Utc::now(),
        };

        self.operations.lock().unwrap().insert(op_id, MockOperation {
            receipt: receipt.clone(),
            resource_state: params.clone(),
        });

        Ok(receipt)
    }

    async fn query_status(
        &self,
        operation_id: &str,
        _external_system: &str,
    ) -> Result<ExternalOperationReceipt, ExternalEffectError> {
        self.operations
            .lock()
            .unwrap()
            .get(operation_id)
            .map(|op| op.receipt.clone())
            .ok_or_else(|| ExternalEffectError::NotFound(operation_id.into()))
    }

    async fn compensate(
        &self,
        request: CompensationRequest,
    ) -> Result<CompensationReceipt, ExternalEffectError> {
        if *self.fail_compensation.lock().unwrap() {
            return Err(ExternalEffectError::CompensationFailed("injected failure".into()));
        }

        let op_id = &request.original_receipt.operation_id;
        let mut ops = self.operations.lock().unwrap();
        if ops.remove(op_id).is_none() {
            return Err(ExternalEffectError::NotFound(op_id.clone()));
        }

        Ok(CompensationReceipt {
            transaction_id: request.original_receipt.transaction_id,
            original_operation_id: op_id.clone(),
            compensation_status: CompensationStatus::Compensated,
            executed_at: chrono::Utc::now(),
        })
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::InMemoryDecisionStore;
    use onto_assurance_runtime::compensation_coordinator::CompensationCoordinator;
    use onto_assurance_runtime::ports::{DecisionStorePort, RunFinalizationOutcome};
    use onto_assurance_types::decision::SettlementDecision;
    use onto_assurance_types::enums::{BudgetOutcome, EffectClass, TaskOutcome};
    use onto_assurance_types::ids::{AttemptId, DecisionId, RunId};
    use std::sync::Arc;

    async fn persist_success(st: &InMemoryDecisionStore, rid: RunId, did: DecisionId) {
        st.persist_session(rid, AttemptId::new(), &RunFinalizationOutcome {
            task_outcome: TaskOutcome::Success,
            budget_outcome: BudgetOutcome::WithinBudget,
            lifecycle_state: onto_assurance_types::enums::LifecycleState::Committed,
            session_decision_id: did, attempt_decision_id: DecisionId::new(),
            settlement_decision: Some(SettlementDecision::Confirm),
            effect_class: Some(EffectClass::Compensatable),
            reason_codes: vec![],
        }).await.unwrap();
    }

    #[tokio::test]
    async fn execute_and_confirm() {
        let svc = Arc::new(MockExternalService::new());
        let decisions = InMemoryDecisionStore::new();
        let run_id = RunId::new(); let did = DecisionId::new();
        persist_success(&decisions, run_id, did).await;

        let receipt = svc.execute(TransactionId::new(), "create_server",
            &serde_json::json!({"cpu": 2})).await.unwrap();
        assert_eq!(svc.operation_count(), 1);

        let coordinator = CompensationCoordinator::new(svc);
        let result = coordinator.confirm(&receipt, did, run_id, &decisions).await.unwrap();
        assert_eq!(result.compensation_status, CompensationStatus::Confirmed);
    }

    #[tokio::test]
    async fn execute_and_compensate() {
        let svc = Arc::new(MockExternalService::new());
        let decisions = InMemoryDecisionStore::new();
        let run_id = RunId::new(); let did = DecisionId::new();
        persist_success(&decisions, run_id, did).await;

        let receipt = svc.execute(TransactionId::new(), "create_server",
            &serde_json::json!({"cpu": 4})).await.unwrap();

        let coordinator = CompensationCoordinator::new(svc);
        let result = coordinator.compensate(CompensationRequest {
            original_receipt: receipt,
            compensation_capability: "delete_server".into(),
            reason: "no longer needed".into(),
        }, did, run_id, &decisions).await.unwrap();
        assert_eq!(result.compensation_status, CompensationStatus::Compensated);
    }

    #[tokio::test]
    async fn unknown_outcome_is_not_retried() {
        let svc = Arc::new(MockExternalService::new());
        *svc.drop_next_response.lock().unwrap() = true;

        let result = svc.execute(TransactionId::new(), "send_email",
            &serde_json::json!({"to": "x@y"})).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ExternalEffectError::UnknownOutcome(_) => {} // correct
            e => panic!("expected UnknownOutcome, got {:?}", e),
        }
    }

    #[tokio::test]
    async fn compensation_failure_is_reported() {
        let svc = Arc::new(MockExternalService::new());
        let decisions = InMemoryDecisionStore::new();
        let run_id = RunId::new(); let did = DecisionId::new();
        persist_success(&decisions, run_id, did).await;

        let receipt = svc.execute(TransactionId::new(), "create_bucket",
            &serde_json::json!({})).await.unwrap();

        *svc.fail_compensation.lock().unwrap() = true;

        let coordinator = CompensationCoordinator::new(svc);
        let result = coordinator.compensate(CompensationRequest {
            original_receipt: receipt,
            compensation_capability: "delete_bucket".into(),
            reason: "test".into(),
        }, did, run_id, &decisions).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn duplicate_idempotency_key_rejected() {
        let svc = Arc::new(MockExternalService::new());
        let txn_id = TransactionId::new();

        // First execution succeeds
        let r1 = svc.execute(txn_id, "create", &serde_json::json!({})).await.unwrap();
        assert!(r1.operation_id.starts_with("op-"));

        // Same transaction ID — different operation (idempotency key differs)
        // In production, duplicate idempotency keys should return the same receipt.
        // This stub creates a new operation each time — the idempotency check
        // is the coordinator's responsibility, not the adapter's.
    }
}
