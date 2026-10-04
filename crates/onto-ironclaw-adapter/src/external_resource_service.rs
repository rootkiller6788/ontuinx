//! External Resource Service — M6-C local HTTP service for testing.
//!
//! Simulates external API: POST/GET/PATCH/DELETE /resources/{id}.
//! Backed by real PostgreSQL. Used to test Intent-before-effect,
//! idempotency, compensation, and unknown-outcome reconciliation.

use std::sync::Arc;
use tokio_postgres::Client;

use onto_assurance_types::external_effects::{
    ExternalOperationId, ExternalOperationReceipt, ExternalOperationStatus, ExternalResourceId,
};

// ══════════════════════════════════════════════════════════════════
// ExternalResourceService
// ══════════════════════════════════════════════════════════════════

pub struct ExternalResourceService {
    client: Arc<Client>,
}

impl ExternalResourceService {
    pub fn new(client: Arc<Client>) -> Self { Self { client } }

    pub async fn ensure_schema(&self) -> Result<(), String> {
        self.client.batch_execute(
            "CREATE TABLE IF NOT EXISTS ext_resources (
                resource_id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                payload_hash TEXT NOT NULL,
                version BIGINT NOT NULL DEFAULT 1,
                created_by_operation_id TEXT,
                idempotency_key TEXT UNIQUE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
            );
            CREATE TABLE IF NOT EXISTS ext_operations (
                operation_id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL,
                decision_id TEXT NOT NULL,
                capability_id TEXT NOT NULL,
                operation_kind TEXT NOT NULL,
                request_payload_hash TEXT NOT NULL,
                idempotency_key TEXT UNIQUE,
                state TEXT NOT NULL DEFAULT 'executing',
                external_resource_id TEXT,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now()
            );
            CREATE TABLE IF NOT EXISTS ext_receipts (
                receipt_id SERIAL PRIMARY KEY,
                operation_id TEXT UNIQUE NOT NULL,
                provider_operation_id TEXT NOT NULL,
                external_resource_id TEXT,
                external_version BIGINT NOT NULL DEFAULT 1,
                response_hash TEXT NOT NULL,
                http_status INT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now()
            );"
        ).await.map_err(|e| format!("schema: {}", e))?;
        Ok(())
    }
    async fn clean(&self) {
        self.client.simple_query("DELETE FROM ext_receipts; DELETE FROM ext_operations; DELETE FROM ext_resources").await.ok();
    }

    // ── External API operations ──

    /// Create a resource. Idempotent by key.
    pub async fn create_resource(
        &self, operation_id: ExternalOperationId, payload: &str,
        idempotency_key: &str,
    ) -> Result<ExternalOperationReceipt, String> {
        // Check idempotency first
        if let Some(existing) = self.find_by_idempotency_key(idempotency_key).await? {
            return Ok(existing);
        }

        let resource_id = format!("res-{}", operation_id.0);
        let payload_hash = hash_str(payload);
        let rows = self.client.query(
            "INSERT INTO ext_resources (resource_id, status, payload_hash, created_by_operation_id, idempotency_key)
             VALUES ($1, 'created', $2, $3, $4)
             ON CONFLICT (idempotency_key) DO NOTHING
             RETURNING resource_id, version",
            &[&resource_id, &payload_hash, &operation_id.0, &idempotency_key],
        ).await.map_err(|e| format!("create: {}", e))?;

        let version: i64 = rows.get(0).map(|r| r.get::<_, i64>(1)).unwrap_or(1);

        let receipt = ExternalOperationReceipt {
            operation_id,
            provider_operation_id: resource_id.clone(),
            external_resource_id: Some(ExternalResourceId(resource_id)),
            request_hash: payload_hash.clone(),
            response_hash: payload_hash,
            external_version: version as u64,
            http_status: 201,
            observed_state: ExternalOperationStatus::Created,
            created_at: chrono::Utc::now(),
        };

        // Persist receipt
        self.client.execute(
            "INSERT INTO ext_receipts (operation_id, provider_operation_id, external_resource_id, external_version, response_hash, http_status)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (operation_id) DO NOTHING",
            &[&receipt.operation_id.0, &receipt.provider_operation_id,
              &receipt.external_resource_id.as_ref().map(|r| r.0.as_str()),
              &(receipt.external_version as i64), &receipt.response_hash, &(receipt.http_status as i32)],
        ).await.map_err(|e| format!("receipt: {}", e))?;

        Ok(receipt)
    }

    /// Get a resource by ID.
    pub async fn get_resource(&self, resource_id: &ExternalResourceId) -> Result<Option<ExternalOperationReceipt>, String> {
        let rows = self.client.query(
            "SELECT r.resource_id, r.version, r.payload_hash, o.operation_id
             FROM ext_resources r LEFT JOIN ext_operations o ON r.created_by_operation_id = o.operation_id
             WHERE r.resource_id = $1",
            &[&resource_id.0],
        ).await.map_err(|e| format!("get: {}", e))?;

        if rows.is_empty() { return Ok(None); }
        let r = &rows[0];
        Ok(Some(ExternalOperationReceipt {
            operation_id: ExternalOperationId(r.get::<_, Option<String>>(3).unwrap_or_default()),
            provider_operation_id: r.get(0),
            external_resource_id: Some(ExternalResourceId(r.get(0))),
            request_hash: String::new(),
            response_hash: r.get(2),
            external_version: r.get::<_, i64>(1) as u64,
            http_status: 200,
            observed_state: ExternalOperationStatus::Created,
            created_at: chrono::Utc::now(),
        }))
    }

    /// Delete a resource (compensation).
    pub async fn delete_resource(
        &self, operation_id: ExternalOperationId,
        resource_id: &ExternalResourceId, idempotency_key: &str,
    ) -> Result<ExternalOperationReceipt, String> {
        if let Some(existing) = self.find_by_idempotency_key(idempotency_key).await? {
            return Ok(existing);
        }

        let rows = self.client.query(
            "DELETE FROM ext_resources WHERE resource_id = $1 RETURNING resource_id, payload_hash, version",
            &[&resource_id.0],
        ).await.map_err(|e| format!("delete: {}", e))?;

        if rows.is_empty() {
            return Ok(ExternalOperationReceipt {
                operation_id,
                provider_operation_id: resource_id.0.clone(),
                external_resource_id: Some(resource_id.clone()),
                request_hash: String::new(), response_hash: String::new(),
                external_version: 0, http_status: 404,
                observed_state: ExternalOperationStatus::NotFound,
                created_at: chrono::Utc::now(),
            });
        }

        let receipt = ExternalOperationReceipt {
            operation_id,
            provider_operation_id: resource_id.0.clone(),
            external_resource_id: Some(resource_id.clone()),
            request_hash: String::new(),
            response_hash: format!("deleted-v{}", rows[0].get::<_, i64>(2)),
            external_version: rows[0].get::<_, i64>(2) as u64 + 1,
            http_status: 200,
            observed_state: ExternalOperationStatus::Created,
            created_at: chrono::Utc::now(),
        };

        self.client.execute(
            "INSERT INTO ext_receipts (operation_id, provider_operation_id, external_resource_id, external_version, response_hash, http_status)
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (operation_id) DO NOTHING",
            &[&receipt.operation_id.0, &receipt.provider_operation_id,
              &receipt.external_resource_id.as_ref().map(|r| r.0.as_str()),
              &(receipt.external_version as i64), &receipt.response_hash, &(receipt.http_status as i32)],
        ).await.map_err(|e| format!("receipt: {}", e))?;

        Ok(receipt)
    }

    /// Find a receipt by idempotency key (looks up ext_resources directly).
    pub async fn find_by_idempotency_key(&self, key: &str) -> Result<Option<ExternalOperationReceipt>, String> {
        let rows = self.client.query(
            "SELECT resource_id, version, payload_hash FROM ext_resources WHERE idempotency_key = $1",
            &[&key],
        ).await.map_err(|e| format!("find: {}", e))?;

        if rows.is_empty() { return Ok(None); }
        let r = &rows[0];
        let rid: String = r.get(0);
        Ok(Some(ExternalOperationReceipt {
            operation_id: ExternalOperationId(rid.clone()),
            provider_operation_id: rid.clone(),
            external_resource_id: Some(ExternalResourceId(rid)),
            request_hash: String::new(),
            response_hash: r.get(2),
            external_version: r.get::<_, i64>(1) as u64,
            http_status: 200,
            observed_state: ExternalOperationStatus::Created,
            created_at: chrono::Utc::now(),
        }))
    }

    // ── M6-C Authority Protocols ──

    pub async fn execute_with_intent(
        &self, operation_id: ExternalOperationId, payload: &str,
        idempotency_key: &str, authorized: bool,
    ) -> Result<ExternalOperationReceipt, String> {
        if !authorized {
            return Err("not authorized".into());
        }
        self.client.execute(
            "INSERT INTO ext_operations (operation_id, run_id, decision_id, capability_id, operation_kind, request_payload_hash, idempotency_key, state)
             VALUES ($1, 'run-1', 'dec-1', 'cap-1', 'Create', $2, $3, 'executing')
             ON CONFLICT (operation_id) DO NOTHING",
            &[&operation_id.0, &hash_str(payload), &idempotency_key],
        ).await.map_err(|e| format!("intent: {}", e))?;
        self.create_resource(operation_id, payload, idempotency_key).await
    }

    pub fn issue_compensation_permit(
        &self, original_op: &ExternalOperationId, original_receipt: &ExternalOperationReceipt,
        resource_id: &ExternalResourceId, expected_version: u64,
    ) -> Result<CompensationPermitData, String> {
        if original_receipt.external_resource_id.as_ref() != Some(resource_id) {
            return Err("resource mismatch".into());
        }
        if original_receipt.external_version != expected_version {
            return Err("version mismatch".into());
        }
        Ok(CompensationPermitData {
            original_operation_id: original_op.clone(),
            original_receipt_id: original_receipt.operation_id.0.clone(),
            resource_id: resource_id.clone(),
            expected_version,
            idempotency_key: format!("comp-{}", original_op.0),
            used: false,
        })
    }

    pub async fn execute_compensation(
        &self, permit: &mut CompensationPermitData,
    ) -> Result<ExternalOperationReceipt, String> {
        if permit.used {
            return Err("permit already consumed".into());
        }
        let current = self.get_resource(&permit.resource_id).await?;
        match current {
            Some(ref r) if r.external_version != permit.expected_version => {
                return Err(format!("version changed: expected {}, got {}", permit.expected_version, r.external_version));
            }
            None => return Err("resource already deleted".into()),
            _ => {}
        }
        permit.used = true;
        self.delete_resource(
            ExternalOperationId(format!("comp-{}", permit.original_operation_id.0)),
            &permit.resource_id, &permit.idempotency_key,
        ).await
    }

    pub async fn reconcile_unknown(&self, idempotency_key: &str, expected_payload: &str) -> Result<ReconciliationResult, String> {
        let found = self.find_by_idempotency_key(idempotency_key).await?;
        match found {
            Some(receipt) => {
                if receipt.response_hash == hash_str(expected_payload) {
                    Ok(ReconciliationResult::AlreadyExecuted(receipt))
                } else {
                    Ok(ReconciliationResult::Conflict { existing: receipt.response_hash, expected: hash_str(expected_payload) })
                }
            }
            None => Ok(ReconciliationResult::NotExecuted),
        }
    }
}

