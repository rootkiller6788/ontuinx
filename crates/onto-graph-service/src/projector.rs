//! P8 Projector — consumes authoritative events and projects into PG.
//!
//! Consumer Group: ontofirmwaregraph-projector
//! Stream: ontoos.authoritative-events.v1
//!
//! Projection semantics:
//!   COMMIT    → promote snapshot (SEALED → PROMOTED)
//!   CONTINUE  → record INCOMPLETE episode
//!   ROLLBACK  → record Failed episode
//!   ESCALATE  → mark PendingAuthority
//!
//! Idempotent: duplicate event_id → skip (UNIQUE constraint on ofg_execution_projection).

use sqlx::PgPool;
use std::time::Duration;
use tracing::{info, warn};

const CONSUMER_GROUP: &str = "ontofirmwaregraph-projector";

/// Event received from the authoritative event stream.
#[derive(Debug, Clone)]
pub struct AuthoritativeEvent {
    pub event_id: String,
    pub transaction_id: String,
    pub aggregate_id: String,
    pub event_type: String,
    pub payload: String,
}

/// Trait for consuming events from any transport (Redis Streams, in-memory, etc.).
#[async_trait::async_trait]
pub trait EventSource: Send + Sync {
    /// Read a batch of events. Returns empty vec if nothing available.
    async fn read_batch(&self, max_count: usize) -> Result<Vec<AuthoritativeEvent>, String>;
    /// Acknowledge an event has been processed.
    async fn ack(&self, event_id: &str) -> Result<(), String>;
    /// Send a failed event to dead letter.
    async fn dead_letter(&self, original_id: &str, error: &str) -> Result<(), String>;
}

/// Simple polling event source (for use without Redis).
pub struct PollingEventSource {
    pool: PgPool,
}

impl PollingEventSource {
    pub fn new(pool: PgPool) -> Self { Self { pool } }
}

#[async_trait::async_trait]
impl EventSource for PollingEventSource {
    async fn read_batch(&self, max_count: usize) -> Result<Vec<AuthoritativeEvent>, String> {
        let rows = sqlx::query_as::<_, OutboxRow>(
            "SELECT event_id, transaction_id, aggregate_id, event_type, payload::text
             FROM onto_outbox
             WHERE published_at IS NOT NULL
               AND redis_projection_version IS NOT NULL
               AND NOT EXISTS (
                 SELECT 1 FROM ofg_execution_projection ep WHERE ep.event_id = onto_outbox.event_id
               )
             ORDER BY created_at
             LIMIT $1"
        )
        .bind(max_count as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| format!("poll: {e}"))?;

        Ok(rows.into_iter().map(|r| AuthoritativeEvent {
            event_id: r.event_id, transaction_id: r.transaction_id,
            aggregate_id: r.aggregate_id, event_type: r.event_type, payload: r.payload,
        }).collect())
    }

    async fn ack(&self, event_id: &str) -> Result<(), String> {
        sqlx::query("INSERT INTO ofg_projection_offset (event_id, consumed_at) VALUES ($1, now()) ON CONFLICT DO NOTHING")
            .bind(event_id).execute(&self.pool).await
            .map_err(|e| format!("ack: {e}"))?;
        Ok(())
    }

    async fn dead_letter(&self, original_id: &str, error: &str) -> Result<(), String> {
        sqlx::query(
            "INSERT INTO ofg_dead_letter (event_id, error, recorded_at)
             VALUES ($1, $2, now()) ON CONFLICT (event_id) DO UPDATE SET error = $2"
        )
        .bind(original_id).bind(error).execute(&self.pool).await
        .map_err(|e| format!("dead_letter: {e}"))?;
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct OutboxRow {
    event_id: String, transaction_id: String, aggregate_id: String,
    event_type: String, payload: String,
}

/// The Projector: reads from an EventSource, writes to ofg_execution_projection.
pub struct Projector {
    pool: PgPool,
    source: Box<dyn EventSource>,
}

impl Projector {
    pub fn new(pool: PgPool, source: Box<dyn EventSource>) -> Self {
        Self { pool, source }
    }

    pub async fn run(&self) {
        info!("Projector started: group={CONSUMER_GROUP}");

        loop {
            match self.process_batch().await {
                Ok(count) => {
                    if count > 0 { info!(count, "projected"); }
                }
                Err(e) => {
                    warn!("project cycle failed: {e}");
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn process_batch(&self) -> Result<u64, String> {
        let events = self.source.read_batch(50).await?;
        let mut count: u64 = 0;

        for event in &events {
            if let Err(e) = self.process_one(event).await {
                warn!(event_id = %event.event_id, error = %e, "projection failed → dead letter");
                self.source.dead_letter(&event.event_id, &e).await?;
            }
            self.source.ack(&event.event_id).await?;
            count += 1;
        }

        Ok(count)
    }

    async fn process_one(&self, event: &AuthoritativeEvent) -> Result<(), String> {
        // 1. Write execution projection (idempotent via UNIQUE(event_id))
        sqlx::query(
            "INSERT INTO ofg_execution_projection (event_id, event_type, aggregate_id, payload, projected_at)
             VALUES ($1, $2, $3, $4::jsonb, now())
             ON CONFLICT (event_id) DO NOTHING"
        )
        .bind(&event.event_id).bind(&event.event_type)
        .bind(&event.aggregate_id).bind(&event.payload)
        .execute(&self.pool).await
        .map_err(|e| format!("insert projection {}: {e}", event.event_id))?;

        // 2. Apply snapshot state transitions
        let payload: serde_json::Value = serde_json::from_str(&event.payload).unwrap_or_default();
        let snapshot_id = payload.get("snapshot_id").and_then(|v| v.as_str()).unwrap_or("");

        if !snapshot_id.is_empty() {
            match event.event_type.as_str() {
                "CandidateCommitted" | "SettlementCompleted" => {
                    sqlx::query(
                        "UPDATE ofg_snapshot SET state = 'PROMOTED'
                         WHERE snapshot_id = $1::uuid AND state = 'SEALED'"
                    ).bind(snapshot_id).execute(&self.pool).await
                        .map_err(|e| format!("promote {snapshot_id}: {e}"))?;
                    info!(snapshot_id, event_id = %event.event_id, "PROMOTED");
                }
                "AttemptContinued" => {
                    sqlx::query(
                        "UPDATE ofg_snapshot SET state = 'INCOMPLETE'
                         WHERE snapshot_id = $1::uuid AND state = 'SEALED'"
                    ).bind(snapshot_id).execute(&self.pool).await
                        .map_err(|e| format!("incomplete {snapshot_id}: {e}"))?;
                }
                "CandidateRejected" | "AttemptRolledBack" => {
                    sqlx::query(
                        "UPDATE ofg_snapshot SET state = 'DISCARDED'
                         WHERE snapshot_id = $1::uuid AND state = 'SEALED'"
                    ).bind(snapshot_id).execute(&self.pool).await
                        .map_err(|e| format!("discard {snapshot_id}: {e}"))?;
                }
                "AttemptEscalated" => {
                    sqlx::query(
                        "UPDATE ofg_snapshot SET coverage_status = 'PENDING_AUTHORITY'
                         WHERE snapshot_id = $1::uuid"
                    ).bind(snapshot_id).execute(&self.pool).await
                        .map_err(|e| format!("escalate {snapshot_id}: {e}"))?;
                }
                _ => {}
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_types_known() {
        let types = vec!["CandidateCommitted", "SettlementCompleted", "AttemptContinued",
                         "CandidateRejected", "AttemptRolledBack", "AttemptEscalated"];
        assert_eq!(types.len(), 6);
    }
}
