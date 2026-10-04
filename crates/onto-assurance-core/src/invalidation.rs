//! Evidence invalidation — when should old evidence be rejected?
//!
//! Pure function — deterministic, no I/O.
//!
//! Invariants:
//!   - Code changed → old Evidence invalidated
//!   - Requirements changed → old Evidence invalidated
//!   - Verifier changed → old Evidence invalidated
//!   - Cross-attempt Evidence → rejected
//!   - Evidence reordered → decision unchanged (hash chain handles this)

use onto_assurance_types::evidence::EvidenceRecord;
use onto_assurance_types::ids::AttemptId;

// ══════════════════════════════════════════════════════════════════
// Invalidation Rules
// ══════════════════════════════════════════════════════════════════

/// Check whether an EvidenceRecord is still valid for the current attempt.
///
/// Cross-attempt evidence is always rejected.
pub fn is_valid_for_attempt(
    _record: &EvidenceRecord,
    current_attempt: AttemptId,
    record_attempt: AttemptId,
) -> bool {
    record_attempt == current_attempt
}

/// Check whether evidence is invalidated by a code change.
///
/// If the artifact hash has changed since the evidence was recorded,
/// the evidence may no longer apply.  This is a conservative check:
/// any hash mismatch → invalidated.
pub fn is_invalidated_by_code_change(
    _record: &EvidenceRecord,
    original_artifact_hash: &str,
    current_artifact_hash: &str,
) -> bool {
    original_artifact_hash != current_artifact_hash
}

/// Check whether evidence is invalidated by a requirement change.
pub fn is_invalidated_by_requirement_change(
    _record: &EvidenceRecord,
    original_requirement_hash: &str,
    current_requirement_hash: &str,
) -> bool {
    original_requirement_hash != current_requirement_hash
}

/// Check whether evidence is invalidated by a verifier version change.
pub fn is_invalidated_by_verifier_change(
    _record: &EvidenceRecord,
    original_verifier_version: &str,
    current_verifier_version: &str,
) -> bool {
    original_verifier_version != current_verifier_version
}

/// Context for evidence validation — bundles all comparison parameters.
#[derive(Debug, Clone)]
pub struct ValidationContext {
    pub current_attempt: AttemptId,
    pub record_attempt: AttemptId,
    pub original_artifact_hash: String,
    pub current_artifact_hash: String,
    pub original_requirement_hash: String,
    pub current_requirement_hash: String,
    pub original_verifier_version: String,
    pub current_verifier_version: String,
}

/// Aggregate invalidation check.
///
/// Returns `EvidenceValidity::Valid` if the evidence is still valid,
/// or `Invalid { reason }` if it should be discarded and re-generated.
pub fn validate_evidence(
    record: &EvidenceRecord,
    ctx: &ValidationContext,
) -> EvidenceValidity {
    if !is_valid_for_attempt(record, ctx.current_attempt, ctx.record_attempt) {
        return EvidenceValidity::Invalid {
            reason: "cross-attempt evidence rejected".into(),
        };
    }

    if is_invalidated_by_code_change(
        record,
        &ctx.original_artifact_hash,
        &ctx.current_artifact_hash,
    ) {
        return EvidenceValidity::Invalid {
            reason: "code artifact changed".into(),
        };
    }

    if is_invalidated_by_requirement_change(
        record,
        &ctx.original_requirement_hash,
        &ctx.current_requirement_hash,
    ) {
        return EvidenceValidity::Invalid {
            reason: "requirements changed".into(),
        };
    }

    if is_invalidated_by_verifier_change(
        record,
        &ctx.original_verifier_version,
        &ctx.current_verifier_version,
    ) {
        return EvidenceValidity::Invalid {
            reason: "verifier version changed".into(),
        };
    }

    EvidenceValidity::Valid
}

// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceValidity {
    Valid,
    Invalid { reason: String },
}

impl EvidenceValidity {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use onto_assurance_types::evidence::{EvidenceRecord, EvidenceRecordKind};
    use onto_assurance_types::ids::{CriterionId, EvidenceId, TransactionId};

    fn make_record() -> EvidenceRecord {
        EvidenceRecord {
            evidence_id: EvidenceId::new(),
            transaction_id: TransactionId::new(),
            criterion_id: CriterionId::new(),
            kind: EvidenceRecordKind::TestOutput,
            payload: serde_json::json!({"passed": true}),
            recorded_at: Utc::now(),
        }
    }

    fn ctx(
        current_attempt: AttemptId,
        record_attempt: AttemptId,
        artifact_old: &str,
        artifact_new: &str,
        req_old: &str,
        req_new: &str,
        ver_old: &str,
        ver_new: &str,
    ) -> ValidationContext {
        ValidationContext {
            current_attempt,
            record_attempt,
            original_artifact_hash: artifact_old.into(),
            current_artifact_hash: artifact_new.into(),
            original_requirement_hash: req_old.into(),
            current_requirement_hash: req_new.into(),
            original_verifier_version: ver_old.into(),
            current_verifier_version: ver_new.into(),
        }
    }

    #[test]
    fn cross_attempt_evidence_rejected() {
        let a1 = AttemptId::new();
        let a2 = AttemptId::new();
        let record = make_record();
        let result = validate_evidence(&record, &ctx(a1, a2, "h1", "h1", "h1", "h1", "v1", "v1"));
        assert!(!result.is_valid());
    }

    #[test]
    fn code_change_invalidates() {
        let a = AttemptId::new();
        let record = make_record();
        let result = validate_evidence(&record, &ctx(a, a, "old", "new", "h1", "h1", "v1", "v1"));
        assert!(!result.is_valid());
    }

    #[test]
    fn no_change_valid() {
        let a = AttemptId::new();
        let record = make_record();
        let result = validate_evidence(&record, &ctx(a, a, "h1", "h1", "h1", "h1", "v1", "v1"));
        assert!(result.is_valid());
    }

    #[test]
    fn requirement_change_invalidates() {
        let a = AttemptId::new();
        let record = make_record();
        let result = validate_evidence(&record, &ctx(a, a, "h1", "h1", "old-req", "new-req", "v1", "v1"));
        assert!(!result.is_valid());
    }

    #[test]
    fn verifier_change_invalidates() {
        let a = AttemptId::new();
        let record = make_record();
        let result = validate_evidence(&record, &ctx(a, a, "h1", "h1", "h1", "h1", "v1", "v2"));
        assert!(!result.is_valid());
    }
}
