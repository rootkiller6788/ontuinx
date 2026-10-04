//! Sandbox types — Validation Sandbox request, execution, and results.

use crate::check::ExternalCheckRequirement;
use crate::digest::Digest;
use std::time::Duration;

// ── SandboxValidationRequest ──

/// Request to execute all external checks in a single gVisor Sandbox.
/// L1 declares what evidence it needs; L2 resolves to actual commands.
#[derive(Debug, Clone)]
pub struct SandboxValidationRequest {
    pub request_id: String,
    pub attempt_id: String,
    pub candidate_id: String,
    pub candidate_digest: Digest,
    pub sealed_candidate_ref: String,
    pub plan_digest: Digest,
    pub gvisor_profile_id: String,
    pub expected_profile_digest: Digest,
    pub checks: Vec<ExternalCheckRequirement>,
    pub filesystem_policy: FilesystemPolicy,
    pub process_policy: ProcessPolicy,
    pub network_policy: NetworkPolicy,
    pub environment_policy: EnvironmentPolicy,
    pub allowed_outputs: Vec<String>,
    pub required_observations: Vec<ObservationKind>,
    pub sandbox_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct FilesystemPolicy {
    pub source_read_only: bool,
    pub build_dir_writable: bool,
    pub deny_host_paths: bool,
}

impl Default for FilesystemPolicy {
    fn default() -> Self {
        Self { source_read_only: true, build_dir_writable: true, deny_host_paths: true }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessPolicy {
    pub max_forks: u32,
    pub deny_ptrace: bool,
}

impl Default for ProcessPolicy {
    fn default() -> Self {
        Self { max_forks: 100, deny_ptrace: true }
    }
}

#[derive(Debug, Clone)]
pub struct NetworkPolicy {
    pub allow_outbound: bool,
    pub allowed_hosts: Vec<String>,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self { allow_outbound: false, allowed_hosts: vec![] }
    }
}

#[derive(Debug, Clone)]
pub struct EnvironmentPolicy {
    pub deny_secrets: bool,
    pub deny_host_env: bool,
}

impl Default for EnvironmentPolicy {
    fn default() -> Self {
        Self { deny_secrets: true, deny_host_env: true }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObservationKind {
    FilesystemEffects,
    ProcessTree,
    NetworkAttempts,
    ResourceUsage,
    ProducedArtifacts,
    ExitStatus,
}

// ── Sandbox Execution Result ──

#[derive(Debug, Clone)]
pub struct SandboxRunResult {
    pub request_id: String,
    pub request_digest: Digest,
    pub attempt_id: String,
    pub candidate_id: String,
    pub candidate_digest: Digest,
    pub status: SandboxExecutionStatus,
    pub check_results: Vec<RawCheckResult>,
    pub observations: Vec<RuntimeObservation>,
    pub environment: SandboxEnvironmentIdentity,
    pub filesystem_diff: ArtifactDelta,
    pub resource_usage: ResourceUsage,
}

/// Pure execution status — Runtime reports facts, Assure judges compliance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxExecutionStatus {
    Completed,
    PartiallyCompleted,
    TimedOut,
    EvidenceIncomplete,
}

// ── Per-Check Raw Result ──

#[derive(Debug, Clone)]
pub struct RawCheckResult {
    pub requirement_id: String,
    pub check_id: String,
    pub status: CheckExecutionStatus,
    pub stdout_ref: Option<String>,
    pub stderr_ref: Option<String>,
    pub produced_artifacts: Vec<String>,
    pub duration_ms: u64,
}

/// Status of a single check execution — no compliance judgment.
#[derive(Debug, Clone)]
pub enum CheckExecutionStatus {
    Exited { exit_code: i32 },
    ToolNotFound,
    SpawnFailed { message: String },
    TimedOut,
    Killed,
    PrerequisiteFailed { prerequisite_check_id: String },
}

// ── Sandbox-level errors ──

#[derive(Debug, Clone)]
pub enum SandboxInvocationError {
    StartupFailed {
        request_id: String,
        request_digest: Digest,
        diagnostic_ref: String,
    },
    StartupTimeout {
        request_id: String,
        request_digest: Digest,
        diagnostic_ref: String,
    },
    PermissionDenied {
        request_id: String,
        request_digest: Digest,
        diagnostic_ref: String,
    },
    RuntimeLost {
        request_id: String,
        request_digest: Digest,
        partial_evidence_refs: Vec<String>,
    },
}

// ── Environment Identity ──

#[derive(Debug, Clone)]
pub struct SandboxEnvironmentIdentity {
    pub runsc_version: String,
    pub profile_digest: Digest,
    pub image_digest: Digest,
    pub toolchain_digest: Digest,
}

// ── Runtime Observations ──

#[derive(Debug, Clone)]
pub struct RuntimeObservation {
    pub kind: ObservationKind,
    pub data: String,
}

// ── Artifact Delta ──

#[derive(Debug, Clone)]
pub struct ArtifactDelta {
    pub created: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
}

// ── Resource Usage ──

#[derive(Debug, Clone)]
pub struct ResourceUsage {
    pub cpu_seconds: f64,
    pub memory_mb: f64,
    pub disk_mb: f64,
    pub network_bytes_sent: u64,
    pub network_bytes_recv: u64,
}
