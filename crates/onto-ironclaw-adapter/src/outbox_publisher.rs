//! Outbox Publisher — P8
//!
//! Background task that polls `onto_outbox` for unpublished events,
//! publishes them to Redis Streams `ontoos.authoritative-events.v1`,
//! and marks them as published.
//!
//! Architecture:
//!   onto_outbox (PG) → OutboxPublisher → Redis Streams → Projector

use redis::AsyncCommands;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_postgres::Client;
// Note: this crate does not use the tracing crate; fall back to eprintln for diagnostics.

const STREAM_KEY: &str = "ontoos.authoritative-events.v1";
const POLL_INTERVAL_MS: u64 = 1000;
const MAX_BATCH_SIZE: usize = 100;

/// Publishes events from PostgreSQL outbox to Redis Streams.
pub struct OutboxPublisher {
    pg_client: Arc<Client>,
    redis_client: redis::aio::MultiplexedConnection,
    last_published_count: Arc<Mutex<u64>>,
    dead_letter_key: String,
}

impl OutboxPublisher {
    /// Connect to Redis and return a publisher ready to run.
    pub async fn new(
        pg_client: Arc<Client>,
        redis_url: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let redis_client = redis::Client::open(redis_url)?;
        let conn = redis_client.get_multiplexed_tokio_connection().await?;

        Ok(Self {
            pg_client,
            redis_client: conn,
            last_published_count: Arc::new(Mutex::new(0)),
            dead_letter_key: format!("{}:dlq", STREAM_KEY),
        })
    }

    /// Run the publisher loop. Blocks until cancelled.
    pub async fn run(&self) {
        eprintln!("[OutboxPublisher] started: stream={STREAM_KEY}");

        loop {
            match self.poll_and_publish().await {
                Ok(count) => {
                    if count > 0 {
                        *self.last_published_count.lock().await = count;
                        eprintln!("[OutboxPublisher] published {count} events");
                    }
                }
                Err(e) => {
                    eprintln!("[OutboxPublisher] publish cycle failed: {e}");
                }
            }
            tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
        }
    }

    /// Single poll-and-publish cycle. Returns number of events published.
    async fn poll_and_publish(&self) -> Result<u64, Box<dyn std::error::Error>> {
        // 1. Read unpublished events
        let rows = self.pg_client.query(
            "SELECT event_id, transaction_id, aggregate_id, event_type, payload::text
             FROM onto_outbox
             WHERE published_at IS NULL
             ORDER BY created_at
             LIMIT $1",
            &[&(MAX_BATCH_SIZE as i64)],
        ).await.map_err(|e| format!("outbox poll: {e}"))?;

        let mut count: u64 = 0;
        let mut rconn = self.redis_client.clone();

        for row in &rows {
            let event_id: &str = row.get(0);
            let transaction_id: &str = row.get(1);
            let aggregate_id: &str = row.get(2);
            let event_type: &str = row.get(3);
            let payload: &str = row.get(4);

            // 2. Build the stream message
            let fields: Vec<(&str, &str)> = vec![
                ("event_id", event_id),
                ("transaction_id", transaction_id),
                ("aggregate_id", aggregate_id),
                ("event_type", event_type),
                ("payload", payload),
            ];

            // 3. Publish to Redis Streams
            let stream_id: String = rconn
                .xadd(STREAM_KEY, "*", &fields)
                .await
                .map_err(|e| {
                    eprintln!("[OutboxPublisher] XADD failed for {event_id}: {e}");
                    format!("XADD failed for {event_id}: {e}")
                })?;

            // 4. Mark as published in PG
            self.pg_client.execute(
                "UPDATE onto_outbox SET published_at = now(), redis_projection_version = $1
                 WHERE event_id = $2",
                &[&stream_id, &event_id],
            ).await.map_err(|e| format!("mark published {event_id}: {e}"))?;

            count += 1;
        }

        Ok(count)
    }

    /// Publish a single event directly (for sync-within-transaction use).
    pub async fn publish_one(
        &self,
        event_id: &str,
        transaction_id: &str,
        aggregate_id: &str,
        event_type: &str,
        payload: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let fields: Vec<(&str, &str)> = vec![
            ("event_id", event_id),
            ("transaction_id", transaction_id),
            ("aggregate_id", aggregate_id),
            ("event_type", event_type),
            ("payload", payload),
        ];
        let mut rc = self.redis_client.clone();
        let id: String = rc.xadd(STREAM_KEY, "*", &fields).await?;
        Ok(id)
    }

    /// Send an event to the dead letter queue.
    pub async fn send_to_dead_letter(
        &self,
        event_id: &str,
        error: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ts = chrono::Utc::now().to_rfc3339();
        let fields: Vec<(&str, &str)> = vec![
            ("original_event_id", event_id),
            ("error", error),
            ("timestamp", &ts),
        ];
        let mut rc = self.redis_client.clone();
        let _: String = rc.xadd(&self.dead_letter_key, "*", &fields).await?;
        Ok(())
    }

    /// Return the count of events published in the last cycle.
    pub async fn last_count(&self) -> u64 {
        *self.last_published_count.lock().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_key_format() {
        assert_eq!(STREAM_KEY, "ontoos.authoritative-events.v1");
    }
}
