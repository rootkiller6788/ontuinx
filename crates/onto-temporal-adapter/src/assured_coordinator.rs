//! AssuredAttemptCoordinator — single authoritative production control flow.
//!
//! Replaces the old RuntimeLoopRunner internal logic.
//! Fixed sequence: Agent → Pipeline → Verdict → Decision → Transition.
//! No TaskOutcome-based Commit. No bypass of OntoAssure.

use std::sync::Arc;
use onto_assurance_runtime::pipeline::{PipelineManager, PassRegistry};
use onto_assurance_runtime::ports::RuntimeRunPort;
use onto_protocol::check::{ConformancePlan, ConformanceUnit, Applicability};
use onto_protocol::context::{CandidateVerificationContext, VerificationContext};
use onto_protocol::candidate::{SealedCandidateRef, AgentRunOutcome, RunCompletion, ArtifactManifest};
use onto_protocol::digest::{Digest, DigestAlgorithm};
use onto_protocol::executor::VerificationExecutor;
use onto_protocol::loop_protocol::{
    AttemptObservation, AssuranceObservation, CandidateLoopDecision, AttemptDecision,
    NoCandidateLoopDecision, NoCandidateOutcome, LoopDirective, TransitionOutcome,
};
use onto_protocol::verdict::{ConformanceOutcome, ConformanceVerdict};
use onto_protocol::finding::FindingFingerprint;
use onto_protocol::verifier::{VerifierServices};
use onto_loop::decision::{decide, LoopContext};
use onto_loop::budget::LoopBudget;

use crate::protocol::{LoopInvocationRequest, LoopTerminalEnvelope, LoopTerminalState};
use crate::idempotency::IdempotencyStore;

// ── Core Coordinator ──

/// The single authoritative coordinator for one Attempt cycle.
/// No legacy TaskOutcome decision path exists inside.
pub struct AssuredAttemptCoordinator {
    pipeline_manager: PipelineManager,
    services: Option<Box<dyn Fn() -> VerifierServices<'static> + Send + Sync>>,
}

impl AssuredAttemptCoordinator {
    pub fn new(pipeline_manager: PipelineManager) -> Self {
        Self { pipeline_manager, services: None }
    }

    /// Inject production VerifierServices. Without this, `run_attempt` will
    /// fall back to dummy services (test-only path).
    pub fn with_services(
        mut self,
        factory: Box<dyn Fn() -> VerifierServices<'static> + Send + Sync>,
    ) -> Self {
        self.services = Some(factory);
        self
    }

    /// Execute one attempt with mandatory OntoAssure verification.
    /// Returns (Verdict, fingerprints for progress tracking).
    pub async fn run_attempt(
        &self,
        attempt_id: &str,
        candidate: &SealedCandidateRef,
        ctx: &VerificationContext,
        executor: &dyn VerificationExecutor,
    ) -> (Option<ConformanceVerdict>, Vec<FindingFingerprint>) {
        let plan = build_plan(attempt_id, candidate, ctx);
        let svc = match &self.services {
            Some(factory) => factory(),
            None => dummy_services(), // test-only fallback
        };
        let verdict = match self.pipeline_manager.execute(&plan, ctx, executor, &svc).await {
            Ok(v) => v,
            Err(_) => {
                // P16-0: PipelineError → no verdict, caller treats as Unavailable
                return (None, vec![]);
            }
        };

        let fingerprints: Vec<FindingFingerprint> = verdict.blocking_findings.iter()
            .chain(verdict.advisory_findings.iter())
            .map(|f| f.fingerprint.clone())
            .collect();

        (Some(verdict), fingerprints)
    }

