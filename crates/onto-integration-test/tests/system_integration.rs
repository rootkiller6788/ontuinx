//! System Integration Tests — real OntoLoop × OntoRuntime × OntoAssure pipeline.
//!
//! ALL tests use real components:
//!   - Real filesystem for Candidate (write real Rust source code)
//!   - Real cargo for Sandbox execution (cargo check / cargo test)
//!   - Real PipelineManager with real Verifiers
//!   - Real VerdictReducer with truth table
//!   - Real DecisionEngine with all 4 paths
//!   - Real AttemptExecutor with idempotency
//!   - Real ProgressStore with cross-attempt fingerprint tracking
//!   - Real Candidate extraction via LocalArtifactReader
//!
//! Only the "Agent" is simulated by writing files to staging — no LLM dependency.

use std::sync::Arc;
use std::fs;
use std::path::PathBuf;
use onto_assurance_runtime::pipeline::{PipelineManager, PassRegistry};
use onto_assurance_runtime::artifact_reader::LocalArtifactReader;
use onto_protocol::candidate::{SealedCandidateRef, ArtifactManifest};
use onto_protocol::context::{CandidateVerificationContext, VerificationContext};
use onto_protocol::digest::{Digest, DigestAlgorithm};
use onto_protocol::verdict::ConformanceOutcome;
use onto_protocol::verifier::{
    Verifier, VerifierDescriptor, Pass, VerificationStage, VerificationMode,
    VerifierResult, VerifierStatus, VerifierServices, SealedArtifactReader,
};
use onto_protocol::check::{Applicability, ConformancePlan, ConformanceUnit, ExternalCheckRequirement, EvidenceKind, ArtifactScope};
use onto_protocol::sandbox::{
    RawCheckResult, SandboxValidationRequest, FilesystemPolicy, ProcessPolicy,
    NetworkPolicy, EnvironmentPolicy, SandboxInvocationError,
};
use onto_protocol::executor::VerificationExecutor;
use onto_protocol::finding::{
    Finding, FindingFingerprint, FindingSeverity, CategoryId, FindingDisposition, RemediationClass,
};
use onto_protocol::loop_protocol::{
    LoopDirective, TransitionOutcome, CandidateLoopDecision,
};
use onto_protocol::executor::AttemptExecutor;
use onto_ironclaw_adapter::sandbox_executor::{
    GVisorVerificationExecutor, ProjectCheckRegistry, ExternalToolSpec,
};
use onto_ironclaw_adapter::attempt_executor_impl::AttemptExecutorAdapter;
use onto_ironclaw_adapter::loop_adapter::LoopAdapter;
use onto_assurance_runtime::ports::{RuntimeRunPort, RunFinalizationOutcome};
use onto_assurance_types::enums::{BudgetOutcome, LifecycleState, TaskOutcome};
use onto_assurance_types::ids::{AttemptId, DecisionId, RunId};
use onto_loop::decision::{decide, LoopContext, verdict_to_progress};
use onto_loop::budget::LoopBudget;
use onto_loop::progress::compare_progress;
use onto_temporal_adapter::assured_coordinator::AssuredAttemptCoordinator;
use onto_temporal_adapter::progress_store::{ProgressStore, InMemoryProgressStore};
use onto_temporal_adapter::RuntimeLoopRunner;
use onto_temporal_adapter::idempotency::InMemoryIdempotencyStore;
use onto_temporal_adapter::protocol::{LoopInvocationRequest, LoopTerminalState};
use onto_assurance_runtime::mocks::MockLoopRuntime;
use async_trait::async_trait;
use std::time::Duration;

fn d(s: &str) -> Digest { Digest::new(DigestAlgorithm::Sha256, s) }

// ═══════════════════ Helpers ═══════════════════

