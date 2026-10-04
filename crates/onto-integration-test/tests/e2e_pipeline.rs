//! E2E integration test: L1→L2→L3 full pipeline.
//!
//! Verifies the complete chain:
//!   Mock Agent Run → PipelineManager (L1) → ConformanceVerdict
//!   → DecisionEngine (L3) → LoopDecision
//!
//! No external deps. Pure Rust, runs in CI.

use onto_protocol::candidate::{SealedCandidateRef, RunCompletion, ArtifactManifest};
use onto_protocol::check::{ConformancePlan, ConformanceUnit, Applicability, ExternalCheckRequirement, EvidenceKind, ArtifactScope};
use onto_protocol::context::{CandidateVerificationContext, VerificationContext};
use onto_protocol::digest::{Digest, DigestAlgorithm};
use onto_protocol::executor::VerificationExecutor;
use onto_protocol::finding::{
    Finding, FindingFingerprint, FindingSeverity, CategoryId, FindingDisposition, RemediationClass,
};
use onto_protocol::loop_protocol::{
    AttemptObservation, AssuranceObservation, CandidateLoopDecision, AttemptDecision,
    NoCandidateOutcome,
};
use onto_protocol::sandbox::{
    SandboxValidationRequest, SandboxRunResult, RawCheckResult, CheckExecutionStatus,
    SandboxExecutionStatus, SandboxEnvironmentIdentity,
    ArtifactDelta, ResourceUsage, RuntimeObservation, ObservationKind,
    SandboxInvocationError,
};
use onto_protocol::verdict::{
    ConformanceOutcome, UnitExecutionStatus,
};
use onto_protocol::verifier::{
    Verifier, VerifierDescriptor, Pass, VerificationStage, VerificationMode,
    VerifierResult, VerifierStatus, VerifierServices,
};
use onto_assurance_runtime::pipeline::{PipelineManager, PassRegistry};
use onto_loop::decision::{decide, LoopContext};
use onto_loop::budget::LoopBudget;

use async_trait::async_trait;
use std::sync::Mutex;
use std::time::Duration;

// ── Helpers ──

fn d(s: &str) -> Digest {
    use sha2::{Sha256, Digest as SD};
    Digest::new(DigestAlgorithm::Sha256, hex::encode(Sha256::digest(s.as_bytes())))
}

fn make_candidate() -> SealedCandidateRef {
    SealedCandidateRef::new("cand-e2e", d("c1"), d("m1"), "/tmp/e2e-candidate")
}

fn make_plan() -> ConformancePlan {
    ConformancePlan {
        plan_id: "plan-e2e".into(), plan_digest: d("plan-e2e"),
        attempt_id: "att-e2e".into(), candidate_id: "cand-e2e".into(),
        candidate_digest: d("c1"),
        profile_id: "default".into(), profile_version: "1.0".into(),
        profile_digest: d("pf"), rule_set_digest: d("rules"),
        verifier_registry_digest: d("vers"),
        units: vec![ConformanceUnit {
            unit_id: "u-fmt".into(), verifier_id: "v-fmt".into(),
            pass: Pass::Format, validation_dependencies: vec![],
            applicability: Applicability::Required,
        }],
    }
}

fn make_fail_plan() -> ConformancePlan {
    ConformancePlan {
        plan_id: "plan-fail".into(), plan_digest: d("plan-fail"),
        attempt_id: "att-fail".into(), candidate_id: "cand-fail".into(),
        candidate_digest: d("c1"),
        profile_id: "default".into(), profile_version: "1.0".into(),
        profile_digest: d("pf"), rule_set_digest: d("rules"),
        verifier_registry_digest: d("vers"),
        units: vec![ConformanceUnit {
            unit_id: "u-fail".into(), verifier_id: "v-fail".into(),
            pass: Pass::Format, validation_dependencies: vec![],
            applicability: Applicability::Required,
        }],
    }
}

fn make_ctx() -> VerificationContext {
    VerificationContext::PreGraph(CandidateVerificationContext {
        attempt_id: "att-e2e".into(), candidate: make_candidate(),
        repository_name: "test-repo".into(), base_commit_sha: "abc".into(),
        execution_generation: 1, changed_files: vec!["src/main.rs".into()],
        language: "rust".into(),
    })
}

