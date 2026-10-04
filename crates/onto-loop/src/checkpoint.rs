//! L3: Checkpoint + Evidence + Decision binding.
//!
//! Invariants:
//! - Attempt N's Evidence can only prove Attempt N's Checkpoint
//! - Decision must bind to the current Attempt's output hash
//! - Old-branch Evidence cannot commit new-branch artifacts

use onto_assurance_types::ids::{AttemptId, BundleId, CheckpointId, DecisionId};
use onto_assurance_types::transaction::ContentHash;

/// Each Attempt's input/output snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptCheckpoint {
    pub checkpoint_id: CheckpointId,
    pub attempt_id: AttemptId,
    pub input_state_hash: ContentHash,
    pub output_state_hash: ContentHash,
    pub parent_checkpoint_id: Option<CheckpointId>,
}

/// Full binding: Attempt → Checkpoint → Evidence → Decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptAuthorityBinding {
    pub attempt_id: AttemptId,
    pub checkpoint_id: CheckpointId,
    pub output_state_hash: ContentHash,
    pub evidence_bundle_id: BundleId,
    pub evidence_bundle_hash: ContentHash,
    pub decision_id: DecisionId,
}

impl AttemptAuthorityBinding {
    /// L3-1: Evidence must match the checkpoint it claims to prove.
    pub fn validate_evidence_matches_checkpoint(
        &self, actual_output_hash: &ContentHash,
    ) -> Result<(), String> {
        if &self.output_state_hash != actual_output_hash {
            return Err(format!(
                "evidence bound to hash {} but checkpoint has hash {}",
                self.output_state_hash, actual_output_hash
            ));
        }
        Ok(())
    }

    /// L3-2: Decision must bind to the current checkpoint.
    pub fn validate_decision_matches_binding(
        decision_id: DecisionId, binding: &Self,
    ) -> Result<(), String> {
        if decision_id != binding.decision_id {
            return Err("decision does not match binding".into());
        }
        Ok(())
    }
}

/// L3-3: Select the correct parent checkpoint for rollback.
pub fn select_rollback_checkpoint(
    current: &AttemptCheckpoint, history: &[AttemptCheckpoint],
) -> Option<AttemptCheckpoint> {
    current.parent_checkpoint_id.and_then(|parent_id| {
        history.iter().find(|c| c.checkpoint_id == parent_id).cloned()
    })
}

/// L3-4: Evidence from old branch must not apply to new branch.
pub fn validate_no_cross_branch_evidence(
    old_binding: &AttemptAuthorityBinding, new_checkpoint: &AttemptCheckpoint,
) -> Result<(), String> {
    if old_binding.output_state_hash != new_checkpoint.output_state_hash
        && old_binding.attempt_id != new_checkpoint.attempt_id
    {
        // Different attempt + different hash → cross-branch evidence rejected
        return Err("cross-branch evidence not allowed".into());
    }
    Ok(())
}

// ══════════════════════════════════════════════════════════════════
// Tests — L3.1 to L3.4
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(s: &str) -> ContentHash { ContentHash::new(s) }

    #[test]
    fn l3_1_evidence_must_match_checkpoint() {
        let binding = AttemptAuthorityBinding {
            attempt_id: AttemptId::new(), checkpoint_id: CheckpointId::new(),
            output_state_hash: hash("hash-A"), evidence_bundle_id: BundleId::new(),
            evidence_bundle_hash: hash("ev-A"), decision_id: DecisionId::new(),
        };
        // Evidence bound to hash-A, but actual is hash-B → rejected
        assert!(binding.validate_evidence_matches_checkpoint(&hash("hash-B")).is_err());
        // Evidence bound to hash-A, actual is hash-A → ok
        assert!(binding.validate_evidence_matches_checkpoint(&hash("hash-A")).is_ok());
    }

    #[test]
    fn l3_2_decision_must_bind_current_checkpoint() {
        let did1 = DecisionId::new();
        let did2 = DecisionId::new();
        let binding = AttemptAuthorityBinding {
            attempt_id: AttemptId::new(), checkpoint_id: CheckpointId::new(),
            output_state_hash: hash("h"), evidence_bundle_id: BundleId::new(),
            evidence_bundle_hash: hash("e"), decision_id: did1,
        };
        assert!(AttemptAuthorityBinding::validate_decision_matches_binding(did1, &binding).is_ok());
        assert!(AttemptAuthorityBinding::validate_decision_matches_binding(did2, &binding).is_err());
    }

    #[test]
    fn l3_3_rollback_selects_parent_checkpoint() {
        let c0 = CheckpointId::new();
        let c1 = CheckpointId::new();
        let history = vec![
            AttemptCheckpoint { checkpoint_id: c0, attempt_id: AttemptId::new(), input_state_hash: hash("base"), output_state_hash: hash("c0"), parent_checkpoint_id: None },
            AttemptCheckpoint { checkpoint_id: c1, attempt_id: AttemptId::new(), input_state_hash: hash("c0"), output_state_hash: hash("c1"), parent_checkpoint_id: Some(c0) },
        ];
        let current = &history[1];
        let rollback = select_rollback_checkpoint(current, &history);
        assert!(rollback.is_some());
        assert_eq!(rollback.unwrap().checkpoint_id, c0, "must rollback to parent C0");
    }

    #[test]
    fn l3_4_cross_branch_evidence_rejected() {
        let old = AttemptAuthorityBinding {
            attempt_id: AttemptId::new(), checkpoint_id: CheckpointId::new(),
            output_state_hash: hash("hash-old"), evidence_bundle_id: BundleId::new(),
            evidence_bundle_hash: hash("ev-old"), decision_id: DecisionId::new(),
        };
        let new_cp = AttemptCheckpoint {
            checkpoint_id: CheckpointId::new(), attempt_id: AttemptId::new(),
            input_state_hash: hash("base"), output_state_hash: hash("hash-new"),
            parent_checkpoint_id: None,
        };
        assert!(validate_no_cross_branch_evidence(&old, &new_cp).is_err());
    }
}
