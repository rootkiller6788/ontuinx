//! ProtocolEnvelope — schema versioning for all persistent objects.
//!
//! Every cross-layer message is wrapped in Versioned<T> to ensure
//! schema evolution does not silently break evidence chains.

use serde::{Deserialize, Serialize};

/// Protocol version metadata carried by every persistent object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolEnvelope {
    pub schema_version: u32,
    pub canonicalization_version: u32,
}

impl Default for ProtocolEnvelope {
    fn default() -> Self {
        Self {
            schema_version: 1,
            canonicalization_version: 1,
        }
    }
}

/// Versioned wrapper for persistent and cross-layer types.
///
/// Digest computation rules:
/// - Compute over Versioned.payload canonical bytes
/// - Exclude the object's own *_digest fields
/// - Include schema_version and canonicalization_version
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Versioned<T> {
    pub protocol: ProtocolEnvelope,
    pub payload: T,
}

impl<T> Versioned<T> {
    pub fn new(payload: T) -> Self {
        Self {
            protocol: ProtocolEnvelope::default(),
            payload,
        }
    }

    pub fn with_version(payload: T, schema_version: u32, canonicalization_version: u32) -> Self {
        Self {
            protocol: ProtocolEnvelope {
                schema_version,
                canonicalization_version,
            },
            payload,
        }
    }
}
