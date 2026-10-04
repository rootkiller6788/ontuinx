//! P16-P2: PostgreSQL-backed Assurance stores.
//!
//! Implements:
//! - AssuranceRunStore (plan, evidence, verdict persistence)
//! - AssuranceFailureStore (failure persistence)
//!
//! Connection failure → BootstrapError, IronClaw refuses to start.
//! No InMemory fallback in production.

use std::sync::Arc;
use async_trait::async_trait;
use tokio_postgres::{Client, NoTls};
use onto_assurance_runtime::assurance_run::{
    AssuranceRunHandle, AssuranceRunState, AssuranceRunStore,
    AssuranceFailureStore, EvidenceBundle, StoreError,
};
use onto_protocol::check::ConformancePlan;
use onto_protocol::persistence::{
    StoredConformancePlanV1, StoredConformanceVerdictV1, StoredEvidenceBundleV1,
};
use onto_protocol::verdict::{ConformanceVerdict, PersistedAssuranceFailure};

/// Combined PostgreSQL store for Assurance runs and failures.
pub struct PgAssuranceStore {
    client: Client,
}

impl PgAssuranceStore {
    pub async fn connect(conn_str: &str) -> Result<Self, String> {
        let (client, connection) = tokio_postgres::connect(conn_str, NoTls)
            .await
            .map_err(|e| format!("PG connect failed: {}", e))?;
        tokio::spawn(async move { if let Err(e) = connection.await { tracing::error!("PG connection lost: {}", e); } });
        Ok(Self { client })
    }

    /// Run migrations. Idempotent.
    pub async fn migrate(&self) -> Result<(), String> {
        self.client.batch_execute(include_str!("../sql/p16_assurance.sql"))
            .await
            .map_err(|e| format!("PG migration failed: {}", e))
    }
}

#[async_trait]
impl AssuranceRunStore for PgAssuranceStore {
    async fn begin_run(&self, attempt_id: &str) -> Result<AssuranceRunHandle, StoreError> {
        self.client.execute(
            "INSERT INTO p16_assurance_runs (run_id, state, created_at) VALUES ($1, $2, now()) ON CONFLICT DO NOTHING",
            &[&attempt_id, &"created"],
        ).await.map_err(|e| StoreError::Storage(e.to_string()))?;
        Ok(AssuranceRunHandle { attempt_id: attempt_id.to_string(), evidence_ref: None })
    }