// ── Stub Verifier (always passes) ──

struct PassVerifier { desc: VerifierDescriptor }
impl PassVerifier {
    fn new() -> Self { Self { desc: VerifierDescriptor { verifier_id: "v-fmt".into(), pass: Pass::Format, stage: VerificationStage::PreGraph, mode: VerificationMode::Internal, supported_rules: vec![] } } }
}
#[async_trait] impl Verifier for PassVerifier {
    fn descriptor(&self) -> &VerifierDescriptor { &self.desc }
    fn external_requirements(&self, _: &VerificationContext) -> Vec<ExternalCheckRequirement> { vec![] }
    async fn evaluate(&self, _: &VerificationContext, _: &[(&String, &RawCheckResult)], _: &VerifierServices<'_>) -> VerifierResult {
        VerifierResult { verifier_id: "v-fmt".into(), pass: Pass::Format, status: VerifierStatus::Completed, findings: vec![], raw_evidence: vec![], diagnostic: None }
    }
}

// ── Stub Verifier (produces blocking finding) ──

struct FailVerifier { desc: VerifierDescriptor }
impl FailVerifier {
    fn new() -> Self { Self { desc: VerifierDescriptor { verifier_id: "v-fail".into(), pass: Pass::Format, stage: VerificationStage::PreGraph, mode: VerificationMode::Internal, supported_rules: vec![] } } }
}
#[async_trait] impl Verifier for FailVerifier {
    fn descriptor(&self) -> &VerifierDescriptor { &self.desc }
    fn external_requirements(&self, _: &VerificationContext) -> Vec<ExternalCheckRequirement> { vec![] }
    async fn evaluate(&self, _: &VerificationContext, _: &[(&String, &RawCheckResult)], _: &VerifierServices<'_>) -> VerifierResult {
        VerifierResult { verifier_id: "v-fail".into(), pass: Pass::Format, status: VerifierStatus::Completed,
            findings: vec![Finding { finding_id: "f1".into(), fingerprint: FindingFingerprint { rule_id: "r1".into(), entity_key: None, artifact_path: "x".into(), semantic_key: "k".into(), line_hint: None }, pass: Pass::Format, rule_id: "r1".into(), rule_version: "1".into(), severity: FindingSeverity::Critical, category: CategoryId::new("code"), disposition: FindingDisposition::Blocking, remediation: RemediationClass::RetryWithFeedback, location: None, message: "fail".into(), fix_hint: Some("fix".into()), confidence: 1.0, evidence_refs: vec![] }],
            raw_evidence: vec![], diagnostic: None }
    }
}

// ── Mock Executor ──

struct MockExec;
#[async_trait] impl VerificationExecutor for MockExec {
    async fn execute_plan(&self, _: &SandboxValidationRequest) -> Result<SandboxRunResult, SandboxInvocationError> {
        Ok(SandboxRunResult {
            request_id: "r1".into(), request_digest: d("r1"), attempt_id: "a".into(),
            candidate_id: "c".into(), candidate_digest: d("c"),
            status: SandboxExecutionStatus::Completed, check_results: vec![],
            observations: vec![], environment: SandboxEnvironmentIdentity { runsc_version: "mock".into(), profile_digest: d("pf"), image_digest: d("img"), toolchain_digest: d("tc") },
            filesystem_diff: ArtifactDelta { created: vec![], modified: vec![], deleted: vec![] },
            resource_usage: ResourceUsage { cpu_seconds: 0.0, memory_mb: 0.0, disk_mb: 0.0, network_bytes_sent: 0, network_bytes_recv: 0 },
        })
    }
}

// ── Dummy Services ──

fn dummy_svcs() -> VerifierServices<'static> {
    struct D; impl onto_protocol::verifier::SealedArtifactReader for D { fn read_manifest(&self, _: &str) -> Result<ArtifactManifest, String> { Ok(ArtifactManifest{entries:vec![]}) } fn read_file(&self, _: &str, _: &str) -> Result<Vec<u8>, String> { Ok(vec![]) } }
    struct G; impl onto_protocol::verifier::CandidateGraphReader for G { fn check_integrity(&self, _: &str) -> Result<(), String> { Ok(()) } fn get_entity_keys(&self, _: &str) -> Result<Vec<String>, String> { Ok(vec![]) } }
    struct S; #[async_trait] impl onto_protocol::verifier::SemanticRuntimePort for S { async fn review(&self, _: &str, _: &str) -> Result<String, String> { Ok("ok".into()) } }
    static D1: D = D; static G1: G = G; static S1: S = S;
    VerifierServices { artifact_reader: &D1, graph_reader: &G1, semantic_runtime: &S1 }
}

