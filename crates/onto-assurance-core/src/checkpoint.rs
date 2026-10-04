//! Checkpoint binding — state snapshot for recovery and replay.
//!
//! Deterministic: same inputs always produce the same checkpoint binding
//! (timestamp is provided by the caller, not read from the system clock).

use chrono::{DateTime, Utc};
use onto_assurance_types::hash::{HashDomain, HashPurpose};
use onto_assurance_types::ids::{CheckpointId, TransactionId};
use onto_assurance_types::transaction::CheckpointBinding;

use crate::canonical;

// ══════════════════════════════════════════════════════════════════
// Checkpoint
// ══════════════════════════════════════════════════════════════════

/// Create a checkpoint binding for a transaction at a specific timestamp.
///
/// The `context_hash` binds the verification context (artifact tree,
/// requirement set, verifier version, toolchain, environment, policy
/// version, attempt number) at this point in the execution.
///
/// Timestamp is provided by the caller so replay is deterministic.
pub fn bind_checkpoint_at(
    transaction_id: TransactionId,
    context_hash: String,
    at: DateTime<Utc>,
) -> CheckpointBinding {
    CheckpointBinding {
        checkpoint_id: CheckpointId::new(),
        transaction_id,
        context_hash,
        created_at: at,
    }
}

/// Convenience wrapper that uses the current wall-clock time.
/// Not deterministic — use `bind_checkpoint_at` for replay.
pub fn bind_checkpoint(
    transaction_id: TransactionId,
    context_hash: String,
) -> CheckpointBinding {
    bind_checkpoint_at(transaction_id, context_hash, Utc::now())
}

/// Verify that a checkpoint's context hash matches the expected value.
pub fn verify_checkpoint(
    binding: &CheckpointBinding,
    expected_context_hash: &str,
) -> bool {
    binding.context_hash == expected_context_hash
}

/// Compute the context hash for a set of verification inputs.
///
/// The context hash binds:
///   - Artifact tree hashes
///   - Requirement set hashes
///   - Verifier version
///   - Toolchain
///   - Policy version
///   - Attempt number
pub fn compute_context_hash(
    artifact_hashes: &[String],
    requirement_hash: &str,
    verifier_version: &str,
    toolchain: &str,
    policy_version: &str,
    attempt: u32,
) -> Result<String, canonical::CanonicalError> {
    let domain = HashDomain::new(HashPurpose::Context, "CHECKPOINT");

    let input = serde_json::json!({
        "artifact_hashes": artifact_hashes,
        "requirement_hash": requirement_hash,
        "verifier_version": verifier_version,
        "toolchain": toolchain,
        "policy_version": policy_version,
        "attempt": attempt,
    });

    let hash = canonical::compute_hash(&input, &domain)?;
    Ok(hex::encode(hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_binding_roundtrip() {
        let txn_id = TransactionId::new();
        let binding = bind_checkpoint(txn_id, "test-context-hash".into());
        assert_eq!(binding.transaction_id, txn_id);
        assert!(verify_checkpoint(&binding, "test-context-hash"));
        assert!(!verify_checkpoint(&binding, "wrong-hash"));
    }

    #[test]
    fn context_hash_deterministic() {
        let h1 = compute_context_hash(
            &["hash1".into(), "hash2".into()],
            "req-hash",
            "1.0.0",
            "gcc-14",
            "v1",
            1,
        )
        .unwrap();

        let h2 = compute_context_hash(
            &["hash1".into(), "hash2".into()],
            "req-hash",
            "1.0.0",
            "gcc-14",
            "v1",
            1,
        )
        .unwrap();

        assert_eq!(h1, h2);
    }
}
