//! CandidateGraphPort — OntoGraph's Snapshot lifecycle interface.
//!
//! OntoGraph owns Snapshot lifecycle. Assure declares the need;
//! OntoGraph builds and seals; Assure consumes and verifies.

use async_trait::async_trait;
use crate::candidate::SealedCandidateRef;
use crate::digest::Digest;

/// OntoGraph builds a Candidate Snapshot and returns its identity.
#[async_trait]
pub trait CandidateGraphPort: Send + Sync {
    async fn build_candidate_snapshot(
        &self,
        candidate_ref: &SealedCandidateRef,
        expected_digest: &Digest,
    ) -> Result<SealedCandidateSnapshot, GraphBuildFailure>;
}

#[derive(Debug, Clone)]
pub struct SealedCandidateSnapshot {
    pub snapshot_id: String,
    pub snapshot_digest: Digest,
    pub source_candidate_digest: Digest,  // MUST == candidate.digest
}

/// OntoGraph diagnoses what went wrong — does NOT produce L1 Findings.
#[derive(Debug, Clone)]
pub enum GraphBuildFailure {
    ArtifactInvalid {
        issues: Vec<GraphBuildIssue>,
    },
    ServiceUnavailable {
        diagnostic_ref: String,
    },
    SnapshotStale {
        snapshot_id: String,
    },
    InternalFailure {
        diagnostic_ref: String,
    },
}

/// Raw diagnostic from OntoGraph — Assure's GraphIntegrityVerifier interprets as Findings.
#[derive(Debug, Clone)]
pub struct GraphBuildIssue {
    pub entity_key: Option<String>,
    pub file_path: Option<String>,
    pub message: String,
}
