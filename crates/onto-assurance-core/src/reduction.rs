//! Deterministic Evidence Reduction.
//!
//! Answers:
//!   - Which evidence supports which criterion?
//!   - Are all blocking requirements satisfied?
//!   - What is the aggregate RequirementVerdict?
//!
//! Pure function — no side effects, no I/O, no OntoRuntime dependencies.

use onto_assurance_types::contract::AcceptanceCriterion;
use onto_assurance_types::evidence::{CriterionVerdict, EvidenceRecord, RequirementVerdict};
use onto_assurance_types::ids::CriterionId;

// ══════════════════════════════════════════════════════════════════
// Reduction
// ══════════════════════════════════════════════════════════════════

/// Reduce evidence records to per-criterion verdicts and an aggregate result.
///
/// Invariants:
///   - No valid Evidence → every criterion is UNSATISFIED
///   - Any blocking UNSATISFIED → overall_passed = false
///   - EnvironmentError → no COMMIT permitted (checked upstream)
pub fn reduce(
    criteria: &[AcceptanceCriterion],
    evidence: &[EvidenceRecord],
) -> RequirementVerdict {
    let mut criterion_verdicts: Vec<CriterionVerdict> = Vec::new();

    for criterion in criteria {
        let supporting: Vec<_> = evidence
            .iter()
            .filter(|e| e.criterion_id == criterion.criterion_id)
            .collect();

        let satisfied = !supporting.is_empty()
            && supporting.iter().all(|e| is_evidence_positive(e));

        let evidence_ids: Vec<_> = supporting.iter().map(|e| e.evidence_id).collect();

        criterion_verdicts.push(CriterionVerdict {
            criterion_id: criterion.criterion_id,
            satisfied,
            evidence_ids,
            detail: if satisfied {
                format!("{} evidence record(s) support this criterion", supporting.len())
            } else if supporting.is_empty() {
                "no evidence provided".into()
            } else {
                "evidence does not satisfy this criterion".into()
            },
        });
    }

    let overall_passed = !criterion_verdicts.iter().any(|v| {
        let criterion = criteria.iter().find(|c| c.criterion_id == v.criterion_id);
        criterion.map(|c| c.is_blocking && !v.satisfied).unwrap_or(false)
    });

    let blocking_unsatisfied: Vec<CriterionId> = criteria
        .iter()
        .filter(|c| {
            c.is_blocking
                && criterion_verdicts
                    .iter()
                    .any(|v| v.criterion_id == c.criterion_id && !v.satisfied)
        })
        .map(|c| c.criterion_id)
        .collect();

    let non_blocking_unsatisfied: Vec<CriterionId> = criteria
        .iter()
        .filter(|c| {
            !c.is_blocking
                && criterion_verdicts
                    .iter()
                    .any(|v| v.criterion_id == c.criterion_id && !v.satisfied)
        })
        .map(|c| c.criterion_id)
        .collect();

    RequirementVerdict {
        overall_passed,
        blocking_unsatisfied,
        non_blocking_unsatisfied,
        criterion_verdicts,
    }
}

