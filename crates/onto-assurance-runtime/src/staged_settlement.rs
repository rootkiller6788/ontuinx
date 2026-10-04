//! StagedSettlementCoordinator — M6-A2 Decision-gated publish authority.
//!
//! This is the ONLY code path that can produce a [`CommitPermit`].
//! Lives in `onto-assurance-runtime` so it can access `pub(crate)` fields.

use std::sync::Arc;

use crate::ports::{
    CommitPermit, DecisionStorePort, PublishAuthorizationError,
    PublishReceiptStorePort, StageError, StagedFilesystemPort,
};
use onto_assurance_types::ids::{AttemptId, DecisionId, RunId, TransactionId};
use onto_assurance_types::transaction::{ContentHash, FilesystemStageHandle, PublishReceipt, PublishStageRequest};

pub struct StagedSettlementCoordinator {
    filesystem: Arc<dyn StagedFilesystemPort>,
    receipt_store: Arc<dyn PublishReceiptStorePort>,
}

impl StagedSettlementCoordinator {
    pub fn new(
        filesystem: Arc<dyn StagedFilesystemPort>,
        receipt_store: Arc<dyn PublishReceiptStorePort>,
    ) -> Self {
        Self { filesystem, receipt_store }
    }

    /// Verify the durable Decision and issue a [`CommitPermit`].
    pub async fn authorize_publish(
        &self,
        decision_id: DecisionId,
        run_id: RunId,
        attempt_id: AttemptId,
        transaction_id: TransactionId,
        expected_baseline: ContentHash,
        expected_manifest: ContentHash,
        decision_store: &dyn DecisionStorePort,
    ) -> Result<CommitPermit, PublishAuthorizationError> {
        let outcome = decision_store
            .load_session(run_id)
            .await
            .map_err(|e| PublishAuthorizationError::Internal(format!("decision store: {}", e)))?
            .ok_or_else(|| PublishAuthorizationError::DecisionNotFound(decision_id.to_string()))?;

        if outcome.session_decision_id != decision_id {
            return Err(PublishAuthorizationError::DecisionNotFound(decision_id.to_string()));
        }
        if outcome.task_outcome != onto_assurance_types::enums::TaskOutcome::Success {
            return Err(PublishAuthorizationError::TaskNotSuccessful(outcome.task_outcome));
        }
        if outcome.lifecycle_state != onto_assurance_types::enums::LifecycleState::Committed
            && outcome.lifecycle_state != onto_assurance_types::enums::LifecycleState::Finalizing
        {
            return Err(PublishAuthorizationError::InvalidTransactionState(
                onto_assurance_types::enums::TransactionState::Prepared,
            ));
        }

        // M6-A2c: SettlementDecision must be Commit
        match outcome.settlement_decision {
            Some(onto_assurance_types::decision::SettlementDecision::Commit) => {}
            Some(settlement) => {
                return Err(PublishAuthorizationError::SettlementNotCommit(settlement));
            }
            None => {
                return Err(PublishAuthorizationError::Internal(
                    "settlement decision not recorded".into(),
                ));
            }
        }

        // M6-A2c: EffectClass must be Staged (this is the filesystem port)
        match outcome.effect_class {
            Some(onto_assurance_types::enums::EffectClass::Staged)
            | Some(onto_assurance_types::enums::EffectClass::Transactional) => {}
            Some(_unsupported) => {
                return Err(PublishAuthorizationError::EffectClassMismatch);
            }
            None => {
                return Err(PublishAuthorizationError::Internal(
                    "effect class not recorded".into(),
                ));
            }
        }

        Ok(CommitPermit::issue(
            decision_id,
            transaction_id,
            attempt_id,
            expected_baseline,
            expected_manifest,
        ))
    }

    /// Execute the full publish sequence: authorize → PUBLISHING → publish → Receipt.
    pub async fn execute_publish(
        &self,
        stage: &FilesystemStageHandle,
        permit: &CommitPermit,
    ) -> Result<PublishReceipt, StageError> {
        if stage.transaction_id != permit.transaction_id() {
            return Err(StageError::NotAuthorized("transaction mismatch".into()));
        }

        let request = PublishStageRequest {
            stage: stage.clone(),
            expected_baseline_hash: permit.baseline_hash().clone(),
            approved_manifest_hash: permit.manifest_hash().clone(),
            decision_id: permit.decision_id(),
            idempotency_key: permit.idempotency_key().clone(),
        };

        let receipt = self.filesystem.publish(request, permit).await?;
        let _ = self.receipt_store.store_receipt(&receipt).await;
        Ok(receipt)
    }
}