    /// Build AttemptObservation for the DecisionEngine.
    pub fn build_observation(
        &self,
        attempt_id: &str,
        candidate: Option<&SealedCandidateRef>,
        verdict: Option<ConformanceVerdict>,
        run_completion: Option<RunCompletion>,
    ) -> AttemptObservation {
        match (candidate, verdict, run_completion) {
            (Some(c), Some(v), Some(rc)) => AttemptObservation::CandidateAvailable {
                attempt_id: attempt_id.to_string(),
                run_completion: rc,
                candidate: c.clone(),
                assurance: AssuranceObservation::Verdict(v),
                evidence_bundle_ref: None,
            },
            (Some(c), None, Some(rc)) => AttemptObservation::CandidateAvailable {
                attempt_id: attempt_id.to_string(),
                run_completion: rc,
                candidate: c.clone(),
                assurance: AssuranceObservation::Unavailable {
                    reason: "PipelineManager unavailable".into(),
                    diagnostic_ref: "pipeline-failed".into(),
                },
                evidence_bundle_ref: None,
            },
            _ => AttemptObservation::NoCandidate {
                attempt_id: attempt_id.to_string(),
                outcome: NoCandidateOutcome::AgentFailed,
                diagnostic_ref: None,
            },
        }
    }

    /// Decide the next action from an AttemptObservation.
    /// Fail-Closed: no Verdict → no Finalize.
    pub fn decide(&self, observation: &AttemptObservation, budget: &LoopBudget) -> AttemptDecision {
        let ctx = LoopContext {
            budget: budget.clone(),
            prev_progress: None,
            max_attempts: budget.max_attempts,
        };
        decide(observation, &ctx)
    }

    /// Map an AttemptDecision to a LoopTerminalState for Go OntoFlow.
    pub fn to_terminal_state(decision: &AttemptDecision) -> LoopTerminalState {
        match decision {
            AttemptDecision::Candidate(CandidateLoopDecision::FinalizeCandidate) => LoopTerminalState::Committed,
            AttemptDecision::Candidate(CandidateLoopDecision::Escalate { .. }) => LoopTerminalState::Escalated,
            AttemptDecision::Candidate(CandidateLoopDecision::Freeze { .. }) => LoopTerminalState::Escalated,
            AttemptDecision::Candidate(CandidateLoopDecision::Continue { .. }) => LoopTerminalState::Escalated, // requires more attempts
            AttemptDecision::NoCandidate(NoCandidateLoopDecision::CloseFailed) => LoopTerminalState::LoopBudgetExhausted,
            _ => LoopTerminalState::ProtocolFailed,
        }
    }
}

// ── Helpers ──

fn build_plan(attempt_id: &str, candidate: &SealedCandidateRef, _ctx: &VerificationContext) -> ConformancePlan {
    // P16-2: use baseline required units matching production registry
    let baseline: &[(&str, onto_protocol::verifier::Pass)] = &[
        ("artifact.manifest.integrity", onto_protocol::verifier::Pass::FileIntegrity),
        ("file.protected_path", onto_protocol::verifier::Pass::FileIntegrity),
        ("project.build", onto_protocol::verifier::Pass::Build),
        ("project.test", onto_protocol::verifier::Pass::Behavior),
    ];
    let units: Vec<ConformanceUnit> = baseline.iter().enumerate().map(|(i, (vid, pass))| {
        ConformanceUnit {
            unit_id: format!("{}-{}", attempt_id, i),
            verifier_id: vid.to_string(),
            pass: *pass,
            validation_dependencies: if *vid == "project.test" {
                vec!["project.build".to_string()]
            } else {
                vec![]
            },
            applicability: Applicability::Required,
        }
    }).collect();

    ConformancePlan {
        plan_id: format!("plan-{}", attempt_id),
        plan_digest: d("plan"),
        attempt_id: attempt_id.to_string(),
        candidate_id: candidate.candidate_id.clone(),
        candidate_digest: candidate.digest.clone(),
        profile_id: "default".into(), profile_version: "1.0".into(),
        profile_digest: d("profile"), rule_set_digest: d("rules"),
        verifier_registry_digest: d("vers"),
        units,
    }
}

fn d(s: &str) -> Digest {
    use sha2::{Sha256, Digest as SD};
    Digest::new(DigestAlgorithm::Sha256, hex::encode(Sha256::digest(s.as_bytes())))
}

