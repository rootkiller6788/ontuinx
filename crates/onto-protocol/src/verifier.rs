//! Verifier — the unified Verifier trait.
//!
//! All Conformance Verifiers implement this trait. External verifiers declare
//! requirements that Runtime executes in the Sandbox; internal verifiers
//! (gRPC, filesystem checks) return no requirements and do their own work.

use async_trait::async_trait;
use crate::check::ExternalCheckRequirement;
use crate::sandbox::RawCheckResult;
use crate::context::VerificationContext;
use crate::finding::Finding;

// ── Pass ──

/// The 9 Conformance Passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pass {
    Format,
    Static,
    Build,
    Behavior,
    DependencySafety,
    FileIntegrity,
    GraphIntegrity,
    GraphRisk,
    SemanticRules,
}

impl Pass {
    pub fn all() -> Vec<Pass> {
        vec![
            Self::Format, Self::Static, Self::Build, Self::Behavior,
            Self::DependencySafety, Self::FileIntegrity, Self::GraphIntegrity,
            Self::GraphRisk, Self::SemanticRules,
        ]
    }
}

/// Which phase a Verifier runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationStage {
    PreGraph,
    PostGraph,
    SandboxEvidence,
    PostSandbox,
}

/// Whether a Verifier needs external tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationMode {
    Internal,
    ExternalEvidence,
}

/// Static metadata for a Verifier.
#[derive(Debug, Clone)]
pub struct VerifierDescriptor {
    pub verifier_id: String,
    pub pass: Pass,
    pub stage: VerificationStage,
    pub mode: VerificationMode,
    pub supported_rules: Vec<String>,
}

// ── Services passed to Verifier::evaluate ──

/// Services available to Verifiers during evaluation.
/// Does NOT include a verification_executor — Verifiers don't start Sandboxes.
/// Does NOT include an evidence_store — the Pipeline persists atomically.
pub struct VerifierServices<'a> {
    pub artifact_reader: &'a dyn SealedArtifactReader,
    pub graph_reader: &'a dyn CandidateGraphReader,
    pub semantic_runtime: &'a dyn SemanticRuntimePort,
}

/// Read a sealed Candidate's file contents.
pub trait SealedArtifactReader: Send + Sync {
    fn read_manifest(&self, artifact_ref: &str) -> Result<crate::candidate::ArtifactManifest, String>;
    fn read_file(&self, artifact_ref: &str, path: &str) -> Result<Vec<u8>, String>;
}

/// Read Candidate Graph data (for GraphIntegrity / GraphRisk).
pub trait CandidateGraphReader: Send + Sync {
    fn check_integrity(&self, snapshot_id: &str) -> Result<(), String>;
    fn get_entity_keys(&self, snapshot_id: &str) -> Result<Vec<String>, String>;
}

/// Port for LLM-based semantic review.
#[async_trait]
pub trait SemanticRuntimePort: Send + Sync {
    async fn review(&self, code: &str, rules: &str) -> Result<String, String>;
}

// ── VerifierResult ──

/// What one Verifier run produces.
#[derive(Debug, Clone)]
pub struct VerifierResult {
    pub verifier_id: String,
    pub pass: Pass,
    pub status: VerifierStatus,
    pub findings: Vec<Finding>,
    pub raw_evidence: Vec<String>,
    pub diagnostic: Option<VerifierDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifierStatus {
    Completed,
    PartiallyCompleted,
    PrerequisiteFailed,
    Unavailable,
    TimedOut,
    NotApplicable,
}

#[derive(Debug, Clone)]
pub struct VerifierDiagnostic {
    pub message: String,
    pub detail: Option<String>,
}

impl VerifierDiagnostic {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), detail: None }
    }

    pub fn with_detail(message: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { message: message.into(), detail: Some(detail.into()) }
    }

    pub fn missing_evidence(check_id: impl Into<String>) -> Self {
        Self::with_detail("missing evidence", format!("check_id={}", check_id.into()))
    }
}

// ── The unified Verifier trait ──

#[async_trait]
pub trait Verifier: Send + Sync {
    /// Static metadata.
    fn descriptor(&self) -> &VerifierDescriptor;

    /// External verifiers return 1+ requirements; internal verifiers return [].
    fn external_requirements(&self, ctx: &VerificationContext) -> Vec<ExternalCheckRequirement>;

    /// After the Pipeline has all evidence, evaluate and produce Findings.
    async fn evaluate(
        &self,
        ctx: &VerificationContext,
        evidence: &[(&String, &RawCheckResult)],
        services: &VerifierServices<'_>,
    ) -> VerifierResult;
}