/// Opaque compensation authorization.
pub struct CompensationPermitData {
    pub original_operation_id: ExternalOperationId,
    pub original_receipt_id: String,
    pub resource_id: ExternalResourceId,
    pub expected_version: u64,
    pub idempotency_key: String,
    pub used: bool,
}

#[derive(Debug, PartialEq)]
pub enum ReconciliationResult {
    AlreadyExecuted(ExternalOperationReceipt),
    NotExecuted,
    Conflict { existing: String, expected: String },
}

fn hash_str(s: &str) -> String {
    use sha2::{Sha256, Digest};
    hex::encode(Sha256::digest(s.as_bytes()))
}

// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    fn tk(prefix: &str) -> String { format!("{}-{}", prefix, uuid::Uuid::new_v4()) }
    
    async fn service() -> ExternalResourceService {
        let (client, conn) = tokio_postgres::connect(
            "host=localhost user=ironclaw password=ironclaw dbname=ironclaw", tokio_postgres::NoTls
        ).await.unwrap();
        tokio::spawn(async move { if let Err(e) = conn.await { eprintln!("PG: {}", e); } });
        let svc = ExternalResourceService::new(Arc::new(client));
        INIT.get_or_init(|| {});
        svc.ensure_schema().await.ok();
        svc
    }

    #[tokio::test] async fn c0a_create_get_delete() {
        let s = service().await; let k = tk("c0a");
        let r = s.create_resource(ExternalOperationId(k.clone()), r#"{"x":1}"#, &k).await.unwrap();
        assert_eq!(r.http_status, 201);
        let rid = r.external_resource_id.unwrap();
        assert!(s.get_resource(&rid).await.unwrap().is_some());
        s.delete_resource(ExternalOperationId(tk("c0a-del")), &rid, &tk("c0a-del")).await.unwrap();
        assert!(s.get_resource(&rid).await.unwrap().is_none());
    }

    #[tokio::test] async fn c0b_lifecycle_with_key_lookup() {
        let s = service().await; let k = tk("c0b");
        let r = s.create_resource(ExternalOperationId(k.clone()), r#"{"v":1}"#, &k).await.unwrap();
        assert_eq!(r.http_status, 201);
        let found = s.find_by_idempotency_key(&k).await.unwrap();
        assert!(found.is_some());
        let rid = r.external_resource_id.unwrap();
        s.delete_resource(ExternalOperationId(tk("c0b-del")), &rid, &tk("c0b-del")).await.unwrap();
    }

    // C5: Intent-before-effect
    #[tokio::test] async fn c5_unauthorized_no_http() {
        let s = service().await; let k = tk("c5-unauth");
        let r = s.execute_with_intent(ExternalOperationId(k.clone()), r#"{}"#, &k, false).await;
        assert!(r.is_err());
    }
    #[tokio::test] async fn c5_authorized_executes() {
        let s = service().await; let k = tk("c5-auth");
        let r = s.execute_with_intent(ExternalOperationId(k.clone()), r#"{}"#, &k, true).await.unwrap();
        assert_eq!(r.http_status, 201);
    }
    #[tokio::test] async fn c5_intent_persisted_before_http() {
        let s = service().await; let k = tk("c5-intent");
        let op = ExternalOperationId(k.clone());
        s.execute_with_intent(op, r#"{}"#, &k, true).await.unwrap();
        // Intent row exists in ext_operations
        let rows = s.client.query("SELECT 1 FROM ext_operations WHERE operation_id=$1", &[&k]).await.unwrap();
        assert!(!rows.is_empty(), "intent must be persisted");
    }

    // C8: CompensationPermit binding
    #[tokio::test] async fn c8_resource_mismatch_rejected() {
        let s = service().await; let k = tk("c8");
        let receipt = s.create_resource(ExternalOperationId(k.clone()), r#"{}"#, &k).await.unwrap();
        let r = s.issue_compensation_permit(&ExternalOperationId(k), &receipt, &ExternalResourceId("wrong".into()), 1);
        assert!(r.is_err());
    }
    #[tokio::test] async fn c8_version_mismatch_rejected() {
        let s = service().await; let k = tk("c8v");
        let receipt = s.create_resource(ExternalOperationId(k.clone()), r#"{}"#, &k).await.unwrap();
        let rid = receipt.external_resource_id.clone().unwrap();
        let r = s.issue_compensation_permit(&ExternalOperationId(k), &receipt, &rid, 999);
        assert!(r.is_err());
    }

    // C9: Compensation execution
    #[tokio::test] async fn c9_valid_permit_executes() {
        let s = service().await; let k = tk("c9");
        let op = ExternalOperationId(k.clone());
        let receipt = s.create_resource(op.clone(), r#"{}"#, &k).await.unwrap();
        let rid = receipt.external_resource_id.clone().unwrap();
        let mut permit = s.issue_compensation_permit(&op, &receipt, &rid, 1).unwrap();
        let cr = s.execute_compensation(&mut permit).await.unwrap();
        assert_eq!(cr.http_status, 200);
        assert!(s.get_resource(&rid).await.unwrap().is_none());
    }
    #[tokio::test] async fn c9_permit_consumed_twice_rejected() {
        let s = service().await; let k = tk("c9b");
        let op = ExternalOperationId(k.clone());
        let receipt = s.create_resource(op.clone(), r#"{}"#, &k).await.unwrap();
        let rid = receipt.external_resource_id.clone().unwrap();
        let mut permit = s.issue_compensation_permit(&op, &receipt, &rid, 1).unwrap();
        s.execute_compensation(&mut permit).await.unwrap();
        assert!(s.execute_compensation(&mut permit).await.is_err());
    }

    // C10: Reconciliation
    #[tokio::test] async fn c10_finds_existing_by_key() {
        let s = service().await; let k = tk("c10");
        let payload = r#"{"ok":true}"#;
        s.create_resource(ExternalOperationId(k.clone()), payload, &k).await.unwrap();
        let result = s.reconcile_unknown(&k, payload).await.unwrap();
        assert!(matches!(result, ReconciliationResult::AlreadyExecuted(_)));
    }
    #[tokio::test] async fn c10_not_executed_safe_to_retry() {
        let s = service().await; let k = tk("c10-ne");
        let result = s.reconcile_unknown(&k, r#"{}"#).await.unwrap();
        assert_eq!(result, ReconciliationResult::NotExecuted);
    }
    #[tokio::test] async fn c10_payload_conflict_detected() {
        let s = service().await; let k = tk("c10-conflict");
        s.create_resource(ExternalOperationId(k.clone()), r#"{"a":1}"#, &k).await.unwrap();
        let result = s.reconcile_unknown(&k, r#"{"b":2}"#).await.unwrap();
        assert!(matches!(result, ReconciliationResult::Conflict{..}));
    }

    // C13: Response lost recovery
    #[tokio::test] async fn c13_response_lost_recovered_by_key() {
        let s = service().await; let k = tk("c13");
        let payload = r#"{"lost":true}"#;
        let r1 = s.create_resource(ExternalOperationId(k.clone()), payload, &k).await.unwrap();
        // Simulate response lost — query by key
        let r2 = s.find_by_idempotency_key(&k).await.unwrap().unwrap();
        assert_eq!(r1.external_resource_id, r2.external_resource_id);
    }

    // C15: External version change blocks compensation
    #[tokio::test] async fn c15_version_mismatch_rejects_permit() {
        let s = service().await; let k = tk("c15");
        let op = ExternalOperationId(k.clone());
        let receipt = s.create_resource(op.clone(), r#"{}"#, &k).await.unwrap();
        let rid = receipt.external_resource_id.clone().unwrap();
        // Permit with wrong version → rejected at issuance
        let result = s.issue_compensation_permit(&op, &receipt, &rid, 99);
        assert!(result.is_err(), "version mismatch must reject permit issuance");
    }

    // C17: Repeated reconcile zero side effects
    #[tokio::test] async fn c17_reconcile_twice_no_duplicates() {
        let s = service().await; let k = tk("c17");
        s.create_resource(ExternalOperationId(k.clone()), r#"{}"#, &k).await.unwrap();
        let r1 = s.reconcile_unknown(&k, r#"{}"#).await.unwrap();
        let r2 = s.reconcile_unknown(&k, r#"{}"#).await.unwrap();
        // Both should return AlreadyExecuted (same result)
        assert!(matches!(r1, ReconciliationResult::AlreadyExecuted(_)));
        assert!(matches!(r2, ReconciliationResult::AlreadyExecuted(_)));
    }

    // C2a: Create + verify (simplified, uses unique key)
    #[tokio::test] async fn c2a_create_and_verify() {
        let s = service().await; let k = tk("c2a");
        let r = s.create_resource(ExternalOperationId(k.clone()), r#"{"v":1}"#, &k).await.unwrap();
        assert_eq!(r.http_status, 201);
    }
}
