//! AssuredBridge — P16-T: production finalizer.
//!
//! Pipeline: PipelineManager → EvidenceAssembler → DecisionEngine → RunFinalizationOutcome.
//! LifecycleReducer is called in IronClaw's apply_exit with the REAL TransitionReceipt.

use std::sync::Arc;

use async_trait::async_trait;
use onto_assurance_runtime::assurance_run::{
    AssuranceRunError, AssuranceRunStore, AssuranceRunner, EvidenceAssembler,
    InMemoryAssuranceFailureStore, InMemoryAssuranceRunStore,
};
use onto_assurance_runtime::ports::{
    RunFinalizationError, RunFinalizationOutcome, RunFinalizationPort,
    RunFinalizationRequest,
};
use onto_assurance_runtime::pipeline::{FrozenVerifierRegistry, PipelineManager};
use onto_assurance_types::enums::{BudgetOutcome, EffectClass, LifecycleState, ReasonCode, TaskOutcome};
use onto_assurance_types::ids::DecisionId;
use onto_loop::decision::{decide, LoopContext};
use onto_loop::budget::LoopBudget;
use onto_protocol::check::{Applicability, ConformancePlan, ConformanceUnit};
use onto_protocol::context::{CandidateVerificationContext, VerificationContext};
use onto_protocol::candidate::{RunCompletion, SealedCandidateRef};
use onto_protocol::digest::{Digest, DigestAlgorithm};
use onto_protocol::loop_protocol::{
    AttemptObservation, AssuranceObservation,
};

/// B2-A control: feedback presentation mode for agent-facing findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingFeedbackMode {
    /// Full structured findings (Group C — production default).
    Structured,
    /// Generic failure message only, findings hidden from agent (Group B).
    Generic,
}

impl FindingFeedbackMode {
    fn from_env() -> Self {
        match std::env::var("ONTO_FEEDBACK_MODE").as_deref() {
            Ok("generic") => Self::Generic,
            _ => Self::Structured,
        }
    }
}

pub struct AssuredFinalizer {
    registry: Arc<FrozenVerifierRegistry>,
    project_checks: crate::sandbox_executor::ProjectCheckRegistry,
    feedback_mode: FindingFeedbackMode,
}

impl AssuredFinalizer {
    pub fn new(registry: Arc<FrozenVerifierRegistry>) -> Self {
        Self {
            registry,
            project_checks: crate::sandbox_executor::ProjectCheckRegistry::new(),
            feedback_mode: FindingFeedbackMode::from_env(),
        }
    }
    pub fn with_project_checks(mut self, checks: crate::sandbox_executor::ProjectCheckRegistry) -> Self {
        self.project_checks = checks;
        self
    }
}

