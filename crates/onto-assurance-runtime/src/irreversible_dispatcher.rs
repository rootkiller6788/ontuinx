//! AtMostOnceDispatcher — M6-D irreversible effect authority.
//!
//! Guarantees at-most-once dispatch. Never auto-retries on unknown outcomes.

use std::sync::Arc;

use crate::ports::{IrreversibleDispatchPort, IrreversibleEffectError};
use onto_assurance_types::ids::TransactionId;
use onto_assurance_types::transaction::{
    ContentHash, DispatchIntent, DispatchStatus, ExactInvocationLease,
    ExternalDispatchReceipt, IdempotencyKey, ManualReconciliationCase,
    PreExecutionDecision,
};

pub struct AtMostOnceDispatcher {
    dispatch: Arc<dyn IrreversibleDispatchPort>,
}

impl AtMostOnceDispatcher {
    pub fn new(dispatch: Arc<dyn IrreversibleDispatchPort>) -> Self { Self { dispatch } }

    /// Authorize: verify pre-execution decision and lease.
    pub fn authorize(
        &self,
        pre: &PreExecutionDecision,
        lease: &ExactInvocationLease,
    ) -> Result<(), IrreversibleEffectError> {
        if !pre.authorized {
            return Err(IrreversibleEffectError::NotAuthorized("pre-execution denied".into()));
        }
        if !lease.is_valid() {
            return Err(IrreversibleEffectError::LeaseInvalid);
        }
        Ok(())
    }

    /// Safe dispatch: record intent → dispatch → handle unknown outcome.
    pub async fn safe_dispatch(
        &self,
        txn_id: TransactionId,
        capability: &str,
        params_hash: ContentHash,
        lease: &mut ExactInvocationLease,
        params: &serde_json::Value,
    ) -> Result<ExternalDispatchReceipt, IrreversibleEffectError> {
        // Consume lease before dispatch
        if !lease.consume() {
            return Err(IrreversibleEffectError::LeaseInvalid);
        }

        // Record intent (crash recovery point)
        let intent = DispatchIntent {
            transaction_id: txn_id,
            capability: capability.into(),
            params_hash,
            lease_id: lease.lease_id.clone(),
            idempotency_key: IdempotencyKey::new(format!("irrev-{}", txn_id)),
            recorded_at: chrono::Utc::now(),
        };
        self.dispatch.record_intent(intent.clone()).await?;

        // Dispatch
        let receipt = self.dispatch.dispatch(&intent, lease, params).await?;

        // On unknown outcome → escalate, do NOT retry
        if receipt.status == DispatchStatus::UnknownExternalOutcome {
            return Err(IrreversibleEffectError::UnknownOutcome(
                format!("dispatch {}: outcome unknown after sending", receipt.dispatch_id),
            ));
        }

        Ok(receipt)
    }

    /// Create a manual reconciliation case for unknown outcomes.
    pub fn escalate_unknown(
        &self,
        txn_id: TransactionId,
        dispatch_id: &str,
        reason: &str,
    ) -> ManualReconciliationCase {
        ManualReconciliationCase {
            transaction_id: txn_id,
            dispatch_id: dispatch_id.into(),
            reason: reason.into(),
            recommended_action: "human must query external system and confirm or escalate".into(),
            created_at: chrono::Utc::now(),
        }
    }
}
