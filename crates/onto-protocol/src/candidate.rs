//! Candidate types — sealed artifact identity.
//!
//! A Candidate is what the Agent produces in the Staging Workspace.
//! After sealing, it becomes immutable and all verification reads
//! through the SealedCandidateRef.

use crate::digest::Digest;

/// Immutable reference to a sealed Candidate.
///
/// Seal = Stop Agent writes → Normalize Manifest → Compute Digest
/// → Store in immutable Artifact Store → forbid Staging reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedCandidateRef {
    pub candidate_id: String,
    pub digest: Digest,
    pub manifest_digest: Digest,
    pub artifact_ref: String,
}

impl SealedCandidateRef {
    pub fn new(
        candidate_id: impl Into<String>,
        digest: Digest,
        manifest_digest: Digest,
        artifact_ref: impl Into<String>,
    ) -> Self {
        Self {
            candidate_id: candidate_id.into(),
            digest,
            manifest_digest,
            artifact_ref: artifact_ref.into(),
        }
    }
}

/// Outcome of one Agent Run — may or may not produce a Candidate.
///
/// If no Candidate was produced (Agent crashed / timeout / cancelled),
/// the Attempt can be closed by Loop but MUST NOT be Finalized.
#[derive(Debug, Clone)]
pub enum AgentRunOutcome {
    CandidateProduced {
        completion: RunCompletion,
        candidate: SealedCandidateRef,
    },
    Failed {
        reason: String,
        diagnostic_ref: String,
    },
    Cancelled,
    RuntimeLost,
}

/// What Runtime reports after Agent finishes.
#[derive(Debug, Clone)]
pub struct RunCompletion {
    pub staging_root: String,
    pub artifact_manifest: Option<ArtifactManifest>,
    pub observations: Vec<String>,
    pub duration_ms: u64,
}

/// Lightweight artifact manifest — list of changed files with hashes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactManifest {
    pub entries: Vec<ArtifactEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactEntry {
    pub path: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub is_new: bool,
    pub is_modified: bool,
}
