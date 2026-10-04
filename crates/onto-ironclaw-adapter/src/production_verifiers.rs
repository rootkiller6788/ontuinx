//! Production verifiers — the 4 mandatory core Verifiers for P16-2 baseline.
//!
//! Each verifier implements the unified `Verifier` trait from `onto-protocol`.
//! Build/Test verifiers declare `external_requirements()` and evaluate
//! sandbox evidence — they never call `Command::new()` directly.
//!
//! # Anti-tautology invariant
//!
//! Verifiers that consume sandbox evidence MUST NOT use `evidence.iter().all()`.
//! Empty evidence → `PartiallyCompleted` (not `Completed`).

use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use onto_protocol::check::{ArtifactScope, EvidenceKind, ExternalCheckRequirement};
use onto_protocol::context::VerificationContext;
use onto_protocol::digest::Digest;
use onto_protocol::finding::{
    CategoryId, Finding, FindingDisposition, FindingFingerprint, FindingSeverity,
    RemediationClass,
};
use onto_protocol::sandbox::{CheckExecutionStatus, RawCheckResult};
use onto_protocol::verifier::{
    Pass, SealedArtifactReader, VerificationMode, VerificationStage,
    Verifier, VerifierDescriptor, VerifierDiagnostic, VerifierResult,
    VerifierServices, VerifierStatus,
};

// ══════════════════════════════════════════════════════════════════
// ① artifact.manifest.integrity
// ══════════════════════════════════════════════════════════════════

/// Verifies that the Sealed Candidate's artifact manifest matches its digest.
/// Internal check: reads the manifest via `SealedArtifactReader`, re-computes
/// the digest, and compares against `SealedCandidateRef.manifest_digest`.
pub struct ArtifactManifestIntegrityVerifier {
    desc: VerifierDescriptor,
}

impl ArtifactManifestIntegrityVerifier {
    pub fn new() -> Self {
        Self {
            desc: VerifierDescriptor {
                verifier_id: "artifact.manifest.integrity".into(),
                pass: Pass::FileIntegrity,
                stage: VerificationStage::PreGraph,
                mode: VerificationMode::Internal,
                supported_rules: vec![],
            },
        }
    }

    fn compute_manifest_digest(manifest: &onto_protocol::candidate::ArtifactManifest) -> Digest {
        use sha2::{Digest as _, Sha256};
        let mut entries: Vec<String> = manifest
            .entries
            .iter()
            .map(|e| format!("{}:{}:{}", e.path, e.content_hash, e.size_bytes))
            .collect();
        entries.sort(); // canonical ordering
        let payload = entries.join("\n");
        let mut hasher = Sha256::new();
        hasher.update(payload.as_bytes());
        Digest::new(
            onto_protocol::digest::DigestAlgorithm::Sha256,
            hex::encode(hasher.finalize()),
        )
    }
}

#[async_trait]
impl Verifier for ArtifactManifestIntegrityVerifier {
    fn descriptor(&self) -> &VerifierDescriptor {
        &self.desc
    }

    fn external_requirements(&self, _ctx: &VerificationContext) -> Vec<ExternalCheckRequirement> {
        vec![] // internal — no sandbox needed
    }

