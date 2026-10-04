//! RuntimeObservation types — raw execution facts.
//!
//! Phase 4: OntoRuntime records what happened; OntoAssure judges what
//! those facts prove.
//!
//! Strict separation:
//!   RuntimeObservation = raw execution facts
//!   EvidenceRecord     = proof relationship (fact → criterion)
//!   Verdict            = criterion satisfied or not

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::InvocationId;

// ══════════════════════════════════════════════════════════════════
// RuntimeObservation
// ══════════════════════════════════════════════════════════════════

/// Raw facts about a single capability invocation.
/// OntoRuntime produces this. OntoAssure reads it to build Evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeObservation {
    /// Which invocation this observation describes.
    pub invocation_id: InvocationId,

    /// Reference to the execution outcome.
    pub outcome_ref: OutcomeRef,

    /// Optional reference to filesystem effects.
    pub filesystem_effect_ref: Option<EffectRef>,

    /// Optional reference to network receipts.
    pub network_receipt_ref: Option<ReceiptRef>,

    /// Captured stdout, if any.
    pub stdout_blob: Option<BlobRef>,

    /// Captured stderr, if any.
    pub stderr_blob: Option<BlobRef>,

    /// Process exit code, if applicable.
    pub exit_code: Option<i32>,

    /// Resource consumption during execution.
    pub resource_usage: ResourceUsage,

    /// Structured runtime error, if execution failed.
    pub runtime_error: Option<RuntimeErrorKind>,

    /// When the observation was recorded.
    pub observed_at: DateTime<Utc>,
}

impl RuntimeObservation {
    pub fn new(invocation_id: InvocationId, outcome: OutcomeRef) -> Self {
        Self {
            invocation_id,
            outcome_ref: outcome,
            filesystem_effect_ref: None,
            network_receipt_ref: None,
            stdout_blob: None,
            stderr_blob: None,
            exit_code: None,
            resource_usage: ResourceUsage::default(),
            runtime_error: None,
            observed_at: Utc::now(),
        }
    }

    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    pub fn with_stdout(mut self, blob: BlobRef) -> Self {
        self.stdout_blob = Some(blob);
        self
    }

    pub fn with_error(mut self, err: RuntimeErrorKind) -> Self {
        self.runtime_error = Some(err);
        self
    }

    /// Did the execution succeed at the runtime level?
    /// (This is NOT the same as task success — that requires OntoAssure.)
    pub fn execution_ok(&self) -> bool {
        self.exit_code.map_or(true, |c| c == 0) && self.runtime_error.is_none()
    }
}

// ══════════════════════════════════════════════════════════════════
// Reference types
// ══════════════════════════════════════════════════════════════════

/// Reference to a capability execution outcome (stored in OntoRuntime EventStore).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutcomeRef {
    pub outcome_id: String,
    pub outcome_type: OutcomeType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeType {
    Completed,
    Failed,
    TimedOut,
    Cancelled,
    Blocked,
}

/// Reference to a filesystem or network side-effect artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectRef {
    pub effect_id: String,
    pub content_hash: Option<String>,
}

/// Reference to an external system receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptRef {
    pub receipt_id: String,
    pub external_system: String,
    pub status_code: Option<String>,
}

/// Reference to a binary blob (stdout, stderr, artifact).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobRef {
    pub blob_id: String,
    pub size_bytes: u64,
    pub content_hash: String,
}

// ══════════════════════════════════════════════════════════════════
// Resource usage
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub wall_time_ms: u64,
    pub cpu_time_ms: Option<u64>,
    pub memory_peak_bytes: Option<u64>,
    pub disk_read_bytes: Option<u64>,
    pub disk_write_bytes: Option<u64>,
    pub network_rx_bytes: Option<u64>,
    pub network_tx_bytes: Option<u64>,
}

// ══════════════════════════════════════════════════════════════════
// Runtime error classification
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorKind {
    Timeout,
    OutOfMemory,
    DiskFull,
    NetworkError,
    PermissionDenied,
    SandboxViolation,
    ProcessCrashed,
    Unknown(String),
}

impl std::fmt::Display for RuntimeErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "timeout"),
            Self::OutOfMemory => write!(f, "out_of_memory"),
            Self::DiskFull => write!(f, "disk_full"),
            Self::NetworkError => write!(f, "network_error"),
            Self::PermissionDenied => write!(f, "permission_denied"),
            Self::SandboxViolation => write!(f, "sandbox_violation"),
            Self::ProcessCrashed => write!(f, "process_crashed"),
            Self::Unknown(s) => write!(f, "unknown: {}", s),
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::InvocationId;

    #[test]
    fn execution_ok_with_exit_zero() {
        let obs = RuntimeObservation::new(
            InvocationId::new(),
            OutcomeRef { outcome_id: "o1".into(), outcome_type: OutcomeType::Completed },
        ).with_exit_code(0);
        assert!(obs.execution_ok());
    }

    #[test]
    fn execution_not_ok_with_nonzero_exit() {
        let obs = RuntimeObservation::new(
            InvocationId::new(),
            OutcomeRef { outcome_id: "o2".into(), outcome_type: OutcomeType::Failed },
        ).with_exit_code(1);
        assert!(!obs.execution_ok());
    }

    #[test]
    fn execution_not_ok_with_error() {
        let obs = RuntimeObservation::new(
            InvocationId::new(),
            OutcomeRef { outcome_id: "o3".into(), outcome_type: OutcomeType::Failed },
        ).with_error(RuntimeErrorKind::Timeout);
        assert!(!obs.execution_ok());
    }

    #[test]
    fn observation_does_not_judge_success() {
        // Key invariant: RuntimeObservation tells us what happened,
        // NOT whether the task succeeded. That's OntoAssure's job.
        let obs = RuntimeObservation::new(
            InvocationId::new(),
            OutcomeRef { outcome_id: "o4".into(), outcome_type: OutcomeType::Completed },
        ).with_exit_code(0);

        assert!(obs.execution_ok(), "execution OK");
        // But there's no `is_successful()` method on RuntimeObservation.
        // That would be the Verifier's job.
    }
}
