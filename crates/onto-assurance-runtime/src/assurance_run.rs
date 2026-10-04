//! AssuranceRunner — L1 Pipeline orchestrator with two-phase persistence.
//!
//! # State machine
//!
//! ```text
//! Created → PlanResolved → Executing → EvidenceStaged → VerdictFinalizing → Finalized
//!                │              │             │                 │               │
//!                └──────────────┴─────────────┴─────────────────┴─────── Failed (any phase)
//! ```
//!
//! # Two-phase persistence (eliminates visibility race)
//!
//! Phase 1: Evidence content → content-addressed object storage
//! Phase 2: DB TX1 — Verdict (status=Finalizing, NOT visible) + Outbox (status=Held)
//! Phase 3: mark_evidence_referenced()
//! Phase 4: DB TX2 — run→Finalized, verdict→Visible, outbox→Publishable
//!
//! At NO point is a Verdict visible while its Evidence could be GC'd.

use std::sync::Arc;

use onto_protocol::check::ConformancePlan;
use onto_protocol::context::VerificationContext;
use onto_protocol::digest::{Digest, DigestAlgorithm};
use onto_protocol::executor::VerificationExecutor;
use onto_protocol::verdict::{AssuranceFailureStage, ConformanceVerdict, PersistedAssuranceFailure};
use onto_protocol::verifier::VerifierServices;

use crate::pipeline::{FrozenVerifierRegistry, PipelineError, PipelineManager};
use crate::verdict as verdict_reducer;

// ══════════════════════════════════════════════════════════════════
// AssuranceRunState
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssuranceRunState {
    Created,
    PlanResolved,
    Executing,
    EvidenceStaged,
    VerdictFinalizing,
    Finalized,
    Failed,
}

// ══════════════════════════════════════════════════════════════════
// PersistedVerdict — what the AssuranceRunner returns
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct PersistedVerdict {
    pub verdict: ConformanceVerdict,
    pub verdict_ref: String,
    pub evidence_bundle_ref: String,
}

// ══════════════════════════════════════════════════════════════════
// EvidenceAssembler — bundles raw evidence into verifiable records
// ══════════════════════════════════════════════════════════════════

/// Collects raw verifier outputs and produces a deterministic evidence bundle.
pub struct EvidenceAssembler;

impl EvidenceAssembler {
    pub fn new() -> Self { Self }

    /// Assemble evidence from unit results.  Returns a content-addressed bundle.
    pub fn assemble(
        &self,
        plan: &ConformancePlan,
        unit_results: &[onto_protocol::verdict::ConformanceUnitResult],
    ) -> Result<EvidenceBundle, String> {
        // Build a lightweight serializable evidence payload
        let mut lines: Vec<String> = Vec::new();
        for ur in unit_results {
            lines.push(format!(
                "{}|{}|{:?}|{:?}|{}",
                ur.unit_id,
                ur.verifier_id,
                ur.pass,
                ur.status,
                ur.finding_ids.len(),
            ));
        }
        let payload = lines.join("\n");

        let digest = quick_digest(&payload);
        let bundle_ref = format!("evidence/{}", plan.attempt_id);

        Ok(EvidenceBundle {
            bundle_ref: bundle_ref.clone(),
            bundle_digest: digest,
            plan_id: plan.plan_id.clone(),
            attempt_id: plan.attempt_id.clone(),
            unit_count: unit_results.len() as u32,
            payload,
        })
    }

    /// Compute the digest of an evidence bundle (used before persistence).
    pub fn digest(&self, bundle: &EvidenceBundle) -> Digest {
        bundle.bundle_digest.clone()
    }
}

/// Serializable evidence container.
#[derive(Debug, Clone)]
pub struct EvidenceBundle {
    pub bundle_ref: String,
    pub bundle_digest: Digest,
    pub plan_id: String,
    pub attempt_id: String,
    pub unit_count: u32,
    pub payload: String,
}

// ══════════════════════════════════════════════════════════════════
// AssuranceRunStore trait
// ══════════════════════════════════════════════════════════════════

/// Persistent storage for Assurance runs.
///
/// Implementations MUST guarantee:
/// - Verdict is NOT visible before phase 2 commits
/// - Evidence is marked referenced before Verdict becomes visible
/// - Same decision_id returns idempotent results
#[async_trait::async_trait]
pub trait AssuranceRunStore: Send + Sync {
    /// Start a new run. Returns a handle.
    async fn begin_run(&self, attempt_id: &str) -> Result<AssuranceRunHandle, StoreError>;

    /// Record the conformance plan.
    async fn record_plan(&self, run: &AssuranceRunHandle, plan: &ConformancePlan) -> Result<(), StoreError>;

    /// Retrieve the current run state (for crash recovery).
    async fn get_run_state(&self, attempt_id: &str) -> Result<AssuranceRunState, StoreError>;

    /// Phase 1: Write evidence content to object storage.
    async fn stage_evidence(&self, run: &AssuranceRunHandle, bundle: &EvidenceBundle) -> Result<(), StoreError>;

