//! TransactionalDatabasePort adapter — M6-B stub.
//!
//! In-memory stub for testing the Decision-gated COMMIT/ROLLBACK flow.
//! Production: wraps a real PostgreSQL connection.

use std::collections::HashMap;
use std::sync::Mutex;

use onto_assurance_runtime::ports::{
    DatabaseCommitPermit, DatabaseError, TransactionalDatabasePort,
};
use onto_assurance_types::transaction::{
    BeginDatabaseTransactionRequest, DatabaseCandidateSnapshot, DatabaseCommand,
    DatabaseCommandOutcome, DatabaseRollbackReceipt, DatabaseTransactionHandle,
    DatabaseTransactionReceipt,
};

pub struct InMemoryDatabaseAdapter {
    transactions: Mutex<HashMap<String, InMemoryTx>>,
}

struct InMemoryTx {
    #[allow(dead_code)]
    handle: DatabaseTransactionHandle,
    rows: Vec<HashMap<String, serde_json::Value>>,
    committed: bool,
}

impl InMemoryDatabaseAdapter {
    pub fn new() -> Self {
        Self { transactions: Mutex::new(HashMap::new()) }
    }
}

#[async_trait::async_trait]
impl TransactionalDatabasePort for InMemoryDatabaseAdapter {
    async fn begin(
        &self,
        request: BeginDatabaseTransactionRequest,
    ) -> Result<DatabaseTransactionHandle, DatabaseError> {
        let handle = DatabaseTransactionHandle {
            transaction_id: request.transaction_id,
            attempt_id: request.attempt_id,
            database_resource: request.database_resource,
            begun_at: chrono::Utc::now(),
        };
        self.transactions.lock().unwrap().insert(
            request.transaction_id.to_string(),
            InMemoryTx { handle: handle.clone(), rows: vec![], committed: false },
        );
        Ok(handle)
    }

    async fn execute(
        &self,
        tx: &DatabaseTransactionHandle,
        command: DatabaseCommand,
    ) -> Result<DatabaseCommandOutcome, DatabaseError> {
        let mut txs = self.transactions.lock().unwrap();
        let entry = txs.get_mut(&tx.transaction_id.to_string())
            .ok_or_else(|| DatabaseError::TransactionError("tx not found".into()))?;
        if entry.committed {
            return Err(DatabaseError::TransactionError("already committed".into()));
        }
        let n = match &command {
            DatabaseCommand::Insert { table, values, .. } => {
                let mut row = HashMap::new();
                for (i, v) in values.iter().enumerate() {
                    row.insert(format!("col{}", i), v.clone());
                }
                row.insert("_table".into(), serde_json::Value::String(table.clone()));
                entry.rows.push(row);
                1
            }
            _ => 0,
        };
        Ok(DatabaseCommandOutcome { rows_affected: n, command: format!("{:?}", command) })
    }

    async fn inspect_candidate(
        &self,
        tx: &DatabaseTransactionHandle,
    ) -> Result<DatabaseCandidateSnapshot, DatabaseError> {
        let txs = self.transactions.lock().unwrap();
        let entry = txs.get(&tx.transaction_id.to_string())
            .ok_or_else(|| DatabaseError::TransactionError("tx not found".into()))?;
        Ok(DatabaseCandidateSnapshot {
            transaction_id: tx.transaction_id,
            row_counts: vec![("rows".into(), entry.rows.len() as u64)],
            checksum: None,
        })
    }

    async fn commit(
        &self,
        tx: DatabaseTransactionHandle,
        permit: &DatabaseCommitPermit,
    ) -> Result<DatabaseTransactionReceipt, DatabaseError> {
        // Verify permit matches transaction
        if tx.transaction_id != permit.transaction_id() {
            return Err(DatabaseError::PermitMismatch);
        }
        let mut txs = self.transactions.lock().unwrap();
        let entry = txs.get_mut(&tx.transaction_id.to_string())
            .ok_or_else(|| DatabaseError::TransactionError("tx not found".into()))?;
        entry.committed = true;
        Ok(DatabaseTransactionReceipt {
            transaction_id: tx.transaction_id,
            before_snapshot_hash: onto_assurance_types::transaction::ContentHash::new("before"),
            after_mutation_hash: onto_assurance_types::transaction::ContentHash::new("after"),
            committed_at: chrono::Utc::now(),
        })
    }

