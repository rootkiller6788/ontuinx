//! Finding — structured conformance diagnostics.
//!
//! Findings are the unified output of all Verifiers. They carry enough
//! identity to be compared across Attempts (via FindingFingerprint)
//! and enough policy data for the Loop to decide remediation.

use crate::verifier::Pass;

// ── Finding ──

#[derive(Debug, Clone)]
pub struct Finding {
    pub finding_id: String,
    pub fingerprint: FindingFingerprint,
    pub pass: Pass,
    pub rule_id: String,
    pub rule_version: String,
    pub severity: FindingSeverity,
    pub category: CategoryId,
    pub disposition: FindingDisposition,
    pub remediation: RemediationClass,
    pub location: Option<EvidenceLocation>,
    pub message: String,
    pub fix_hint: Option<String>,
    pub confidence: f64,
    pub evidence_refs: Vec<String>,
}

// ── Severity ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FindingSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

// ── Category — extensible, not locked to code ──

/// Extensible category identifier.
/// Examples: "code.bug", "code.security", "graph.integrity", "artifact.format".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CategoryId(pub String);

impl CategoryId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl std::fmt::Display for CategoryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Disposition ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindingDisposition {
    Blocking,
    Advisory,
}

/// How severity maps to disposition — set by the Rule Registry, not hardcoded.
#[derive(Debug, Clone)]
pub struct FindingPolicy {
    pub severity: FindingSeverity,
    pub disposition: FindingDisposition,
}

// ── Remediation ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemediationClass {
    AutoRepairable,
    RetryWithFeedback,
    HumanDecisionRequired,
    NonRemediable,
}

// ── Location ──

#[derive(Debug, Clone)]
pub struct EvidenceLocation {
    pub artifact_path: String,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub start_column: Option<u32>,
    pub end_column: Option<u32>,
    pub entity_key: Option<String>,
}

// ── Fingerprint (stable across attempts) ──

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FindingFingerprint {
    pub rule_id: String,
    pub entity_key: Option<String>,
    pub artifact_path: String,
    pub semantic_key: String,
    pub line_hint: Option<u32>,
}

impl FindingFingerprint {
    /// Matching priority: EntityKey → semantic_key → artifact_path → line_hint fallback.
    pub fn match_strength(&self, other: &Self) -> u32 {
        let mut score = 0u32;
        if let (Some(a), Some(b)) = (&self.entity_key, &other.entity_key) {
            if a == b { score += 100; } else { return 0; }
        }
        if self.semantic_key == other.semantic_key { score += 50; }
        if self.artifact_path == other.artifact_path { score += 10; }
        if let (Some(a), Some(b)) = (&self.line_hint, &other.line_hint) {
            if a == b { score += 1; }
        }
        score
    }
}
