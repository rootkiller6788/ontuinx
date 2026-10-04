//! Digest — content-addressable identity.
//!
//! All persistent objects carry a Digest to enable tamper-evident verification.

use serde::{Deserialize, Serialize};

/// Algorithm used to compute a Digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestAlgorithm {
    Sha256,
    Blake3,
}

impl std::fmt::Display for DigestAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sha256 => write!(f, "sha256"),
            Self::Blake3 => write!(f, "blake3"),
        }
    }
}

impl Default for DigestAlgorithm {
    fn default() -> Self {
        Self::Sha256
    }
}

/// Content-addressable identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Digest {
    pub algorithm: DigestAlgorithm,
    pub value: String,
}

impl Digest {
    pub fn new(algo: DigestAlgorithm, value: impl Into<String>) -> Self {
        Self {
            algorithm: algo,
            value: value.into(),
        }
    }
}

impl std::fmt::Display for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.value)
    }
}