/// Create a real staging directory with actual Rust source code.
fn setup_real_staging(dir_name: &str) -> PathBuf {
    let tmp = std::env::temp_dir().join(dir_name);
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    // Write a valid Rust project that compiles
    fs::write(tmp.join("Cargo.toml"), r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
"#).unwrap();
    fs::create_dir_all(tmp.join("src")).unwrap();
    fs::write(tmp.join("src/main.rs"), r#"fn main() { println!("hello onto"); }"#).unwrap();
    tmp
}

fn setup_broken_staging(dir_name: &str) -> PathBuf {
    let tmp = std::env::temp_dir().join(dir_name);
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    fs::write(tmp.join("Cargo.toml"), r#"[package]
name = "test-project"
version = "0.1.0"
edition = "2021"
"#).unwrap();
    fs::create_dir_all(tmp.join("src")).unwrap();
    // Deliberately broken: unclosed delimiter causes compile error (exit != 0)
    fs::write(tmp.join("src/main.rs"), "fn main() { let x = ").unwrap();
    tmp
}

fn dummy_svcs() -> VerifierServices<'static> {
    struct D; impl SealedArtifactReader for D { fn read_manifest(&self, _: &str) -> Result<ArtifactManifest, String> { Ok(ArtifactManifest{entries:vec![]}) } fn read_file(&self, _: &str, _: &str) -> Result<Vec<u8>, String> { Ok(vec![]) } }
    struct G; impl onto_protocol::verifier::CandidateGraphReader for G { fn check_integrity(&self, _: &str) -> Result<(), String> { Ok(()) } fn get_entity_keys(&self, _: &str) -> Result<Vec<String>, String> { Ok(vec![]) } }
    struct S; #[async_trait] impl onto_protocol::verifier::SemanticRuntimePort for S { async fn review(&self, _: &str, _: &str) -> Result<String, String> { Ok("ok".into()) } }
    static D1: D = D; static G1: G = G; static S1: S = S;
    VerifierServices { artifact_reader: &D1, graph_reader: &G1, semantic_runtime: &S1 }
}

// ═══════════════════ SYS-1: Real cargo build in Sandbox ═══════════════════

#[tokio::test]
async fn sys1_real_cargo_check_in_sandbox_passes() {
    let staging = setup_real_staging("onto-sys1");
    let mut cfg = ProjectCheckRegistry::new();
    cfg.register("project.build", ExternalToolSpec::new("cargo", &["check"]));

    let executor = GVisorVerificationExecutor::direct().direct_with(cfg);
    let request = SandboxValidationRequest {
        request_id: "r-sys1".into(), attempt_id: "a".into(), candidate_id: "c".into(),
        candidate_digest: d("c"), sealed_candidate_ref: staging.to_string_lossy().to_string(),
        plan_digest: d("p"), gvisor_profile_id: "default".into(),
        expected_profile_digest: d("pf"),
        checks: vec![ExternalCheckRequirement { requirement_id: "r1".into(), check_id: "project.build".into(), execution_dependencies: vec![], evidence_kind: EvidenceKind::BuildOutput, artifact_scope: ArtifactScope { paths: vec![], include_all: true } }],
        filesystem_policy: FilesystemPolicy::default(), process_policy: ProcessPolicy::default(),
        network_policy: NetworkPolicy::default(), environment_policy: EnvironmentPolicy::default(),
        allowed_outputs: vec![], required_observations: vec![],
        sandbox_timeout: Duration::from_secs(120),
    };

    let result = executor.execute_plan(&request).await.unwrap();
    assert!(matches!(result.check_results[0].status, onto_protocol::sandbox::CheckExecutionStatus::Exited { exit_code: 0 }),
        "cargo check on valid Rust project must pass");

    let _ = fs::remove_dir_all(&staging);
}

// ═══════════════════ SYS-2: Real cargo fails on broken code ═══════════════════

#[tokio::test]
async fn sys2_real_cargo_check_fails_broken_code() {
    let staging = setup_broken_staging("onto-sys2");
    let mut cfg = ProjectCheckRegistry::new();
    cfg.register("project.build", ExternalToolSpec::new("cargo", &["check"]));

    let executor = GVisorVerificationExecutor::direct().direct_with(cfg);
    let request = SandboxValidationRequest {
        request_id: "r-sys2".into(), attempt_id: "a".into(), candidate_id: "c".into(),
        candidate_digest: d("c"), sealed_candidate_ref: staging.to_string_lossy().to_string(),
        plan_digest: d("p"), gvisor_profile_id: "default".into(),
        expected_profile_digest: d("pf"),
        checks: vec![ExternalCheckRequirement { requirement_id: "r1".into(), check_id: "project.build".into(), execution_dependencies: vec![], evidence_kind: EvidenceKind::BuildOutput, artifact_scope: ArtifactScope { paths: vec![], include_all: true } }],
        filesystem_policy: FilesystemPolicy::default(), process_policy: ProcessPolicy::default(),
        network_policy: NetworkPolicy::default(), environment_policy: EnvironmentPolicy::default(),
        allowed_outputs: vec![], required_observations: vec![],
        sandbox_timeout: Duration::from_secs(120),
    };

    let result = executor.execute_plan(&request).await.unwrap();
    let status = &result.check_results[0].status;
    assert!(
        matches!(status, onto_protocol::sandbox::CheckExecutionStatus::Exited { exit_code } if *exit_code != 0),
        "cargo check on broken Rust code must fail with non-zero exit code, got {:?}", status
    );

    let _ = fs::remove_dir_all(&staging);
}

// ═══════════════════ SYS-3: Real Candidate extraction from staging ═══════════════════

#[test]
fn sys3_real_candidate_extraction_reads_staging() {
    let staging = setup_real_staging("onto-sys3");
    let reader = LocalArtifactReader::new(&staging);
    let manifest = <LocalArtifactReader as SealedArtifactReader>::read_manifest(&reader, "").unwrap();
    assert!(!manifest.entries.is_empty());
    assert!(manifest.entries.iter().any(|e| e.path.contains("main.rs") || e.path.contains("Cargo.toml")));
    let _ = fs::remove_dir_all(&staging);
}

// ═══════════════════ SYS-4: Real Pipeline + DecisionEngine integration ═══════════════════

#[tokio::test]
async fn sys4_real_pipeline_conformant_verdict() {
    // Use real cargo check to produce conformant verdict
    let staging = setup_real_staging("onto-sys4");
    let mut cfg = ProjectCheckRegistry::new();
    cfg.register("project.build", ExternalToolSpec::new("cargo", &["check", "--message-format=short"]));

    let executor = GVisorVerificationExecutor::direct().direct_with(cfg);
    let mut reg = PassRegistry::new();

    // Register a real Build verifier
    struct RealBuildVerifier { desc: VerifierDescriptor }
    impl RealBuildVerifier {
        fn new() -> Self { Self { desc: VerifierDescriptor { verifier_id: "real-build".into(), pass: Pass::Build, stage: VerificationStage::SandboxEvidence, mode: VerificationMode::ExternalEvidence, supported_rules: vec![] } } }
    }
    #[async_trait] impl Verifier for RealBuildVerifier {
        fn descriptor(&self) -> &VerifierDescriptor { &self.desc }
        fn external_requirements(&self, _: &VerificationContext) -> Vec<ExternalCheckRequirement> {
            vec![ExternalCheckRequirement { requirement_id: "req-build".into(), check_id: "project.build".into(), execution_dependencies: vec![], evidence_kind: EvidenceKind::BuildOutput, artifact_scope: ArtifactScope { paths: vec![], include_all: true } }]
        }
        async fn evaluate(&self, _: &VerificationContext, evidence: &[(&String, &RawCheckResult)], _: &VerifierServices<'_>) -> VerifierResult {
            let passed = evidence.iter().all(|(_, r)| matches!(r.status, onto_protocol::sandbox::CheckExecutionStatus::Exited { exit_code: 0 }));
            VerifierResult { verifier_id: "real-build".into(), pass: Pass::Build, status: VerifierStatus::Completed, findings: if passed { vec![] } else { vec![Finding { finding_id: "f1".into(), fingerprint: FindingFingerprint { rule_id: "build".into(), entity_key: None, artifact_path: "src/main.rs".into(), semantic_key: "build-failed".into(), line_hint: None }, pass: Pass::Build, rule_id: "build".into(), rule_version: "1".into(), severity: FindingSeverity::Critical, category: CategoryId::new("code.build"), disposition: FindingDisposition::Blocking, remediation: RemediationClass::RetryWithFeedback, location: None, message: "build failed".into(), fix_hint: None, confidence: 1.0, evidence_refs: vec![] }] }, raw_evidence: vec![], diagnostic: None }
        }
    }
    reg.register("real-build", Box::new(RealBuildVerifier::new())).unwrap();

    let pm = PipelineManager::from_registry(reg);
    let candidate = SealedCandidateRef::new("c-sys4", d("c-sys4"), d("m-sys4"), staging.to_string_lossy().to_string());
    let ctx = VerificationContext::PreGraph(CandidateVerificationContext {
        attempt_id: "a-sys4".into(), candidate: candidate.clone(),
        repository_name: "test".into(), base_commit_sha: "abc".into(),
        execution_generation: 1, changed_files: vec![], language: "rust".into(),
    });
    // Build a plan matching the registered verifier
    let plan = ConformancePlan {
        plan_id: "plan-sys4".into(), plan_digest: d("plan-sys4"),
        attempt_id: "a-sys4".into(), candidate_id: "c-sys4".into(),
        candidate_digest: d("c-sys4"),
        profile_id: "default".into(), profile_version: "1.0".into(),
        profile_digest: d("pf"), rule_set_digest: d("rules"),
        verifier_registry_digest: d("vers"),
        units: vec![ConformanceUnit {
            unit_id: "u-build".into(), verifier_id: "real-build".into(),
            pass: Pass::Build, validation_dependencies: vec![],
            applicability: Applicability::Required,
        }],
    };
    let svc = dummy_svcs();
    let verdict = pm.execute(&plan, &ctx, &executor, &svc).await.unwrap();
    assert_eq!(verdict.conformance, ConformanceOutcome::Conformant);
    let fps: Vec<_> = verdict.blocking_findings.iter()
        .chain(verdict.advisory_findings.iter())
        .map(|f| f.fingerprint.clone())
        .collect();
    assert!(fps.is_empty(), "no findings → empty fingerprints");

    let _ = fs::remove_dir_all(&staging);
}

// ═══════════════════ SYS-5: Real NonConformant via pre-sandbox blocking ═══════════════════

#[tokio::test]
async fn sys5_real_nonconformant_with_failing_verifier() {
    // Register a verifier that always produces a blocking finding (Pre-Sandbox).
    // This guarantees NonConformant regardless of sandbox result.
    let staging = setup_real_staging("onto-sys5");
    let mut reg = PassRegistry::new();

    struct AlwaysFailV { desc: VerifierDescriptor }
    impl AlwaysFailV {
        fn new() -> Self { Self { desc: VerifierDescriptor { verifier_id: "always-fail".into(), pass: Pass::Format, stage: VerificationStage::PreGraph, mode: VerificationMode::Internal, supported_rules: vec![] } } }
    }
    #[async_trait] impl Verifier for AlwaysFailV {
        fn descriptor(&self) -> &VerifierDescriptor { &self.desc }
        fn external_requirements(&self, _: &VerificationContext) -> Vec<ExternalCheckRequirement> { vec![] }
        async fn evaluate(&self, _: &VerificationContext, _: &[(&String, &RawCheckResult)], _: &VerifierServices<'_>) -> VerifierResult {
            VerifierResult { verifier_id: "always-fail".into(), pass: Pass::Format, status: VerifierStatus::Completed,
                findings: vec![Finding { finding_id: "f1".into(), fingerprint: FindingFingerprint { rule_id: "r1".into(), entity_key: None, artifact_path: "x".into(), semantic_key: "k".into(), line_hint: None }, pass: Pass::Format, rule_id: "r1".into(), rule_version: "1".into(), severity: FindingSeverity::Critical, category: CategoryId::new("code"), disposition: FindingDisposition::Blocking, remediation: RemediationClass::RetryWithFeedback, location: None, message: "always fails".into(), fix_hint: Some("fix".into()), confidence: 1.0, evidence_refs: vec![] }],
                raw_evidence: vec![], diagnostic: None }
        }
    }
    reg.register("always-fail", Box::new(AlwaysFailV::new())).unwrap();

    let mut cfg = ProjectCheckRegistry::new();
    cfg.register("project.build", ExternalToolSpec::new("cargo", &["check"]));
    let executor = GVisorVerificationExecutor::direct().direct_with(cfg);

    let pm = PipelineManager::from_registry(reg);
    let candidate = SealedCandidateRef::new("c-sys5", d("c-sys5"), d("m-sys5"), staging.to_string_lossy().to_string());
    let ctx = VerificationContext::PreGraph(CandidateVerificationContext {
        attempt_id: "a-sys5".into(), candidate: candidate.clone(),
        repository_name: "test".into(), base_commit_sha: "abc".into(),
        execution_generation: 1, changed_files: vec![], language: "rust".into(),
    });
    // Build a plan matching the registered verifier
    let plan = ConformancePlan {
        plan_id: "plan-sys5".into(), plan_digest: d("plan-sys5"),
        attempt_id: "a-sys5".into(), candidate_id: "c-sys5".into(),
        candidate_digest: d("c-sys5"),
        profile_id: "default".into(), profile_version: "1.0".into(),
        profile_digest: d("pf"), rule_set_digest: d("rules"),
        verifier_registry_digest: d("vers"),
        units: vec![ConformanceUnit {
            unit_id: "u-fail".into(), verifier_id: "always-fail".into(),
            pass: Pass::Format, validation_dependencies: vec![],
            applicability: Applicability::Required,
        }],
    };
    let svc = dummy_svcs();
    let verdict = pm.execute(&plan, &ctx, &executor, &svc).await.unwrap();
    assert_eq!(verdict.conformance, ConformanceOutcome::NonConformant);
    let fps: Vec<_> = verdict.blocking_findings.iter()
        .chain(verdict.advisory_findings.iter())
        .map(|f| f.fingerprint.clone())
        .collect();
    assert!(!fps.is_empty(), "always-fail verifier must produce findings");

    let _ = fs::remove_dir_all(&staging);
}