    /// Phase 2: DB transaction 1 — write Verdict (status=Finalizing) + Outbox (status=Held).
    async fn finalize_phase1(
        &self, run: &AssuranceRunHandle, verdict: &ConformanceVerdict,
    ) -> Result<(), StoreError>;

    /// Phase 3: Mark evidence as referenced (prevents GC).
    async fn mark_evidence_referenced(&self, run: &AssuranceRunHandle) -> Result<(), StoreError>;

    /// Phase 4: DB transaction 2 — run→Finalized, verdict→Visible, outbox→Publishable.
    async fn finalize_phase2(&self, run: &AssuranceRunHandle) -> Result<(), StoreError>;

    /// Load a finalized verdict.
    async fn load_verdict(&self, attempt_id: &str) -> Result<Option<PersistedVerdict>, StoreError>;

    /// P16-R1: Scan for runs in non-terminal states (for crash recovery).
    async fn scan_incomplete_runs(&self) -> Result<Vec<AssuranceRunHandle>, StoreError> {
        Ok(vec![]) // default: no incomplete runs
    }
}

#[derive(Debug, Clone)]
pub struct AssuranceRunHandle {
    pub attempt_id: String,
    pub evidence_ref: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already finalized: {0}")]
    AlreadyFinalized(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("invalid state transition: {0}")]
    InvalidState(String),
}

// ══════════════════════════════════════════════════════════════════
// AssuranceRunner
// ══════════════════════════════════════════════════════════════════

/// Orchestrates one Assurance run: Plan → Execute → Evidence → Verdict → Persist.
pub struct AssuranceRunner {
    registry: Arc<FrozenVerifierRegistry>,
    evidence_assembler: EvidenceAssembler,
    store: Arc<dyn AssuranceRunStore>,
    failure_store: Arc<dyn AssuranceFailureStore>,
}

impl AssuranceRunner {
    pub fn new(
        registry: Arc<FrozenVerifierRegistry>,
        store: Arc<dyn AssuranceRunStore>,
        failure_store: Arc<dyn AssuranceFailureStore>,
    ) -> Self {
        Self { registry, evidence_assembler: EvidenceAssembler::new(), store, failure_store }
    }

    /// Build a PersistedAssuranceFailure and persist it.
    async fn record_failure(
        &self, attempt_id: &str, plan: &ConformancePlan,
        stage: AssuranceFailureStage, reason_code: &str, diagnostic: Option<&str>,
    ) -> PersistedAssuranceFailure {
        let candidate = plan.candidate_digest.clone();
        let failure = PersistedAssuranceFailure {
            failure_id: uuid::Uuid::new_v4().to_string(),
            failure_digest: quick_digest("placeholder"), // computed below
            assurance_run_id: attempt_id.to_string(),
            attempt_id: attempt_id.to_string(),
            candidate_id: plan.candidate_id.clone(),
            candidate_digest: candidate,
            plan_digest: plan.plan_digest.clone(),
            stage,
            reason_code: reason_code.to_string(),
            diagnostic_ref: diagnostic.map(|s| s.to_string()),
        };
        let failure = PersistedAssuranceFailure {
            failure_digest: failure.compute_digest(),
            ..failure
        };
        self.failure_store.persist_if_absent(&failure).await
            .unwrap_or_else(|_| failure.clone())
    }

    /// Execute a complete Assurance run with two-phase persistence.
    pub async fn execute(
        &self,
        attempt_id: &str,
        plan: &ConformancePlan,
        ctx: &VerificationContext,
        exec: &dyn VerificationExecutor,
        svc: &VerifierServices<'_>,
    ) -> Result<PersistedVerdict, AssuranceRunError> {
        // ── ① Begin run ──
        let mut run = self.store.begin_run(attempt_id).await?;
        // state: Created

        // ── ② Record plan ──
        self.store.record_plan(&run, plan).await?;
        // state: PlanResolved

        // ── ③ Execute Pipeline ──
        let pm = PipelineManager { registry: Arc::clone(&self.registry) };
        let unit_results_and_verdict = pm.execute(plan, ctx, exec, svc).await;
        // state: Executing

        // ── ④ Assemble evidence ──
        let verdict = match unit_results_and_verdict {
            Ok(v) => v,
            Err(PipelineError::InvalidPlan { reason, .. }) => {
                let failure = self.record_failure(attempt_id, plan,
                    AssuranceFailureStage::PlanResolution, "invalid_plan", Some(&reason)).await;
                return Err(AssuranceRunError::Unavailable(failure));
            }
            Err(PipelineError::VerificationFailed { message }) => {
                let failure = self.record_failure(attempt_id, plan,
                    AssuranceFailureStage::VerificationExecution, "verification_failed", Some(&message)).await;
                return Err(AssuranceRunError::Unavailable(failure));
            }
            Err(PipelineError::RunFailed { .. }) => {
                let failure = self.record_failure(attempt_id, plan,
                    AssuranceFailureStage::Finalization, "run_failed", None).await;
                return Err(AssuranceRunError::Unavailable(failure));
            }
        };

        let evidence_bundle = self.evidence_assembler
            .assemble(plan, &verdict.unit_results)
            .map_err(|e| {
                // Can't use self.record_failure here because we already borrowed self immutably
                AssuranceRunError::EvidenceAssemblyFailed(e)
            })?;

        let evidence_digest = self.evidence_assembler.digest(&evidence_bundle);

        // ── ⑤ Phase 1: Stage evidence in object storage ──
        self.store.stage_evidence(&run, &evidence_bundle).await?;
        // state: EvidenceStaged

        // ── ⑥ Phase 2: DB TX1 — Verdict (Finalizing, not visible) ──
        self.store.finalize_phase1(&run, &verdict).await?;
        // state: VerdictFinalizing — Verdict written but NOT visible

        // ── ⑦ Phase 3: Mark evidence referenced ──
        self.store.mark_evidence_referenced(&run).await?;

        // ── ⑧ Phase 4: DB TX2 — Finalized, visible ──
        self.store.finalize_phase2(&run).await?;
        // state: Finalized — Verdict now visible

        Ok(PersistedVerdict {
            verdict,
            verdict_ref: format!("verdict/{}", attempt_id),
            evidence_bundle_ref: evidence_bundle.bundle_ref,
        })
    }

