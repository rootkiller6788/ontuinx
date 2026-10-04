//! L4: Deterministic Progress / Regression detection.
//!
//! Judgment comes from OntoAssure structured results, not from LLM self-assessment.

use onto_assurance_types::ids::CriterionId;
use onto_assurance_types::transaction::ContentHash;

/// Snapshot of what was satisfied/unsatisfied in one attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgressSnapshot {
    pub satisfied_criteria: Vec<CriterionId>,
    pub unsatisfied_criteria: Vec<CriterionId>,
    pub protected_scope_violations: u32,
    pub failed_verifiers: u32,
    pub environment_errors: u32,
    pub checkpoint_hash: ContentHash,
}

/// Result of comparing two attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressComparison {
    Improved,
    Unchanged,
    Regressed,
    Incomparable,
}

/// L4: Pure function — compare two snapshots deterministically.
fn count_not_in(a: &[CriterionId], b: &[CriterionId]) -> usize {
    a.iter().filter(|id| !b.contains(id)).count()
}

pub fn compare_progress(prev: &ProgressSnapshot, curr: &ProgressSnapshot) -> ProgressComparison {
    let gained = count_not_in(&curr.satisfied_criteria, &prev.satisfied_criteria);
    let lost = count_not_in(&prev.satisfied_criteria, &curr.satisfied_criteria);

    if lost == 0 && gained > 0
        && curr.protected_scope_violations <= prev.protected_scope_violations
        && curr.environment_errors <= prev.environment_errors
    { return ProgressComparison::Improved; }

    if gained == 0 && lost == 0
        && curr.unsatisfied_criteria == prev.unsatisfied_criteria
        && curr.protected_scope_violations == prev.protected_scope_violations
    { return ProgressComparison::Unchanged; }

    let has_regression = lost > 0
        || curr.protected_scope_violations > prev.protected_scope_violations
        || curr.failed_verifiers > prev.failed_verifiers;

    let has_improvement = gained > 0
        && curr.protected_scope_violations <= prev.protected_scope_violations;

    if has_regression && has_improvement {
        return ProgressComparison::Incomparable;
    }
    if has_regression {
        return ProgressComparison::Regressed;
    }

    ProgressComparison::Improved
}

// ══════════════════════════════════════════════════════════════════
// Tests — L4.1 to L4.4
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // Use fixed IDs so compare_progress works correctly
    static C1: std::sync::OnceLock<CriterionId> = std::sync::OnceLock::new();
    static C2: std::sync::OnceLock<CriterionId> = std::sync::OnceLock::new();
    static C3: std::sync::OnceLock<CriterionId> = std::sync::OnceLock::new();
    fn c1() -> CriterionId { *C1.get_or_init(CriterionId::new) }
    fn c2() -> CriterionId { *C2.get_or_init(CriterionId::new) }
    fn c3() -> CriterionId { *C3.get_or_init(CriterionId::new) }
    fn h(s: &str) -> ContentHash { ContentHash::new(s) }
    fn snap(sat: &[CriterionId], unsat: &[CriterionId], violations: u32) -> ProgressSnapshot {
        ProgressSnapshot {
            satisfied_criteria: sat.to_vec(),
            unsatisfied_criteria: unsat.to_vec(),
            protected_scope_violations: violations,
            failed_verifiers: unsat.len() as u32,
            environment_errors: 0,
            checkpoint_hash: h("h"),
        }
    }

    #[test]
    fn l4_1_gained_criterion_improved() {
        let prev = snap(&[c1()], &[c2(), c3()], 0);
        let curr = snap(&[c1(), c2()], &[c3()], 0);
        assert_eq!(compare_progress(&prev, &curr), ProgressComparison::Improved);
    }
    #[test]
    fn l4_2_hash_changed_but_criteria_unchanged() {
        let prev = snap(&[c1()], &[c2()], 0);
        let mut curr = prev.clone();
        curr.checkpoint_hash = h("different-hash");
        assert_eq!(compare_progress(&prev, &curr), ProgressComparison::Unchanged);
    }
    #[test]
    fn l4_3_lost_criterion_regressed() {
        let prev = snap(&[c1(), c2()], &[], 0);
        let curr = snap(&[c2()], &[c1()], 0);
        assert_eq!(compare_progress(&prev, &curr), ProgressComparison::Regressed);
    }
    #[test]
    fn l4_4_gained_and_lost_incomparable() {
        let prev = snap(&[c1()], &[c2()], 0);
        let curr = snap(&[c2()], &[c1()], 0);
        assert_eq!(compare_progress(&prev, &curr), ProgressComparison::Incomparable);
    }
    #[test]
    fn l4_5_violations_increased_regressed() {
        let prev = snap(&[c1()], &[], 0);
        let curr = snap(&[c1()], &[], 1);
        assert_eq!(compare_progress(&prev, &curr), ProgressComparison::Regressed);
    }
}