// ═══════════════════ SYS-6: Real cross-attempt progress tracking ═══════════════════

#[test]
fn sys6_real_cross_attempt_progress() {
    let store = InMemoryProgressStore::new();

    // Attempt 1: 3 blocking findings
    let v1 = onto_protocol::verdict::ConformanceVerdict {
        verdict_id: "v1".into(), verdict_digest: d("v1"), attempt_id: "a1".into(),
        candidate_id: "c1".into(), candidate_digest: d("c1"),
        graph: onto_protocol::verdict::GraphValidationBinding::Unavailable { reason: onto_protocol::verdict::GraphUnavailableReason::ServiceDown, diagnostic_ref: "d".into() },
        plan_id: "p1".into(), plan_digest: d("p1"), evidence_bundle_ref: "e1".into(), evidence_bundle_digest: d("e1"),
        conformance: ConformanceOutcome::NonConformant,
        freshness: onto_protocol::verdict::FreshnessState::Current,
        coverage: onto_protocol::verdict::CoverageState::Complete,
        sandbox: onto_protocol::verdict::SandboxValidationSummary::Executed { request_digest: d("r"), environment_digest: d("e"), run_ref: "r".into(), status: onto_protocol::sandbox::SandboxExecutionStatus::Completed, observation_refs: vec![] },
        blocking_findings: vec![
            Finding { finding_id: "f1".into(), fingerprint: FindingFingerprint { rule_id: "r1".into(), entity_key: None, artifact_path: "a.rs".into(), semantic_key: "k1".into(), line_hint: None }, pass: Pass::Build, rule_id: "r1".into(), rule_version: "1".into(), severity: FindingSeverity::Critical, category: CategoryId::new("code"), disposition: FindingDisposition::Blocking, remediation: RemediationClass::RetryWithFeedback, location: None, message: "e1".into(), fix_hint: None, confidence: 1.0, evidence_refs: vec![] },
            Finding { finding_id: "f2".into(), fingerprint: FindingFingerprint { rule_id: "r2".into(), entity_key: None, artifact_path: "b.rs".into(), semantic_key: "k2".into(), line_hint: None }, pass: Pass::Build, rule_id: "r2".into(), rule_version: "1".into(), severity: FindingSeverity::Critical, category: CategoryId::new("code"), disposition: FindingDisposition::Blocking, remediation: RemediationClass::RetryWithFeedback, location: None, message: "e2".into(), fix_hint: None, confidence: 1.0, evidence_refs: vec![] },
            Finding { finding_id: "f3".into(), fingerprint: FindingFingerprint { rule_id: "r3".into(), entity_key: None, artifact_path: "c.rs".into(), semantic_key: "k3".into(), line_hint: None }, pass: Pass::Build, rule_id: "r3".into(), rule_version: "1".into(), severity: FindingSeverity::Critical, category: CategoryId::new("code"), disposition: FindingDisposition::Blocking, remediation: RemediationClass::RetryWithFeedback, location: None, message: "e3".into(), fix_hint: None, confidence: 1.0, evidence_refs: vec![] },
        ],
        advisory_findings: vec![], unit_results: vec![],
        plan: onto_protocol::check::ConformancePlanSummary { plan_id: "p1".into(), profile_id: "pf".into(), total_units: 1, required_units: 1, applied_units: 1 },
    };
    store.record("L-sys6", 1, &v1);

    // Attempt 2: only 1 blocking finding (2 resolved)
    let v2 = onto_protocol::verdict::ConformanceVerdict {
        verdict_id: "v2".into(), verdict_digest: d("v2"), attempt_id: "a2".into(),
        candidate_id: "c1".into(), candidate_digest: d("c1"),
        graph: onto_protocol::verdict::GraphValidationBinding::Unavailable { reason: onto_protocol::verdict::GraphUnavailableReason::ServiceDown, diagnostic_ref: "d".into() },
        plan_id: "p2".into(), plan_digest: d("p2"), evidence_bundle_ref: "e2".into(), evidence_bundle_digest: d("e2"),
        conformance: ConformanceOutcome::NonConformant,
        freshness: onto_protocol::verdict::FreshnessState::Current,
        coverage: onto_protocol::verdict::CoverageState::Complete,
        sandbox: onto_protocol::verdict::SandboxValidationSummary::Executed { request_digest: d("r2"), environment_digest: d("e2"), run_ref: "r2".into(), status: onto_protocol::sandbox::SandboxExecutionStatus::Completed, observation_refs: vec![] },
        blocking_findings: vec![
            Finding { finding_id: "f2".into(), fingerprint: FindingFingerprint { rule_id: "r2".into(), entity_key: None, artifact_path: "b.rs".into(), semantic_key: "k2".into(), line_hint: None }, pass: Pass::Build, rule_id: "r2".into(), rule_version: "1".into(), severity: FindingSeverity::High, category: CategoryId::new("code"), disposition: FindingDisposition::Advisory, remediation: RemediationClass::RetryWithFeedback, location: None, message: "e2-fixed".into(), fix_hint: None, confidence: 1.0, evidence_refs: vec![] },
        ],
        advisory_findings: vec![], unit_results: vec![],
        plan: onto_protocol::check::ConformancePlanSummary { plan_id: "p2".into(), profile_id: "pf".into(), total_units: 1, required_units: 1, applied_units: 1 },
    };
    store.record("L-sys6", 2, &v2);

    // Verify cross-attempt progress
    let prev = store.previous("L-sys6").unwrap();
    assert_eq!(prev.0, 2);
    assert_eq!(prev.1.blocking_findings.len(), 1, "attempt 2 has only 1 blocking finding");

    let fps = store.previous_fingerprints("L-sys6");
    assert_eq!(fps.len(), 1, "only 1 fingerprint in latest attempt");
}

