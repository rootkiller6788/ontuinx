//! VerdictReducer — truth-table based conformance reduction.
use onto_protocol::check::{Applicability, ConformancePlan, ConformancePlanSummary};
use onto_protocol::digest::{Digest, DigestAlgorithm};
use onto_protocol::finding::Finding;
use onto_protocol::verdict::{
    ConformanceVerdict, ConformanceOutcome, CoverageState, FreshnessState,
    ConformanceUnitResult, GraphValidationBinding, SandboxValidationSummary,
    UnitExecutionStatus, GraphUnavailableReason,
};

pub fn reduce(
    plan: &ConformancePlan,
    blocking: Vec<Finding>,
    advisory: Vec<Finding>,
    unit_results: Vec<ConformanceUnitResult>,
    sandbox: SandboxValidationSummary,
    graph: GraphValidationBinding,
) -> ConformanceVerdict {
    let required: Vec<&ConformanceUnitResult> = unit_results.iter()
        .filter(|u| u.applicability == Applicability::Required).collect();
    let all_ok = required.iter().all(|u| u.status == UnitExecutionStatus::Passed);
    let any_fail = required.iter().any(|u| matches!(u.status, UnitExecutionStatus::Failed));
    let any_infra = required.iter().any(|u| matches!(u.status,
        UnitExecutionStatus::Unavailable | UnitExecutionStatus::PrerequisiteFailed
        | UnitExecutionStatus::TimedOut | UnitExecutionStatus::EvidenceIncomplete));

    let (c, f, cov) = if !blocking.is_empty() {
        (ConformanceOutcome::NonConformant, FreshnessState::Current, CoverageState::Complete)
    } else if all_ok {
        (ConformanceOutcome::Conformant, FreshnessState::Current, CoverageState::Complete)
    } else if any_infra {
        (ConformanceOutcome::Inconclusive, FreshnessState::Current, CoverageState::Partial)
    } else if any_fail {
        (ConformanceOutcome::NonConformant, FreshnessState::Current, CoverageState::Partial)
    } else {
        (ConformanceOutcome::Inconclusive, FreshnessState::Current, CoverageState::Partial)
    };

    ConformanceVerdict {
        verdict_id: uuid::Uuid::new_v4().to_string(),
        verdict_digest: d(&format!("v-{}-{}", plan.attempt_id, uuid::Uuid::new_v4())),
        attempt_id: plan.attempt_id.clone(),
        candidate_id: plan.candidate_id.clone(),
        candidate_digest: plan.candidate_digest.clone(),
        graph,
        plan_id: plan.plan_id.clone(),
        plan_digest: plan.plan_digest.clone(),
        evidence_bundle_ref: format!("evidence/{}", plan.attempt_id),
        evidence_bundle_digest: d("evidence"),
        conformance: c, freshness: f, coverage: cov, sandbox,
        blocking_findings: blocking, advisory_findings: advisory, unit_results,
        plan: ConformancePlanSummary { plan_id: plan.plan_id.clone(), profile_id: plan.profile_id.clone(), total_units: plan.units.len() as u32, required_units: plan.units.iter().filter(|u| u.applicability == Applicability::Required).count() as u32, applied_units: plan.units.len() as u32 },
    }
}

fn d(data: &str) -> Digest {
    use sha2::{Sha256, Digest as SD};
    Digest::new(DigestAlgorithm::Sha256, hex::encode(Sha256::digest(data.as_bytes())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_protocol::check::ConformanceUnit;
    use onto_protocol::finding::{FindingFingerprint, FindingSeverity, CategoryId, FindingDisposition, RemediationClass};
    use onto_protocol::verifier::Pass;
    use onto_protocol::sandbox::SandboxExecutionStatus;

    fn ph() -> Digest { Digest::new(DigestAlgorithm::Sha256, "aa") }
    fn plan() -> ConformancePlan { ConformancePlan { plan_id: "p1".into(), plan_digest: ph(), attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: ph(), profile_id: "pf".into(), profile_version: "1".into(), profile_digest: ph(), rule_set_digest: ph(), verifier_registry_digest: ph(), units: vec![ConformanceUnit { unit_id: "u1".into(), verifier_id: "v1".into(), pass: Pass::Build, validation_dependencies: vec![], applicability: Applicability::Required }] } }
    fn ur(status: UnitExecutionStatus) -> ConformanceUnitResult { ConformanceUnitResult { unit_id: "u1".into(), verifier_id: "v1".into(), pass: Pass::Build, applicability: Applicability::Required, status, finding_ids: vec![], evidence_refs: vec![], duration_ms: 0 } }
    fn bf(msg: &str) -> Finding { Finding { finding_id: "f1".into(), fingerprint: FindingFingerprint { rule_id: "r1".into(), entity_key: None, artifact_path: "x".into(), semantic_key: "k".into(), line_hint: None }, pass: Pass::Build, rule_id: "r1".into(), rule_version: "1".into(), severity: FindingSeverity::Critical, category: CategoryId::new("c"), disposition: FindingDisposition::Blocking, remediation: RemediationClass::RetryWithFeedback, location: None, message: msg.into(), fix_hint: None, confidence: 1.0, evidence_refs: vec![] } }
    fn gb() -> GraphValidationBinding { GraphValidationBinding::Sealed { snapshot_id: "s1".into(), snapshot_digest: ph(), source_candidate_digest: ph() } }
    fn sb() -> SandboxValidationSummary { SandboxValidationSummary::Executed { request_digest: ph(), environment_digest: ph(), run_ref: "r".into(), status: SandboxExecutionStatus::Completed, observation_refs: vec![] } }

    #[test] fn all_passed_is_conformant() { let v = reduce(&plan(), vec![], vec![], vec![ur(UnitExecutionStatus::Passed)], sb(), gb()); assert_eq!(v.conformance, ConformanceOutcome::Conformant); }
    #[test] fn blocking_is_nonconformant() { let v = reduce(&plan(), vec![bf("e")], vec![], vec![], sb(), gb()); assert_eq!(v.conformance, ConformanceOutcome::NonConformant); }
    #[test] fn unavailable_is_inconclusive() { let v = reduce(&plan(), vec![], vec![], vec![ur(UnitExecutionStatus::Unavailable)], SandboxValidationSummary::StartupFailed { request_digest: ph(), diagnostic_ref: "d".into() }, gb()); assert_eq!(v.conformance, ConformanceOutcome::Inconclusive); }
}
