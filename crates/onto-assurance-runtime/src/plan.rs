//! ConformancePlan builder — constructs plan from profile + artifact.
use onto_protocol::check::{ConformancePlan, ConformanceUnit, Applicability};
use onto_protocol::candidate::SealedCandidateRef;
use onto_protocol::context::VerificationContext;
use onto_protocol::digest::{Digest, DigestAlgorithm};
use onto_protocol::verifier::Pass;

fn d(s: &str) -> Digest {
    use sha2::{Sha256, Digest as SD};
    Digest::new(DigestAlgorithm::Sha256, hex::encode(Sha256::digest(s.as_bytes())))
}

/// Build a ConformancePlan for a given Candidate.
///
/// Currently generates a plan with all 9 Passes as Required units.
/// Future: read profile config to determine which passes are Required/Optional/NotApplicable.
pub fn build_plan(
    attempt_id: &str,
    candidate: &SealedCandidateRef,
    ctx: &VerificationContext,
) -> ConformancePlan {
    let plan_id = format!("plan-{}", attempt_id);
    let units: Vec<ConformanceUnit> = Pass::all().into_iter().enumerate().map(|(i, pass)| {
        ConformanceUnit {
            unit_id: format!("{}-{}", plan_id, i),
            verifier_id: format!("verifier-{:?}", pass).to_lowercase(),
            pass,
            validation_dependencies: vec![],
            applicability: Applicability::Required,
        }
    }).collect();

    ConformancePlan {
        plan_id: plan_id.clone(),
        plan_digest: d(&plan_id),
        attempt_id: attempt_id.to_string(),
        candidate_id: candidate.candidate_id.clone(),
        candidate_digest: candidate.digest.clone(),
        profile_id: "default".to_string(),
        profile_version: "1.0".to_string(),
        profile_digest: d("profile-default"),
        rule_set_digest: d("rules-default"),
        verifier_registry_digest: d("verifiers-default"),
        units,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_protocol::context::CandidateVerificationContext;

    #[test]
    fn builds_plan_with_9_units() {
        let c = SealedCandidateRef::new("c1", d("c1"), d("m1"), "/tmp");
        let ctx = VerificationContext::PreGraph(CandidateVerificationContext {
            attempt_id: "a1".into(), candidate: c.clone(),
            repository_name: "r".into(), base_commit_sha: "s".into(),
            execution_generation: 1, changed_files: vec![], language: "rust".into(),
        });
        let plan = build_plan("a1", &c, &ctx);
        assert_eq!(plan.units.len(), 9);
        assert_eq!(plan.candidate_digest, c.digest);
    }
}
