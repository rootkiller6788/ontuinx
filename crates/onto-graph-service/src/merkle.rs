//! Merkle hash tree for OntoFirmwareGraph (P5).
//!
//! Hierarchy:
//!   FileFactHash = SHA-256(file_content)
//!       ↓
//!   EntityVersionHash = SHA-256(entity facts in file, sorted by entity_id)
//!       ↓
//!   FileVersionHash = SHA-256(FileFactHash || EntityVersionHashes)
//!       ↓
//!   Snapshot Merkle Root = SHA-256(all FileVersionHashes, sorted by file_id)
//!
//! Incremental: Candidate only recomputes changed partitions.

use sha2::{Sha256, Digest};
use std::collections::BTreeMap;
use uuid::Uuid;

/// A single entity version's hash contribution.
#[derive(Debug, Clone)]
pub struct EntityVersionFact {
    pub entity_id: Uuid,
    pub structural_hash: String,
    pub qualified_name: Option<String>,
}

/// A single file's hash contribution.
#[derive(Debug, Clone)]
pub struct FileFact {
    pub file_id: Uuid,
    pub content_sha256: String,
    pub entity_facts: Vec<EntityVersionFact>,
}

/// Compute the Merkle root for a snapshot.
///
/// `files`: all files in the snapshot.  For an incremental Candidate,
/// only changed files need to be provided; unchanged files' FileVersionHashes
/// are carried forward from the previous snapshot.
pub fn compute_snapshot_root(
    files: &[FileFact],
    previous_file_hashes: &BTreeMap<Uuid, String>,
) -> String {
    let mut file_hashes: BTreeMap<Uuid, String> = previous_file_hashes.clone();

    for file in files {
        let fh = compute_file_version_hash(file);
        file_hashes.insert(file.file_id, fh);
    }

    // Snapshot root = SHA-256 of all file hashes, sorted by file_id.
    let mut hasher = Sha256::new();
    for (_file_id, hash) in &file_hashes {
        hasher.update(hash.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Compute FileVersionHash for one file.
fn compute_file_version_hash(file: &FileFact) -> String {
    let mut hasher = Sha256::new();

    // Layer 1: FileFactHash
    hasher.update(file.content_sha256.as_bytes());

    // Layer 2: EntityVersionHashes, sorted by entity_id.
    let mut entity_hashes: Vec<(Uuid, String)> = file
        .entity_facts
        .iter()
        .map(|e| (e.entity_id, compute_entity_version_hash(e)))
        .collect();
    entity_hashes.sort_by_key(|(id, _)| *id);

    for (_id, hash) in &entity_hashes {
        hasher.update(hash.as_bytes());
    }

    hex::encode(hasher.finalize())
}

/// Compute EntityVersionHash for a single entity.
fn compute_entity_version_hash(fact: &EntityVersionFact) -> String {
    let mut hasher = Sha256::new();
    hasher.update(fact.structural_hash.as_bytes());
    if let Some(qn) = &fact.qualified_name {
        hasher.update(qn.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Convenience: compute SHA-256 of raw bytes.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_root() {
        let f1 = FileFact {
            file_id: Uuid::nil(),
            content_sha256: "abc".into(),
            entity_facts: vec![EntityVersionFact {
                entity_id: Uuid::nil(),
                structural_hash: "def".into(),
                qualified_name: Some("test".into()),
            }],
        };
        let root1 = compute_snapshot_root(&[f1.clone()], &BTreeMap::new());
        let root2 = compute_snapshot_root(&[f1], &BTreeMap::new());
        assert_eq!(root1, root2, "same input → same root");
    }

    #[test]
    fn test_changed_file_changes_root() {
        let f1 = FileFact {
            file_id: Uuid::nil(),
            content_sha256: "abc".into(),
            entity_facts: vec![],
        };
        let f2 = FileFact {
            file_id: Uuid::nil(),
            content_sha256: "xyz".into(), // different content
            entity_facts: vec![],
        };
        let root1 = compute_snapshot_root(&[f1], &BTreeMap::new());
        let root2 = compute_snapshot_root(&[f2], &BTreeMap::new());
        assert_ne!(root1, root2, "different content → different root");
    }

    #[test]
    fn test_incremental_candidate() {
        let file_a = FileFact {
            file_id: Uuid::new_v4(),
            content_sha256: "a".into(),
            entity_facts: vec![],
        };
        let file_b = FileFact {
            file_id: Uuid::new_v4(),
            content_sha256: "b".into(),
            entity_facts: vec![],
        };

        // Baseline: both files.
        let prev_hashes: BTreeMap<Uuid, String> = BTreeMap::new();
        let baseline = compute_snapshot_root(&[file_a.clone(), file_b.clone()], &prev_hashes);

        // Candidate: only file_a changed.  Pass previous hashes for file_b.
        let file_a_changed = FileFact {
            file_id: file_a.file_id,
            content_sha256: "a-v2".into(),
            entity_facts: vec![],
        };
        let mut carry_forward: BTreeMap<Uuid, String> = BTreeMap::new();
        // We'd compute file_b's hash from the baseline, but for the test we simulate it.
        carry_forward.insert(
            file_b.file_id,
            "simulated-file-b-hash".into(),
        );

        let candidate =
            compute_snapshot_root(&[file_a_changed], &carry_forward);
        assert_ne!(baseline, candidate, "candidate root differs from baseline");
    }
}