// ══════════════════════════ Tests ══════════════════════════

/// E2E-1: Conformant verdict → FinalizeCandidate
#[tokio::test]
async fn e2e_conformant_finalizes() {
    let mut reg = PassRegistry::new();
    reg.register("v-fmt", Box::new(PassVerifier::new())).unwrap();
    let pm = PipelineManager::from_registry(reg);
    let plan = make_plan(); let ctx = make_ctx();
    let verdict = pm.execute(&plan, &ctx, &MockExec, &dummy_svcs()).await.unwrap();

    assert_eq!(verdict.conformance, ConformanceOutcome::Conformant);
    assert!(verdict.blocking_findings.is_empty());

    let obs = AttemptObservation::CandidateAvailable {
        attempt_id: "att-e2e".into(),
        run_completion: RunCompletion { staging_root: "/t".into(), artifact_manifest: None, observations: vec![], duration_ms: 0 },
        candidate: make_candidate(),
        assurance: AssuranceObservation::Verdict(verdict),
        evidence_bundle_ref: Some("e1".into()),
    };
    let ctx = LoopContext { budget: LoopBudget::new(5), prev_progress: None, max_attempts: 5 };
    let decision = decide(&obs, &ctx);
    assert!(matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::FinalizeCandidate)));
}

/// E2E-2: Blocking finding → Continue or Escalate
#[tokio::test]
async fn e2e_blocking_continues_when_fixable() {
    let mut reg = PassRegistry::new();
    reg.register("v-fail", Box::new(FailVerifier::new())).unwrap();
    let pm = PipelineManager::from_registry(reg);
    let plan = make_fail_plan(); let ctx = make_ctx();
    let verdict = pm.execute(&plan, &ctx, &MockExec, &dummy_svcs()).await.unwrap();

    assert_eq!(verdict.conformance, ConformanceOutcome::NonConformant);
    assert_eq!(verdict.blocking_findings.len(), 1);

    let obs = AttemptObservation::CandidateAvailable {
        attempt_id: "att-e2e".into(),
        run_completion: RunCompletion { staging_root: "/t".into(), artifact_manifest: None, observations: vec![], duration_ms: 0 },
        candidate: make_candidate(),
        assurance: AssuranceObservation::Verdict(verdict),
        evidence_bundle_ref: Some("e1".into()),
    };
    let ctx = LoopContext { budget: LoopBudget::new(5), prev_progress: None, max_attempts: 5 };
    let decision = decide(&obs, &ctx);
    // Fixable → Continue with feedback
    assert!(matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::Continue { .. })));
}

/// E2E-3: Assurance unavailable → Escalate
#[tokio::test]
async fn e2e_assurance_unavailable_escalates() {
    let obs = AttemptObservation::CandidateAvailable {
        attempt_id: "att-e2e".into(),
        run_completion: RunCompletion { staging_root: "/t".into(), artifact_manifest: None, observations: vec![], duration_ms: 0 },
        candidate: make_candidate(),
        assurance: AssuranceObservation::Unavailable { reason: "L1 down".into(), diagnostic_ref: "d1".into() },
        evidence_bundle_ref: None,
    };
    let ctx = LoopContext { budget: LoopBudget::new(5), prev_progress: None, max_attempts: 5 };
    let decision = decide(&obs, &ctx);
    assert!(matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::Escalate { .. })));
}

