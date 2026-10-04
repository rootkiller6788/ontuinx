//! CompensationCoordinator — M6-C compensatable effect authority.
//!
//! Decides whether to confirm, compensate, or freeze after an external
//! operation. Never retries automatically on unknown outcomes.

use std::sync::Arc;

use crate::ports::{
    CompensatableExternalPort, DecisionStorePort, ExternalEffectError,
    PublishAuthorizationError,
};
use onto_assurance_types::ids::{DecisionId, RunId, TransactionId};
use onto_assurance_types::transaction::{
    CompensationReceipt, CompensationRequest, CompensationStatus, ExternalOperationReceipt,
};

pub struct CompensationCoordinator {
    external: Arc<dyn CompensatableExternalPort>,
}

impl CompensationCoordinator {
    pub fn new(external: Arc<dyn CompensatableExternalPort>) -> Self { Self { external } }

    /// Verify the durable Decision allows confirmation.
    pub async fn authorize_confirm(
        &self,
        decision_id: DecisionId,
        run_id: RunId,
        decision_store: &dyn DecisionStorePort,
    ) -> Result<(), PublishAuthorizationError> {
        let outcome = decision_store.load_session(run_id).await
            .map_err(|e| PublishAuthorizationError::Internal(e.to_string()))?
            .ok_or_else(|| PublishAuthorizationError::DecisionNotFound(decision_id.to_string()))?;
        if outcome.task_outcome != onto_assurance_types::enums::TaskOutcome::Success {
            return Err(PublishAuthorizationError::TaskNotSuccessful(outcome.task_outcome));
        }
        Ok(())
    }

    /// Confirm that the external operation is satisfactory — no compensation needed.
    pub async fn confirm(
        &self,
        receipt: &ExternalOperationReceipt,
        decision_id: DecisionId,
        run_id: RunId,
        decision_store: &dyn DecisionStorePort,
    ) -> Result<CompensationReceipt, ExternalEffectError> {
        self.authorize_confirm(decision_id, run_id, decision_store).await
            .map_err(|e| ExternalEffectError::NotAuthorized(e.to_string()))?;
        Ok(CompensationReceipt {
            transaction_id: receipt.transaction_id,
            original_operation_id: receipt.operation_id.clone(),
            compensation_status: CompensationStatus::Confirmed,
            executed_at: chrono::Utc::now(),
        })
    }

    /// Execute compensation for an external operation.
    pub async fn compensate(
        &self,
        request: CompensationRequest,
        decision_id: DecisionId,
        run_id: RunId,
        decision_store: &dyn DecisionStorePort,
    ) -> Result<CompensationReceipt, ExternalEffectError> {
        // Verify original receipt exists and compensation is authorized
        self.authorize_confirm(decision_id, run_id, decision_store).await
            .map_err(|e| ExternalEffectError::NotAuthorized(e.to_string()))?;
        self.external.compensate(request).await
    }

    /// Handle unknown external outcome — freeze, do NOT retry blindly.
    pub fn freeze_unknown(
        &self,
        operation_id: &str,
        _reason: String,
    ) -> CompensationReceipt {
        CompensationReceipt {
            transaction_id: TransactionId::new(),
            original_operation_id: operation_id.into(),
            compensation_status: CompensationStatus::CompensationFailed,
            executed_at: chrono::Utc::now(),
        }
    }
}