/// Check whether a single evidence record is "positive" (supports satisfaction).
fn is_evidence_positive(record: &EvidenceRecord) -> bool {
    // Heuristic: check payload for failure indicators.
    // This is intentionally simple — the VerifierBinding determines what
    // "positive" means for each evidence kind.
    let payload = &record.payload;

    // Explicit failure flag
    if let Some(false) = payload.get("passed").and_then(|v| v.as_bool()) {
        return false;
    }
    if let Some(true) = payload.get("failed").and_then(|v| v.as_bool()) {
        return false;
    }
    if let Some(code) = payload.get("exit_code").and_then(|v| v.as_i64()) {
        if code != 0 {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::contract::{AcceptanceCriterion, CriterionKind};
    use onto_assurance_types::evidence::{EvidenceRecord, EvidenceRecordKind};
    use onto_assurance_types::ids::{CriterionId, EvidenceId, TransactionId};
    use chrono::Utc;

    fn make_criterion(id: CriterionId, name: &str, blocking: bool) -> AcceptanceCriterion {
        AcceptanceCriterion {
            criterion_id: id,
            name: name.into(),
            kind: CriterionKind::TestPass,
            description: "test".into(),
            is_blocking: blocking,
        }
    }

    fn make_evidence(criterion_id: CriterionId, passed: bool) -> EvidenceRecord {
        EvidenceRecord {
            evidence_id: EvidenceId::new(),
            transaction_id: TransactionId::new(),
            criterion_id,
            kind: EvidenceRecordKind::TestOutput,
            payload: serde_json::json!({"passed": passed}),
            recorded_at: Utc::now(),
        }
    }

    #[test]
    fn all_satisfied_passes() {
        let cid = CriterionId::new();
        let criteria = vec![make_criterion(cid, "tests", true)];
        let evidence = vec![make_evidence(cid, true)];

        let verdict = reduce(&criteria, &evidence);
        assert!(verdict.overall_passed);
        assert!(verdict.blocking_unsatisfied.is_empty());
    }

    #[test]
    fn blocking_unsatisfied_fails() {
        let cid = CriterionId::new();
        let criteria = vec![make_criterion(cid, "tests", true)];
        let evidence = vec![make_evidence(cid, false)];

        let verdict = reduce(&criteria, &evidence);
        assert!(!verdict.overall_passed);
        assert_eq!(verdict.blocking_unsatisfied.len(), 1);
    }

    #[test]
    fn no_evidence_no_success() {
        let cid = CriterionId::new();
        let criteria = vec![make_criterion(cid, "tests", true)];
        let evidence = vec![];

        let verdict = reduce(&criteria, &evidence);
        assert!(!verdict.overall_passed);
    }

    #[test]
    fn non_blocking_unsatisfied_still_passes() {
        let cid1 = CriterionId::new();
        let cid2 = CriterionId::new();
        let criteria = vec![
            make_criterion(cid1, "blocking", true),
            make_criterion(cid2, "non-blocking", false),
        ];
        let evidence = vec![make_evidence(cid1, true)];

        let verdict = reduce(&criteria, &evidence);
        assert!(verdict.overall_passed);
        assert_eq!(verdict.non_blocking_unsatisfied.len(), 1);
    }

    #[test]
    fn duplicate_evidence_no_increase() {
        let cid = CriterionId::new();
        let criteria = vec![make_criterion(cid, "tests", true)];
        let e = make_evidence(cid, true);
        let evidence = vec![e.clone(), e.clone()]; // duplicate

        let verdict = reduce(&criteria, &evidence);
        assert!(verdict.overall_passed);
        // Duplicate copies don't increase proof weight
    }

    #[test]
    fn property_no_criteria_no_problem() {
        // Empty criteria with evidence → should trivially pass
        let verdict = reduce(&[], &[]);
        assert!(verdict.overall_passed);
    }

    #[test]
    fn property_evidence_for_unknown_criterion_ignored() {
        let cid1 = CriterionId::new();
        let cid2 = CriterionId::new();
        let criteria = vec![make_criterion(cid1, "only-this", true)];
        // Evidence for cid2 (not in criteria) + evidence for cid1
        let evidence = vec![
            make_evidence(cid2, true), // unknown → ignored
            make_evidence(cid1, true), // known → counted
        ];
        let verdict = reduce(&criteria, &evidence);
        assert!(verdict.overall_passed);
    }

    #[test]
    fn property_cross_criterion_evidence_independence() {
        let cid1 = CriterionId::new();
        let cid2 = CriterionId::new();
        let criteria = vec![
            make_criterion(cid1, "tests", true),
            make_criterion(cid2, "lint", true),
        ];
        // Evidence only for cid1 — cid2 unsatisfied
        let evidence = vec![make_evidence(cid1, true)];
        let verdict = reduce(&criteria, &evidence);
        assert!(!verdict.overall_passed); // cid2 is blocking + unsatisfied
        assert_eq!(verdict.blocking_unsatisfied.len(), 1);
        assert_eq!(verdict.blocking_unsatisfied[0], cid2);
    }
}