    async fn record_plan(&self, run: &AssuranceRunHandle, plan: &ConformancePlan) -> Result<(), StoreError> {
        let stored = StoredConformancePlanV1::try_from(plan).map_err(|e| StoreError::Storage(e))?;
        let json = serde_json::to_string(&stored).map_err(|e| StoreError::Storage(e.to_string()))?;
        self.client.execute(
            "UPDATE p16_assurance_runs SET plan_json = $1, state = $2 WHERE run_id = $3",
            &[&json, &"plan_resolved", &run.attempt_id],
        ).await.map_err(|e| StoreError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn get_run_state(&self, attempt_id: &str) -> Result<AssuranceRunState, StoreError> {
        let row = self.client.query_opt(
            "SELECT state FROM p16_assurance_runs WHERE run_id = $1", &[&attempt_id],
        ).await.map_err(|e| StoreError::Storage(e.to_string()))?;
        match row {
            Some(r) => {
                let s: String = r.get(0);
                match s.as_str() {
                    "created" => Ok(AssuranceRunState::Created),
                    "plan_resolved" => Ok(AssuranceRunState::PlanResolved),
                    "executing" => Ok(AssuranceRunState::Executing),
                    "evidence_staged" => Ok(AssuranceRunState::EvidenceStaged),
                    "verdict_finalizing" => Ok(AssuranceRunState::VerdictFinalizing),
                    "finalized" => Ok(AssuranceRunState::Finalized),
                    "failed" => Ok(AssuranceRunState::Failed),
                    _ => Err(StoreError::Storage(format!("unknown state: {}", s))),
                }
            }
            None => Err(StoreError::NotFound(attempt_id.to_string())),
        }
    }

    async fn stage_evidence(&self, run: &AssuranceRunHandle, bundle: &EvidenceBundle) -> Result<(), StoreError> {
        let stored = StoredEvidenceBundleV1 {
            schema_version: 1,
            bundle_ref: bundle.bundle_ref.clone(),
            bundle_digest: bundle.bundle_digest.value.clone(),
            plan_id: bundle.plan_id.clone(),
            attempt_id: bundle.attempt_id.clone(),
            unit_count: bundle.unit_count,
            payload: bundle.payload.clone(),
        };
        let json = serde_json::to_string(&stored).map_err(|e| StoreError::Storage(e.to_string()))?;
        self.client.execute(
            "UPDATE p16_assurance_runs SET evidence_json = $1, state = $2 WHERE run_id = $3",
            &[&json, &"evidence_staged", &run.attempt_id],
        ).await.map_err(|e| StoreError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn finalize_phase1(&self, run: &AssuranceRunHandle, verdict: &ConformanceVerdict) -> Result<(), StoreError> {
        let stored = StoredConformanceVerdictV1::try_from(verdict).map_err(|e| StoreError::Storage(e))?;
        let json = serde_json::to_string(&stored).map_err(|e| StoreError::Storage(e.to_string()))?;
        self.client.execute(
            "INSERT INTO p16_verdicts (verdict_id, run_id, verdict_json, created_at) VALUES ($1, $2, $3, now()) ON CONFLICT (verdict_id) DO NOTHING",
            &[&verdict.verdict_id, &run.attempt_id, &json],
        ).await.map_err(|e| StoreError::Storage(e.to_string()))?;
        self.client.execute(
            "UPDATE p16_assurance_runs SET state = $1 WHERE run_id = $2",
            &[&"verdict_finalizing", &run.attempt_id],
        ).await.map_err(|e| StoreError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn mark_evidence_referenced(&self, run: &AssuranceRunHandle) -> Result<(), StoreError> {
        self.client.execute(
            "UPDATE p16_assurance_runs SET evidence_referenced = true WHERE run_id = $1",
            &[&run.attempt_id],
        ).await.map_err(|e| StoreError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn finalize_phase2(&self, run: &AssuranceRunHandle) -> Result<(), StoreError> {
        self.client.execute(
            "UPDATE p16_assurance_runs SET state = $1 WHERE run_id = $2",
            &[&"finalized", &run.attempt_id],
        ).await.map_err(|e| StoreError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn scan_incomplete_runs(&self) -> Result<Vec<AssuranceRunHandle>, StoreError> {
        let rows = self.client.query(
            "SELECT run_id FROM p16_assurance_runs WHERE state NOT IN ('finalized', 'failed')",
            &[],
        ).await.map_err(|e| StoreError::Storage(e.to_string()))?;
        Ok(rows.iter().map(|r| {
            let id: String = r.get(0);
            AssuranceRunHandle { attempt_id: id, evidence_ref: None }
        }).collect())
    }

    async fn load_verdict(&self, attempt_id: &str) -> Result<Option<onto_assurance_runtime::assurance_run::PersistedVerdict>, StoreError> {
        let row = self.client.query_opt(
            "SELECT verdict_json FROM p16_verdicts WHERE run_id = $1", &[&attempt_id],
        ).await.map_err(|e| StoreError::Storage(e.to_string()))?;
        match row {
            Some(r) => {
                let json: String = r.get(0);
                let _stored: StoredConformanceVerdictV1 = serde_json::from_str(&json).map_err(|e| StoreError::Storage(e.to_string()))?;
                // Reconstruct from stored DTO (full domain reconstruction deferred to P16-P2C)
                Err(StoreError::Storage("verdict deserialization from DTO not yet implemented".into()))
            }
            None => Ok(None),
        }
    }
}

#[async_trait]
impl AssuranceFailureStore for PgAssuranceStore {
    async fn persist_if_absent(&self, failure: &PersistedAssuranceFailure) -> Result<PersistedAssuranceFailure, StoreError> {
        let json = serde_json::to_string(failure).map_err(|e| StoreError::Storage(e.to_string()))?;
        self.client.execute(
            "INSERT INTO p16_assurance_failures (failure_id, run_id, failure_json, created_at) VALUES ($1, $2, $3, now()) ON CONFLICT (failure_id) DO NOTHING",
            &[&failure.failure_id, &failure.assurance_run_id, &json],
        ).await.map_err(|e| StoreError::Storage(e.to_string()))?;
        Ok(failure.clone())
    }
}