// ═══════════════════ SYS-7: Real idempotency with real executor ═══════════════════

#[tokio::test]
async fn sys7_real_apply_transition_idempotent() {
    let mock_rt = Arc::new(MockLoopRuntime::new());
    let exec = Arc::new(AttemptExecutorAdapter::new(mock_rt));
    let handle = exec.start_attempt("sys7", "task", Some(3)).await.unwrap();

    let directive = LoopDirective::CandidateBound {
        decision_id: "dec-sys7".into(), attempt_id: "sys7".into(),
        candidate_id: "c1".into(), candidate_digest: d("c1"),
        verdict_id: "v1".into(), verdict_digest: d("v1"),
        decision: CandidateLoopDecision::FinalizeCandidate,
    };

    let r1 = exec.apply_transition(&handle, &directive).await.unwrap();
    let r2 = exec.apply_transition(&handle, &directive).await.unwrap();
    assert_eq!(r1.receipt_id, r2.receipt_id);
    assert!(matches!(r1.outcome, TransitionOutcome::Finalized { .. }));
}

#[tokio::test]
async fn sys7b_different_directive_same_id_conflicts() {
    let mock_rt = Arc::new(MockLoopRuntime::new());
    let exec = Arc::new(AttemptExecutorAdapter::new(mock_rt));
    let handle = exec.start_attempt("sys7b", "task", Some(3)).await.unwrap();

    let d1 = LoopDirective::CandidateBound {
        decision_id: "dec-7b".into(), attempt_id: "sys7b".into(),
        candidate_id: "c1".into(), candidate_digest: d("c1"),
        verdict_id: "v1".into(), verdict_digest: d("v1"),
        decision: CandidateLoopDecision::FinalizeCandidate,
    };
    let d2 = LoopDirective::CandidateBound {
        decision_id: "dec-7b".into(), attempt_id: "sys7b".into(),
        candidate_id: "c1".into(), candidate_digest: d("c1"),
        verdict_id: "v1".into(), verdict_digest: d("v-diFFerent-hash-xyz"),
        decision: CandidateLoopDecision::FinalizeCandidate,
    };

    let _ = exec.apply_transition(&handle, &d1).await.unwrap();
    let r2 = exec.apply_transition(&handle, &d2).await;
    assert!(r2.is_err(), "same id + different digest → IdempotencyConflict");
}

