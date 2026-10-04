//! Canonical hash computation — MUST match Python reference byte-for-byte.
//!
//! This is the most critical module in the entire kernel.  Any divergence
//! between Python and Rust hash output means the migration is blocked.

use onto_assurance_types::hash::{ContentHash, HashAlgorithm, HashDomain};
use serde::Serialize;
use sha2::{Digest, Sha256};

// ══════════════════════════════════════════════════════════════════
// Error
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, thiserror::Error)]
pub enum CanonicalError {
    #[error("serialization failed: {0}")]
    Serialization(String),
    #[error("NaN or Infinity in numeric field — rejected")]
    NonFiniteNumber,
}

// ══════════════════════════════════════════════════════════════════
// Public API
// ══════════════════════════════════════════════════════════════════

/// Compute a ContentHash for any serializable value.
///
/// The value is serialized with canonical JSON rules:
///   - Sorted map keys
///   - No trailing zeros on numbers
///   - UTC timestamps
///   - Domain separator prepended
pub fn content_hash<T: Serialize>(
    value: &T,
    domain: &HashDomain,
) -> Result<ContentHash, CanonicalError> {
    let json = canonical_json(value)?;
    let hash = hash_with_domain(&json, &domain.separator());
    Ok(ContentHash {
        algorithm: HashAlgorithm::Sha256,
        domain: domain.clone(),
        value: hex::encode(hash),
    })
}

/// Compute a raw SHA-256 hash of canonical JSON with domain separator.
pub fn compute_hash<T: Serialize>(
    value: &T,
    domain: &HashDomain,
) -> Result<Vec<u8>, CanonicalError> {
    let json = canonical_json(value)?;
    Ok(hash_with_domain(&json, &domain.separator()))
}

/// Verify that two values produce the same ContentHash.
pub fn verify_hash_match<T: Serialize + PartialEq>(
    a: &T,
    b: &T,
    domain: &HashDomain,
) -> Result<bool, CanonicalError> {
    let ha = content_hash(a, domain)?;
    let hb = content_hash(b, domain)?;
    Ok(ha.value == hb.value)
}

// ══════════════════════════════════════════════════════════════════
// Internal
// ══════════════════════════════════════════════════════════════════

/// Serialize a value to canonical JSON.
///
/// Rules (LOCKED — must match Python reference):
///   1. Map keys sorted lexicographically
///   2. No trailing zeros (serde_json uses minimal representation)
///   3. No NaN or Infinity (detected via tree-walk, not substring match)
///   4. UTF-8
fn canonical_json<T: Serialize>(value: &T) -> Result<String, CanonicalError> {
    let json = serde_json::to_string(value)
        .map_err(|e| CanonicalError::Serialization(e.to_string()))?;

    // Re-parse into serde_json::Value to enable tree-walk validation.
    // serde_json::Value::Object uses BTreeMap internally, so re-serializing
    // produces sorted map keys automatically. No custom Formatter needed.
    let parsed: serde_json::Value = serde_json::from_str(&json)
        .map_err(|e| CanonicalError::Serialization(e.to_string()))?;

    // Reject NaN/Infinity via tree-walk (avoids false positives on
    // string values like "NaN-checker").
    reject_non_finite(&parsed)?;

    // Re-serialize: sorted keys (BTreeMap), compact output, no NaN.
    let canonical = serde_json::to_string(&parsed)
        .map_err(|e| CanonicalError::Serialization(e.to_string()))?;

    Ok(canonical)
}

/// Walk a serde_json::Value tree and reject NaN/Infinity in any numeric position.
fn reject_non_finite(value: &serde_json::Value) -> Result<(), CanonicalError> {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                if f.is_nan() || f.is_infinite() {
                    return Err(CanonicalError::NonFiniteNumber);
                }
            }
            Ok(())
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                reject_non_finite(v)?;
            }
            Ok(())
        }
        serde_json::Value::Object(map) => {
            for (_k, v) in map {
                reject_non_finite(v)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn hash_with_domain(canonical_json: &str, domain_separator: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(domain_separator.as_bytes());
    hasher.update(b"\x00"); // null byte separator
    hasher.update(canonical_json.as_bytes());
    hasher.finalize().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::enums::TaskOutcome;

    #[test]
    fn deterministic_hash_same_input() {
        let domain = HashDomain::new(
            onto_assurance_types::hash::HashPurpose::Content,
            "TEST",
        );
        let h1 = content_hash(&TaskOutcome::Success, &domain).unwrap();
        let h2 = content_hash(&TaskOutcome::Success, &domain).unwrap();
        assert_eq!(h1.value, h2.value);
    }

    #[test]
    fn different_inputs_different_hash() {
        let domain = HashDomain::new(
            onto_assurance_types::hash::HashPurpose::Content,
            "TEST",
        );
        let h1 = content_hash(&TaskOutcome::Success, &domain).unwrap();
        let h2 = content_hash(&TaskOutcome::Failed, &domain).unwrap();
        assert_ne!(h1.value, h2.value);
    }

    #[test]
    fn different_domains_different_hash() {
        let d1 = HashDomain::new(
            onto_assurance_types::hash::HashPurpose::Content,
            "TEST",
        );
        let d2 = HashDomain::new(
            onto_assurance_types::hash::HashPurpose::Envelope,
            "TEST",
        );
        let h1 = content_hash(&TaskOutcome::Success, &d1).unwrap();
        let h2 = content_hash(&TaskOutcome::Success, &d2).unwrap();
        assert_ne!(h1.value, h2.value);
    }

    #[test]
    fn nan_number_rejected_by_serde_json() {
        // serde_json refuses to construct Number::from_f64(NaN) —
        // this is the first line of defense.
        let n = serde_json::Number::from_f64(f64::NAN);
        assert!(n.is_none(), "serde_json must reject NaN at Number construction");
    }

    #[test]
    fn infinity_number_rejected_by_serde_json() {
        let n = serde_json::Number::from_f64(f64::INFINITY);
        assert!(n.is_none(), "serde_json must reject Infinity at Number construction");
    }

    #[test]
    fn valid_float_accepted() {
        use serde_json::json;
        let good = json!({"value": 3.14});
        let result = canonical_json(&good);
        assert!(result.is_ok());
    }

    #[test]
    fn string_containing_nan_is_accepted() {
        // "NaN" inside a string value must NOT be rejected
        use serde_json::json;
        let good = json!({"name": "NaN-checker", "value": 42});
        let result = canonical_json(&good);
        assert!(result.is_ok());
    }

    #[test]
    fn sorted_keys_vs_insertion_order() {
        use serde_json::json;
        // Keys in non-alphabetical order should produce sorted output
        let input = json!({"zebra": 1, "alpha": 2, "beta": 3});
        let output = canonical_json(&input).unwrap();
        // "alpha" should appear before "zebra" in the output
        let alpha_pos = output.find("alpha").unwrap();
        let zebra_pos = output.find("zebra").unwrap();
        assert!(alpha_pos < zebra_pos);
    }
}
