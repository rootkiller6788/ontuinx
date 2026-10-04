//! Onto Pack SDK — Verifier traits and artifact adapters.
//!
//! Industry packs (Code, Ops, Data, Workflow, Robotics, Industrial)
//! implement the `PackVerifier` trait to provide domain-specific
//! verification logic.  The Onto kernel calls these verifiers through
//! the `VerifierPort` trait.
//!
//! ## Pack Structure
//!
//! ```text
//! packs/
//! ├── code/         → onto-code-pack
//! ├── ops/          → onto-ops-pack (future)
//! ├── data/         → onto-data-pack (future)
//! ├── workflow/     → onto-workflow-pack (future)
//! ├── robotics/     → onto-robotics-pack (future)
//! └── industrial/   → onto-industrial-pack (future)
//! ```

pub mod packs;

use onto_assurance_types::ids::TransactionId;

// ══════════════════════════════════════════════════════════════════
// PackVerifier trait
// ══════════════════════════════════════════════════════════════════

/// Implemented by every industry pack.
///
/// Each pack verifier receives a staged environment and produces a
/// structured `PackVerificationReport` that the Onto kernel reduces
/// into evidence and a verdict.
#[async_trait::async_trait]
pub trait PackVerifier: Send + Sync {
    /// Unique identifier for this verifier.
    fn verifier_id(&self) -> &str;

    /// Version string for invalidation tracking.
    fn version(&self) -> &str;

    /// Toolchain identifier, if applicable (e.g. "gcc-14", "rustc-1.80").
    fn toolchain(&self) -> Option<&str>;

    /// Run verification against a staged environment.
    async fn verify(
        &self,
        transaction_id: TransactionId,
        workspace_path: &str,
        criteria: &[onto_assurance_types::contract::AcceptanceCriterion],
    ) -> Result<PackVerificationReport, PackVerifierError>;
}

// ══════════════════════════════════════════════════════════════════
// PackVerificationReport
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PackVerificationReport {
    pub transaction_id: TransactionId,
    pub verifier_id: String,
    pub verifier_version: String,
    pub toolchain: Option<String>,
    pub passed: bool,
    pub total_checks: u32,
    pub passed_checks: u32,
    pub failed_checks: u32,
    pub skipped_checks: u32,
    pub per_criterion: Vec<CriterionCheckResult>,
    pub artifacts: Vec<ArtifactDescriptor>,
    pub raw_output: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CriterionCheckResult {
    pub criterion_id: String,
    pub criterion_name: String,
    pub satisfied: bool,
    pub detail: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArtifactDescriptor {
    pub path: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub kind: ArtifactKind,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    SourceFile,
    Binary,
    TestReport,
    BuildLog,
    Diff,
    Custom(String),
}

// ══════════════════════════════════════════════════════════════════
// PackVerifierError
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, thiserror::Error)]
pub enum PackVerifierError {
    #[error("verifier not found: {0}")]
    NotFound(String),
    #[error("verification execution failed: {0}")]
    ExecutionFailed(String),
    #[error("environment setup failed: {0}")]
    EnvironmentError(String),
    #[error("timeout after {0}s")]
    Timeout(u64),
}