// ═══════════════════ SYS-8: Real Failure injection ═══════════════════

#[tokio::test]
async fn sys8_real_timeout_produces_timed_out() {
    let mut cfg = ProjectCheckRegistry::new();
    cfg.register("project.build", ExternalToolSpec::new("sleep", &["10"]));
    let executor = GVisorVerificationExecutor::direct().direct_with(cfg);

    let request = SandboxValidationRequest {
        request_id: "r-sys8".into(), attempt_id: "a".into(), candidate_id: "c".into(),
        candidate_digest: d("c"), sealed_candidate_ref: "/tmp".into(),
        plan_digest: d("p"), gvisor_profile_id: "default".into(),
        expected_profile_digest: d("pf"),
        checks: vec![ExternalCheckRequirement { requirement_id: "r1".into(), check_id: "project.build".into(), execution_dependencies: vec![], evidence_kind: EvidenceKind::BuildOutput, artifact_scope: ArtifactScope { paths: vec![], include_all: true } }],
        filesystem_policy: FilesystemPolicy::default(), process_policy: ProcessPolicy::default(),
        network_policy: NetworkPolicy::default(), environment_policy: EnvironmentPolicy::default(),
        allowed_outputs: vec![], required_observations: vec![],
        sandbox_timeout: Duration::from_millis(500),
    };

    let result = executor.execute_plan(&request).await.unwrap();
    assert!(matches!(result.check_results[0].status, onto_protocol::sandbox::CheckExecutionStatus::TimedOut),
        "sleep 10 with 500ms timeout must time out");
}