    async fn evaluate(
        &self,
        ctx: &VerificationContext,
        _evidence: &[(&String, &RawCheckResult)],
        services: &VerifierServices<'_>,
    ) -> VerifierResult {
        let candidate = ctx.candidate();
        let artifact_ref = &candidate.candidate.artifact_ref;

        let manifest = match services.artifact_reader.read_manifest(artifact_ref) {
            Ok(m) => m,
            Err(e) => {
                return VerifierResult {
                    verifier_id: self.desc.verifier_id.clone(),
                    pass: self.desc.pass,
                    status: VerifierStatus::Unavailable,
                    findings: vec![],
                    raw_evidence: vec![],
                    diagnostic: Some(VerifierDiagnostic {
                        message: format!("Cannot read manifest: {}", e),
                        detail: None,
                    }),
                };
            }
        };

        let actual_digest = Self::compute_manifest_digest(&manifest);
        let expected_digest = &candidate.candidate.manifest_digest;

        if actual_digest.value != expected_digest.value
            || actual_digest.algorithm != expected_digest.algorithm
        {
            return VerifierResult {
                verifier_id: self.desc.verifier_id.clone(),
                pass: self.desc.pass,
                status: VerifierStatus::Completed,
                findings: vec![blocking_finding(
                    "artifact.manifest.integrity",
                    Pass::FileIntegrity,
                    format!(
                        "Manifest digest mismatch: expected {}/{:?}, got {}/{:?}",
                        expected_digest.value,
                        expected_digest.algorithm,
                        actual_digest.value,
                        actual_digest.algorithm,
                    ),
                )],
                raw_evidence: vec![],
                diagnostic: None,
            };
        }

        // Check that every manifest entry references a real file
        for entry in &manifest.entries {
            match services.artifact_reader.read_file(artifact_ref, &entry.path) {
                Ok(content) => {
                    let computed_hash = {
                        use sha2::{Digest as _, Sha256};
                        let mut h = Sha256::new();
                        h.update(&content);
                        hex::encode(h.finalize())
                    };
                    if computed_hash != entry.content_hash {
                        return VerifierResult {
                            verifier_id: self.desc.verifier_id.clone(),
                            pass: self.desc.pass,
                            status: VerifierStatus::Completed,
                            findings: vec![blocking_finding(
                                "artifact.manifest.integrity",
                                Pass::FileIntegrity,
                                format!(
                                    "File '{}' content hash mismatch: expected {}, got {}",
                                    entry.path, entry.content_hash, computed_hash
                                ),
                            )],
                            raw_evidence: vec![],
                            diagnostic: None,
                        };
                    }
                }
                Err(e) => {
                    return VerifierResult {
                        verifier_id: self.desc.verifier_id.clone(),
                        pass: self.desc.pass,
                        status: VerifierStatus::Completed,
                        findings: vec![blocking_finding(
                            "artifact.manifest.integrity",
                            Pass::FileIntegrity,
                            format!("File '{}' referenced in manifest not found: {}", entry.path, e),
                        )],
                        raw_evidence: vec![],
                        diagnostic: None,
                    };
                }
            }
        }

        VerifierResult {
            verifier_id: self.desc.verifier_id.clone(),
            pass: self.desc.pass,
            status: VerifierStatus::Completed,
            findings: vec![],
            raw_evidence: vec![],
            diagnostic: None,
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// ② file.protected_path
// ══════════════════════════════════════════════════════════════════

/// Verifies that no file in the changed set touches a protected path prefix.
/// Internal check: reads changed file list from VerificationContext.
pub struct ProtectedPathVerifier {
    desc: VerifierDescriptor,
    protected_prefixes: Vec<String>,
}

impl ProtectedPathVerifier {
    pub fn new(protected: Vec<String>) -> Self {
        Self {
            desc: VerifierDescriptor {
                verifier_id: "file.protected_path".into(),
                pass: Pass::FileIntegrity,
                stage: VerificationStage::PreGraph,
                mode: VerificationMode::Internal,
                supported_rules: vec![],
            },
            protected_prefixes: protected,
        }
    }
}

#[async_trait]
impl Verifier for ProtectedPathVerifier {
    fn descriptor(&self) -> &VerifierDescriptor {
        &self.desc
    }

    fn external_requirements(&self, _ctx: &VerificationContext) -> Vec<ExternalCheckRequirement> {
        vec![]
    }

    async fn evaluate(
        &self,
        ctx: &VerificationContext,
        _evidence: &[(&String, &RawCheckResult)],
        _services: &VerifierServices<'_>,
    ) -> VerifierResult {
        let changed = &ctx.candidate().changed_files;

        let violations: Vec<&String> = changed
            .iter()
            .filter(|f| {
                self.protected_prefixes
                    .iter()
                    .any(|prefix| f.starts_with(prefix))
            })
            .collect();

        if violations.is_empty() {
            return VerifierResult {
                verifier_id: self.desc.verifier_id.clone(),
                pass: self.desc.pass,
                status: VerifierStatus::Completed,
                findings: vec![],
                raw_evidence: vec![],
                diagnostic: None,
            };
        }

        let paths: Vec<String> = violations.into_iter().cloned().collect();
        VerifierResult {
            verifier_id: self.desc.verifier_id.clone(),
            pass: self.desc.pass,
            status: VerifierStatus::Completed,
            findings: vec![blocking_finding(
                "file.protected_path",
                Pass::FileIntegrity,
                format!(
                    "Protected path(s) modified: {}. These paths must not be changed by Agent capabilities.",
                    paths.join(", ")
                ),
            )],
            raw_evidence: vec![],
            diagnostic: None,
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// ③ project.build
// ══════════════════════════════════════════════════════════════════

/// Verifies that the project builds successfully.
/// External-evidence verifier: declares `external_requirements()`, receives
/// sandbox `RawCheckResult` via `evaluate()`.
///
/// # Anti-tautology
/// Does NOT use `evidence.iter().all()`.  Requires exact check_id match.
pub struct ProjectBuildVerifier {
    desc: VerifierDescriptor,
}

impl ProjectBuildVerifier {
    pub fn new() -> Self {
        Self {
            desc: VerifierDescriptor {
                verifier_id: "project.build".into(),
                pass: Pass::Build,
                stage: VerificationStage::SandboxEvidence,
                mode: VerificationMode::ExternalEvidence,
                supported_rules: vec![],
            },
        }
    }
}

#[async_trait]
impl Verifier for ProjectBuildVerifier {
    fn descriptor(&self) -> &VerifierDescriptor {
        &self.desc
    }

    fn external_requirements(&self, _ctx: &VerificationContext) -> Vec<ExternalCheckRequirement> {
        vec![ExternalCheckRequirement {
            requirement_id: "req-build".into(),
            check_id: "project.build".into(),
            execution_dependencies: vec![],
            evidence_kind: EvidenceKind::BuildOutput,
            artifact_scope: ArtifactScope {
                paths: vec![],
                include_all: true,
            },
        }]
    }

    async fn evaluate(
        &self,
        _ctx: &VerificationContext,
        evidence: &[(&String, &RawCheckResult)],
        _services: &VerifierServices<'_>,
    ) -> VerifierResult {
        evaluate_sandbox_evidence(
            &self.desc,
            "project.build",
            evidence,
        )
    }
}

// ══════════════════════════════════════════════════════════════════
// ④ project.test
// ══════════════════════════════════════════════════════════════════

/// Verifies that the project tests pass.
/// External-evidence verifier. Requires `project.build` as execution dependency.
///
/// # Anti-tautology
/// Does NOT use `evidence.iter().all()`.  Requires exact check_id match.
pub struct ProjectTestVerifier {
    desc: VerifierDescriptor,
}

impl ProjectTestVerifier {
    pub fn new() -> Self {
        Self {
            desc: VerifierDescriptor {
                verifier_id: "project.test".into(),
                pass: Pass::Behavior,
                stage: VerificationStage::SandboxEvidence,
                mode: VerificationMode::ExternalEvidence,
                supported_rules: vec![],
            },
        }
    }
}

#[async_trait]
impl Verifier for ProjectTestVerifier {
    fn descriptor(&self) -> &VerifierDescriptor {
        &self.desc
    }

    fn external_requirements(&self, _ctx: &VerificationContext) -> Vec<ExternalCheckRequirement> {
        vec![ExternalCheckRequirement {
            requirement_id: "req-test".into(),
            check_id: "project.test".into(),
            execution_dependencies: vec!["project.build".to_string()],
            evidence_kind: EvidenceKind::BuildOutput,
            artifact_scope: ArtifactScope {
                paths: vec![],
                include_all: true,
            },
        }]
    }

    async fn evaluate(
        &self,
        _ctx: &VerificationContext,
        evidence: &[(&String, &RawCheckResult)],
        _services: &VerifierServices<'_>,
    ) -> VerifierResult {
        evaluate_sandbox_evidence(
            &self.desc,
            "project.test",
            evidence,
        )
    }
}

// ══════════════════════════════════════════════════════════════════
// Helpers
// ══════════════════════════════════════════════════════════════════

/// Shared evaluation logic for sandbox-evidence verifiers.
///
/// # Anti-tautology
/// Requires an exact `check_id` match in the evidence slice.
/// Empty evidence → `PartiallyCompleted` (NOT `Completed`).
/// Missing specific check → `PartiallyCompleted` (NOT `Completed`).
fn evaluate_sandbox_evidence(
    desc: &VerifierDescriptor,
    expected_check_id: &str,
    evidence: &[(&String, &RawCheckResult)],
) -> VerifierResult {
    // Anti-tautology: require specific evidence, not [].all()
    let target = evidence
        .iter()
        .find(|(check_id, _)| check_id.as_str() == expected_check_id);

    let (status, findings) = match target {
        None => (
            VerifierStatus::PartiallyCompleted,
            vec![],
        ),
        Some((_, r)) => match &r.status {
            CheckExecutionStatus::Exited { exit_code: 0 } => {
                (VerifierStatus::Completed, vec![])
            }
            CheckExecutionStatus::Exited { exit_code } => (
                VerifierStatus::Completed,
                vec![blocking_finding(
                    &desc.verifier_id,
                    desc.pass,
                    format!(
                        "Check '{}' failed with exit code {}",
                        expected_check_id, exit_code
                    ),
                )],
            ),
            CheckExecutionStatus::ToolNotFound => (
                VerifierStatus::Unavailable,
                vec![],
            ),
            CheckExecutionStatus::TimedOut => (
                VerifierStatus::TimedOut,
                vec![],
            ),
            CheckExecutionStatus::SpawnFailed { message } => (
                VerifierStatus::Unavailable,
                vec![],
            ),
            CheckExecutionStatus::Killed => (
                VerifierStatus::Unavailable,
                vec![],
            ),
            _ => (
                VerifierStatus::PartiallyCompleted,
                vec![],
            ),
        },
    };

    VerifierResult {
        verifier_id: desc.verifier_id.clone(),
        pass: desc.pass,
        status,
        findings,
        raw_evidence: vec![],
        diagnostic: None,
    }
}

static FINDING_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_finding_id() -> String {
    format!("f-{}", FINDING_COUNTER.fetch_add(1, Ordering::Relaxed))
}

fn blocking_finding(rule_id: &str, pass: Pass, message: String) -> Finding {
    Finding {
        finding_id: next_finding_id(),
        fingerprint: FindingFingerprint {
            rule_id: rule_id.to_string(),
            entity_key: None,
            artifact_path: String::new(),
            semantic_key: rule_id.to_string(),
            line_hint: None,
        },
        pass,
        rule_id: rule_id.to_string(),
        rule_version: "1".into(),
        severity: FindingSeverity::Critical,
        category: CategoryId::new("code"),
        disposition: FindingDisposition::Blocking,
        remediation: RemediationClass::RetryWithFeedback,
        location: None,
        message,
        fix_hint: None,
        confidence: 1.0,
        evidence_refs: vec![],
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use onto_protocol::candidate::{ArtifactEntry, ArtifactManifest, SealedCandidateRef};
    use onto_protocol::context::CandidateVerificationContext;
    use onto_protocol::digest::DigestAlgorithm;
    use onto_protocol::verifier::VerifierDiagnostic;

    fn d(s: &str) -> Digest {
        use sha2::{Digest as SD, Sha256};
        Digest::new(DigestAlgorithm::Sha256, hex::encode(Sha256::digest(s.as_bytes())))
    }

    fn test_ctx() -> VerificationContext {
        let c = SealedCandidateRef::new("c1", d("c1-digest"), d("m1-digest"), "/tmp/test-candidate");
        VerificationContext::PreGraph(CandidateVerificationContext {
            attempt_id: "a1".into(),
            candidate: c,
            repository_name: "test".into(),
            base_commit_sha: "abc".into(),
            execution_generation: 1,
            changed_files: vec!["src/main.rs".into(), "Cargo.toml".into()],
            language: "rust".into(),
        })
    }

    // Dummy services for internal verifier tests
    struct DummyReader {
        manifest: ArtifactManifest,
        files: std::collections::HashMap<String, Vec<u8>>,
    }
    impl SealedArtifactReader for DummyReader {
        fn read_manifest(&self, _: &str) -> Result<ArtifactManifest, String> {
            Ok(self.manifest.clone())
        }
        fn read_file(&self, _: &str, path: &str) -> Result<Vec<u8>, String> {
            self.files.get(path).cloned().ok_or_else(|| format!("not found: {}", path))
        }
    }

    fn dummy_services(reader: &DummyReader) -> VerifierServices<'_> {
        struct G;
        impl onto_protocol::verifier::CandidateGraphReader for G {
            fn check_integrity(&self, _: &str) -> Result<(), String> { Ok(()) }
            fn get_entity_keys(&self, _: &str) -> Result<Vec<String>, String> { Ok(vec![]) }
        }
        struct S;
        #[async_trait::async_trait]
        impl onto_protocol::verifier::SemanticRuntimePort for S {
            async fn review(&self, _: &str, _: &str) -> Result<String, String> {
                Ok("ok".into())
            }
        }
        static G1: G = G;
        static S1: S = S;
        VerifierServices {
            artifact_reader: reader,
            graph_reader: &G1,
            semantic_runtime: &S1,
        }
    }

    // ── P16-2 acceptance tests ──

    #[tokio::test]
    async fn p16_2_1_manifest_integrity_passes_on_valid() {
        let content = b"hello world";
        let content_hash = {
            use sha2::{Digest as _, Sha256};
            hex::encode(Sha256::digest(content))
        };
        let manifest = ArtifactManifest {
            entries: vec![ArtifactEntry {
                path: "src/main.rs".into(),
                content_hash: content_hash.clone(),
                size_bytes: content.len() as u64,
                is_new: true,
                is_modified: false,
            }],
        };
        // Compute expected manifest digest
        let payload = format!("src/main.rs:{}:{}", content_hash, content.len());
        let expected_digest = d(&payload);

        let mut files = std::collections::HashMap::new();
        files.insert("src/main.rs".to_string(), content.to_vec());
        let reader = DummyReader { manifest, files };

        let ctx = VerificationContext::PreGraph(CandidateVerificationContext {
            attempt_id: "a1".into(),
            candidate: SealedCandidateRef::new(
                "c1",
                d("c1-digest"),
                expected_digest,
                "/tmp/test",
            ),
            repository_name: "test".into(),
            base_commit_sha: "abc".into(),
            execution_generation: 1,
            changed_files: vec![],
            language: "rust".into(),
        });
        let svc = dummy_services(&reader);
        let v = ArtifactManifestIntegrityVerifier::new();
        let result = v.evaluate(&ctx, &[], &svc).await;
        assert_eq!(result.status, VerifierStatus::Completed);
        assert!(result.findings.is_empty(),
            "expected no findings, got: {:?}", result.findings);
    }

    #[tokio::test]
    async fn p16_2_2_manifest_digest_mismatch_detected() {
        let manifest = ArtifactManifest { entries: vec![] };
        let reader = DummyReader {
            manifest,
            files: std::collections::HashMap::new(),
        };
        let ctx = VerificationContext::PreGraph(CandidateVerificationContext {
            attempt_id: "a1".into(),
            candidate: SealedCandidateRef::new("c1", d("c1"), d("wrong-digest"), "/tmp"),
            repository_name: "test".into(),
            base_commit_sha: "abc".into(),
            execution_generation: 1,
            changed_files: vec![],
            language: "rust".into(),
        });
        let svc = dummy_services(&reader);
        let v = ArtifactManifestIntegrityVerifier::new();
        let result = v.evaluate(&ctx, &[], &svc).await;
        assert_eq!(result.status, VerifierStatus::Completed);
        assert!(!result.findings.is_empty(), "digest mismatch must produce a finding");
    }

    #[tokio::test]
    async fn p16_2_3_protected_path_violation_detected() {
        let v = ProtectedPathVerifier::new(vec!["src/secret/".into()]);
        let ctx = VerificationContext::PreGraph(CandidateVerificationContext {
            attempt_id: "a1".into(),
            candidate: SealedCandidateRef::new("c1", d("c1"), d("m1"), "/tmp"),
            repository_name: "test".into(),
            base_commit_sha: "abc".into(),
            execution_generation: 1,
            changed_files: vec!["src/secret/keys.json".into(), "src/main.rs".into()],
            language: "rust".into(),
        });
        let result = v.evaluate(&ctx, &[], &dummy_services(&DummyReader {
            manifest: ArtifactManifest { entries: vec![] },
            files: std::collections::HashMap::new(),
        })).await;
        assert!(!result.findings.is_empty(), "protected path violation must produce finding");
    }

    #[tokio::test]
    async fn p16_2_4_build_empty_evidence_is_partially_completed() {
        let v = ProjectBuildVerifier::new();
        // Empty evidence — must NOT return Completed
        let result = v.evaluate(&test_ctx(), &[], &dummy_services(&DummyReader {
            manifest: ArtifactManifest { entries: vec![] },
            files: std::collections::HashMap::new(),
        })).await;
        assert_eq!(result.status, VerifierStatus::PartiallyCompleted,
            "empty evidence must be PartiallyCompleted, got {:?}", result.status);
        assert!(result.findings.is_empty(),
            "no findings on missing evidence (just status = PartiallyCompleted)");
    }

    #[tokio::test]
    async fn p16_2_5_build_exit_nonzero_produces_blocking() {
        let v = ProjectBuildVerifier::new();
        let check_id = "project.build".to_string();
        let raw = RawCheckResult {
            requirement_id: "req-build".into(),
            check_id: check_id.clone(),
            status: CheckExecutionStatus::Exited { exit_code: 1 },
            stdout_ref: None,
            stderr_ref: Some("compilation error".into()),
            produced_artifacts: vec![],
            duration_ms: 100,
        };
        let evidence: Vec<(&String, &RawCheckResult)> = vec![(&check_id, &raw)];
        let result = v.evaluate(&test_ctx(), &evidence, &dummy_services(&DummyReader {
            manifest: ArtifactManifest { entries: vec![] },
            files: std::collections::HashMap::new(),
        })).await;
        assert_eq!(result.status, VerifierStatus::Completed);
        assert!(!result.findings.is_empty(), "exit code != 0 must produce finding");
    }

    #[tokio::test]
    async fn p16_2_6_build_success_passes() {
        let v = ProjectBuildVerifier::new();
        let check_id = "project.build".to_string();
        let raw = RawCheckResult {
            requirement_id: "req-build".into(),
            check_id: check_id.clone(),
            status: CheckExecutionStatus::Exited { exit_code: 0 },
            stdout_ref: None,
            stderr_ref: None,
            produced_artifacts: vec![],
            duration_ms: 100,
        };
        let evidence: Vec<(&String, &RawCheckResult)> = vec![(&check_id, &raw)];
        let result = v.evaluate(&test_ctx(), &evidence, &dummy_services(&DummyReader {
            manifest: ArtifactManifest { entries: vec![] },
            files: std::collections::HashMap::new(),
        })).await;
        assert_eq!(result.status, VerifierStatus::Completed);
        assert!(result.findings.is_empty(), "exit code 0 → no findings");
    }

    #[tokio::test]
    async fn p16_2_7_tool_not_found_is_unavailable() {
        let v = ProjectBuildVerifier::new();
        let check_id = "project.build".to_string();
        let raw = RawCheckResult {
            requirement_id: "req-build".into(),
            check_id: check_id.clone(),
            status: CheckExecutionStatus::ToolNotFound,
            stdout_ref: None,
            stderr_ref: None,
            produced_artifacts: vec![],
            duration_ms: 0,
        };
        let evidence: Vec<(&String, &RawCheckResult)> = vec![(&check_id, &raw)];
        let result = v.evaluate(&test_ctx(), &evidence, &dummy_services(&DummyReader {
            manifest: ArtifactManifest { entries: vec![] },
            files: std::collections::HashMap::new(),
        })).await;
        assert_eq!(result.status, VerifierStatus::Unavailable,
            "ToolNotFound must be Unavailable, got {:?}", result.status);
    }
}
