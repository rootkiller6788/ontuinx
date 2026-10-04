//! Heartbeat types and traits for OntoLoop Worker → OntoFlow.
//!
//! Heartbeats inform liveness, cancellation propagation, timeout detection,
//! and progress display. They do NOT participate in success determination.
//!
//! F1 adds the `HeartbeatSender` trait and channel-based mock.

use async_trait::async_trait;
use std::sync::Mutex;

use crate::protocol::{LoopLifecycle, OntoLoopHeartbeat};

/// Trait for sending heartbeats to OntoFlow during long-running loops.
///
/// Go's Activity Worker calls this periodically. The heartbeat carries
/// current loop state so OntoFlow can detect crashes, propagate
/// cancellation, and display progress.
#[async_trait]
pub trait HeartbeatSender: Send + Sync {
    /// Send a heartbeat to OntoFlow.
    ///
    /// Returns `Err` if the heartbeat fails (e.g., network error,
    /// activity already cancelled). The caller should interpret
    /// errors as a signal to stop the loop.
    async fn send(&self, heartbeat: OntoLoopHeartbeat) -> Result<(), String>;
}

impl OntoLoopHeartbeat {
    /// Create a new heartbeat for an actively executing loop.
    pub fn executing(
        flow_id: String,
        work_item_id: String,
        loop_id: String,
        attempt_id: Option<String>,
        run_id: Option<String>,
    ) -> Self {
        Self {
            flow_id,
            work_item_id,
            loop_id,
            current_attempt_id: attempt_id,
            current_run_id: run_id,
            lifecycle_state: LoopLifecycle::Executing,
            progress_snapshot_hash: None,
            last_decision_id: None,
            current_checkpoint_id: None,
        }
    }

    /// Create a heartbeat for a terminal loop.
    pub fn terminal(
        flow_id: String,
        work_item_id: String,
        loop_id: String,
        decision_id: Option<String>,
    ) -> Self {
        Self {
            flow_id,
            work_item_id,
            loop_id,
            current_attempt_id: None,
            current_run_id: None,
            lifecycle_state: LoopLifecycle::Terminal,
            progress_snapshot_hash: None,
            last_decision_id: decision_id,
            current_checkpoint_id: None,
        }
    }

    /// Update with a progress snapshot hash.
    pub fn with_progress(mut self, hash: Option<String>) -> Self {
        self.progress_snapshot_hash = hash;
        self
    }

    /// Update with checkpoint info.
    pub fn with_checkpoint(mut self, checkpoint_id: Option<String>) -> Self {
        self.current_checkpoint_id = checkpoint_id;
        self
    }
}

// ══════════════════════════════════════════════════════════════════
// MockHeartbeatSender — for testing
// ══════════════════════════════════════════════════════════════════

/// Mock heartbeat sender that records sent heartbeats.
pub struct MockHeartbeatSender {
    pub sent: Mutex<Vec<OntoLoopHeartbeat>>,
    /// If true, subsequent `send()` calls return Err.
    pub fail_next: Mutex<bool>,
}

impl MockHeartbeatSender {
    pub fn new() -> Self {
        Self {
            sent: Mutex::new(vec![]),
            fail_next: Mutex::new(false),
        }
    }

    pub fn sent_count(&self) -> usize {
        self.sent.lock().unwrap().len()
    }

    /// Simulate a heartbeat failure (e.g., network error).
    pub fn set_fail_next(&self, fail: bool) {
        *self.fail_next.lock().unwrap() = fail;
    }
}

#[async_trait]
impl HeartbeatSender for MockHeartbeatSender {
    async fn send(&self, heartbeat: OntoLoopHeartbeat) -> Result<(), String> {
        if *self.fail_next.lock().unwrap() {
            return Err("mock heartbeat failure".to_string());
        }
        self.sent.lock().unwrap().push(heartbeat);
        Ok(())
    }
}
