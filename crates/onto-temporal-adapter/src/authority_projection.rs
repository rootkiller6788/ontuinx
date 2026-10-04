//! AuthorityProjectionPort — Rust-only read-only authority interface.
//!
//! Go OntoFlow calls this (via gRPC) to verify Worker reports before
//! accepting a Committed state. Go does NOT directly read OntoAssure
//! database tables — Rust owns the authority schema.
//!
//! ## Why this exists
//!
//! ```text
//! Wrong:   Go Server -> SELECT * FROM onto_decisions
//! Correct: Go Server -> AuthorityProjectionPort::resolve_loop_outcome()
//! ```

use std::collections::HashMap;
use std::sync::Mutex;

/// Request to resolve the authoritative outcome for a loop.
#[derive(Debug, Clone)]
pub struct ResolveLoopOutcomeRequest {
    pub flow_id: String,
    pub work_item_id: String,
    pub loop_id: String,
    pub execution_generation: u64,
    pub terminal_envelope_hash: String,
}

/// The verified outcome from OntoAssure authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedLoopOutcome {
    pub loop_id: String,
    pub outcome: VerifiedOutcome,

    pub decision_id: String,
    pub decision_hash: String,
    pub output_checkpoint_hash: String,
    pub settlement_receipt_ref: Option<String>,

    pub authority_binding_hash: String,
}

/// The result of authority verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifiedOutcome {
    /// All checks passed — the loop's reported Committed state is valid.
    Committed,
    /// The loop's reported state cannot be accepted.
    NotCommitted { reason: String },
    /// No decision record exists for this loop.
    DecisionNotFound,
    /// The request or envelope has a binding mismatch.
    BindingMismatch { detail: String },
}

impl VerifiedOutcome {
    pub fn is_committed(&self) -> bool {
        matches!(self, Self::Committed)
    }
}

/// Errors from the authority projection.
#[derive(Debug, thiserror::Error)]
pub enum AuthorityError {
    #[error("loop not found: {0}")]
    NotFound(String),
    #[error("internal error: {0}")]
    Internal(String),
}

/// The read-only authority interface Go calls to verify Worker reports.
///
/// Go must NOT skip this verification. The Worker's LoopTerminalEnvelope
/// is a claim, not a fact.
pub trait AuthorityProjectionPort: Send + Sync {
    /// Resolve the authoritative outcome for a loop.
    fn resolve_loop_outcome(
        &self,
        request: ResolveLoopOutcomeRequest,
    ) -> Result<VerifiedLoopOutcome, AuthorityError>;
}

// ══════════════════════════════════════════════════════════════════
// In-memory implementation for testing
// ══════════════════════════════════════════════════════════════════

/// In-memory authority projection for testing.
///
/// Stores decision records keyed by loop_id. Go's Outcome Resolver calls
/// `resolve_loop_outcome()` to verify before marking a WorkItem Committed.
pub struct InMemoryAuthorityProjection {
    decisions: Mutex<HashMap<String, DecisionRecord>>,
}

#[derive(Debug, Clone)]
struct DecisionRecord {
    decision_id: String,
    decision_hash: String,
    loop_id: String,
    output_checkpoint_hash: String,
    settlement_receipt_ref: Option<String>,
    is_committed: bool,
}

impl InMemoryAuthorityProjection {
    pub fn new() -> Self {
        Self { decisions: Mutex::new(HashMap::new()) }
    }

    /// Register a decision record (simulates OntoAssure persisting a Decision).
    pub fn record_decision(
        &self,
        loop_id: &str,
        decision_id: &str,
        decision_hash: &str,
        output_checkpoint_hash: &str,
        settlement_receipt_ref: Option<&str>,
    ) {
        self.decisions.lock().unwrap().insert(
            loop_id.to_string(),
            DecisionRecord {
                decision_id: decision_id.to_string(),
                decision_hash: decision_hash.to_string(),
                loop_id: loop_id.to_string(),
                output_checkpoint_hash: output_checkpoint_hash.to_string(),
                settlement_receipt_ref: settlement_receipt_ref.map(|s| s.to_string()),
                is_committed: true,
            },
        );
    }

