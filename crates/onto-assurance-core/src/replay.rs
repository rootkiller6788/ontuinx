//! Replay engine — deterministic re-execution verification.
//!
//! Pure function.  Given the same inputs, produces the same replay hash.
//! Used to verify that the kernel produces identical results to the
//! Python reference, and that a replayed run matches the original.

use onto_assurance_types::evidence::EvidenceBundle;
use onto_assurance_types::hash::{HashDomain, HashPurpose};

use crate::canonical;

// ══════════════════════════════════════════════════════════════════
// Replay Input / Output
// ══════════════════════════════════════════════════════════════════

/// The input needed to replay a run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReplayInput {
    pub run_id: String,
    pub contract: serde_json::Value,
    pub transactions: Vec<serde_json::Value>,
    pub checkpoint_bindings: Vec<serde_json::Value>,
}

/// The result of replaying a run.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReplayResult {
    pub replay_hash: String,
    pub evidence_hash: String,
    pub decision_hash: String,
    pub matches_original: bool,
}

// ══════════════════════════════════════════════════════════════════
// Replay
// ══════════════════════════════════════════════════════════════════

/// Compute a replay hash for the given input.
///
/// This is a deterministic fingerprint of the entire run: contract +
/// transactions + checkpoints + evidence.  Two runs with identical
/// ReplayInput MUST produce identical ReplayResult.
pub fn compute_replay_hash(input: &ReplayInput) -> Result<String, canonical::CanonicalError> {
    let domain = HashDomain::new(HashPurpose::Content, "REPLAY");
    let hash = canonical::compute_hash(input, &domain)?;
    Ok(hex::encode(hash))
}

/// Compare a replayed result against the original evidence bundle.
pub fn verify_replay(
    original: &EvidenceBundle,
    replayed: &EvidenceBundle,
) -> bool {
    original.tail_hash == replayed.tail_hash
        && original.genesis_hash == replayed.genesis_hash
        && original.record_count == replayed.record_count
}

/// Verify the replay result against expected values from Python reference.
pub fn verify_replay_against_expected(
    result: &ReplayResult,
    expected_evidence_hash: &str,
    expected_decision_hash: &str,
) -> bool {
    result.evidence_hash == expected_evidence_hash
        && result.decision_hash == expected_decision_hash
        && result.matches_original
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_hash_deterministic() {
        let input = ReplayInput {
            run_id: "test-run-1".into(),
            contract: serde_json::json!({"objective": "test"}),
            transactions: vec![],
            checkpoint_bindings: vec![],
        };

        let h1 = compute_replay_hash(&input).unwrap();
        let h2 = compute_replay_hash(&input).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_inputs_different_replay_hash() {
        let input1 = ReplayInput {
            run_id: "test-run-1".into(),
            contract: serde_json::json!({"objective": "test"}),
            transactions: vec![],
            checkpoint_bindings: vec![],
        };

        let input2 = ReplayInput {
            run_id: "test-run-2".into(),
            contract: serde_json::json!({"objective": "different"}),
            transactions: vec![],
            checkpoint_bindings: vec![],
        };

        let h1 = compute_replay_hash(&input1).unwrap();
        let h2 = compute_replay_hash(&input2).unwrap();
        assert_ne!(h1, h2);
    }
}