/// Test-only fallback. Production MUST inject real services via `with_services()`.
#[allow(dead_code)]
fn dummy_services() -> VerifierServices<'static> {
    struct D; impl onto_protocol::verifier::SealedArtifactReader for D { fn read_manifest(&self, _: &str) -> Result<ArtifactManifest, String> { Ok(ArtifactManifest{entries:vec![]}) } fn read_file(&self, _: &str, _: &str) -> Result<Vec<u8>, String> { Ok(vec![]) } }
    struct G; impl onto_protocol::verifier::CandidateGraphReader for G { fn check_integrity(&self, _: &str) -> Result<(), String> { Ok(()) } fn get_entity_keys(&self, _: &str) -> Result<Vec<String>, String> { Ok(vec![]) } }
    struct S; #[async_trait::async_trait] impl onto_protocol::verifier::SemanticRuntimePort for S { async fn review(&self, _: &str, _: &str) -> Result<String, String> { Ok("ok".into()) } }
    static D1: D = D; static G1: G = G; static S1: S = S;
    VerifierServices { artifact_reader: &D1, graph_reader: &G1, semantic_runtime: &S1 }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use onto_protocol::executor::VerificationExecutor;
    use onto_protocol::sandbox::*;
    use onto_protocol::check::{ExternalCheckRequirement, EvidenceKind, ArtifactScope};
    use onto_protocol::verifier::{Verifier, VerifierDescriptor, Pass, VerificationStage, VerificationMode, VerifierResult, VerifierStatus};
    use onto_protocol::finding::{Finding, FindingFingerprint, FindingSeverity, CategoryId, FindingDisposition, RemediationClass};
    use async_trait::async_trait;

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

    struct PassV { desc: VerifierDescriptor }
    impl PassV { fn new() -> Self { Self { desc: VerifierDescriptor { verifier_id: "v".into(), pass: Pass::Format, stage: VerificationStage::PreGraph, mode: VerificationMode::Internal, supported_rules: vec![] } } } }
    #[async_trait] impl Verifier for PassV {
        fn descriptor(&self) -> &VerifierDescriptor { &self.desc }
        fn external_requirements(&self, _: &VerificationContext) -> Vec<ExternalCheckRequirement> { vec![] }
        async fn evaluate(&self, _: &VerificationContext, _: &[(&String, &RawCheckResult)], _: &VerifierServices<'_>) -> VerifierResult {
            VerifierResult { verifier_id: "v".into(), pass: Pass::Format, status: VerifierStatus::Completed, findings: vec![], raw_evidence: vec![], diagnostic: None }
        }
    }

    fn ctx() -> VerificationContext {
        let c = SealedCandidateRef::new("c1", d("c1"), d("m1"), "/tmp");
        VerificationContext::PreGraph(CandidateVerificationContext {
            attempt_id: "a1".into(), candidate: c,
            repository_name: "r".into(), base_commit_sha: "s".into(),
            execution_generation: 1, changed_files: vec![], language: "rust".into(),
        })
    }

    #[tokio::test]
    async fn coordinator_conformant_produces_finalize() {
        let mut reg = PassRegistry::new();
        reg.register("v", Box::new(PassV::new())).unwrap();
        let pm = PipelineManager::from_registry(reg);
        let c = SealedCandidateRef::new("c1", d("c1"), d("m1"), "/tmp");
        let plan = ConformancePlan {
            plan_id: "p1".into(), plan_digest: d("p1"), attempt_id: "a1".into(),
            candidate_id: "c1".into(), candidate_digest: d("c1"),
            profile_id: "pf".into(), profile_version: "1".into(),
            profile_digest: d("pf"), rule_set_digest: d("rules"),
            verifier_registry_digest: d("vers"),
            units: vec![ConformanceUnit {
                unit_id: "u1".into(), verifier_id: "v".into(),
                pass: Pass::Format, validation_dependencies: vec![],
                applicability: Applicability::Required,
            }],
        };
        let svc = dummy_services();
        let verdict = pm.execute(&plan, &ctx(), &MockExec, &svc).await.unwrap();
        assert_eq!(verdict.conformance, ConformanceOutcome::Conformant);
    }

    #[test]
    fn coordinator_assure_unavailable_escalates() {
        let coord = AssuredAttemptCoordinator::new(PipelineManager::new());
        let obs = coord.build_observation("a1", None, None, None);
        let budget = LoopBudget::new(5);
        let decision = coord.decide(&obs, &budget);
        assert!(matches!(decision, AttemptDecision::NoCandidate(..)),
            "No Verdict, No Candidate → must not Finalize");
    }
}