    async fn rollback(
        &self,
        tx: DatabaseTransactionHandle,
    ) -> Result<DatabaseRollbackReceipt, DatabaseError> {
        self.transactions.lock().unwrap().remove(&tx.transaction_id.to_string());
        Ok(DatabaseRollbackReceipt {
            transaction_id: tx.transaction_id,
            rolled_back_at: chrono::Utc::now(),
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
    use onto_assurance_runtime::database_settlement::DatabaseSettlementCoordinator;
    use onto_assurance_runtime::ports::{DecisionStorePort, RunFinalizationOutcome};
    use onto_assurance_types::decision::SettlementDecision;
    use onto_assurance_types::enums::{BudgetOutcome, EffectClass, TaskOutcome};
    use onto_assurance_types::ids::{AttemptId, DecisionId, RunId, TransactionId};
    use std::sync::Arc;
    use onto_assurance_types::transaction::ContentHash;

    async fn persist_success_decision(
        store: &InMemoryDecisionStore,
        run_id: RunId,
        decision_id: DecisionId,
    ) {
        let outcome = RunFinalizationOutcome {
            task_outcome: TaskOutcome::Success,
            budget_outcome: BudgetOutcome::WithinBudget,
            lifecycle_state: onto_assurance_types::enums::LifecycleState::Committed,
            session_decision_id: decision_id,
            attempt_decision_id: DecisionId::new(),
            settlement_decision: Some(SettlementDecision::Commit),
            effect_class: Some(EffectClass::Transactional),
            reason_codes: vec![],
        };
        store.persist_session(run_id, AttemptId::new(), &outcome).await.unwrap();
    }

    #[tokio::test]
    async fn begin_execute_commit_flow() {
        let db = Arc::new(InMemoryDatabaseAdapter::new());
        let handle = db.begin(BeginDatabaseTransactionRequest {
            transaction_id: TransactionId::new(), attempt_id: AttemptId::new(),
            database_resource: "test-db".into(),
        }).await.unwrap();

        let outcome = db.execute(&handle, DatabaseCommand::Insert {
            table: "users".into(),
            columns: vec!["name".into()],
            values: vec![serde_json::Value::String("alice".into())],
        }).await.unwrap();
        assert_eq!(outcome.rows_affected, 1);

        let permit = DatabaseCommitPermit::new_for_test_only(
            DecisionId::new(), handle.transaction_id, handle.attempt_id,
            "test-db".into(), ContentHash::new("mh"),
        );
        let receipt = db.commit(handle.clone(), &permit).await.unwrap();
        assert_eq!(receipt.transaction_id, handle.transaction_id);
    }

    #[tokio::test]
    async fn commit_with_wrong_permit_rejected() {
        let db = Arc::new(InMemoryDatabaseAdapter::new());
        let handle = db.begin(BeginDatabaseTransactionRequest {
            transaction_id: TransactionId::new(), attempt_id: AttemptId::new(),
            database_resource: "test-db".into(),
        }).await.unwrap();

        // Permit for a different transaction
        let wrong_permit = DatabaseCommitPermit::new_for_test_only(
            DecisionId::new(), TransactionId::new(), AttemptId::new(),
            "test-db".into(), ContentHash::new("x"),
        );
        let result = db.commit(handle.clone(), &wrong_permit).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rollback_cleans_up() {
        let db = Arc::new(InMemoryDatabaseAdapter::new());
        let handle = db.begin(BeginDatabaseTransactionRequest {
            transaction_id: TransactionId::new(), attempt_id: AttemptId::new(),
            database_resource: "test-db".into(),
        }).await.unwrap();

        db.execute(&handle, DatabaseCommand::Insert {
            table: "t".into(), columns: vec!["x".into()],
            values: vec![serde_json::Value::String("v".into())],
        }).await.unwrap();

        let receipt = db.rollback(handle.clone()).await.unwrap();
        assert!(!receipt.transaction_id.to_string().is_empty());
    }

    #[tokio::test]
    async fn coordinator_rejects_without_success_decision() {
        let db = Arc::new(InMemoryDatabaseAdapter::new());
        let decisions = InMemoryDecisionStore::new();
        let coordinator = DatabaseSettlementCoordinator::new(db);

        let result = coordinator.authorize_commit(
            DecisionId::new(), RunId::new(), AttemptId::new(), TransactionId::new(),
            "db".into(), ContentHash::new("h"),
            &decisions,
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn coordinator_authorizes_with_valid_decision() {
        let db = Arc::new(InMemoryDatabaseAdapter::new());
        let decisions = InMemoryDecisionStore::new();
        let run_id = RunId::new();
        let decision_id = DecisionId::new();
        persist_success_decision(&decisions, run_id, decision_id).await;

        let coordinator = DatabaseSettlementCoordinator::new(db);
        let permit = coordinator.authorize_commit(
            decision_id, run_id, AttemptId::new(), TransactionId::new(),
            "db".into(), ContentHash::new("h"),
            &decisions,
        ).await.unwrap();

        assert_eq!(permit.database_resource(), "db");
    }
}
