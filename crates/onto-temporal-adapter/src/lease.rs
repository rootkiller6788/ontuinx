//! LoopExecutionLease — prevents dual-worker execution of the same loop.
//!
//! When OntoFlow re-delivers an ActivityTask (Worker crash, heartbeat timeout),
//! two workers might try to execute the same loop simultaneously. The lease
//! ensures at-most-one executor.
//!
//! In production this would be backed by Redis SET NX with TTL or a database
//! row-level lock. F3 uses an in-memory implementation.

/// Result of a lease acquisition attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseResult {
    /// Lease acquired — this worker is the sole executor.
    Acquired,
    /// Another worker holds the lease.
    AlreadyHeld { holder: String },
    /// Loop already reached a terminal state.
    AlreadyTerminal,
}

/// Distributed lease for loop execution.
///
/// # Contract
///
/// - `try_acquire` is atomic (CAS). Only one caller succeeds.
/// - `release` must only be called by the lease holder.
/// - Leases auto-expire after a TTL to handle holder crashes.
pub trait LoopExecutionLease: Send + Sync {
    /// Try to acquire the execution lease for a loop.
    fn try_acquire(&self, loop_id: &str, worker_id: &str) -> LeaseResult;

    /// Release the lease after execution completes.
    fn release(&self, loop_id: &str, worker_id: &str);

    /// Check if a loop already has a terminal state (post-crash re-delivery).
    fn is_terminal(&self, loop_id: &str) -> bool;
}

// ══════════════════════════════════════════════════════════════════
// In-memory implementation
// ══════════════════════════════════════════════════════════════════

use std::collections::HashMap;
use std::sync::Mutex;

pub struct InMemoryLoopLease {
    holders: Mutex<HashMap<String, String>>,
    terminals: Mutex<Vec<String>>,
}

impl InMemoryLoopLease {
    pub fn new() -> Self {
        Self {
            holders: Mutex::new(HashMap::new()),
            terminals: Mutex::new(Vec::new()),
        }
    }

    /// Mark a loop as terminal (called by loop_runner after Commit etc.)
    pub fn mark_terminal(&self, loop_id: &str) {
        self.terminals.lock().unwrap().push(loop_id.to_string());
    }
}

impl LoopExecutionLease for InMemoryLoopLease {
    fn try_acquire(&self, loop_id: &str, worker_id: &str) -> LeaseResult {
        // Check terminal first
        if self.terminals.lock().unwrap().contains(&loop_id.to_string()) {
            return LeaseResult::AlreadyTerminal;
        }

        // CAS: only insert if key doesn't exist
        let mut holders = self.holders.lock().unwrap();
        if let Some(existing) = holders.get(loop_id) {
            LeaseResult::AlreadyHeld {
                holder: existing.clone(),
            }
        } else {
            holders.insert(loop_id.to_string(), worker_id.to_string());
            LeaseResult::Acquired
        }
    }

    fn release(&self, loop_id: &str, worker_id: &str) {
        let mut holders = self.holders.lock().unwrap();
        if holders.get(loop_id).map(|s| s.as_str()) == Some(worker_id) {
            holders.remove(loop_id);
        }
    }

    fn is_terminal(&self, loop_id: &str) -> bool {
        self.terminals.lock().unwrap().contains(&loop_id.to_string())
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_single_worker_acquires() {
        let lease = InMemoryLoopLease::new();
        assert_eq!(
            lease.try_acquire("loop-1", "worker-A"),
            LeaseResult::Acquired
        );
    }

    #[test]
    fn lease_dual_workers_second_rejected() {
        let lease = InMemoryLoopLease::new();
        assert_eq!(
            lease.try_acquire("loop-1", "worker-A"),
            LeaseResult::Acquired
        );
        assert_eq!(
            lease.try_acquire("loop-1", "worker-B"),
            LeaseResult::AlreadyHeld {
                holder: "worker-A".into()
            }
        );
    }

    #[test]
    fn lease_release_allows_reacquire() {
        let lease = InMemoryLoopLease::new();
        lease.try_acquire("loop-1", "worker-A");
        lease.release("loop-1", "worker-A");
        assert_eq!(
            lease.try_acquire("loop-1", "worker-B"),
            LeaseResult::Acquired
        );
    }

    #[test]
    fn lease_wrong_worker_cannot_release() {
        let lease = InMemoryLoopLease::new();
        lease.try_acquire("loop-1", "worker-A");
        lease.release("loop-1", "worker-B"); // wrong worker
        // lease should still be held
        assert_eq!(
            lease.try_acquire("loop-1", "worker-B"),
            LeaseResult::AlreadyHeld {
                holder: "worker-A".into()
            }
        );
    }

    #[test]
    fn lease_terminal_prevents_acquire() {
        let lease = InMemoryLoopLease::new();
        lease.mark_terminal("loop-done");
        assert_eq!(
            lease.try_acquire("loop-done", "worker-A"),
            LeaseResult::AlreadyTerminal
        );
    }
}
