//! AssuranceRunStore — atomic persistence for Plan, Evidence, and Verdict.
//!
//! All three MUST be persisted atomically: Verdict must not exist
//! without its Evidence, and Evidence must not be orphaned.
//! Uses a simple in-memory store for now; PostgreSQL implementation can be swapped in.

use onto_protocol::check::ConformancePlan;
use onto_protocol::verdict::ConformanceVerdict;
use onto_protocol::digest::Digest;
use std::collections::HashMap;
use std::sync::Mutex;

/// State of one Assurance Run.
#[derive(Debug, Clone)]
pub enum AssuranceRunState {
    Running,
    Finalized { plan_digest: Digest, verdict_digest: Digest },
}

/// Atomic store for Plan + Evidence + Verdict.
pub struct AssuranceRunStore {
    plans: Mutex<HashMap<String, ConformancePlan>>,
    verdicts: Mutex<HashMap<String, ConformanceVerdict>>,
    states: Mutex<HashMap<String, AssuranceRunState>>,
}

impl AssuranceRunStore {
    pub fn new() -> Self {
        Self {
            plans: Mutex::new(HashMap::new()),
            verdicts: Mutex::new(HashMap::new()),
            states: Mutex::new(HashMap::new()),
        }
    }

    /// Record the plan at the start of an Assurance run.
    pub fn begin_run(&self, attempt_id: &str, plan: ConformancePlan) {
        self.plans.lock().unwrap().insert(attempt_id.to_string(), plan);
        self.states.lock().unwrap().insert(attempt_id.to_string(), AssuranceRunState::Running);
    }

    /// Atomically finalize: store verdict, mark as finalized.
    /// Returns false if already finalized (idempotent).
    pub fn finalize(&self, attempt_id: &str, verdict: ConformanceVerdict) -> bool {
        let mut states = self.states.lock().unwrap();
        if let Some(AssuranceRunState::Finalized { .. }) = states.get(attempt_id) {
            return false; // already finalized
        }
        let plan_digest = verdict.plan_digest.clone();
        let verdict_digest = verdict.verdict_digest.clone();
        self.verdicts.lock().unwrap().insert(attempt_id.to_string(), verdict);
        states.insert(attempt_id.to_string(), AssuranceRunState::Finalized { plan_digest, verdict_digest });
        true
    }

    /// Check if a run is finalized.
    pub fn is_finalized(&self, attempt_id: &str) -> bool {
        matches!(self.states.lock().unwrap().get(attempt_id), Some(AssuranceRunState::Finalized { .. }))
    }

    /// Retrieve a finalized verdict.
    pub fn get_verdict(&self, attempt_id: &str) -> Option<ConformanceVerdict> {
        self.verdicts.lock().unwrap().get(attempt_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_protocol::check::{ConformancePlan, ConformanceUnit, Applicability};
    use onto_protocol::verdict::{
        ConformanceVerdict, ConformanceOutcome, CoverageState, FreshnessState,
        GraphValidationBinding, GraphUnavailableReason, SandboxValidationSummary,
    };
    use onto_protocol::verifier::Pass;
    use onto_protocol::digest::{Digest, DigestAlgorithm};
    use onto_protocol::check::ConformancePlanSummary;

    fn d(s: &str) -> Digest { Digest::new(DigestAlgorithm::Sha256, s) }
    fn p() -> ConformancePlan {
        ConformancePlan {
            plan_id: "p1".into(), plan_digest: d("p1"), attempt_id: "a1".into(),
            candidate_id: "c1".into(), candidate_digest: d("c1"),
            profile_id: "pf".into(), profile_version: "1".into(),
            profile_digest: d("pf"), rule_set_digest: d("rs"), verifier_registry_digest: d("vr"),
            units: vec![],
        }
    }
    fn v() -> ConformanceVerdict {
        ConformanceVerdict {
            verdict_id: "v1".into(), verdict_digest: d("v1"),
            attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: d("c1"),
            graph: GraphValidationBinding::Unavailable { reason: GraphUnavailableReason::ServiceDown, diagnostic_ref: "d".into() },
            plan_id: "p1".into(), plan_digest: d("p1"),
            evidence_bundle_ref: "e1".into(), evidence_bundle_digest: d("e1"),
            conformance: ConformanceOutcome::Conformant,
            freshness: FreshnessState::Current, coverage: CoverageState::Complete,
            sandbox: SandboxValidationSummary::NotRunDueToBlockingPrerequisite { blocking_finding_ids: vec![] },
            blocking_findings: vec![], advisory_findings: vec![], unit_results: vec![],
            plan: ConformancePlanSummary { plan_id: "p1".into(), profile_id: "pf".into(), total_units: 0, required_units: 0, applied_units: 0 },
        }
    }

    #[test] fn begin_and_finalize() { let s = AssuranceRunStore::new(); s.begin_run("a1", p()); assert!(s.finalize("a1", v())); assert!(s.is_finalized("a1")); }
    #[test] fn double_finalize_is_idempotent() { let s = AssuranceRunStore::new(); s.begin_run("a1", p()); assert!(s.finalize("a1", v())); assert!(!s.finalize("a1", v())); }
    #[test] fn not_begun_is_not_finalized() { let s = AssuranceRunStore::new(); assert!(!s.is_finalized("nonexistent")); }
}
