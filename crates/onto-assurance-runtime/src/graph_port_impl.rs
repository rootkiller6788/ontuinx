//! CandidateGraphPort real implementation — bridges OntoGraph gRPC client.
use async_trait::async_trait;
use onto_protocol::candidate::SealedCandidateRef;
use onto_protocol::digest::Digest;
use onto_protocol::graph_port::{
    CandidateGraphPort, SealedCandidateSnapshot, GraphBuildFailure, GraphBuildIssue,
};
use std::sync::Arc;

/// Real implementation wrapping a SnapshotQueryClient (existing gRPC client in onto-graph-verifiers).
pub struct RealCandidateGraphPort {
    inner: Arc<dyn SnapshotQuery>,
}

/// Minimal snapshot query interface (subset of SnapshotQueryClient).
#[async_trait]
pub trait SnapshotQuery: Send + Sync {
    async fn check_snapshot(&self, repo: &str, checkpoint: &str, generation: u64) -> Result<SnapshotInfo, String>;
}

pub struct SnapshotInfo {
    pub snapshot_id: String,
    pub exists: bool,
    pub is_sealed: bool,
    pub node_count: i64,
    pub edge_count: i64,
}

impl RealCandidateGraphPort {
    pub fn new(inner: Arc<dyn SnapshotQuery>) -> Self { Self { inner } }
}

#[async_trait]
impl CandidateGraphPort for RealCandidateGraphPort {
    async fn build_candidate_snapshot(
        &self, candidate_ref: &SealedCandidateRef, expected_digest: &Digest,
    ) -> Result<SealedCandidateSnapshot, GraphBuildFailure> {
        let info = self.inner.check_snapshot(
            &candidate_ref.candidate_id, // reuse candidate_id as repo for now
            &candidate_ref.digest.value,
            1,
        ).await.map_err(|e| GraphBuildFailure::ServiceUnavailable {
            diagnostic_ref: format!("SnapshotQuery failed: {}", e),
        })?;

        if !info.exists {
            return Err(GraphBuildFailure::ArtifactInvalid {
                issues: vec![GraphBuildIssue {
                    entity_key: None, file_path: None,
                    message: "No graph snapshot found for candidate".into(),
                }],
            });
        }

        if !info.is_sealed {
            return Err(GraphBuildFailure::SnapshotStale {
                snapshot_id: info.snapshot_id.clone(),
            });
        }

        let snapshot_digest = Digest::new(
            onto_protocol::digest::DigestAlgorithm::Sha256,
            format!("snap-{}", info.snapshot_id),
        );

        Ok(SealedCandidateSnapshot {
            snapshot_id: info.snapshot_id,
            snapshot_digest,
            source_candidate_digest: expected_digest.clone(),
        })
    }
}

/// Stub implementation for testing when OntoGraph is not available.
pub struct StubCandidateGraphPort;

#[async_trait]
impl CandidateGraphPort for StubCandidateGraphPort {
    async fn build_candidate_snapshot(
        &self, _: &SealedCandidateRef, expected: &Digest,
    ) -> Result<SealedCandidateSnapshot, GraphBuildFailure> {
        Ok(SealedCandidateSnapshot {
            snapshot_id: "stub-snap-1".into(),
            snapshot_digest: Digest::new(onto_protocol::digest::DigestAlgorithm::Sha256, "stub"),
            source_candidate_digest: expected.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_protocol::digest::{Digest, DigestAlgorithm};

    struct MockQuery { info: Result<SnapshotInfo, String> }
    #[async_trait] impl SnapshotQuery for MockQuery {
        async fn check_snapshot(&self, _: &str, _: &str, _: u64) -> Result<SnapshotInfo, String> {
            match &self.info { Ok(i) => Ok(SnapshotInfo { snapshot_id: i.snapshot_id.clone(), exists: i.exists, is_sealed: i.is_sealed, node_count: i.node_count, edge_count: i.edge_count }), Err(e) => Err(e.clone()) }
        }
    }

    fn ph() -> Digest { Digest::new(DigestAlgorithm::Sha256, "aa") }
    fn cr() -> SealedCandidateRef { SealedCandidateRef::new("c1", ph(), ph(), "/tmp") }

    #[tokio::test] async fn sealed_snapshot_succeeds() {
        let port = RealCandidateGraphPort::new(Arc::new(MockQuery { info: Ok(SnapshotInfo { snapshot_id: "s1".into(), exists: true, is_sealed: true, node_count: 10, edge_count: 20 }) }));
        let result = port.build_candidate_snapshot(&cr(), &ph()).await.unwrap();
        assert_eq!(result.snapshot_id, "s1");
    }

    #[tokio::test] async fn no_snapshot_is_artifact_invalid() {
        let port = RealCandidateGraphPort::new(Arc::new(MockQuery { info: Ok(SnapshotInfo { snapshot_id: "".into(), exists: false, is_sealed: false, node_count: 0, edge_count: 0 }) }));
        assert!(matches!(port.build_candidate_snapshot(&cr(), &ph()).await, Err(GraphBuildFailure::ArtifactInvalid { .. })));
    }

    #[tokio::test] async fn stub_always_succeeds() {
        let result = StubCandidateGraphPort.build_candidate_snapshot(&cr(), &ph()).await.unwrap();
        assert_eq!(result.snapshot_id, "stub-snap-1");
    }
}
