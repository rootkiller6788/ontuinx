//! ProgressStore — cross-Attempt verdict persistence for progress comparison.
//!
//! Stores and retrieves ConformanceVerdict across attempts so that
//! compare_progress can use real Finding fingerprints instead of empty arrays.

use std::collections::HashMap;
use std::sync::Mutex;
use onto_protocol::verdict::ConformanceVerdict;
use onto_protocol::finding::FindingFingerprint;

/// Persists verdicts across attempts for a given task/loop.
pub trait ProgressStore: Send + Sync {
    /// Store a verdict for a specific attempt.
    fn record(&self, loop_id: &str, attempt: u32, verdict: &ConformanceVerdict);
    /// Get the previous attempt's verdict.
    fn previous(&self, loop_id: &str) -> Option<(u32, ConformanceVerdict)>;
    /// Get blocking fingerprints from the previous verdict, for compare_progress.
    fn previous_fingerprints(&self, loop_id: &str) -> Vec<FindingFingerprint> {
        self.previous(loop_id)
            .map(|(_, v)| v.blocking_findings.iter()
                .chain(v.advisory_findings.iter())
                .map(|f| f.fingerprint.clone())
                .collect())
            .unwrap_or_default()
    }
}

/// In-memory implementation for testing and single-process use.
pub struct InMemoryProgressStore {
    records: Mutex<HashMap<String, Vec<(u32, ConformanceVerdict)>>>,
}

impl InMemoryProgressStore {
    pub fn new() -> Self { Self { records: Mutex::new(HashMap::new()) } }
    pub fn record_count(&self, loop_id: &str) -> usize {
        self.records.lock().unwrap().get(loop_id).map(|v| v.len()).unwrap_or(0)
    }
}

impl ProgressStore for InMemoryProgressStore {
    fn record(&self, loop_id: &str, attempt: u32, verdict: &ConformanceVerdict) {
        self.records.lock().unwrap()
            .entry(loop_id.to_string())
            .or_default()
            .push((attempt, verdict.clone()));
    }

    fn previous(&self, loop_id: &str) -> Option<(u32, ConformanceVerdict)> {
        self.records.lock().unwrap()
            .get(loop_id)
            .and_then(|v| v.last().cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_protocol::verdict::{
        ConformanceVerdict, ConformanceOutcome, CoverageState, FreshnessState,
        GraphValidationBinding, GraphUnavailableReason, SandboxValidationSummary,
    };
    use onto_protocol::check::ConformancePlanSummary;
    use onto_protocol::finding::{Finding, FindingFingerprint, FindingSeverity, CategoryId, FindingDisposition, RemediationClass};
    use onto_protocol::digest::{Digest, DigestAlgorithm};

    fn d(s: &str) -> Digest { Digest::new(DigestAlgorithm::Sha256, s) }
    fn v(blocking: usize) -> ConformanceVerdict {
        let mut bfs = vec![];
        for i in 0..blocking {
            bfs.push(Finding {
                finding_id: format!("f{}", i),
                fingerprint: FindingFingerprint { rule_id: format!("r{}", i), entity_key: None, artifact_path: format!("f{}.rs", i), semantic_key: format!("k{}", i), line_hint: None },
                pass: onto_protocol::verifier::Pass::Build, rule_id: format!("r{}", i), rule_version: "1".into(),
                severity: FindingSeverity::Critical, category: CategoryId::new("c"), disposition: FindingDisposition::Blocking, remediation: RemediationClass::RetryWithFeedback,
                location: None, message: format!("e{}", i), fix_hint: None, confidence: 1.0, evidence_refs: vec![],
            });
        }
        ConformanceVerdict {
            verdict_id: "v1".into(), verdict_digest: d("v1"), attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: d("c1"),
            graph: GraphValidationBinding::Unavailable { reason: GraphUnavailableReason::ServiceDown, diagnostic_ref: "d".into() },
            plan_id: "p1".into(), plan_digest: d("p1"), evidence_bundle_ref: "e1".into(), evidence_bundle_digest: d("e1"),
            conformance: if blocking == 0 { ConformanceOutcome::Conformant } else { ConformanceOutcome::NonConformant },
            freshness: FreshnessState::Current, coverage: CoverageState::Complete,
            sandbox: SandboxValidationSummary::NotRunDueToBlockingPrerequisite { blocking_finding_ids: vec![] },
            blocking_findings: bfs, advisory_findings: vec![], unit_results: vec![],
            plan: ConformancePlanSummary { plan_id: "p1".into(), profile_id: "pf".into(), total_units: 1, required_units: 1, applied_units: 1 },
        }
    }

    #[test] fn stores_and_retrieves() { let s = InMemoryProgressStore::new(); s.record("L1", 1, &v(3)); assert_eq!(s.record_count("L1"), 1); let (n, prev) = s.previous("L1").unwrap(); assert_eq!(n, 1); assert_eq!(prev.blocking_findings.len(), 3); }
    #[test] fn previous_returns_latest() { let s = InMemoryProgressStore::new(); s.record("L1", 1, &v(3)); s.record("L1", 2, &v(1)); let (n, _) = s.previous("L1").unwrap(); assert_eq!(n, 2); }
    #[test] fn fingerprints_extract_blocking() { let s = InMemoryProgressStore::new(); s.record("L1", 1, &v(2)); let fps = s.previous_fingerprints("L1"); assert_eq!(fps.len(), 2); }
    #[test] fn empty_store_returns_none() { let s = InMemoryProgressStore::new(); assert!(s.previous("unknown").is_none()); }
}
