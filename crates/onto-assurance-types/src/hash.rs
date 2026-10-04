//! Canonical hash types — three distinct hash domains.
//!
//! Hash input must NOT depend on Python dict ordering or Rust struct layout.
//! Rules: explicit field order, UTF-8, canonical numeric representation,
//! UTC timestamps, NaN/Infinity rejected, map keys sorted lexicographically,
//! non-semantic fields excluded, domain_separator + schema_version included.

use serde::{Deserialize, Serialize};

// ══════════════════════════════════════════════════════════════════
// Three Hash Domains
// ══════════════════════════════════════════════════════════════════

/// Hash of semantic content only — what was decided, not when or by whom.
/// Excludes: created_at, database IDs, storage URIs, processing node.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash {
    pub algorithm: HashAlgorithm,
    pub domain: HashDomain,
    pub value: String, // hex-encoded
}

/// Hash binding the event envelope for tamper-evident storage.
/// Includes: content_hash, run_id, transaction_id, causation_id,
/// correlation_id, created_at, schema_version, kernel_version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EnvelopeHash {
    pub algorithm: HashAlgorithm,
    pub value: String,
}

/// Hash binding the verification context.
/// Includes: artifact tree ContentHashes, requirement set ContentHashes,
/// verifier version, toolchain, environment fingerprint, policy version,
/// attempt number, checkpoint reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContextHash {
    pub algorithm: HashAlgorithm,
    pub value: String,
}

// ══════════════════════════════════════════════════════════════════
// Algorithm + Domain
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HashAlgorithm {
    #[default]
    Sha256,
}

/// Domain separator — prevents cross-domain hash collision.
///
/// Format: `ONTO:<PURPOSE>:<OBJECT>:V1`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HashDomain {
    pub purpose: HashPurpose,
    pub object: String,
    pub version: String,
}

impl HashDomain {
    pub fn new(purpose: HashPurpose, object: impl Into<String>) -> Self {
        Self {
            purpose,
            object: object.into(),
            version: "V1".into(),
        }
    }

    /// Full domain separator string for hash input.
    pub fn separator(&self) -> String {
        format!("ONTO:{}:{}:{}", self.purpose.as_str(), self.object, self.version)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HashPurpose {
    Content,
    Envelope,
    Context,
}

impl HashPurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Content => "CONTENT",
            Self::Envelope => "ENVELOPE",
            Self::Context => "CONTEXT",
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// Canonical serialization rules (documentation)
// ══════════════════════════════════════════════════════════════════

/// Rules for canonical JSON serialization before hashing:
///
/// 1. **Field order**: Explicit, defined per schema.  Not map-insertion-order.
/// 2. **Encoding**: UTF-8 only.
/// 3. **Numbers**: No trailing zeros. `1` not `1.0`.  No scientific notation.
/// 4. **Timestamps**: UTC only, ISO 8601, no timezone abbreviations.
/// 5. **NaN/Infinity**: Rejected at serialization boundary (error, not silently hashed).
/// 6. **Map keys**: Sorted lexicographically.
/// 7. **Non-semantic fields**: `created_at`, DB IDs, storage URIs, processing node — excluded.
/// 8. **Domain separator**: Prepended to hash input. Includes `schema_version`.
/// 9. **Null vs absent**: `null` and absent field are distinct; be explicit.
/// 10. **Empty vs absent**: `[]` and absent are distinct; be explicit.
///
/// These rules are LOCKED.  Changing any of them requires a `schema_version` bump.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_separators_are_distinct() {
        let sep1 = HashDomain::new(HashPurpose::Content, "EVIDENCE").separator();
        let sep2 = HashDomain::new(HashPurpose::Envelope, "EVIDENCE").separator();
        assert_ne!(sep1, sep2);
    }
}
