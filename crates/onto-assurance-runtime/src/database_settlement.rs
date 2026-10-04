//! DatabaseSettlementCoordinator — M6-B Decision-gated database commit.
//!
//! Only a durable SUCCESS + COMMIT + EffectClass::Transactional Decision
//! can authorize a database COMMIT.

use std::sync::Arc;

use crate::ports::{
    DatabaseCommitPermit, DatabaseError, DecisionStorePort,
    PublishAuthorizationError, TransactionalDatabasePort,
};
use onto_assurance_types::ids::{AttemptId, DecisionId, RunId, TransactionId};
use onto_assurance_types::transaction::{ContentHash, DatabaseTransactionHandle, DatabaseTransactionReceipt};

pub struct DatabaseSettlementCoordinator {
    database: Arc<dyn TransactionalDatabasePort>,
}

impl DatabaseSettlementCoordinator {
    pub fn new(database: Arc<dyn TransactionalDatabasePort>) -> Self {
        Self { database }
    }

    /// Authorize a COMMIT by verifying the durable Decision.
    pub async fn authorize_commit(
        &self,
        decision_id: DecisionId,
        run_id: RunId,
        attempt_id: AttemptId,
        transaction_id: TransactionId,
        database_resource: String,
        manifest_hash: ContentHash,
        decision_store: &dyn DecisionStorePort,
    ) -> Result<DatabaseCommitPermit, PublishAuthorizationError> {
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

        match outcome.settlement_decision {
            Some(onto_assurance_types::decision::SettlementDecision::Commit) => {}
            Some(s) => return Err(PublishAuthorizationError::SettlementNotCommit(s)),
            None => return Err(PublishAuthorizationError::Internal("no settlement".into())),
        }
        match outcome.effect_class {
            Some(onto_assurance_types::enums::EffectClass::Transactional) => {}
            Some(_) => return Err(PublishAuthorizationError::EffectClassMismatch),
            None => return Err(PublishAuthorizationError::Internal("no effect class".into())),
        }

        Ok(DatabaseCommitPermit::issue(
            decision_id,
            transaction_id,
            attempt_id,
            database_resource,
            manifest_hash,
        ))
    }

    /// Execute COMMIT with a valid permit.
    pub async fn execute_commit(
        &self,
        tx: DatabaseTransactionHandle,
        permit: &DatabaseCommitPermit,
    ) -> Result<DatabaseTransactionReceipt, DatabaseError> {
        self.database.commit(tx, permit).await
    }
}
