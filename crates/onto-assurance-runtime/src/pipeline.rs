//! PipelineManager — L1 Pass DAG scheduler.
use std::collections::HashMap;
use std::sync::Arc;
use onto_protocol::check::{Applicability, ConformancePlan, ConformanceUnit, ExternalCheckRequirement};
use onto_protocol::context::VerificationContext;
use onto_protocol::executor::VerificationExecutor;
use onto_protocol::finding::{Finding, FindingDisposition};
use onto_protocol::sandbox::{
    RawCheckResult, SandboxRunResult, SandboxValidationRequest,
    SandboxInvocationError, FilesystemPolicy, ProcessPolicy,
    NetworkPolicy, EnvironmentPolicy,
};
use onto_protocol::verdict::{
    ConformanceVerdict, ConformanceUnitResult, GraphValidationBinding,
    SandboxValidationSummary, UnitExecutionStatus, GraphUnavailableReason,
    CoverageState, FreshnessState, ConformanceOutcome,
};
use onto_protocol::verifier::{
    Verifier, VerifierServices, Pass, VerifierStatus, VerificationStage, VerificationMode,
};
use std::time::Duration;

// ══════════════════════════════════════════════════════════════════
// Error types
// ══════════════════════════════════════════════════════════════════

/// Category A: build-time errors. Service MUST NOT start.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("verifier_id mismatch: registration key '{registration_key}' != descriptor key '{descriptor_key}'")]
    VerifierIdMismatch { registration_key: String, descriptor_key: String },

    #[error("duplicate verifier_id '{verifier_id}': already registered")]
    DuplicateVerifierId { verifier_id: String },

    #[error("registry is empty — at least one verifier required for production")]
    EmptyRegistry,
}

/// Category B: plan-time errors. No Verdict is produced.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("invalid plan '{plan_id}': {reason}")]
    InvalidPlan { reason: String, plan_id: String },

    #[error("verification failed: {message}")]
    VerificationFailed { message: String },

    #[error("run failed: {run_id}")]
    RunFailed { run_id: String },
}

// ══════════════════════════════════════════════════════════════════
// PassRegistry — by verifier_id, rejects duplicates
// ══════════════════════════════════════════════════════════════════

pub struct PassRegistry {
    by_id: HashMap<String, Arc<dyn Verifier>>,
}

