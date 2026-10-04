//! Evidence types — the proof that criteria were met.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{BundleId, CriterionId, EvidenceId, RunId, TransactionId, VerifierId};

// ══════════════════════════════════════════════════════════════════
// EvidenceRecord — one piece of evidence (L1: structured, no crypto)
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub evidence_id: EvidenceId,
    pub transaction_id: TransactionId,
    pub criterion_id: CriterionId,
    pub kind: EvidenceRecordKind,
    pub payload: serde_json::Value,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRecordKind {
    TestOutput,
    DiffHunk,
    LintOutput,
    BuildLog,
    ArtifactHash,
    HumanApproval,
    VerifierReport,
    SideEffectLog,
    Custom(String),
}

// ══════════════════════════════════════════════════════════════════
// EvidenceBundle — a sealed collection of records (L2: hash-chain)
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceBundle {
    pub bundle_id: BundleId,
    pub run_id: RunId,
    pub records: Vec<ChainedRecord>,
    pub genesis_hash: String,
    pub tail_hash: String,
    pub record_count: u32,
    pub sealed_at: DateTime<Utc>,
    pub verifier_binding: VerifierBinding,
}

/// One link in the SHA-256 evidence chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainedRecord {
    pub record: EvidenceRecord,
    pub self_hash: String,
    pub prev_hash: String,
    pub chain_index: u64,
}

/// Which verifier produced this evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierBinding {
    pub verifier_id: VerifierId,
    pub verifier_version: String,
    pub toolchain: Option<String>,
    pub environment_hash: Option<String>,
}

// ══════════════════════════════════════════════════════════════════
// CriterionVerdict — one criterion's evaluation
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriterionVerdict {
    pub criterion_id: CriterionId,
    pub satisfied: bool,
    pub evidence_ids: Vec<EvidenceId>,
    pub detail: String,
}

// ══════════════════════════════════════════════════════════════════
// RequirementVerdict — aggregate across all criteria
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementVerdict {
    pub overall_passed: bool,
    pub blocking_unsatisfied: Vec<CriterionId>,
    pub non_blocking_unsatisfied: Vec<CriterionId>,
    pub criterion_verdicts: Vec<CriterionVerdict>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{BundleId, EvidenceId, RunId, TransactionId, VerifierId};

    #[test]
    fn evidence_record_serde_roundtrip() {
        let record = EvidenceRecord {
            evidence_id: EvidenceId::new(),
            transaction_id: TransactionId::new(),
            criterion_id: crate::ids::CriterionId::new(),
            kind: EvidenceRecordKind::TestOutput,
            payload: serde_json::json!({"passed": true, "tests": 9}),
            recorded_at: chrono::Utc::now(),
        };
        let json = serde_json::to_string(&record).unwrap();
        let back: EvidenceRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record.evidence_id, back.evidence_id);
        assert_eq!(record.kind, back.kind);
    }

    #[test]
    fn verifier_binding_serde() {
        let binding = VerifierBinding {
            verifier_id: VerifierId::new(),
            verifier_version: "2.1.0".into(),
            toolchain: Some("rustc 1.80".into()),
            environment_hash: Some("abc123".into()),
        };
        let json = serde_json::to_string(&binding).unwrap();
        let back: VerifierBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(binding.verifier_version, back.verifier_version);
        assert_eq!(binding.toolchain, back.toolchain);
    }
}