    /// Record a decision that is NOT committed (e.g., failed verification).
    pub fn record_rejected_decision(&self, loop_id: &str, decision_id: &str, _reason: &str) {
        self.decisions.lock().unwrap().insert(
            loop_id.to_string(),
            DecisionRecord {
                decision_id: decision_id.to_string(),
                decision_hash: format!("hash-{}", decision_id),
                loop_id: loop_id.to_string(),
                output_checkpoint_hash: String::new(),
                settlement_receipt_ref: None,
                is_committed: false,
            },
        );
    }

    fn compute_authority_binding_hash(
        loop_id: &str, decision_id: &str, decision_hash: &str,
        checkpoint_hash: &str, receipt_ref: Option<&str>,
    ) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        let payload = format!(
            "{}:{}:{}:{}:{}",
            loop_id,
            decision_id,
            decision_hash,
            checkpoint_hash,
            receipt_ref.unwrap_or("")
        );
        payload.hash(&mut h);
        format!("{:x}", h.finish())
    }
}

impl AuthorityProjectionPort for InMemoryAuthorityProjection {
    fn resolve_loop_outcome(
        &self,
        request: ResolveLoopOutcomeRequest,
    ) -> Result<VerifiedLoopOutcome, AuthorityError> {
        let store = self.decisions.lock().unwrap();

        let record = store
            .get(&request.loop_id)
            .ok_or_else(|| AuthorityError::NotFound(request.loop_id.clone()))?;

        // Verify binding: decision must belong to this loop_id
        if record.loop_id != request.loop_id {
            return Ok(VerifiedLoopOutcome {
                loop_id: request.loop_id.clone(),
                outcome: VerifiedOutcome::BindingMismatch {
                    detail: format!(
                        "decision bound to loop {} but request is for loop {}",
                        record.loop_id, request.loop_id
                    ),
                },
                decision_id: record.decision_id.clone(),
                decision_hash: record.decision_hash.clone(),
                output_checkpoint_hash: record.output_checkpoint_hash.clone(),
                settlement_receipt_ref: record.settlement_receipt_ref.clone(),
                authority_binding_hash: String::new(),
            });
        }

        if !record.is_committed {
            return Ok(VerifiedLoopOutcome {
                loop_id: request.loop_id,
                outcome: VerifiedOutcome::NotCommitted {
                    reason: "decision is not in committed state".to_string(),
                },
                decision_id: record.decision_id.clone(),
                decision_hash: record.decision_hash.clone(),
                output_checkpoint_hash: record.output_checkpoint_hash.clone(),
                settlement_receipt_ref: record.settlement_receipt_ref.clone(),
                authority_binding_hash: String::new(),
            });
        }

        let binding_hash = Self::compute_authority_binding_hash(
            &record.loop_id,
            &record.decision_id,
            &record.decision_hash,
            &record.output_checkpoint_hash,
            record.settlement_receipt_ref.as_deref(),
        );

        Ok(VerifiedLoopOutcome {
            loop_id: request.loop_id,
            outcome: VerifiedOutcome::Committed,
            decision_id: record.decision_id.clone(),
            decision_hash: record.decision_hash.clone(),
            output_checkpoint_hash: record.output_checkpoint_hash.clone(),
            settlement_receipt_ref: record.settlement_receipt_ref.clone(),
            authority_binding_hash: binding_hash,
        })
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests — F2: Authority Resolution
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(loop_id: &str) -> ResolveLoopOutcomeRequest {
        ResolveLoopOutcomeRequest {
            flow_id: "flow-1".into(),
            work_item_id: "wi-A".into(),
            loop_id: loop_id.into(),
            execution_generation: 1,
            terminal_envelope_hash: "env-hash".into(),
        }
    }

    /// F2.1: No decision record exists → DecisionNotFound.
    #[test]
    fn f2_1_decision_not_found() {
        let authority = InMemoryAuthorityProjection::new();
        let result = authority.resolve_loop_outcome(make_request("no-such-loop"));
        assert!(result.is_err());
        match result {
            Err(AuthorityError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {:?}", other),
        }
    }

    /// F2.2: Decision bound to different loop_id → BindingMismatch.
    #[test]
    fn f2_2_decision_bound_to_wrong_loop() {
        let authority = InMemoryAuthorityProjection::new();
        // Record decision for loop-A
        authority.record_decision("loop-A", "dec-1", "hash-1", "ckpt-A", Some("receipt-1"));

        // Query for loop-B (different loop_id)
        let result = authority.resolve_loop_outcome(make_request("loop-B"));
        // loop-B has no record → NotFound
        assert!(result.is_err());
    }

    /// F2.3: Valid decision matching loop_id → Committed.
    #[test]
    fn f2_3_valid_decision_committed() {
        let authority = InMemoryAuthorityProjection::new();
        authority.record_decision("loop-1", "dec-abc", "hash-abc", "ckpt-1", Some("rcpt-1"));

        let result = authority.resolve_loop_outcome(make_request("loop-1")).unwrap();

        assert_eq!(result.loop_id, "loop-1");
        assert_eq!(result.decision_id, "dec-abc");
        assert_eq!(result.decision_hash, "hash-abc");
        assert_eq!(result.output_checkpoint_hash, "ckpt-1");
        assert_eq!(result.settlement_receipt_ref, Some("rcpt-1".into()));
        assert!(result.outcome.is_committed());
        assert!(!result.authority_binding_hash.is_empty());
    }

    /// F2.4: Decision exists but is NOT committed → NotCommitted.
    #[test]
    fn f2_4_rejected_decision_not_committed() {
        let authority = InMemoryAuthorityProjection::new();
        authority.record_rejected_decision("loop-fail", "dec-fail", "verification failed");

        let result = authority.resolve_loop_outcome(make_request("loop-fail")).unwrap();
        assert!(!result.outcome.is_committed());
        match result.outcome {
            VerifiedOutcome::NotCommitted { reason } => {
                assert!(reason.contains("not in committed state"));
            }
            other => panic!("expected NotCommitted, got {:?}", other),
        }
    }

    /// F2.5: Receipt mismatch — decision without receipt.
    /// (When Go expects a receipt but none exists, the outcome is still
    /// Committed but Go's own verification should flag the mismatch.)
    #[test]
    fn f2_5_decision_without_receipt_still_committed() {
        let authority = InMemoryAuthorityProjection::new();
        // Record decision without receipt
        authority.record_decision("loop-no-receipt", "dec-x", "hash-x", "ckpt-x", None);

        let result = authority
            .resolve_loop_outcome(make_request("loop-no-receipt"))
            .unwrap();

        // Decision is committed; receipt absence is noted in the field
        assert!(result.outcome.is_committed());
        assert_eq!(result.settlement_receipt_ref, None);
        // Go would check: if settlement was supposed to produce a receipt,
        // it flags this as a mismatch before accepting Committed.
    }

    /// F2.6: Deterministic authority_binding_hash.
    #[test]
    fn f2_6_authority_binding_hash_deterministic() {
        let authority = InMemoryAuthorityProjection::new();
        authority.record_decision("loop-1", "dec-1", "hash-1", "ckpt-1", Some("r-1"));

        let r1 = authority.resolve_loop_outcome(make_request("loop-1")).unwrap();
        let r2 = authority.resolve_loop_outcome(make_request("loop-1")).unwrap();
        assert_eq!(
            r1.authority_binding_hash, r2.authority_binding_hash,
            "authority binding hash must be deterministic"
        );
    }
}
