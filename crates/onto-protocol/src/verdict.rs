//! ConformanceVerdict — the authoritative diagnosis from L1.

use crate::check::{Applicability, ConformancePlanSummary, ConformanceUnit};
use crate::digest::Digest;
use crate::finding::Finding;
use crate::sandbox::SandboxExecutionStatus;
use crate::verifier::Pass;

/// The single authoritative output of one Assure Pipeline run.
#[derive(Debug, Clone)]
pub struct ConformanceVerdict {
    pub verdict_id: String,
    pub verdict_digest: Digest,
    pub attempt_id: String,
    pub candidate_id: String,
    pub candidate_digest: Digest,
    pub graph: GraphValidationBinding,
    pub plan_id: String,
    pub plan_digest: Digest,
    pub evidence_bundle_ref: String,
    pub evidence_bundle_digest: Digest,
    pub conformance: ConformanceOutcome,
    pub freshness: FreshnessState,
    pub coverage: CoverageState,
    pub sandbox: SandboxValidationSummary,
    pub blocking_findings: Vec<Finding>,
    pub advisory_findings: Vec<Finding>,
    pub unit_results: Vec<ConformanceUnitResult>,
    pub plan: ConformancePlanSummary,
}

// ── Three-dimensional Verdict ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConformanceOutcome {
    Conformant,
    NonConformant,
    Inconclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessState {
    Current,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageState {
    Complete,
    Partial,
    Unavailable,
}

// ── Graph Validation Binding — not a mandatory snapshot pair ──

#[derive(Debug, Clone)]
pub enum GraphValidationBinding {
    Sealed {
        snapshot_id: String,
        snapshot_digest: Digest,
        source_candidate_digest: Digest,
    },
    ArtifactInvalid {
        diagnostic_refs: Vec<String>,
    },
    Unavailable {
        reason: GraphUnavailableReason,
        diagnostic_ref: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphUnavailableReason {
    ServiceDown,
    BuildFailed,
    InternalError,
}

// ── Per-Unit Results (authoritative coverage proof) ──

#[derive(Debug, Clone)]
pub struct ConformanceUnitResult {
    pub unit_id: String,
    pub verifier_id: String,
    pub pass: Pass,
    pub applicability: crate::check::Applicability,
    pub status: UnitExecutionStatus,
    pub finding_ids: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub duration_ms: u64,
}

/// Status of one ConformanceUnit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitExecutionStatus {
    Passed,
    Failed,
    NotApplicable,
    PrerequisiteFailed,
    Unavailable,
    TimedOut,
    PartiallyCompleted,
    EvidenceIncomplete,
}

// ── Sandbox Validation Summary (enum with payloads) ──

#[derive(Debug, Clone)]
pub enum SandboxValidationSummary {
    Executed {
        request_digest: Digest,
        environment_digest: Digest,
        run_ref: String,
        status: SandboxExecutionStatus,
        observation_refs: Vec<String>,
    },
    NotRunDueToBlockingPrerequisite {
        blocking_finding_ids: Vec<String>,
    },
    StartupFailed {
        request_digest: Digest,
        diagnostic_ref: String,
    },
    RuntimeLost {
        request_digest: Digest,
        partial_evidence_refs: Vec<String>,
    },
}

// ── Helpers ──

impl ConformanceUnitResult {
    /// Produce a `NotApplicable` result for a unit whose verifier was not registered.
    pub fn not_applicable(unit: &ConformanceUnit) -> Self {
        ConformanceUnitResult {
            unit_id: unit.unit_id.clone(),
            verifier_id: unit.verifier_id.clone(),
            pass: unit.pass,
            applicability: unit.applicability,
            status: UnitExecutionStatus::NotApplicable,
            finding_ids: vec![],
            evidence_refs: vec![],
            duration_ms: 0,
        }
    }
}

impl ConformanceVerdict {
    /// Every Required unit must have a determinate (non-PartiallyCompleted) result.
    pub fn all_required_units_have_determinate_result(&self) -> bool {
        self.unit_results
            .iter()
            .filter(|u| u.applicability == crate::check::Applicability::Required)
            .all(|u| !matches!(u.status,
                UnitExecutionStatus::PartiallyCompleted
                | UnitExecutionStatus::EvidenceIncomplete
                | UnitExecutionStatus::Unavailable
                | UnitExecutionStatus::TimedOut
            ))
    }
}

impl SandboxValidationSummary {
    /// True iff the sandbox ran and completed, or no sandbox was needed
    /// (no external requirements and no blocking prerequisites).
    pub fn is_executed_and_completed(&self) -> bool {
        match self {
            SandboxValidationSummary::Executed {
                status: SandboxExecutionStatus::Completed, ..
            } => true,
            SandboxValidationSummary::NotRunDueToBlockingPrerequisite { blocking_finding_ids }
                if blocking_finding_ids.is_empty() => true,
            _ => false,
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// P16-P1: AssuranceFailure — authoritative L1 failure record
// ══════════════════════════════════════════════════════════════════

/// An Assurance run that could not produce a Verdict.
/// Persisted BEFORE any L3 decision is made.
#[derive(Debug, Clone)]
pub struct PersistedAssuranceFailure {
    pub failure_id: String,
    pub failure_digest: Digest,
    pub assurance_run_id: String,
    pub attempt_id: String,
    pub candidate_id: String,
    pub candidate_digest: Digest,
    pub plan_digest: Digest,
    pub stage: AssuranceFailureStage,
    pub reason_code: String,
    pub diagnostic_ref: Option<String>,
}

/// Which stage of the Assurance pipeline failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssuranceFailureStage {
    PlanResolution,
    ArtifactAccess,
    VerificationExecution,
    EvidencePersistence,
    VerdictReduction,
    Finalization,
}

/// P16-P1: Unified binding to either a Verdict or a Failure.
/// Used in LoopDirective so downstream can verify the binding digest.
#[derive(Debug, Clone)]
pub enum AssuranceBinding {
    Verdict {
        verdict_id: String,
        verdict_digest: Digest,
    },
    Unavailable {
        failure_id: String,
        failure_digest: Digest,
    },
}

impl PersistedAssuranceFailure {
    pub fn compute_digest(&self) -> Digest {
        use sha2::{Digest as _, Sha256};
        let payload = format!(
            "1|{}|{}|{}|{}|{}|{}|{:?}|{}|{}",
            self.failure_id,
            self.assurance_run_id,
            self.attempt_id,
            self.candidate_id,
            self.candidate_digest.value,
            self.plan_digest.value,
            self.stage,
            self.reason_code,
            self.diagnostic_ref.as_deref().unwrap_or(""),
        );
        Digest::new(
            crate::digest::DigestAlgorithm::Sha256,
            hex::encode(Sha256::digest(payload.as_bytes())),
        )
    }
}

// ── Reduction truth table (hard constraints) ──
//
// All Required Units pass                                     → Conformant / Complete
// Deterministic blocking Findings exist                       → NonConformant
// Required Unit tool unavailable / infrastructure failure     → Inconclusive / Partial
// Sandbox did not run AND no blocking Pre-Sandbox Findings    → Inconclusive
//
// Hard constraint: Conformant ⇒ freshness==Current ∧ coverage==Complete
//                  ∧ SandboxExecution==Completed ∧ all Required Units have a result
