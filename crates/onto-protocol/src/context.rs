//! VerificationContext — phased artifact metadata for Verifiers.
//!
//! Before Candidate Graph is built, only CandidateVerificationContext is available.
//! After the Graph Snapshot is sealed, GraphVerificationContext provides entity-level
//! information. Graph Verifiers that need entity data MUST check `graph()` and
//! return PrerequisiteFailed if it is None.

use crate::candidate::SealedCandidateRef;

/// Phase 1 context — before Graph Snapshot exists.
#[derive(Debug, Clone)]
pub struct CandidateVerificationContext {
    pub attempt_id: String,
    pub candidate: SealedCandidateRef,
    pub repository_name: String,
    pub base_commit_sha: String,
    pub execution_generation: u64,
    pub changed_files: Vec<String>,
    pub language: String,
}

/// Phase 4+ context — Graph Snapshot sealed, entity keys available.
#[derive(Debug, Clone)]
pub struct GraphVerificationContext {
    pub candidate: CandidateVerificationContext,
    pub snapshot_id: String,
    pub snapshot_digest: crate::digest::Digest,
    pub changed_entity_keys: Vec<String>,
}

/// Unified context enum — Verifiers use accessors, never match directly.
#[derive(Debug, Clone)]
pub enum VerificationContext {
    PreGraph(CandidateVerificationContext),
    WithGraph(GraphVerificationContext),
}

impl VerificationContext {
    /// Always available — the Candidate identity.
    pub fn candidate(&self) -> &CandidateVerificationContext {
        match self {
            Self::PreGraph(candidate) => candidate,
            Self::WithGraph(graph) => &graph.candidate,
        }
    }

    /// Only available after Graph Snapshot is sealed.
    /// Graph Verifiers MUST check this — return PrerequisiteFailed if None.
    pub fn graph(&self) -> Option<&GraphVerificationContext> {
        match self {
            Self::PreGraph(_) => None,
            Self::WithGraph(graph) => Some(graph),
        }
    }
}