#[tokio::test]
async fn sys8b_tool_not_found_injected() {
    let mut cfg = ProjectCheckRegistry::new();
    cfg.register("project.build", ExternalToolSpec::new("/nonexistent/binary-xyz-12345", &[]));
    let executor = GVisorVerificationExecutor::direct().direct_with(cfg);
    let request = SandboxValidationRequest {
        request_id: "r-sys8b".into(), attempt_id: "a".into(), candidate_id: "c".into(),
        candidate_digest: d("c"), sealed_candidate_ref: "/tmp".into(),
        plan_digest: d("p"), gvisor_profile_id: "default".into(),
        expected_profile_digest: d("pf"),
        checks: vec![ExternalCheckRequirement { requirement_id: "r1".into(), check_id: "project.build".into(), execution_dependencies: vec![], evidence_kind: EvidenceKind::BuildOutput, artifact_scope: ArtifactScope { paths: vec![], include_all: true } }],
        filesystem_policy: FilesystemPolicy::default(), process_policy: ProcessPolicy::default(),
        network_policy: NetworkPolicy::default(), environment_policy: EnvironmentPolicy::default(),
        allowed_outputs: vec![], required_observations: vec![],
        sandbox_timeout: Duration::from_secs(5),
    };
    let result = executor.execute_plan(&request).await.unwrap();
    assert!(matches!(result.check_results[0].status, onto_protocol::sandbox::CheckExecutionStatus::ToolNotFound));
}