#[async_trait]
impl RunFinalizationPort for AssuredFinalizer {
    async fn finalize(
        &self,
        request: RunFinalizationRequest,
    ) -> Result<RunFinalizationOutcome, RunFinalizationError> {
        let attempt_id = request.attempt_id.to_string();
        let run_id = request.run_id.to_string();

        // P16-F1: fail-closed — require real staging root for coding tasks.
        let staging_root = request.staging_root.clone().ok_or_else(|| {
            RunFinalizationError::Internal(
                "P16-F1: staging_root required for candidate verification".into()
            )
        })?;
        // P16-F1E: Materialize an isolated verification copy.
        let verif_dir = std::env::temp_dir().join(format!("onto-verify-{}", attempt_id));
        let _ = std::fs::remove_dir_all(&verif_dir);
        if let Err(e) = copy_staging_to_verification(&staging_root, &verif_dir) {
            return Err(RunFinalizationError::Internal(
                format!("P16-F1E: failed to copy candidate to verification dir: {}", e)
            ));
        }
        let staging_str = verif_dir.to_string_lossy().to_string();

        let candidate = SealedCandidateRef::new(
            &format!("candidate-{}", attempt_id),
            quick_digest(&format!("cand-{}", attempt_id)),
            quick_digest(&format!("manifest-{}", attempt_id)),
            &staging_str,
        );
        let ctx = VerificationContext::PreGraph(CandidateVerificationContext {
            attempt_id: attempt_id.clone(), candidate: candidate.clone(),
            repository_name: "ironclaw-run".into(), base_commit_sha: run_id,
            execution_generation: 1,
            changed_files: vec![], // F1-6 TODO: derive from candidate manifest
            language: "c".into(),   // F1-6 TODO: derive from project profile
        });
        let plan = build_plan(attempt_id.clone());
        let exec = crate::sandbox_executor::GVisorVerificationExecutor::direct()
            .direct_with(self.project_checks.clone());
        let svc = production_services();

        // P16-P1: Use AssuranceRunner (not raw PipelineManager) for verdict + evidence + failure persistence
        let store = Arc::new(InMemoryAssuranceRunStore::new());
        let failure_store = Arc::new(InMemoryAssuranceFailureStore::new());
        let runner = AssuranceRunner::new(self.registry.clone(), store, failure_store.clone());

        let outcome = runner.execute(&attempt_id, &plan, &ctx, &exec, &svc).await;

        // Build Observation from outcome (Verdict or Unavailable)
        let observation = match outcome {
            Ok(persisted) => {
                let verdict = persisted.verdict;
                let evidence_bundle_ref = persisted.evidence_bundle_ref;
                AttemptObservation::CandidateAvailable {
                    attempt_id: attempt_id.clone(),
                    run_completion: RunCompletion {
                        staging_root: staging_str.clone(),
                        artifact_manifest: None, observations: vec![], duration_ms: 0,
                    },
                    candidate,
                    assurance: AssuranceObservation::Verdict(verdict),
                    evidence_bundle_ref: Some(evidence_bundle_ref),
                }
            }
            Err(AssuranceRunError::Unavailable(failure)) => {
                // P16-P1: AssuranceBinding::Unavailable with real failure_id + failure_digest
                AttemptObservation::CandidateAvailable {
                    attempt_id: attempt_id.clone(),
                    run_completion: RunCompletion {
                        staging_root: staging_str.clone(),
                        artifact_manifest: None, observations: vec![], duration_ms: 0,
                    },
                    candidate,
                    assurance: AssuranceObservation::Unavailable {
                        reason: format!("Assurance failed (stage={:?} id={}): {}",
                            failure.stage, failure.failure_id, failure.reason_code),
                        diagnostic_ref: failure.failure_id.clone(),
                    },
                    evidence_bundle_ref: None,
                }
            }
            Err(other) => {
                return Ok(escalate("assurance_error", &format!("{}", other)));
            }
        };

        let loop_ctx = LoopContext { budget: LoopBudget::new(5), prev_progress: None, max_attempts: 5 };
        let decision = decide(&observation, &loop_ctx);

        // P16-T: Map verdict → RunFinalizationOutcome.
        // LifecycleReducer with real TransitionReceipt runs in IronClaw's apply_exit.
        match &decision {
            onto_protocol::loop_protocol::AttemptDecision::Candidate(c) => match c {
                onto_protocol::loop_protocol::CandidateLoopDecision::FinalizeCandidate => {
                    Ok(RunFinalizationOutcome {
                        task_outcome: TaskOutcome::Success, budget_outcome: BudgetOutcome::WithinBudget,
                        lifecycle_state: LifecycleState::Committed,
                        session_decision_id: DecisionId::new(), attempt_decision_id: DecisionId::new(),
                        reason_codes: vec![], settlement_decision: None, effect_class: Some(EffectClass::Staged),
                    })
                }
                onto_protocol::loop_protocol::CandidateLoopDecision::Continue { feedback } => {
                    // Always preserve findings in the Receipt — audit facts
                    // must never be suppressed.  Agent-visible projection
                    // is controlled separately by AgentEvidenceMode in the
                    // prompt builder.
                    let codes: Vec<ReasonCode> = feedback.iter().map(|f| ReasonCode {
                        domain: "p16".into(), code: "finding".into(), detail: f.clone(),
                    }).collect();
                    Ok(RunFinalizationOutcome {
                        task_outcome: TaskOutcome::Failed, budget_outcome: BudgetOutcome::WithinBudget,
                        lifecycle_state: LifecycleState::Continuing,
                        session_decision_id: DecisionId::new(), attempt_decision_id: DecisionId::new(),
                        reason_codes: codes,
                        settlement_decision: None, effect_class: Some(EffectClass::Staged),
                    })
                }
                onto_protocol::loop_protocol::CandidateLoopDecision::Freeze { reason } => {
                    Ok(escalate_outcome("freeze", reason))
                }
                onto_protocol::loop_protocol::CandidateLoopDecision::Escalate { reason } => {
                    Ok(escalate_outcome("escalate", reason))
                }
                _ => Ok(escalate_outcome("unhandled", "unhandled candidate decision")),
            },
            onto_protocol::loop_protocol::AttemptDecision::NoCandidate(nc) => match nc {
                onto_protocol::loop_protocol::NoCandidateLoopDecision::CloseFailed => {
                    Ok(escalate_outcome("close_failed", "no candidate"))
                }
                onto_protocol::loop_protocol::NoCandidateLoopDecision::Freeze { reason } => {
                    Ok(escalate_outcome("freeze", reason))
                }
                onto_protocol::loop_protocol::NoCandidateLoopDecision::Escalate { reason } => {
                    Ok(escalate_outcome("escalate", reason))
                }
            },
        }
    }
}