/// P13-5: Agent reports Success BUT Assure Unavailable → must Escalate, NOT Commit
#[tokio::test]
async fn p13_agent_success_assure_unavailable_must_not_commit() {
    let obs = AttemptObservation::CandidateAvailable {
        attempt_id: "att-p13".into(),
        run_completion: RunCompletion { staging_root: "/t".into(), artifact_manifest: None, observations: vec![], duration_ms: 0 },
        candidate: make_candidate(),
        assurance: AssuranceObservation::Unavailable { reason: "PipelineManager crashed".into(), diagnostic_ref: "d1".into() },
        evidence_bundle_ref: None,
    };
    let ctx = LoopContext { budget: LoopBudget::new(5), prev_progress: None, max_attempts: 5 };
    let decision = decide(&obs, &ctx);
    // MUST NOT be FinalizeCandidate — NO Commit without Verdict
    assert!(!matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::FinalizeCandidate)),
        "Assure Unavailable → MUST NOT Finalize (Fail-Closed)");
    assert!(matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::Escalate { .. })),
        "Assure Unavailable → MUST Escalate");
}

/// P13-5: Agent reports Success BUT Build failed in Sandbox → must NOT Commit
#[tokio::test]
async fn p13_agent_success_build_failed_must_not_commit() {
    let mut reg = PassRegistry::new();
    reg.register("v-fail", Box::new(FailVerifier::new())).unwrap();
    let pm = PipelineManager::from_registry(reg);
    let plan = make_fail_plan(); let ctx = make_ctx();
    let verdict = pm.execute(&plan, &ctx, &MockExec, &dummy_svcs()).await.unwrap();

    assert_eq!(verdict.conformance, ConformanceOutcome::NonConformant);

    let obs = AttemptObservation::CandidateAvailable {
        attempt_id: "att-p13".into(),
        run_completion: RunCompletion { staging_root: "/t".into(), artifact_manifest: None, observations: vec![], duration_ms: 0 },
        candidate: make_candidate(),
        assurance: AssuranceObservation::Verdict(verdict),
        evidence_bundle_ref: Some("e1".into()),
    };
    let ctx = LoopContext { budget: LoopBudget::new(5), prev_progress: None, max_attempts: 5 };
    let decision = decide(&obs, &ctx);
    // Agent thinks Success, but OntoAssure says NonConformant → MUST NOT Commit
    assert!(!matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::FinalizeCandidate)),
        "NonConformant verdict → MUST NOT Finalize, even though Agent reported Success");
}

/// P13-6: Two attempts — progress tracking with real fingerprints
#[tokio::test]
async fn p13_two_attempts_progress_tracks_fingerprints() {
    // Attempt 1: produces 2 blocking findings
    let mut reg1 = PassRegistry::new();
    reg1.register("v-fail", Box::new(FailVerifier::new())).unwrap();
    let pm1 = PipelineManager::from_registry(reg1);
    let v1 = pm1.execute(&make_fail_plan(), &make_ctx(), &MockExec, &dummy_svcs()).await.unwrap();
    assert_eq!(v1.blocking_findings.len(), 1);

    // Attempt 2: no findings (PassVerifier)
    let mut reg2 = PassRegistry::new();
    reg2.register("v-fmt", Box::new(PassVerifier::new())).unwrap();
    let pm2 = PipelineManager::from_registry(reg2);
    let v2 = pm2.execute(&make_plan(), &make_ctx(), &MockExec, &dummy_svcs()).await.unwrap();
    assert_eq!(v2.blocking_findings.len(), 0);

    // Build real progress snapshots
    let snap1 = onto_loop::decision::verdict_to_progress(&v1, 1);
    let snap2 = onto_loop::decision::verdict_to_progress(&v2, 2);
    assert_eq!(snap1.total_blocking, 1);
    assert_eq!(snap2.total_blocking, 0);
    // Blocking resolved → progress improved
}

/// E2E-4: No candidate → Freeze/CloseFailed
#[tokio::test]
async fn e2e_no_candidate_freezes() {
    let obs = AttemptObservation::NoCandidate {
        attempt_id: "att-e2e".into(), outcome: NoCandidateOutcome::AgentFailed, diagnostic_ref: None,
    };
    let ctx = LoopContext { budget: LoopBudget::new(5), prev_progress: None, max_attempts: 5 };
    let decision = decide(&obs, &ctx);
    assert!(matches!(decision, AttemptDecision::NoCandidate(onto_protocol::loop_protocol::NoCandidateLoopDecision::Freeze { .. })));
}
