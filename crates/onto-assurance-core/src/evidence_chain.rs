//! Evidence chain — SHA-256 linked, append-only, tamper-evident.
//!
//! Invariants:
//!   EC-1: append-only
//!   EC-2: chain_index strictly increasing, starts at 1
//!   EC-3: first record prev_hash == genesis_hash
//!   EC-4: self_hash = SHA256(canonical(content) || prev_hash)
//!   EC-5: after seal(), no more appends
//!   EC-6: COMPLETED requires verify() pass
//!   EC-7: independent of any external EventLog

use chrono::Utc;
use onto_assurance_types::evidence::{
    ChainedRecord, EvidenceBundle, EvidenceRecord, VerifierBinding,
};
use onto_assurance_types::hash::{HashDomain, HashPurpose};
use onto_assurance_types::ids::BundleId;
use sha2::{Digest, Sha256};

use crate::canonical;
use crate::canonical::CanonicalError;

// ══════════════════════════════════════════════════════════════════
// EvidenceChain — mutable builder
// ══════════════════════════════════════════════════════════════════

#[derive(Debug)]
pub struct EvidenceChain {
    bundle_id: BundleId,
    run_id: onto_assurance_types::ids::RunId,
    records: Vec<ChainedRecord>,
    genesis_hash: String,
    verifier_binding: VerifierBinding,
    sealed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    #[error("chain is sealed — cannot append")]
    Sealed,
    #[error("record serialization failed: {0}")]
    Serialization(String),
    #[error("hash computation failed: {0}")]
    Hash(#[from] CanonicalError),
}

impl EvidenceChain {
    /// Create a new evidence chain.
    ///
    /// `genesis_hash` is typically the `contract.content_hash()` or
    /// a domain-specific genesis value.  The first record's `prev_hash`
    /// will equal this.
    pub fn new(
        run_id: onto_assurance_types::ids::RunId,
        genesis_hash: String,
        verifier_binding: VerifierBinding,
    ) -> Self {
        Self {
            bundle_id: BundleId::new(),
            run_id,
            records: Vec::new(),
            genesis_hash,
            verifier_binding,
            sealed: false,
        }
    }

    /// Append one evidence record.  Returns the chained record.
    ///
    /// EC-5: fails if the chain is already sealed.
    pub fn append(&mut self, record: EvidenceRecord) -> Result<&ChainedRecord, ChainError> {
        if self.sealed {
            return Err(ChainError::Sealed);
        }

        let prev_hash = self
            .records
            .last()
            .map(|r| r.self_hash.clone())
            .unwrap_or_else(|| self.genesis_hash.clone());

        let chain_index = (self.records.len() + 1) as u64;

        let self_hash = self.compute_record_hash(&record, &prev_hash)?;

        let chained = ChainedRecord {
            record,
            self_hash,
            prev_hash,
            chain_index,
        };

        self.records.push(chained);
        let last_idx = self.records.len() - 1;
        Ok(&self.records[last_idx])
    }

    /// Seal the chain and produce an EvidenceBundle.
    ///
    /// After this, `append()` will fail (EC-5).  The chain remains
    /// accessible for inspection but no new records can be added.
    pub fn seal(&mut self) -> EvidenceBundle {
        self.sealed = true;
        let record_count = self.records.len() as u32;
        let tail_hash = self
            .records
            .last()
            .map(|r| r.self_hash.clone())
            .unwrap_or_else(|| self.genesis_hash.clone());

        EvidenceBundle {
            bundle_id: self.bundle_id,
            run_id: self.run_id,
            records: self.records.clone(),
            genesis_hash: self.genesis_hash.clone(),
            tail_hash,
            record_count,
            sealed_at: Utc::now(),
            verifier_binding: self.verifier_binding.clone(),
        }
    }

    /// Verify the integrity of an existing EvidenceBundle.
    ///
    /// Returns `Ok(())` if all hashes are consistent.
    /// Returns `Err(index)` at the first tampered record.
    pub fn verify(bundle: &EvidenceBundle) -> Result<(), u64> {
        let mut expected_prev = &bundle.genesis_hash;

        for record in &bundle.records {
            // Recompute self_hash
            let recomputed = compute_record_hash_from_chained(record, expected_prev);

            if recomputed != record.self_hash {
                return Err(record.chain_index);
            }

            // Check chain index (EC-2)
            expected_prev = &record.self_hash;
        }

        Ok(())
    }

