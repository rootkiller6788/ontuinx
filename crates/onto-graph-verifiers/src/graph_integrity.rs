//! GraphIntegrityVerifier — P7
//!
//! Enforced for ALL code changes.
//! Checks: snapshot exists, SEALED, checkpoint matches, generation matches,
//! all files processed, coverage complete.

use async_trait::async_trait;
use onto_protocol::context::VerificationContext;
use onto_protocol::finding::{
    Finding, FindingFingerprint, FindingSeverity, CategoryId, FindingDisposition, RemediationClass,
};
use onto_protocol::sandbox::RawCheckResult;
use onto_protocol::verifier::{
    Verifier, VerifierDescriptor, Pass, VerificationStage, VerificationMode,
    VerifierResult, VerifierStatus, VerifierServices,
};
use std::sync::Arc;
use tracing::info;

use crate::client::{GraphClientError, SnapshotQueryClient};

pub struct GraphIntegrityVerifier {
    client: Arc<dyn SnapshotQueryClient>,
    default_passes: Vec<String>,
    proto_desc: VerifierDescriptor,
}

impl GraphIntegrityVerifier {
    pub fn new(client: Arc<dyn SnapshotQueryClient>) -> Self {
        Self {
            client,
            default_passes: vec!["parse".into(), "extract".into(), "resolve".into()],
            proto_desc: VerifierDescriptor {
                verifier_id: "graph-integrity".to_string(),
                pass: Pass::GraphIntegrity,
                stage: VerificationStage::PostGraph,
                mode: VerificationMode::Internal,
                supported_rules: vec![
                    "graph_integrity.snapshot_exists".to_string(),
                    "graph_integrity.is_sealed".to_string(),
                    "graph_integrity.checkpoint_matches".to_string(),
                    "graph_integrity.generation_matches".to_string(),
                    "graph_integrity.all_files_processed".to_string(),
                    "graph_integrity.coverage_complete".to_string(),
                ],
            },
        }
    }
}

fn make_finding(rule_id: &str, msg: String, severity: FindingSeverity, cat: &str) -> Finding {
    Finding {
        finding_id: uuid::Uuid::new_v4().to_string(),
        fingerprint: FindingFingerprint {
            rule_id: rule_id.to_string(), entity_key: None,
            artifact_path: String::new(), semantic_key: msg.clone(), line_hint: None,
        },
        pass: Pass::GraphIntegrity, rule_id: rule_id.to_string(), rule_version: "1.0".to_string(),
        severity, category: CategoryId::new(cat),
        disposition: FindingDisposition::Blocking,
        remediation: RemediationClass::RetryWithFeedback,
        location: None, message: msg, fix_hint: Some("Rebuild graph snapshot".to_string()),
        confidence: 1.0, evidence_refs: vec![],
    }
}

#[async_trait]
impl Verifier for GraphIntegrityVerifier {
    fn descriptor(&self) -> &VerifierDescriptor { &self.proto_desc }

    fn external_requirements(&self, _ctx: &VerificationContext) -> Vec<onto_protocol::check::ExternalCheckRequirement> {
        vec![]
    }