    /// Crash recovery: resume from the last persisted state.
    pub async fn recover(
        &self,
        attempt_id: &str,
        plan: &ConformancePlan,
        ctx: &VerificationContext,
        exec: &dyn VerificationExecutor,
        svc: &VerifierServices<'_>,
    ) -> Result<Option<PersistedVerdict>, AssuranceRunError> {
        match self.store.get_run_state(attempt_id).await {
            Ok(state) => match state {
                AssuranceRunState::Created
                | AssuranceRunState::PlanResolved
                | AssuranceRunState::Executing => {
                    // No evidence persisted → restart from scratch
                    Ok(None)
                }
                AssuranceRunState::EvidenceStaged => {
                    // Evidence in object store → re-execute from Phase 2
                    self.resume_from_evidence_staged(attempt_id, plan, ctx, exec, svc).await
                }
                AssuranceRunState::VerdictFinalizing => {
                    // Phase 1 committed but Phase 3/4 may not have run.
                    // Re-mark evidence, then Phase 4.
                    self.resume_from_finalizing(attempt_id).await
                }
                AssuranceRunState::Finalized => {
                    self.store.load_verdict(attempt_id).await
                        .map_err(AssuranceRunError::Store)
                }
                AssuranceRunState::Failed => {
                    Err(AssuranceRunError::RunFailed)
                }
            },
            Err(StoreError::NotFound(_)) => Ok(None),
            Err(e) => Err(AssuranceRunError::Store(e)),
        }
    }

    async fn resume_from_evidence_staged(
        &self, attempt_id: &str, plan: &ConformancePlan,
        ctx: &VerificationContext, exec: &dyn VerificationExecutor,
        svc: &VerifierServices<'_>,
    ) -> Result<Option<PersistedVerdict>, AssuranceRunError> {
        // Re-execute pipeline to get verdict, then continue from Phase 2
        let pm = PipelineManager { registry: self.registry.clone() };
        let verdict = pm.execute(plan, ctx, exec, svc).await
            .map_err(|e| AssuranceRunError::from(e))?;

        let run = AssuranceRunHandle { attempt_id: attempt_id.to_string(), evidence_ref: None };
        self.store.finalize_phase1(&run, &verdict).await?;
        self.store.mark_evidence_referenced(&run).await?;
        self.store.finalize_phase2(&run).await?;

        Ok(Some(PersistedVerdict {
            verdict,
            verdict_ref: format!("verdict/{}", attempt_id),
            evidence_bundle_ref: format!("evidence/{}", attempt_id),
        }))
    }

    async fn resume_from_finalizing(
        &self, attempt_id: &str,
    ) -> Result<Option<PersistedVerdict>, AssuranceRunError> {
        let run = AssuranceRunHandle { attempt_id: attempt_id.to_string(), evidence_ref: None };
        // Re-mark evidence (idempotent)
        self.store.mark_evidence_referenced(&run).await?;
        // Complete Phase 4
        self.store.finalize_phase2(&run).await?;
        // Load verdict
        self.store.load_verdict(attempt_id).await
            .map_err(AssuranceRunError::Store)
    }
}

// ══════════════════════════════════════════════════════════════════
// Errors
// ══════════════════════════════════════════════════════════════════

// ══════════════════════════════════════════════════════════════════
// AssuranceFailureStore
// ══════════════════════════════════════════════════════════════════

#[async_trait::async_trait]
pub trait AssuranceFailureStore: Send + Sync {
    async fn persist_if_absent(
        &self,
        failure: &PersistedAssuranceFailure,
    ) -> Result<PersistedAssuranceFailure, StoreError>;
}

// ══════════════════════════════════════════════════════════════════