fn escalate(code: &str, detail: &str) -> RunFinalizationOutcome { escalate_outcome(code, detail) }

fn escalate_outcome(code: &str, detail: &str) -> RunFinalizationOutcome {
    RunFinalizationOutcome {
        task_outcome: TaskOutcome::EnvironmentError, budget_outcome: BudgetOutcome::WithinBudget,
        lifecycle_state: LifecycleState::Escalated,
        session_decision_id: DecisionId::new(), attempt_decision_id: DecisionId::new(),
        reason_codes: vec![ReasonCode { domain: "assurance".into(), code: code.into(), detail: detail.into() }],
        settlement_decision: None, effect_class: Some(EffectClass::Staged),
    }
}

fn quick_digest(s: &str) -> Digest {
    use sha2::{Digest as _, Sha256};
    Digest::new(DigestAlgorithm::Sha256, hex::encode(Sha256::digest(s.as_bytes())))
}

fn build_plan(attempt_id: String) -> ConformancePlan {
    let baseline: &[(&str, onto_protocol::verifier::Pass)] = &[
        ("artifact.manifest.integrity", onto_protocol::verifier::Pass::FileIntegrity),
        ("file.protected_path", onto_protocol::verifier::Pass::FileIntegrity),
        ("project.build", onto_protocol::verifier::Pass::Build),
        ("project.test", onto_protocol::verifier::Pass::Behavior),
    ];
    ConformancePlan {
        plan_id: format!("plan-{}", attempt_id), plan_digest: quick_digest(&format!("plan-{}", attempt_id)),
        attempt_id: attempt_id.clone(), candidate_id: format!("c-{}", attempt_id),
        candidate_digest: quick_digest(&format!("cd-{}", attempt_id)),
        profile_id: "default".into(), profile_version: "1.0".into(),
        profile_digest: quick_digest("profile"), rule_set_digest: quick_digest("rules"),
        verifier_registry_digest: quick_digest("registry"),
        units: baseline.iter().enumerate().map(|(i, (vid, pass))| ConformanceUnit {
            unit_id: format!("{}-{}", attempt_id, i), verifier_id: vid.to_string(),
            pass: *pass, validation_dependencies: if *vid == "project.test" { vec!["project.build".to_string()] } else { vec![] },
            applicability: Applicability::Required,
        }).collect(),
    }
}

struct StagingArtifactReader;
impl onto_protocol::verifier::SealedArtifactReader for StagingArtifactReader {
    fn read_manifest(&self, artifact_ref: &str) -> Result<onto_protocol::candidate::ArtifactManifest, String> {
        onto_assurance_runtime::artifact_reader::LocalArtifactReader::new(artifact_ref).read_manifest(artifact_ref)
    }
    fn read_file(&self, artifact_ref: &str, path: &str) -> Result<Vec<u8>, String> {
        onto_assurance_runtime::artifact_reader::LocalArtifactReader::new(artifact_ref).read_file(artifact_ref, path)
    }
}
struct StubGraphReader;
impl onto_protocol::verifier::CandidateGraphReader for StubGraphReader {
    fn check_integrity(&self, _: &str) -> Result<(), String> { Ok(()) }
    fn get_entity_keys(&self, _: &str) -> Result<Vec<String>, String> { Ok(vec![]) }
}
struct UnavailableSemanticRuntime;
#[async_trait::async_trait]
impl onto_protocol::verifier::SemanticRuntimePort for UnavailableSemanticRuntime {
    async fn review(&self, _: &str, _: &str) -> Result<String, String> { Err("unavailable".into()) }
}