    async fn evaluate(
        &self,
        ctx: &VerificationContext,
        _evidence: &[(&String, &RawCheckResult)],
        _services: &VerifierServices<'_>,
    ) -> VerifierResult {
        let graph_ctx = match ctx.graph() {
            Some(g) => g,
            None => {
                return VerifierResult {
                    verifier_id: self.proto_desc.verifier_id.clone(),
                    pass: Pass::GraphIntegrity,
                    status: VerifierStatus::PrerequisiteFailed,
                    findings: vec![], raw_evidence: vec![],
                    diagnostic: Some(onto_protocol::verifier::VerifierDiagnostic::new(
                        "GraphIntegrity requires GraphVerificationContext"
                    )),
                };
            }
        };

        let repo = &ctx.candidate().repository_name;
        match self.client.check_integrity(
            repo,
            &ctx.candidate().base_commit_sha,
            &ctx.candidate().candidate.digest.value,
            ctx.candidate().execution_generation,
            &ctx.candidate().changed_files,
            &self.default_passes,
        ).await {
            Ok(resp) => {
                let mut findings = Vec::new();
                if !resp.snapshot_exists || !resp.is_sealed
                    || !resp.checkpoint_matches || !resp.generation_matches
                {
                    findings.push(make_finding("graph_integrity.integrity",
                        format!("Snapshot {} integrity check failed", resp.snapshot_id),
                        FindingSeverity::Critical, "graph.integrity"));
                }
                if !resp.all_files_processed {
                    for f in &resp.unprocessed_files {
                        findings.push(make_finding("graph_integrity.changed_files_processed",
                            format!("File '{}' changed but not in snapshot {}", f, resp.snapshot_id),
                            FindingSeverity::Critical, "graph.integrity"));
                    }
                }
                if !resp.coverage_complete {
                    for p in &resp.missing_passes {
                        findings.push(make_finding("graph_integrity.coverage_complete",
                            format!("Required pass '{}' not complete in snapshot {}", p, resp.snapshot_id),
                            FindingSeverity::High, "graph.integrity"));
                    }
                }
                if findings.is_empty() {
                    info!(snapshot_id = %resp.snapshot_id, "GraphIntegrity: PASSED");
                }
                VerifierResult {
                    verifier_id: self.proto_desc.verifier_id.clone(),
                    pass: Pass::GraphIntegrity,
                    status: VerifierStatus::Completed,
                    findings, raw_evidence: vec![], diagnostic: None,
                }
            }
            Err(e) => VerifierResult {
                verifier_id: self.proto_desc.verifier_id.clone(),
                pass: Pass::GraphIntegrity,
                status: VerifierStatus::Unavailable,
                findings: vec![make_finding("graph_integrity.unavailable",
                    e.to_string(), FindingSeverity::Critical, "graph.integrity")],
                raw_evidence: vec![], diagnostic: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{IntegrityResult, MockSnapshotClient};
    use onto_protocol::context::{CandidateVerificationContext, GraphVerificationContext};
    use onto_protocol::candidate::SealedCandidateRef;
    use onto_protocol::digest::{Digest, DigestAlgorithm};

    fn ph() -> Digest { Digest::new(DigestAlgorithm::Sha256, "aa") }

    fn make_ctx(snapshot: bool) -> VerificationContext {
        let c = SealedCandidateRef::new("c1", ph(), ph(), "/tmp");
        let candidate = CandidateVerificationContext {
            attempt_id: "a1".to_string(), candidate: c,
            repository_name: "test-repo".to_string(), base_commit_sha: "abc123".to_string(),
            execution_generation: 1, changed_files: vec!["src/main.rs".to_string()],
            language: "rust".to_string(),
        };
        if snapshot {
            VerificationContext::WithGraph(GraphVerificationContext {
                candidate,
                snapshot_id: "snap-1".to_string(), snapshot_digest: ph(),
                changed_entity_keys: vec![],
            })
        } else {
            VerificationContext::PreGraph(candidate)
        }
    }

    fn ok_response() -> IntegrityResult {
        IntegrityResult { snapshot_exists: true, is_sealed: true, checkpoint_matches: true,
            generation_matches: true, all_files_processed: true, coverage_complete: true,
            snapshot_id: "snap-1".into(), node_count: 42, edge_count: 99, ..Default::default() }
    }

    #[tokio::test] async fn sealed_snapshot_passes() {
        let v = GraphIntegrityVerifier::new(Arc::new(MockSnapshotClient { response: ok_response(), error: None }));
        let result = v.evaluate(&make_ctx(true), &[], &dummy_services()).await;
        assert_eq!(result.status, VerifierStatus::Completed);
        assert!(result.findings.is_empty());
    }

    #[tokio::test] async fn no_graph_context_is_prerequisite_failed() {
        let v = GraphIntegrityVerifier::new(Arc::new(MockSnapshotClient { response: ok_response(), error: None }));
        let result = v.evaluate(&make_ctx(false), &[], &dummy_services()).await;
        assert_eq!(result.status, VerifierStatus::PrerequisiteFailed);
    }

    #[tokio::test] async fn not_sealed_produces_finding() {
        let mut resp = ok_response(); resp.is_sealed = false;
        let v = GraphIntegrityVerifier::new(Arc::new(MockSnapshotClient { response: resp, error: None }));
        let result = v.evaluate(&make_ctx(true), &[], &dummy_services()).await;
        assert!(!result.findings.is_empty());
    }

    #[tokio::test] async fn unprocessed_files_produce_findings() {
        let mut resp = ok_response(); resp.all_files_processed = false;
        resp.unprocessed_files = vec!["src/secret.rs".into()];
        let v = GraphIntegrityVerifier::new(Arc::new(MockSnapshotClient { response: resp, error: None }));
        let result = v.evaluate(&make_ctx(true), &[], &dummy_services()).await;
        assert!(result.findings.iter().any(|f| f.rule_id.contains("changed_files")));
    }

    fn dummy_services() -> VerifierServices<'static> {
        struct DummyReader;
        impl onto_protocol::verifier::SealedArtifactReader for DummyReader {
            fn read_manifest(&self, _: &str) -> Result<onto_protocol::candidate::ArtifactManifest, String> { Ok(onto_protocol::candidate::ArtifactManifest { entries: vec![] }) }
            fn read_file(&self, _: &str, _: &str) -> Result<Vec<u8>, String> { Ok(vec![]) }
        }
        struct DummyGraph;
        impl onto_protocol::verifier::CandidateGraphReader for DummyGraph {
            fn check_integrity(&self, _: &str) -> Result<(), String> { Ok(()) }
            fn get_entity_keys(&self, _: &str) -> Result<Vec<String>, String> { Ok(vec![]) }
        }
        struct DummySemantic;
        #[async_trait] impl onto_protocol::verifier::SemanticRuntimePort for DummySemantic {
            async fn review(&self, _: &str, _: &str) -> Result<String, String> { Ok("ok".to_string()) }
        }
        static READER: DummyReader = DummyReader;
        static GRAPH: DummyGraph = DummyGraph;
        static SEMANTIC: DummySemantic = DummySemantic;
        VerifierServices { artifact_reader: &READER, graph_reader: &GRAPH, semantic_runtime: &SEMANTIC }
    }
}