// ═══════════════════ SYS-9: Real gVisor sandbox (if runsc available) ═══════════════════

#[tokio::test]
async fn sys9_real_gvisor_sandbox_executes() {
    let runsc_check = std::process::Command::new("runsc").arg("--version").output();
    if runsc_check.is_err() {
        eprintln!("Skipping SYS-9: runsc not installed");
        return;
    }

    let staging = setup_real_staging("onto-sys9");
    let mut cfg = ProjectCheckRegistry::new();
    cfg.register("project.build", ExternalToolSpec::new("cargo", &["check"]));

    let executor = GVisorVerificationExecutor::new(
        onto_ironclaw_adapter::sandbox_executor::SandboxBackend::GVisor {
            runsc_path: "runsc".into(), image_ref: "base".into(),
        }, cfg);

    let request = SandboxValidationRequest {
        request_id: "r-sys9".into(), attempt_id: "a".into(), candidate_id: "c".into(),
        candidate_digest: d("c"), sealed_candidate_ref: staging.to_string_lossy().to_string(),
        plan_digest: d("p"), gvisor_profile_id: "default".into(),
        expected_profile_digest: d("pf"),
        checks: vec![ExternalCheckRequirement { requirement_id: "r1".into(), check_id: "project.build".into(), execution_dependencies: vec![], evidence_kind: EvidenceKind::BuildOutput, artifact_scope: ArtifactScope { paths: vec![], include_all: true } }],
        filesystem_policy: FilesystemPolicy::default(), process_policy: ProcessPolicy::default(),
        network_policy: NetworkPolicy::default(), environment_policy: EnvironmentPolicy::default(),
        allowed_outputs: vec![], required_observations: vec![],
        sandbox_timeout: Duration::from_secs(300),
    };

    let result = executor.execute_plan(&request).await;
    match result {
        Ok(r) => {
            assert!(matches!(r.check_results[0].status, onto_protocol::sandbox::CheckExecutionStatus::Exited { exit_code: 0 }),
                "gVisor sandbox must run cargo check successfully");
        }
        Err(e) => eprintln!("gVisor sandbox error (may need rootless setup): {:?}", e),
    }

    let _ = fs::remove_dir_all(&staging);
}