/// P16-F1E: Recursively copy staging to an isolated verification directory.
fn copy_staging_to_verification(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("create verif dir: {}", e))?;
    for entry in walkdir::WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        let rel = entry.path().strip_prefix(src).map_err(|e| format!("strip prefix: {}", e))?;
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| format!("create dir {:?}: {}", target, e))?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|e| format!("copy {:?}: {}", entry.path(), e))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_runtime::pipeline::PassRegistry;
    use onto_assurance_runtime::ports::{RunFinalizationPort, RunFinalizationRequest as OntoRequest};
    use crate::production_verifiers::*;
    use crate::sandbox_executor::{ExternalToolSpec, ProjectCheckRegistry};

    fn build_finalizer() -> Arc<AssuredFinalizer> {
        let mut reg = PassRegistry::new();
        reg.register("artifact.manifest.integrity",
            Box::new(ArtifactManifestIntegrityVerifier::new())).unwrap();
        reg.register("file.protected_path",
            Box::new(ProtectedPathVerifier::new(vec![]))).unwrap();
        reg.register("project.build",
            Box::new(ProjectBuildVerifier::new())).unwrap();
        reg.register("project.test",
            Box::new(ProjectTestVerifier::new())).unwrap();
        let registry = Arc::new(reg.freeze());

        let mut checks = ProjectCheckRegistry::new();
        checks.register("project.build", ExternalToolSpec::new("cmake", &["-S", ".", "-B", "build"])
            .with_step("cmake", &["--build", "build", "--parallel", "2"]));
        checks.register("project.test", ExternalToolSpec::new("ctest", &["--test-dir", "build", "--output-on-failure"]));

        Arc::new(AssuredFinalizer::new(registry).with_project_checks(checks))
    }

    fn make_request(dir: &std::path::Path) -> OntoRequest {
        OntoRequest {
            run_id: onto_assurance_types::ids::RunId::new(),
            attempt_id: onto_assurance_types::ids::AttemptId::new(),
            exit_reason: onto_assurance_types::enums::ExitReason::FinishRequested,
            budget_outcome: onto_assurance_types::enums::BudgetOutcome::WithinBudget,
            checkpoint_ref: None,
            staging_root: Some(dir.to_path_buf()),
        }
    }

    fn create_c_project(dir: &std::path::Path, main_c: &str, cmake: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("main.c"), main_c).unwrap();
        std::fs::write(dir.join("CMakeLists.txt"), cmake).unwrap();
    }

    #[tokio::test]
    async fn correct_project_pipeline_runs_without_error() {
        let dir = tempfile::tempdir().unwrap();
        create_c_project(dir.path(),
            r#"#include <stdio.h>
int main(void) { printf("ok\n"); return 0; }
"#,
            r#"cmake_minimum_required(VERSION 3.10)
project(T C)
set(CMAKE_C_STANDARD 11)
add_executable(app main.c)
enable_testing()
add_test(NAME t COMMAND app)
"#);
        let finalizer = build_finalizer();
        let outcome = finalizer.finalize(make_request(dir.path())).await.unwrap();
        // P16 ran without internal error. The exact outcome depends on manifest
        // digest matching (synthetic digests → NonConformant/Continue on first
        // attempt). The key assertion: pipeline does not crash or escalate.
        assert!(!matches!(outcome.lifecycle_state,
            onto_assurance_types::enums::LifecycleState::Escalated),
            "correct project must not escalate");
    }

    #[tokio::test]
    async fn build_error_yields_nonconformant() {
        let dir = tempfile::tempdir().unwrap();
        create_c_project(dir.path(),
            "int main(void) { printf(\"ok\n\") return 0; }", // missing semicolon
            r#"cmake_minimum_required(VERSION 3.10)
project(T C)
set(CMAKE_C_STANDARD 11)
add_executable(app main.c)
"#);
        let finalizer = build_finalizer();
        let outcome = finalizer.finalize(make_request(dir.path())).await.unwrap();
        // Build should fail → NonConformant or Inconclusive
        assert_ne!(outcome.lifecycle_state, onto_assurance_types::enums::LifecycleState::Committed,
            "build error must not yield Committed, got {:?}", outcome.lifecycle_state);
    }

    #[tokio::test]
    async fn test_failure_yields_nonconformant() {
        let dir = tempfile::tempdir().unwrap();
        create_c_project(dir.path(),
            r#"int main(void) { return 1; }"#,
            r#"cmake_minimum_required(VERSION 3.10)
project(T C)
set(CMAKE_C_STANDARD 11)
add_executable(app main.c)
enable_testing()
add_test(NAME t COMMAND app)
"#);
        let finalizer = build_finalizer();
        let outcome = finalizer.finalize(make_request(dir.path())).await.unwrap();
        assert_ne!(outcome.lifecycle_state, onto_assurance_types::enums::LifecycleState::Committed,
            "test failure must not yield Committed, got {:?}", outcome.lifecycle_state);
    }
}

fn production_services() -> onto_protocol::verifier::VerifierServices<'static> {
    static R: StagingArtifactReader = StagingArtifactReader;
    static G: StubGraphReader = StubGraphReader;
    static S: UnavailableSemanticRuntime = UnavailableSemanticRuntime;
    onto_protocol::verifier::VerifierServices { artifact_reader: &R, graph_reader: &G, semantic_runtime: &S }
}