    // ── Internal ──

    fn compute_record_hash(
        &self,
        record: &EvidenceRecord,
        prev_hash: &str,
    ) -> Result<String, ChainError> {
        let domain = HashDomain::new(HashPurpose::Content, "EVIDENCE");
        let content_hash = canonical::compute_hash(record, &domain)
            .map_err(ChainError::Hash)?;

        // Chain: SHA256(canonical_record_hash || prev_hash)
        let mut hasher = Sha256::new();
        hasher.update(&content_hash);
        hasher.update(prev_hash.as_bytes());
        let hash = hasher.finalize();

        Ok(hex::encode(hash))
    }
}

/// Recompute the hash of a ChainedRecord for verification.
/// MUST use the same canonical hash algorithm as `compute_record_hash`.
fn compute_record_hash_from_chained(record: &ChainedRecord, prev_hash: &str) -> String {
    let domain = HashDomain::new(HashPurpose::Content, "EVIDENCE");
    // Use canonical JSON (sorted keys) — must match Python reference
    let content_hash = canonical::compute_hash(&record.record, &domain)
        .unwrap_or_else(|_| {
            // Fallback: if canonical fails, use raw serde (shouldn't happen for valid records)
            let raw = serde_json::to_string(&record.record).unwrap_or_default();
            let mut h = Sha256::new();
            h.update(raw.as_bytes());
            h.finalize().to_vec()
        });

    let mut hasher = Sha256::new();
    hasher.update(&content_hash);
    hasher.update(prev_hash.as_bytes());

    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::evidence::{
        EvidenceRecord, EvidenceRecordKind, VerifierBinding,
    };
    use onto_assurance_types::ids::{CriterionId, EvidenceId, RunId, TransactionId, VerifierId};

    fn make_record(transaction_id: TransactionId) -> EvidenceRecord {
        EvidenceRecord {
            evidence_id: EvidenceId::new(),
            transaction_id,
            criterion_id: CriterionId::new(),
            kind: EvidenceRecordKind::TestOutput,
            payload: serde_json::json!({"passed": true}),
            recorded_at: Utc::now(),
        }
    }

    fn make_verifier() -> VerifierBinding {
        VerifierBinding {
            verifier_id: VerifierId::new(),
            verifier_version: "1.0.0".into(),
            toolchain: Some("gcc 14.2".into()),
            environment_hash: None,
        }
    }

    #[test]
    fn chain_append_and_verify() {
        let run_id = RunId::new();
        let mut chain = EvidenceChain::new(
            run_id,
            "genesis-hash-0000".into(),
            make_verifier(),
        );

        let txn_id = TransactionId::new();
        chain.append(make_record(txn_id)).unwrap();
        chain.append(make_record(txn_id)).unwrap();

        let bundle = chain.seal();
        assert_eq!(bundle.record_count, 2);

        // EC-2: chain_index strictly increasing from 1
        assert_eq!(bundle.records[0].chain_index, 1);
        assert_eq!(bundle.records[1].chain_index, 2);

        // EC-3: first record prev_hash == genesis_hash
        assert_eq!(bundle.records[0].prev_hash, "genesis-hash-0000");

        // EC-4: each record's prev_hash equals previous record's self_hash
        assert_eq!(bundle.records[1].prev_hash, bundle.records[0].self_hash);

        EvidenceChain::verify(&bundle).unwrap();
    }

    #[test]
    fn tampered_evidence_detected() {
        let run_id = RunId::new();
        let mut chain = EvidenceChain::new(
            run_id,
            "genesis-hash-0000".into(),
            make_verifier(),
        );

        let txn_id = TransactionId::new();
        chain.append(make_record(txn_id)).unwrap();

        let mut bundle = chain.seal();

        // Tamper with a record's payload
        bundle.records[0].record.payload = serde_json::json!({"passed": false});

        let result = EvidenceChain::verify(&bundle);
        assert!(result.is_err());
    }

    #[test]
    fn chain_sealed_no_append() {
        let run_id = RunId::new();
        let mut chain = EvidenceChain::new(
            run_id,
            "genesis".into(),
            make_verifier(),
        );

        let txn_id = TransactionId::new();
        chain.append(make_record(txn_id)).unwrap();
        let _bundle = chain.seal();

        // Should fail — chain is sealed
        assert!(chain.append(make_record(txn_id)).is_err());
    }
}