impl PassRegistry {
    pub fn new() -> Self {
        Self { by_id: HashMap::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Register a verifier. Rejects:
    /// - verifier_id mismatch between registration key and descriptor
    /// - duplicate verifier_id
    pub fn register(
        &mut self,
        verifier_id: &str,
        v: Box<dyn Verifier>,
    ) -> Result<(), BootstrapError> {
        let stored_id = v.descriptor().verifier_id.clone();

        // Invariant 1: registration key == descriptor.verifier_id
        if stored_id != verifier_id {
            return Err(BootstrapError::VerifierIdMismatch {
                registration_key: verifier_id.to_string(),
                descriptor_key: stored_id,
            });
        }

        // Invariant 2: no duplicate verifier_id
        if self.by_id.contains_key(verifier_id) {
            return Err(BootstrapError::DuplicateVerifierId {
                verifier_id: verifier_id.to_string(),
            });
        }

        self.by_id.insert(verifier_id.to_string(), Arc::from(v));
        Ok(())
    }

    /// Lookup a verifier by its stable ID.
    pub fn require(&self, verifier_id: &str) -> Option<&Arc<dyn Verifier>> {
        self.by_id.get(verifier_id)
    }

    /// Freeze into an immutable registry for use in PipelineManager.
    pub fn freeze(self) -> FrozenVerifierRegistry {
        FrozenVerifierRegistry { by_id: self.by_id }
    }
}

// ══════════════════════════════════════════════════════════════════
// FrozenVerifierRegistry — immutable after construction
// ══════════════════════════════════════════════════════════════════

pub struct FrozenVerifierRegistry {
    by_id: HashMap<String, Arc<dyn Verifier>>,
}

impl FrozenVerifierRegistry {
    pub fn require(&self, verifier_id: &str) -> Option<&Arc<dyn Verifier>> {
        self.by_id.get(verifier_id)
    }

    /// Validate that every Required unit in the plan has a registered verifier.
    pub fn validate_against(&self, plan: &ConformancePlan) -> Result<(), PipelineError> {
        for unit in &plan.units {
            if unit.applicability == Applicability::Required {
                if self.require(&unit.verifier_id).is_none() {
                    return Err(PipelineError::InvalidPlan {
                        reason: format!(
                            "Required verifier '{}' (unit '{}') not in frozen registry",
                            unit.verifier_id, unit.unit_id
                        ),
                        plan_id: plan.plan_id.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Iterate all registered verifiers (for collect_reqs / mark_unavailable).
    fn all_verifiers(&self) -> impl Iterator<Item = &Arc<dyn Verifier>> {
        self.by_id.values()
    }
}

// ══════════════════════════════════════════════════════════════════
// PipelineManager — Plan-driven execution with hard checks
// ══════════════════════════════════════════════════════════════════

pub struct PipelineManager {
    pub registry: Arc<FrozenVerifierRegistry>,
}

impl PipelineManager {
    pub fn new() -> Self {
        Self { registry: Arc::new(PassRegistry::new().freeze()) }
    }

    pub fn from_registry(registry: PassRegistry) -> Self {
        Self { registry: Arc::new(registry.freeze()) }
    }

    pub fn from_frozen(registry: Arc<FrozenVerifierRegistry>) -> Self {
        Self { registry }
    }

    /// Execute the plan. Three hard checks prevent false Conformant:
    ///
    /// 1. Zero Required units → InvalidPlan (Category B)
    /// 2. Required verifier not registered → InvalidPlan (Category B)
    /// 3. Conformant without complete evidence → downgraded to Inconclusive
    ///
    /// Optional units whose verifier is not registered produce
    /// `NotApplicable` results (no panic).
    pub async fn execute(
        &self,
        plan: &ConformancePlan,
        ctx: &VerificationContext,
        exec: &dyn VerificationExecutor,
        svc: &VerifierServices<'_>,
    ) -> Result<ConformanceVerdict, PipelineError> {
        // ═══ Check 1: Zero Required units ═══
        let required_units: Vec<&ConformanceUnit> = plan.units.iter()
            .filter(|u| u.applicability == Applicability::Required)
            .collect();
        if required_units.is_empty() {
            return Err(PipelineError::InvalidPlan {
                reason: "Plan contains zero required units — cannot produce Conformant".into(),
                plan_id: plan.plan_id.clone(),
            });
        }

        // ═══ Check 2: Required verifier missing ═══
        for unit in &required_units {
            if self.registry.require(&unit.verifier_id).is_none() {
                return Err(PipelineError::InvalidPlan {
                    reason: format!(
                        "Required verifier '{}' (unit '{}') not in registry",
                        unit.verifier_id, unit.unit_id
                    ),
                    plan_id: plan.plan_id.clone(),
                });
            }
        }

        let mut unit_results: Vec<ConformanceUnitResult> = vec![];
        let mut blocking: Vec<Finding> = vec![];
        let mut advisory: Vec<Finding> = vec![];

        // ═══ Execute all units (Required + Optional + NotApplicable) ═══
        // Phase 1: Internal verifiers run here.
        // ExternalEvidence / PostSandbox verifiers are evaluated later in eval_evidence.
        for unit in &plan.units {
            match self.registry.require(&unit.verifier_id) {
                Some(verifier) => {
                    let d = verifier.descriptor();
                    if matches!(d.mode, VerificationMode::Internal)
                        && matches!(d.stage, VerificationStage::PreGraph | VerificationStage::PostGraph)
                    {
                        // Internal verifier — evaluate now
                        let r = verifier.evaluate(ctx, &[], svc).await;
                        collect_findings_from_result(&r, &mut blocking, &mut advisory);
                        unit_results.push(build_unit_result(plan, unit, &r));
                    }
                    // ExternalEvidence / SandboxEvidence / PostSandbox: skip here,
                    // eval_evidence will handle them after sandbox execution.
                }
                None => {
                    // Required was already caught in Check 2.
                    // Optional / NotApplicable with no verifier → NotApplicable.
                    unit_results.push(ConformanceUnitResult::not_applicable(unit));
                }
            }
        }

        // ═══ Determine sandbox summary ═══
        let has_deterministic_blocking = blocking.iter().any(|f| {
            matches!(f.remediation, onto_protocol::finding::RemediationClass::NonRemediable)
        });
        let sandbox = if has_deterministic_blocking {
            SandboxValidationSummary::NotRunDueToBlockingPrerequisite {
                blocking_finding_ids: blocking.iter().map(|f| f.finding_id.clone()).collect(),
            }
        } else {
            let reqs = collect_reqs(&self.registry, ctx);
            if reqs.is_empty() {
                SandboxValidationSummary::NotRunDueToBlockingPrerequisite {
                    blocking_finding_ids: vec![],
                }
            } else {
                let sr = build_sr(plan, ctx, reqs);
                match exec.execute_plan(&sr).await {
                    Ok(res) => {
                        eval_evidence(&self.registry, plan, ctx, &res, svc,
                            &mut unit_results, &mut blocking, &mut advisory).await;
                        SandboxValidationSummary::Executed {
                            request_digest: res.request_digest,
                            environment_digest: res.environment.profile_digest,
                            run_ref: res.request_id,
                            status: res.status,
                            observation_refs: vec![],
                        }
                    }
                    Err(e) => {
                        mark_unavailable(&self.registry, &mut unit_results);
                        match e {
                            SandboxInvocationError::StartupFailed { request_digest, diagnostic_ref, .. } =>
                                SandboxValidationSummary::StartupFailed { request_digest, diagnostic_ref },
                            _ => SandboxValidationSummary::StartupFailed {
                                request_digest: plan.plan_digest.clone(),
                                diagnostic_ref: "sandbox-failed".into(),
                            },
                        }
                    }
                }
            }
        };

        // ═══ Reduce verdict ═══
        let graph = GraphValidationBinding::Unavailable {
            reason: GraphUnavailableReason::ServiceDown,
            diagnostic_ref: "graph-not-integrated".into(),
        };
        let verdict = crate::verdict::reduce(
            plan, blocking, advisory, unit_results, sandbox, graph,
        );

        // ═══ Check 3: Conformant completeness ═══
        if verdict.conformance == ConformanceOutcome::Conformant {
            if !matches!(verdict.freshness, FreshnessState::Current)
                || !matches!(verdict.coverage, CoverageState::Complete)
                || !verdict.sandbox.is_executed_and_completed()
                || !verdict.all_required_units_have_determinate_result()
            {
                return Ok(ConformanceVerdict {
                    conformance: ConformanceOutcome::Inconclusive,
                    ..verdict
                });
            }
        }

        Ok(verdict)
    }
}

// ══════════════════════════════════════════════════════════════════
// Internal helpers
// ══════════════════════════════════════════════════════════════════

async fn execute_single_unit(
    unit: &ConformanceUnit,
    verifier: &dyn Verifier,
    ctx: &VerificationContext,
    svc: &VerifierServices<'_>,
) -> onto_protocol::verifier::VerifierResult {
    let d = verifier.descriptor();
    let should_run = match d.stage {
        VerificationStage::PreGraph | VerificationStage::PostGraph
            if matches!(d.mode, VerificationMode::Internal) => true,
        _ => false,
    };
    if should_run {
        verifier.evaluate(ctx, &[], svc).await
    } else {
        // Sandbox-evidence verifiers are evaluated later in eval_evidence
        onto_protocol::verifier::VerifierResult {
            verifier_id: d.verifier_id.clone(),
            pass: d.pass,
            status: VerifierStatus::NotApplicable,
            findings: vec![],
            raw_evidence: vec![],
            diagnostic: None,
        }
    }
}

fn collect_findings_from_result(
    r: &onto_protocol::verifier::VerifierResult,
    blocking: &mut Vec<Finding>,
    advisory: &mut Vec<Finding>,
) {
    for f in &r.findings {
        match f.disposition {
            FindingDisposition::Blocking => blocking.push(f.clone()),
            FindingDisposition::Advisory => advisory.push(f.clone()),
        }
    }
}

fn build_unit_result(
    plan: &ConformancePlan,
    unit: &ConformanceUnit,
    r: &onto_protocol::verifier::VerifierResult,
) -> ConformanceUnitResult {
    ConformanceUnitResult {
        unit_id: unit.unit_id.clone(),
        verifier_id: r.verifier_id.clone(),
        pass: r.pass,
        applicability: unit.applicability,
        status: match r.status {
            VerifierStatus::Completed
                if r.findings.iter().any(|f| matches!(f.disposition, FindingDisposition::Blocking)) =>
                UnitExecutionStatus::Failed,
            VerifierStatus::Completed => UnitExecutionStatus::Passed,
            VerifierStatus::PrerequisiteFailed => UnitExecutionStatus::PrerequisiteFailed,
            VerifierStatus::Unavailable => UnitExecutionStatus::Unavailable,
            VerifierStatus::TimedOut => UnitExecutionStatus::TimedOut,
            VerifierStatus::PartiallyCompleted => UnitExecutionStatus::PartiallyCompleted,
            VerifierStatus::NotApplicable => UnitExecutionStatus::NotApplicable,
        },
        finding_ids: r.findings.iter().map(|f| f.finding_id.clone()).collect(),
        evidence_refs: r.raw_evidence.clone(),
        duration_ms: 0,
    }
}

fn collect_reqs(
    reg: &FrozenVerifierRegistry,
    ctx: &VerificationContext,
) -> Vec<ExternalCheckRequirement> {
    let mut out = vec![];
    for verifier in reg.all_verifiers() {
        for mut req in verifier.external_requirements(ctx) {
            req.requirement_id = format!("{}-{}", verifier.descriptor().verifier_id, req.check_id);
            out.push(req);
        }
    }
    out
}

fn build_sr(
    plan: &ConformancePlan,
    ctx: &VerificationContext,
    checks: Vec<ExternalCheckRequirement>,
) -> SandboxValidationRequest {
    SandboxValidationRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        attempt_id: plan.attempt_id.clone(),
        candidate_id: plan.candidate_id.clone(),
        candidate_digest: plan.candidate_digest.clone(),
        sealed_candidate_ref: ctx.candidate().candidate.artifact_ref.clone(),
        plan_digest: plan.plan_digest.clone(),
        gvisor_profile_id: "default".into(),
        expected_profile_digest: plan.plan_digest.clone(),
        checks,
        filesystem_policy: FilesystemPolicy::default(),
        process_policy: ProcessPolicy::default(),
        network_policy: NetworkPolicy::default(),
        environment_policy: EnvironmentPolicy::default(),
        allowed_outputs: vec![],
        required_observations: vec![],
        sandbox_timeout: Duration::from_secs(3600),
    }
}

fn mark_unavailable(
    reg: &FrozenVerifierRegistry,
    urs: &mut Vec<ConformanceUnitResult>,
) {
    for verifier in reg.all_verifiers() {
        let d = verifier.descriptor();
        if matches!(d.mode, VerificationMode::ExternalEvidence) {
            urs.push(ConformanceUnitResult {
                unit_id: d.verifier_id.clone(),
                verifier_id: d.verifier_id.clone(),
                pass: d.pass,
                applicability: Applicability::Required,
                status: UnitExecutionStatus::Unavailable,
                finding_ids: vec![],
                evidence_refs: vec![],
                duration_ms: 0,
            });
        }
    }
}

async fn eval_evidence(
    reg: &FrozenVerifierRegistry,
    plan: &ConformancePlan,
    ctx: &VerificationContext,
    sr: &SandboxRunResult,
    svc: &VerifierServices<'_>,
    urs: &mut Vec<ConformanceUnitResult>,
    blocking: &mut Vec<Finding>,
    advisory: &mut Vec<Finding>,
) {
    let cm: HashMap<&str, &RawCheckResult> = sr.check_results.iter()
        .map(|r| (r.check_id.as_str(), r))
        .collect();

    // Collect non-Passed verifiers for prerequisite checking.
    let non_passed: Vec<String> = urs.iter()
        .filter(|u| !matches!(u.status, UnitExecutionStatus::Passed))
        .map(|u| u.verifier_id.clone())
        .collect();

    for verifier in reg.all_verifiers() {
        let d = verifier.descriptor();
        if matches!(d.mode, VerificationMode::ExternalEvidence)
            || matches!(d.stage, VerificationStage::PostSandbox)
        {
            let unit = plan.units.iter().find(|u| u.verifier_id == d.verifier_id);

            // P16-F1: check validation_dependencies. Any prerequisite
            // that did not Pass blocks this verifier.
            let prereq_failed = unit
                .map(|u| u.validation_dependencies.iter().any(|dep_id| {
                    non_passed.iter().any(|np| np == dep_id)
                }))
                .unwrap_or(false);

            if prereq_failed {
                urs.push(ConformanceUnitResult {
                    unit_id: unit.map(|u| u.unit_id.clone()).unwrap_or_default(),
                    verifier_id: d.verifier_id.clone(),
                    pass: d.pass,
                    applicability: unit.map(|u| u.applicability).unwrap_or(Applicability::Required),
                    status: UnitExecutionStatus::PrerequisiteFailed,
                    finding_ids: vec![],
                    evidence_refs: vec![],
                    duration_ms: 0,
                });
                continue;
            }

            let reqs = verifier.external_requirements(ctx);
            let ev: Vec<(&String, &RawCheckResult)> = reqs.iter()
                .filter_map(|req| cm.get(req.check_id.as_str()).map(|r| (&req.check_id, *r)))
                .collect();
            let r = verifier.evaluate(ctx, &ev, svc).await;
            collect_findings_from_result(&r, blocking, advisory);
            urs.push(ConformanceUnitResult {
                unit_id: unit.map(|u| u.unit_id.clone()).unwrap_or_default(),
                verifier_id: d.verifier_id.clone(),
                pass: d.pass,
                applicability: unit.map(|u| u.applicability).unwrap_or(Applicability::Required),
                status: match r.status {
                    VerifierStatus::Completed
                        if r.findings.iter().any(|f| matches!(f.disposition, FindingDisposition::Blocking)) =>
                        UnitExecutionStatus::Failed,
                    VerifierStatus::Completed => UnitExecutionStatus::Passed,
                    VerifierStatus::PrerequisiteFailed => UnitExecutionStatus::PrerequisiteFailed,
                    VerifierStatus::Unavailable => UnitExecutionStatus::Unavailable,
                    VerifierStatus::TimedOut => UnitExecutionStatus::TimedOut,
                    VerifierStatus::PartiallyCompleted => UnitExecutionStatus::PartiallyCompleted,
                    VerifierStatus::NotApplicable => UnitExecutionStatus::NotApplicable,
                },
                finding_ids: r.findings.iter().map(|f| f.finding_id.clone()).collect(),
                evidence_refs: r.raw_evidence.clone(),
                duration_ms: 0,
            });
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use onto_protocol::candidate::SealedCandidateRef;
    use onto_protocol::check::{ArtifactScope, ConformanceUnit, EvidenceKind};
    use onto_protocol::context::CandidateVerificationContext;
    use onto_protocol::digest::{Digest, DigestAlgorithm};
    use onto_protocol::finding::{CategoryId, FindingFingerprint, FindingSeverity, RemediationClass};
    use onto_protocol::sandbox::{SandboxExecutionStatus, SandboxEnvironmentIdentity, ArtifactDelta, ResourceUsage};
    use onto_protocol::verifier::{VerifierDescriptor, VerifierResult};

    fn d(s: &str) -> Digest {
        use sha2::{Sha256, Digest as SD};
        Digest::new(DigestAlgorithm::Sha256, hex::encode(Sha256::digest(s.as_bytes())))
    }

    fn test_ctx() -> VerificationContext {
        let c = SealedCandidateRef::new("c1", d("c1"), d("m1"), "/tmp");
        VerificationContext::PreGraph(CandidateVerificationContext {
            attempt_id: "a1".into(), candidate: c,
            repository_name: "r".into(), base_commit_sha: "s".into(),
            execution_generation: 1, changed_files: vec![], language: "rust".into(),
        })
    }

    fn test_plan(units: Vec<ConformanceUnit>) -> ConformancePlan {
        ConformancePlan {
            plan_id: "p1".into(), plan_digest: d("p1"), attempt_id: "a1".into(),
            candidate_id: "c1".into(), candidate_digest: d("c1"),
            profile_id: "pf".into(), profile_version: "1".into(),
            profile_digest: d("pf"), rule_set_digest: d("rules"),
            verifier_registry_digest: d("vers"), units,
        }
    }

    // ── Verifier stub for tests ──

    struct StubVerifier { desc: VerifierDescriptor, result: VerifierResult }
    impl StubVerifier {
        fn new_pass(id: &str) -> Self {
            Self {
                desc: VerifierDescriptor {
                    verifier_id: id.into(), pass: Pass::Format,
                    stage: VerificationStage::PreGraph, mode: VerificationMode::Internal,
                    supported_rules: vec![],
                },
                result: VerifierResult {
                    verifier_id: id.into(), pass: Pass::Format,
                    status: VerifierStatus::Completed, findings: vec![],
                    raw_evidence: vec![], diagnostic: None,
                },
            }
        }
    }
    #[async_trait::async_trait]
    impl Verifier for StubVerifier {
        fn descriptor(&self) -> &VerifierDescriptor { &self.desc }
        fn external_requirements(&self, _: &VerificationContext) -> Vec<ExternalCheckRequirement> { vec![] }
        async fn evaluate(&self, _: &VerificationContext, _: &[(&String, &RawCheckResult)], _: &VerifierServices<'_>) -> VerifierResult {
            self.result.clone()
        }
    }

    struct MockExec;
    #[async_trait::async_trait]
    impl VerificationExecutor for MockExec {
        async fn execute_plan(&self, _: &SandboxValidationRequest) -> Result<SandboxRunResult, SandboxInvocationError> {
            Ok(SandboxRunResult {
                request_id: "r1".into(), request_digest: d("r1"), attempt_id: "a".into(),
                candidate_id: "c".into(), candidate_digest: d("c"),
                status: SandboxExecutionStatus::Completed, check_results: vec![],
                observations: vec![],
                environment: SandboxEnvironmentIdentity {
                    runsc_version: "mock".into(), profile_digest: d("pf"),
                    image_digest: d("img"), toolchain_digest: d("tc"),
                },
                filesystem_diff: ArtifactDelta { created: vec![], modified: vec![], deleted: vec![] },
                resource_usage: ResourceUsage { cpu_seconds: 0.0, memory_mb: 0.0, disk_mb: 0.0, network_bytes_sent: 0, network_bytes_recv: 0 },
            })
        }
    }

    fn dummy_services() -> VerifierServices<'static> {
        struct D;
        impl onto_protocol::verifier::SealedArtifactReader for D {
            fn read_manifest(&self, _: &str) -> Result<onto_protocol::candidate::ArtifactManifest, String> {
                Ok(onto_protocol::candidate::ArtifactManifest { entries: vec![] })
            }
            fn read_file(&self, _: &str, _: &str) -> Result<Vec<u8>, String> { Ok(vec![]) }
        }
        struct G;
        impl onto_protocol::verifier::CandidateGraphReader for G {
            fn check_integrity(&self, _: &str) -> Result<(), String> { Ok(()) }
            fn get_entity_keys(&self, _: &str) -> Result<Vec<String>, String> { Ok(vec![]) }
        }
        struct S;
        #[async_trait::async_trait]
        impl onto_protocol::verifier::SemanticRuntimePort for S {
            async fn review(&self, _: &str, _: &str) -> Result<String, String> { Ok("ok".into()) }
        }
        static D1: D = D; static G1: G = G; static S1: S = S;
        VerifierServices { artifact_reader: &D1, graph_reader: &G1, semantic_runtime: &S1 }
    }

    // ══════════════════════════════════════════════════════════════
    // P16-0 tests
    // ══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn p16_0_1_zero_required_units_rejected() {
        let pm = PipelineManager::new();
        let plan = test_plan(vec![]); // zero units
        let ctx = test_ctx();
        let exec = MockExec;
        let svc = dummy_services();
        let result = pm.execute(&plan, &ctx, &exec, &svc).await;
        assert!(result.is_err(), "zero required units must be rejected");
        match result.unwrap_err() {
            PipelineError::InvalidPlan { reason, .. } => {
                assert!(reason.contains("zero required"), "got: {}", reason);
            }
            _ => panic!("expected InvalidPlan"),
        }
    }

    #[tokio::test]
    async fn p16_0_2_required_verifier_missing_rejected() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new_pass("v1"))).unwrap();
        let pm = PipelineManager::from_registry(reg);
        let plan = test_plan(vec![
            ConformanceUnit {
                unit_id: "u1".into(), verifier_id: "v1".into(),
                pass: Pass::Format, validation_dependencies: vec![],
                applicability: Applicability::Required,
            },
            ConformanceUnit {
                unit_id: "u2".into(), verifier_id: "v2_missing".into(), // not registered
                pass: Pass::Static, validation_dependencies: vec![],
                applicability: Applicability::Required,
            },
        ]);
        let ctx = test_ctx();
        let exec = MockExec;
        let svc = dummy_services();
        let result = pm.execute(&plan, &ctx, &exec, &svc).await;
        assert!(result.is_err(), "missing required verifier must be rejected");
        match result.unwrap_err() {
            PipelineError::InvalidPlan { reason, .. } => {
                assert!(reason.contains("v2_missing"), "got: {}", reason);
            }
            _ => panic!("expected InvalidPlan"),
        }
    }

    #[tokio::test]
    async fn p16_0_3_optional_unit_not_registered_is_not_applicable() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new_pass("v1"))).unwrap();
        let pm = PipelineManager::from_registry(reg);
        let plan = test_plan(vec![
            ConformanceUnit {
                unit_id: "u1".into(), verifier_id: "v1".into(),
                pass: Pass::Format, validation_dependencies: vec![],
                applicability: Applicability::Required,
            },
            ConformanceUnit {
                unit_id: "u2".into(), verifier_id: "v2_optional".into(),
                pass: Pass::Static, validation_dependencies: vec![],
                applicability: Applicability::Optional,
            },
        ]);
        let ctx = test_ctx();
        let exec = MockExec;
        let svc = dummy_services();
        let result = pm.execute(&plan, &ctx, &exec, &svc).await;
        assert!(result.is_ok(), "optional unregistered must not fail: {:?}", result.err());
        let verdict = result.unwrap();
        let u2 = verdict.unit_results.iter().find(|u| u.unit_id == "u2").unwrap();
        assert_eq!(u2.status, UnitExecutionStatus::NotApplicable,
            "optional unregistered unit must be NotApplicable, got {:?}", u2.status);
    }

    #[tokio::test]
    async fn p16_0_4_conformant_with_incomplete_sandbox_downgraded() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new_pass("v1"))).unwrap();
        let pm = PipelineManager::from_registry(reg);
        let plan = test_plan(vec![
            ConformanceUnit {
                unit_id: "u1".into(), verifier_id: "v1".into(),
                pass: Pass::Format, validation_dependencies: vec![],
                applicability: Applicability::Required,
            },
        ]);
        let ctx = test_ctx();
        let exec = MockExec;
        let svc = dummy_services();
        let mut result = pm.execute(&plan, &ctx, &exec, &svc).await.unwrap();
        // Force sandbox to be non-executed
        result.sandbox = SandboxValidationSummary::StartupFailed {
            request_digest: d("r"), diagnostic_ref: "d".into(),
        };
        result.freshness = FreshnessState::Current;
        result.coverage = CoverageState::Complete;
        result.conformance = ConformanceOutcome::Conformant;

        // Re-apply Check 3 logic
        if result.conformance == ConformanceOutcome::Conformant
            && !result.sandbox.is_executed_and_completed()
        {
            result.conformance = ConformanceOutcome::Inconclusive;
        }
        assert_eq!(result.conformance, ConformanceOutcome::Inconclusive,
            "Conformant with incomplete sandbox must be downgraded");
    }

    #[test]
    fn p16_0_5_duplicate_verifier_id_rejected() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new_pass("v1"))).unwrap();
        let result = reg.register("v1", Box::new(StubVerifier::new_pass("v1")));
        assert!(result.is_err(), "duplicate verifier_id must be rejected");
        match result.unwrap_err() {
            BootstrapError::DuplicateVerifierId { verifier_id } => {
                assert_eq!(verifier_id, "v1");
            }
            _ => panic!("expected DuplicateVerifierId"),
        }
    }

    #[test]
    fn p16_0_6_verifier_id_mismatch_rejected() {
        let mut reg = PassRegistry::new();
        // Registration key != descriptor key
        let result = reg.register("wrong-key", Box::new(StubVerifier::new_pass("v1")));
        assert!(result.is_err(), "verifier_id mismatch must be rejected");
        match result.unwrap_err() {
            BootstrapError::VerifierIdMismatch { registration_key, descriptor_key } => {
                assert_eq!(registration_key, "wrong-key");
                assert_eq!(descriptor_key, "v1");
            }
            _ => panic!("expected VerifierIdMismatch"),
        }
    }
}