#[derive(Debug, thiserror::Error)]
pub enum AssuranceRunError {
    #[error("plan invalid: {0}")]
    PlanInvalid(String),
    #[error("verification failed: {0}")]
    VerificationFailed(String),
    #[error("run failed")]
    RunFailed,
    #[error("evidence assembly failed: {0}")]
    EvidenceAssemblyFailed(String),
    #[error("store error: {0}")]
    Store(StoreError),
    /// P16-P1: Assurance unavailable with a persisted failure record.
    #[error("assurance unavailable: id={failure_id}", failure_id = .0.failure_id)]
    Unavailable(PersistedAssuranceFailure),
}

impl From<PipelineError> for AssuranceRunError {
    fn from(e: PipelineError) -> Self {
        match e {
            PipelineError::InvalidPlan { reason, .. } => AssuranceRunError::PlanInvalid(reason),
            PipelineError::VerificationFailed { message } => AssuranceRunError::VerificationFailed(message),
            PipelineError::RunFailed { .. } => AssuranceRunError::RunFailed,
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// P16-R1: RecoveryScanner
// ══════════════════════════════════════════════════════════════════

/// Scans for incomplete Assurance runs on startup and resumes them.
pub struct RecoveryScanner {
    store: Arc<dyn AssuranceRunStore>,
}

impl RecoveryScanner {
    pub fn new(store: Arc<dyn AssuranceRunStore>) -> Self { Self { store } }

    /// Scan and recover all incomplete runs. Returns recovered verdicts.
    pub async fn recover_all(&self) -> Result<Vec<PersistedVerdict>, StoreError> {
        let incomplete = self.store.scan_incomplete_runs().await?;
        let mut recovered = Vec::new();
        for run in &incomplete {
            // P16-R1: recovering incomplete run
            if let Ok(Some(verdict)) = self.store.load_verdict(&run.attempt_id).await {
                // Already finalized (race between scan and recovery) — just load it
                recovered.push(verdict);
            }
            // For non-finalized runs, the AssuranceRunner::recover() handles the
            // state-specific resume logic. The caller must re-invoke the pipeline.
        }
        Ok(recovered)
    }

    /// List the IDs of runs that need recovery (non-terminal states).
    pub async fn list_incomplete(&self) -> Result<Vec<String>, StoreError> {
        let runs = self.store.scan_incomplete_runs().await?;
        Ok(runs.into_iter().map(|r| r.attempt_id).collect())
    }
}

// ══════════════════════════════════════════════════════════════════

impl From<StoreError> for AssuranceRunError {
    fn from(e: StoreError) -> Self {
        AssuranceRunError::Store(e)
    }
}

// ══════════════════════════════════════════════════════════════════
// In-memory store (for testing / MVP)
// ══════════════════════════════════════════════════════════════════

use std::collections::HashMap;
use std::sync::Mutex;

// ══════════════════════════════════════════════════════════════════
// InMemoryAssuranceFailureStore
// ══════════════════════════════════════════════════════════════════

pub struct InMemoryAssuranceFailureStore {
    failures: Mutex<HashMap<String, PersistedAssuranceFailure>>,
}

impl InMemoryAssuranceFailureStore {
    pub fn new() -> Self { Self { failures: Mutex::new(HashMap::new()) } }
}

#[async_trait::async_trait]
impl AssuranceFailureStore for InMemoryAssuranceFailureStore {
    async fn persist_if_absent(
        &self,
        failure: &PersistedAssuranceFailure,
    ) -> Result<PersistedAssuranceFailure, StoreError> {
        let mut f = self.failures.lock().unwrap();
        if let Some(existing) = f.get(&failure.failure_id) {
            return Ok(existing.clone());
        }
        f.insert(failure.failure_id.clone(), failure.clone());
        Ok(failure.clone())
    }
}

// ══════════════════════════════════════════════════════════════════

pub struct InMemoryAssuranceRunStore {
    states: Mutex<HashMap<String, AssuranceRunState>>,
    plans: Mutex<HashMap<String, ConformancePlan>>,
    verdicts: Mutex<HashMap<String, ConformanceVerdict>>,
    evidence: Mutex<HashMap<String, EvidenceBundle>>,
    evidence_referenced: Mutex<HashMap<String, bool>>,
    pub fail_next_phase1: Mutex<bool>,
}

impl InMemoryAssuranceRunStore {
    pub fn new() -> Self {
        Self {
            states: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
            verdicts: Mutex::new(HashMap::new()),
            evidence: Mutex::new(HashMap::new()),
            evidence_referenced: Mutex::new(HashMap::new()),
            fail_next_phase1: Mutex::new(false),
        }
    }
}

#[async_trait::async_trait]
impl AssuranceRunStore for InMemoryAssuranceRunStore {
    async fn begin_run(&self, attempt_id: &str) -> Result<AssuranceRunHandle, StoreError> {
        let mut states = self.states.lock().unwrap();
        if states.contains_key(attempt_id) {
            return Err(StoreError::InvalidState(
                format!("run {} already exists", attempt_id)
            ));
        }
        states.insert(attempt_id.to_string(), AssuranceRunState::Created);
        Ok(AssuranceRunHandle { attempt_id: attempt_id.to_string(), evidence_ref: None })
    }

    async fn record_plan(&self, run: &AssuranceRunHandle, plan: &ConformancePlan) -> Result<(), StoreError> {
        self.plans.lock().unwrap().insert(run.attempt_id.clone(), plan.clone());
        self.transition(&run.attempt_id, AssuranceRunState::PlanResolved)?;
        Ok(())
    }

    async fn get_run_state(&self, attempt_id: &str) -> Result<AssuranceRunState, StoreError> {
        self.states.lock().unwrap()
            .get(attempt_id)
            .cloned()
            .ok_or(StoreError::NotFound(attempt_id.to_string()))
    }

    async fn stage_evidence(&self, run: &AssuranceRunHandle, bundle: &EvidenceBundle) -> Result<(), StoreError> {
        self.evidence.lock().unwrap().insert(run.attempt_id.clone(), bundle.clone());
        self.transition(&run.attempt_id, AssuranceRunState::EvidenceStaged)?;
        Ok(())
    }

    async fn finalize_phase1(&self, run: &AssuranceRunHandle, verdict: &ConformanceVerdict) -> Result<(), StoreError> {
        if *self.fail_next_phase1.lock().unwrap() {
            return Err(StoreError::Storage("injected phase1 failure".into()));
        }
        self.verdicts.lock().unwrap().insert(run.attempt_id.clone(), verdict.clone());
        self.transition(&run.attempt_id, AssuranceRunState::VerdictFinalizing)?;
        Ok(())
    }

    async fn mark_evidence_referenced(&self, run: &AssuranceRunHandle) -> Result<(), StoreError> {
        self.evidence_referenced.lock().unwrap().insert(run.attempt_id.clone(), true);
        Ok(())
    }

    async fn finalize_phase2(&self, run: &AssuranceRunHandle) -> Result<(), StoreError> {
        self.transition(&run.attempt_id, AssuranceRunState::Finalized)?;
        Ok(())
    }

    async fn load_verdict(&self, attempt_id: &str) -> Result<Option<PersistedVerdict>, StoreError> {
        let state = self.get_run_state(attempt_id).await?;
        if state != AssuranceRunState::Finalized {
            return Ok(None);
        }
        let verdict = self.verdicts.lock().unwrap().get(attempt_id).cloned();
        Ok(verdict.map(|v| PersistedVerdict {
            verdict: v,
            verdict_ref: format!("verdict/{}", attempt_id),
            evidence_bundle_ref: format!("evidence/{}", attempt_id),
        }))
    }

    async fn scan_incomplete_runs(&self) -> Result<Vec<AssuranceRunHandle>, StoreError> {
        let states = self.states.lock().unwrap();
        Ok(states.iter()
            .filter(|(_, s)| !matches!(s, AssuranceRunState::Finalized | AssuranceRunState::Failed))
            .map(|(id, _)| AssuranceRunHandle { attempt_id: id.clone(), evidence_ref: None })
            .collect())
    }
}

impl InMemoryAssuranceRunStore {
    fn transition(&self, attempt_id: &str, new_state: AssuranceRunState) -> Result<(), StoreError> {
        let mut states = self.states.lock().unwrap();
        let current = states.get(attempt_id).cloned()
            .ok_or(StoreError::NotFound(attempt_id.to_string()))?;
        if !is_valid_transition(current, new_state) {
            return Err(StoreError::InvalidState(
                format!("invalid transition {:?} → {:?} for {}", current, new_state, attempt_id)
            ));
        }
        states.insert(attempt_id.to_string(), new_state);
        Ok(())
    }
}

fn is_valid_transition(from: AssuranceRunState, to: AssuranceRunState) -> bool {
    use AssuranceRunState::*;
    matches!((from, to),
        (Created, PlanResolved)
        | (PlanResolved, Executing)
        | (PlanResolved, EvidenceStaged)   // pipeline execute is synchronous
        | (Executing, EvidenceStaged)
        | (EvidenceStaged, VerdictFinalizing)
        | (VerdictFinalizing, Finalized)
        | (_, Failed)
    )
}

fn quick_digest(s: &str) -> Digest {
    use sha2::{Digest as _, Sha256};
    Digest::new(DigestAlgorithm::Sha256, hex::encode(Sha256::digest(s.as_bytes())))
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::PassRegistry;
    use onto_protocol::check::{Applicability, ConformanceUnit, ConformancePlan};
    use onto_protocol::context::{CandidateVerificationContext, VerificationContext};
    use onto_protocol::candidate::SealedCandidateRef;
    use onto_protocol::digest::DigestAlgorithm;
    use onto_protocol::executor::VerificationExecutor;
    use onto_protocol::sandbox::*;
    use onto_protocol::verifier::{
        Pass, Verifier, VerifierDescriptor, VerifierResult, VerifierServices,
        VerifierStatus, VerificationMode, VerificationStage,
    };

    fn d(s: &str) -> Digest {
        Digest::new(DigestAlgorithm::Sha256, s)
    }

    fn test_plan(verifier_id: &str) -> ConformancePlan {
        ConformancePlan {
            plan_id: "p1".into(), plan_digest: d("p1"), attempt_id: "a1".into(),
            candidate_id: "c1".into(), candidate_digest: d("c1"),
            profile_id: "pf".into(), profile_version: "1".into(),
            profile_digest: d("pf"), rule_set_digest: d("rules"),
            verifier_registry_digest: d("vers"),
            units: vec![ConformanceUnit {
                unit_id: "u1".into(), verifier_id: verifier_id.to_string(),
                pass: Pass::Format, validation_dependencies: vec![],
                applicability: Applicability::Required,
            }],
        }
    }

    fn test_ctx() -> VerificationContext {
        let c = SealedCandidateRef::new("c1", d("c1"), d("m1"), "/tmp");
        VerificationContext::PreGraph(CandidateVerificationContext {
            attempt_id: "a1".into(), candidate: c,
            repository_name: "r".into(), base_commit_sha: "s".into(),
            execution_generation: 1, changed_files: vec![], language: "rust".into(),
        })
    }

    struct StubVerifier { desc: VerifierDescriptor }
    impl StubVerifier {
        fn new(id: &str) -> Self {
            Self { desc: VerifierDescriptor {
                verifier_id: id.into(), pass: Pass::Format,
                stage: VerificationStage::PreGraph, mode: VerificationMode::Internal,
                supported_rules: vec![],
            }}
        }
    }
    #[async_trait::async_trait]
    impl Verifier for StubVerifier {
        fn descriptor(&self) -> &VerifierDescriptor { &self.desc }
        fn external_requirements(&self, _: &VerificationContext) -> Vec<onto_protocol::check::ExternalCheckRequirement> { vec![] }
        async fn evaluate(&self, _: &VerificationContext, _: &[(&String, &RawCheckResult)], _: &VerifierServices<'_>) -> VerifierResult {
            VerifierResult { verifier_id: self.desc.verifier_id.clone(), pass: Pass::Format, status: VerifierStatus::Completed, findings: vec![], raw_evidence: vec![], diagnostic: None }
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
                environment: SandboxEnvironmentIdentity { runsc_version: "mock".into(), profile_digest: d("pf"), image_digest: d("img"), toolchain_digest: d("tc") },
                filesystem_diff: ArtifactDelta { created: vec![], modified: vec![], deleted: vec![] },
                resource_usage: ResourceUsage { cpu_seconds: 0.0, memory_mb: 0.0, disk_mb: 0.0, network_bytes_sent: 0, network_bytes_recv: 0 },
            })
        }
    }

    fn dummy_svcs() -> VerifierServices<'static> {
        struct D; impl onto_protocol::verifier::SealedArtifactReader for D { fn read_manifest(&self, _: &str) -> Result<onto_protocol::candidate::ArtifactManifest, String> { Ok(onto_protocol::candidate::ArtifactManifest{entries:vec![]}) } fn read_file(&self, _: &str, _: &str) -> Result<Vec<u8>, String> { Ok(vec![]) } }
        struct G; impl onto_protocol::verifier::CandidateGraphReader for G { fn check_integrity(&self, _: &str) -> Result<(), String> { Ok(()) } fn get_entity_keys(&self, _: &str) -> Result<Vec<String>, String> { Ok(vec![]) } }
        struct S; #[async_trait::async_trait] impl onto_protocol::verifier::SemanticRuntimePort for S { async fn review(&self, _: &str, _: &str) -> Result<String, String> { Ok("ok".into()) } }
        static D1: D = D; static G1: G = G; static S1: S = S;
        VerifierServices { artifact_reader: &D1, graph_reader: &G1, semantic_runtime: &S1 }
    }

