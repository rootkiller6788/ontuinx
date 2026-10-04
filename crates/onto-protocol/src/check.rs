//! Check requirements and ConformancePlan.
//!
//! L1 declares what evidence it needs; L2 resolves it to actual commands.

use crate::digest::Digest;

/// L1 declares what kind of evidence it needs.
/// L2 resolves the check_id to actual tool commands via ProjectCheckRegistry.
#[derive(Debug, Clone)]
pub struct ExternalCheckRequirement {
    pub requirement_id: String,
    pub check_id: String,                       // "project.build" / "project.test"
    pub execution_dependencies: Vec<String>,     // CheckRequirementId — Test depends on Build
    pub evidence_kind: EvidenceKind,
    pub artifact_scope: ArtifactScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    BuildOutput,
    TestReport,
    LintOutput,
    FormatOutput,
    AuditReport,
    SimulationOutput,
    RawLog,
}

#[derive(Debug, Clone)]
pub struct ArtifactScope {
    pub paths: Vec<String>,
    pub include_all: bool,
}

// ── ConformancePlan ──

/// The plan of which Verifiers run, with what profile and rules.
#[derive(Debug, Clone)]
pub struct ConformancePlan {
    pub plan_id: String,
    pub plan_digest: Digest,
    pub attempt_id: String,
    pub candidate_id: String,
    pub candidate_digest: Digest,
    pub profile_id: String,
    pub profile_version: String,
    pub profile_digest: Digest,
    pub rule_set_digest: Digest,
    pub verifier_registry_digest: Digest,
    pub units: Vec<ConformanceUnit>,
}

/// One Verifier unit in the plan — with semantic dependencies,
/// not execution dependencies (those are on ExternalCheckRequirement).
#[derive(Debug, Clone)]
pub struct ConformanceUnit {
    pub unit_id: String,
    pub verifier_id: String,
    pub pass: crate::verifier::Pass,
    pub validation_dependencies: Vec<String>,  // VerificationUnitId — GraphRisk depends on GraphIntegrity
    pub applicability: Applicability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applicability {
    Required,
    Optional,
    NotApplicable,
}

/// Summary of the ConformancePlan for inclusion in the Verdict.
#[derive(Debug, Clone)]
pub struct ConformancePlanSummary {
    pub plan_id: String,
    pub profile_id: String,
    pub total_units: u32,
    pub required_units: u32,
    pub applied_units: u32,
}
