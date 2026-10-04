//! Idempotency guard for OntoLoop executions.
//!
//! OntoFlow may re-deliver the same ActivityTask after Worker crashes.
//! The idempotency store ensures:
//!
//! - Same loop_id + generation → return existing terminal envelope
//! - Different generation for same flow+work_item → ProtocolFailed
//! - New loop_id → create fresh execution
//!
//! Mapping: flow_id + work_item_id + generation → unique loop_id

use std::collections::HashMap;
use std::sync::Mutex;

use crate::protocol::LoopTerminalEnvelope;

/// Errors from idempotency checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdempotencyError {
    /// An existing loop has a different generation or contract — protocol violation.
    Conflict {
        existing_loop_id: String,
        existing_generation: u64,
        detail: String,
    },
    /// The loop exists and is already terminal.
    AlreadyTerminal,
    /// The loop exists and is still running.
    AlreadyRunning,
}

/// Persistence for idempotency records.
///
/// In production this would be backed by PostgreSQL or Redis.
/// F0 uses an in-memory implementation.
pub trait IdempotencyStore: Send + Sync {
    /// Check whether this invocation has been seen before.
    /// Returns the stored envelope if the loop already reached a terminal state.
    fn check(
        &self,
        flow_id: &str,
        work_item_id: &str,
        generation: u64,
        loop_id: &str,
    ) -> Result<IdempotencyResult, IdempotencyError>;

    /// Record a new loop as running.
    fn record_running(&self, flow_id: &str, work_item_id: &str, generation: u64, loop_id: &str);

    /// Record that a loop has reached a terminal state.
    fn record_terminal(&self, loop_id: &str, envelope: LoopTerminalEnvelope);

    /// F3: Atomically check-and-record completion.
    ///
    /// Called at the END of execution, before returning to OntoFlow.
    /// If a terminal state was already recorded (previous completion whose
    /// response was lost), return the cached envelope for re-Respond.
    ///
    /// Returns `Some(envelope)` if already completed (idempotent re-Respond).
    /// Returns `None` if this is the first completion (proceed to record).
    fn try_complete(&self, loop_id: &str) -> Option<LoopTerminalEnvelope>;
}

/// Result of an idempotency check.
#[derive(Debug, Clone)]
pub enum IdempotencyResult {
    /// No prior record — create a new loop.
    New,
    /// Loop exists and is running — resume.
    Running,
    /// Loop reached terminal state — return cached envelope.
    Terminal(LoopTerminalEnvelope),
}

/// In-memory idempotency store for F0.
pub struct InMemoryIdempotencyStore {
    /// Identity key → (generation, loop_id, state)
    records: Mutex<HashMap<String, IdempotencyRecord>>,
    /// loop_id → terminal envelope
    terminals: Mutex<HashMap<String, LoopTerminalEnvelope>>,
}

#[derive(Debug, Clone)]
struct IdempotencyRecord {
    generation: u64,
    loop_id: String,
    is_running: bool,
}

impl InMemoryIdempotencyStore {
    pub fn new() -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            terminals: Mutex::new(HashMap::new()),
        }
    }

    fn identity_key(flow_id: &str, work_item_id: &str) -> String {
        format!("{}:{}", flow_id, work_item_id)
    }
}

impl IdempotencyStore for InMemoryIdempotencyStore {
    fn check(
        &self,
        flow_id: &str,
        work_item_id: &str,
        generation: u64,
        loop_id: &str,
    ) -> Result<IdempotencyResult, IdempotencyError> {
        let key = Self::identity_key(flow_id, work_item_id);

        // Check if already terminal
        if let Some(envelope) = self.terminals.lock().unwrap().get(loop_id) {
            return Ok(IdempotencyResult::Terminal(envelope.clone()));
        }

        // Check existing record
        if let Some(record) = self.records.lock().unwrap().get(&key) {
            if record.loop_id != loop_id {
                return Err(IdempotencyError::Conflict {
                    existing_loop_id: record.loop_id.clone(),
                    existing_generation: record.generation,
                    detail: format!(
                        "flow={} work_item={}: existing loop_id={}, requested loop_id={}",
                        flow_id, work_item_id, record.loop_id, loop_id
                    ),
                });
            }
            if record.generation != generation {
                return Err(IdempotencyError::Conflict {
                    existing_loop_id: record.loop_id.clone(),
                    existing_generation: record.generation,
                    detail: format!(
                        "generation mismatch: existing={}, requested={}",
                        record.generation, generation
                    ),
                });
            }
            if record.is_running {
                return Ok(IdempotencyResult::Running);
            }
        }

        Ok(IdempotencyResult::New)
    }

    fn record_running(&self, flow_id: &str, work_item_id: &str, generation: u64, loop_id: &str) {
        let key = Self::identity_key(flow_id, work_item_id);
        self.records.lock().unwrap().insert(
            key,
            IdempotencyRecord {
                generation,
                loop_id: loop_id.to_string(),
                is_running: true,
            },
        );
    }

    fn record_terminal(&self, loop_id: &str, envelope: LoopTerminalEnvelope) {
        // Mark running record as done
        for (_key, record) in self.records.lock().unwrap().iter_mut() {
            if record.loop_id == loop_id {
                record.is_running = false;
                break;
            }
        }
        // Store terminal envelope
        self.terminals
            .lock()
            .unwrap()
            .insert(loop_id.to_string(), envelope);
    }

    fn try_complete(&self, loop_id: &str) -> Option<LoopTerminalEnvelope> {
        self.terminals.lock().unwrap().get(loop_id).cloned()
    }
}

impl Default for InMemoryIdempotencyStore {
    fn default() -> Self {
        Self::new()
    }
}