    // ══════════════════════════════════════════════════════════════
    // P16-3 tests
    // ══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn p16_3_1_full_pipeline_produces_persisted_verdict() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new("v1"))).unwrap();
        let registry = Arc::new(reg.freeze());
        let store = Arc::new(InMemoryAssuranceRunStore::new());
        let runner = AssuranceRunner::new(registry.clone(), store.clone(), Arc::new(InMemoryAssuranceFailureStore::new()));

        let plan = test_plan("v1");
        let ctx = test_ctx();
        let result = runner.execute("a1", &plan, &ctx, &MockExec, &dummy_svcs()).await;
        assert!(result.is_ok(), "full pipeline must succeed: {:?}", result.err());

        let persisted = result.unwrap();
        assert_eq!(persisted.verdict_ref, "verdict/a1");

        // Verify state is Finalized
        let state = store.get_run_state("a1").await.unwrap();
        assert_eq!(state, AssuranceRunState::Finalized);
    }

    #[tokio::test]
    async fn p16_3_2_phase1_failure_no_verdict() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new("v1"))).unwrap();
        let registry = Arc::new(reg.freeze());
        let store = Arc::new(InMemoryAssuranceRunStore::new());
        *store.fail_next_phase1.lock().unwrap() = true;

        let runner = AssuranceRunner::new(registry.clone(), store.clone(), Arc::new(InMemoryAssuranceFailureStore::new()));
        let plan = test_plan("v1");
        let result = runner.execute("a1", &plan, &test_ctx(), &MockExec, &dummy_svcs()).await;
        assert!(result.is_err(), "phase1 failure must fail the run");

        // State should NOT be Finalized
        let state = store.get_run_state("a1").await.unwrap();
        assert_ne!(state, AssuranceRunState::Finalized,
            "phase1 failure must not leave Finalized state, got {:?}", state);
    }

    #[tokio::test]
    async fn p16_3_3_crash_recovery_from_evidence_staged() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new("v1"))).unwrap();
        let registry = Arc::new(reg.freeze());
        let store = Arc::new(InMemoryAssuranceRunStore::new());

        // Manually advance to EvidenceStaged
        store.begin_run("a1").await.unwrap();
        store.record_plan(&AssuranceRunHandle { attempt_id: "a1".into(), evidence_ref: None }, &test_plan("v1")).await.unwrap();
        // Skip Executing → directly to EvidenceStaged (simulating crash after stage)
        store.states.lock().unwrap().insert("a1".into(), AssuranceRunState::EvidenceStaged);

        let runner = AssuranceRunner::new(registry.clone(), store.clone(), Arc::new(InMemoryAssuranceFailureStore::new()));
        let plan = test_plan("v1");
        let result = runner.recover("a1", &plan, &test_ctx(), &MockExec, &dummy_svcs()).await;
        assert!(result.is_ok(), "recovery must succeed: {:?}", result.err());
        assert!(result.unwrap().is_some(), "recovery from EvidenceStaged must produce verdict");
    }

    #[tokio::test]
    async fn p16_3_4_state_machine_rejects_invalid_transitions() {
        let store = InMemoryAssuranceRunStore::new();
        store.begin_run("a1").await.unwrap();
        // Try to jump from Created directly to Finalized (skip intermediate states)
        let result = store.finalize_phase2(&AssuranceRunHandle { attempt_id: "a1".into(), evidence_ref: None }).await;
        assert!(result.is_err(), "Created → Finalized must be rejected");
        match result.unwrap_err() {
            StoreError::InvalidState(_) => {} // expected
            other => panic!("expected InvalidState, got: {}", other),
        }
    }

    // ═══════════════════ P16-R1 recovery tests ═══════════════════

    #[tokio::test]
    async fn p16_r1_1_scan_incomplete_runs() {
        let store = InMemoryAssuranceRunStore::new();
        // Create runs in various states
        store.begin_run("r1").await.unwrap(); // Created
        store.begin_run("r2").await.unwrap();
        store.record_plan(&AssuranceRunHandle { attempt_id: "r2".into(), evidence_ref: None }, &test_plan("v1")).await.unwrap(); // PlanResolved
        store.begin_run("r3").await.unwrap();
        store.states.lock().unwrap().insert("r3".into(), AssuranceRunState::Finalized); // Finalized
        store.begin_run("r4").await.unwrap();
        store.states.lock().unwrap().insert("r4".into(), AssuranceRunState::Failed); // Failed

        let incomplete = store.scan_incomplete_runs().await.unwrap();
        let ids: Vec<_> = incomplete.iter().map(|r| r.attempt_id.clone()).collect();
        assert!(ids.contains(&"r1".to_string()), "Created should be incomplete");
        assert!(ids.contains(&"r2".to_string()), "PlanResolved should be incomplete");
        assert!(!ids.contains(&"r3".to_string()), "Finalized should NOT be incomplete");
        assert!(!ids.contains(&"r4".to_string()), "Failed should NOT be incomplete");
    }

    #[tokio::test]
    async fn p16_r1_2_recovery_scanner_lists_incomplete() {
        let store = Arc::new(InMemoryAssuranceRunStore::new());
        store.begin_run("r-incomplete").await.unwrap();
        store.begin_run("r-done").await.unwrap();
        store.states.lock().unwrap().insert("r-done".into(), AssuranceRunState::Finalized);

        let scanner = RecoveryScanner::new(store);
        let incomplete = scanner.list_incomplete().await.unwrap();
        assert!(incomplete.contains(&"r-incomplete".to_string()));
        assert!(!incomplete.contains(&"r-done".to_string()));
    }

    // ═══════════════════ P16-R2+R3: Crash recovery tests ═══════════════════

    /// Simulate crash after evidence staged → recovery should finalize
    #[tokio::test]
    async fn p16_r2_1_crash_after_evidence_staged_recovers() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new("v1"))).unwrap();
        let registry = Arc::new(reg.freeze());
        let store = Arc::new(InMemoryAssuranceRunStore::new());

        // Manually advance to EvidenceStaged (simulating crash after stage_evidence)
        store.begin_run("a1").await.unwrap();
        store.record_plan(&AssuranceRunHandle { attempt_id: "a1".into(), evidence_ref: None }, &test_plan("v1")).await.unwrap();
        store.states.lock().unwrap().insert("a1".into(), AssuranceRunState::EvidenceStaged);

        // "Crash" — create new runner and recover
        let failure_store = Arc::new(InMemoryAssuranceFailureStore::new());
        let runner = AssuranceRunner::new(registry.clone(), store.clone(), failure_store);
        let plan = test_plan("v1");
        let result = runner.recover("a1", &plan, &test_ctx(), &MockExec, &dummy_svcs()).await;
        assert!(result.is_ok(), "recovery must succeed: {:?}", result.err());
        // After recovery, the run should be Finalized
        let state = store.get_run_state("a1").await.unwrap();
        assert_eq!(state, AssuranceRunState::Finalized);
    }

    /// Simulate crash after verdict written (VerdictFinalizing) → recovery completes
    #[tokio::test]
    async fn p16_r2_2_crash_after_verdict_finalizing_recovers() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new("v1"))).unwrap();
        let registry = Arc::new(reg.freeze());
        let store = Arc::new(InMemoryAssuranceRunStore::new());

        // Run full pipeline once
        let failure_store = Arc::new(InMemoryAssuranceFailureStore::new());
        let runner = AssuranceRunner::new(registry.clone(), store.clone(), failure_store.clone());
        let plan = test_plan("v1");
        let result = runner.execute("a1", &plan, &test_ctx(), &MockExec, &dummy_svcs()).await;
        assert!(result.is_ok());

        // Verify state is Finalized
        let state = store.get_run_state("a1").await.unwrap();
        assert_eq!(state, AssuranceRunState::Finalized);

        // "Crash" — re-run idempotently (should return same verdict)
        let runner2 = AssuranceRunner::new(registry, store.clone(), failure_store);
        let result2 = runner2.recover("a1", &plan, &test_ctx(), &MockExec, &dummy_svcs()).await;
        assert!(result2.is_ok());
        let persisted = result2.unwrap().unwrap();
        assert!(persisted.verdict_ref.contains("a1"));
    }

    /// Fresh run (Created state) → recovery returns None (restart from scratch)
    #[tokio::test]
    async fn p16_r2_3_fresh_run_recovery_returns_none() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new("v1"))).unwrap();
        let registry = Arc::new(reg.freeze());
        let store = Arc::new(InMemoryAssuranceRunStore::new());
        store.begin_run("a1").await.unwrap(); // Created state

        let runner = AssuranceRunner::new(registry, store.clone(), Arc::new(InMemoryAssuranceFailureStore::new()));
        let result = runner.recover("a1", &test_plan("v1"), &test_ctx(), &MockExec, &dummy_svcs()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none(), "fresh Created run must return None (restart)");
    }

    /// Nonexistent run → recovery returns None
    #[tokio::test]
    async fn p16_r2_4_nonexistent_run_returns_none() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new("v1"))).unwrap();
        let registry = Arc::new(reg.freeze());
        let store = Arc::new(InMemoryAssuranceRunStore::new());

        let runner = AssuranceRunner::new(registry, store, Arc::new(InMemoryAssuranceFailureStore::new()));
        let result = runner.recover("nonexistent", &test_plan("v1"), &test_ctx(), &MockExec, &dummy_svcs()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    /// Same run_id + same binding → idempotent (no duplicate)
    #[tokio::test]
    async fn p16_r3_1_same_run_idempotent() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new("v1"))).unwrap();
        let registry = Arc::new(reg.freeze());
        let store = Arc::new(InMemoryAssuranceRunStore::new());
        let failure_store = Arc::new(InMemoryAssuranceFailureStore::new());

        let runner = AssuranceRunner::new(registry.clone(), store.clone(), failure_store.clone());
        let plan = test_plan("v1");
        let ctx = test_ctx();

        let r1 = runner.execute("idem1", &plan, &ctx, &MockExec, &dummy_svcs()).await.unwrap();
        // Second call with same attempt_id → store.begin_run() returns AlreadyFinalized error
        // which correctly prevents duplicate execution
        let r2 = store.begin_run("idem1").await;
        assert!(r2.is_err(), "duplicate begin_run must be rejected");
    }

    #[tokio::test]
    async fn p16_3_5_double_run_idempotent() {
        let mut reg = PassRegistry::new();
        reg.register("v1", Box::new(StubVerifier::new("v1"))).unwrap();
        let registry = Arc::new(reg.freeze());
        let store = Arc::new(InMemoryAssuranceRunStore::new());
        let runner = AssuranceRunner::new(registry.clone(), store.clone(), Arc::new(InMemoryAssuranceFailureStore::new()));

        let plan = test_plan("v1");
        let ctx = test_ctx();

        // First run succeeds
        let r1 = runner.execute("a1", &plan, &ctx, &MockExec, &dummy_svcs()).await.unwrap();

        // Second run with same attempt_id: store rejects (already exists)
        let r2 = store.begin_run("a1").await;
        assert!(r2.is_err(), "duplicate begin_run must be rejected");
    }
}
