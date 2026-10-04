//! Progress — cross-Attempt Finding comparison for Loop convergence.

use crate::finding::FindingFingerprint;

/// Snapshot of what was found in one Attempt — used to detect progress/regression.
#[derive(Debug, Clone)]
pub struct ProgressSnapshot {
    pub attempt_number: u32,
    pub total_blocking: u32,
    pub total_advisory: u32,
    pub resolved_fingerprints: Vec<FindingFingerprint>,
    pub new_fingerprints: Vec<FindingFingerprint>,
    pub persistent_fingerprints: Vec<FindingFingerprint>,
    pub regressions: Vec<FindingFingerprint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressComparison {
    Improved,
    Unchanged,
    Regressed,
    Incomparable,
}

/// Compare two Attempts at the Finding fingerprint level.
pub fn compare_progress(_prev: &ProgressSnapshot, curr: &ProgressSnapshot) -> ProgressComparison {
    let resolved = curr.resolved_fingerprints.len();
    let new_count = curr.new_fingerprints.len();
    let regression_count = curr.regressions.len();

    if resolved > 0 && new_count == 0 && regression_count == 0 {
        ProgressComparison::Improved
    } else if resolved == 0 && new_count == 0 && regression_count == 0 {
        ProgressComparison::Unchanged
    } else if regression_count > 0 && resolved == 0 {
        ProgressComparison::Regressed
    } else if regression_count > 0 && resolved > 0 {
        ProgressComparison::Incomparable
    } else if new_count > 0 {
        ProgressComparison::Improved // new findings but no regressions = still moving
    } else {
        ProgressComparison::Unchanged
    }
}
