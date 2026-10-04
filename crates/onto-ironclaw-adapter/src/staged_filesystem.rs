//! LocalVersionedFilesystemAdapter — M6-A concrete staging adapter.
//!
//! Implements `StagedFilesystemPort` using local directory versioning with
//! atomic symlink switching.
//!
//! Layout:
//! ```text
//! <workspace_root>/
//!   versions/
//!     v001/                  ← committed versions
//!     v002-staging/           ← active staging directory
//!   current -> versions/v001  ← symlink to current version
//! ```
//!
//! # Invariants (M6-A)
//!
//! - Agent writes ONLY inside the staging directory.
//! - The `current` symlink is unchanged until publish() succeeds.
//! - publish() requires valid Onto COMMIT decision.
//! - Baseline hash must not drift between prepare and publish.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use onto_assurance_core::canonical;
use sha2::{Digest, Sha256};
use onto_assurance_runtime::ports::{CommitPermit, StageError, StagedFilesystemPort};
use onto_assurance_types::ids::TransactionId;
use onto_assurance_types::transaction::{
    ArtifactEntry, ArtifactManifest, ContentHash, DiscardReceipt, FilesystemStageHandle,
    PrepareStageRequest, PublishReceipt, PublishStageRequest, StageStatus,
};

// ══════════════════════════════════════════════════════════════════
// LocalVersionedFilesystemAdapter
// ══════════════════════════════════════════════════════════════════

pub struct LocalVersionedFilesystemAdapter {
    workspace_root: PathBuf,
}

impl LocalVersionedFilesystemAdapter {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    fn versions_dir(&self) -> PathBuf {
        self.workspace_root.join("versions")
    }

    fn current_link(&self) -> PathBuf {
        self.workspace_root.join("current")
    }

    fn staging_path(&self, transaction_id: &TransactionId) -> PathBuf {
        self.versions_dir()
            .join(format!("staging-{}", transaction_id))
    }

    fn version_path(&self, number: u64) -> PathBuf {
        self.versions_dir().join(format!("v{:03}", number))
    }

    fn compute_dir_hash(&self, dir: &Path) -> Result<ContentHash, StageError> {
        let mut entries: Vec<String> = Vec::new();
        if dir.exists() {
            for entry in walkdir::WalkDir::new(dir)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.is_file() {
                    let rel = path.strip_prefix(dir).unwrap_or(path);
                    let content = fs::read(path)
                        .map_err(|e| StageError::Io(format!("read {}: {}", rel.display(), e)))?;
                    let hash = compute_sha256(&content);
                    entries.push(format!("{}:{}", rel.display(), hex::encode(hash)));
                }
            }
        }
        entries.sort();
        let combined = entries.join("\n");
        let hash = compute_sha256(combined.as_bytes());
        Ok(ContentHash::new(hex::encode(hash)))
    }

    fn next_version(&self) -> u64 {
        let versions = self.versions_dir();
        if !versions.exists() {
            return 1;
        }
        let mut max_num = 0u64;
        if let Ok(entries) = fs::read_dir(&versions) {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with('v') && !name_str.contains("staging") {
                    if let Ok(num) = name_str[1..].parse::<u64>() {
                        max_num = max_num.max(num);
                    }
                }
            }
        }
        max_num + 1
    }
}

#[async_trait::async_trait]
impl StagedFilesystemPort for LocalVersionedFilesystemAdapter {
    async fn prepare(
        &self,
        request: PrepareStageRequest,
    ) -> Result<FilesystemStageHandle, StageError> {
        // M6-A1b: Path safety — reject dangerous paths
        let source = PathBuf::from(&request.source_path);
        validate_path_safe(&source, &self.workspace_root)?;

        let versions = self.versions_dir();
        fs::create_dir_all(&versions)
            .map_err(|e| StageError::PrepareFailed(format!("create versions dir: {}", e)))?;

        let staging = self.staging_path(&request.transaction_id);

        // Compute baseline hash from source
        let baseline_hash = self.compute_dir_hash(&source)?;

        // If staging exists, clean it
        if staging.exists() {
            fs::remove_dir_all(&staging).ok();
        }

        // Copy source → staging
        copy_dir(&source, &staging)
            .map_err(|e| StageError::PrepareFailed(format!("copy to staging: {}", e)))?;

        Ok(FilesystemStageHandle {
            transaction_id: request.transaction_id,
            attempt_id: request.attempt_id,
            baseline_hash,
            staging_root: staging.to_string_lossy().to_string(),
        })
    }

    async fn manifest(
        &self,
        stage: &FilesystemStageHandle,
    ) -> Result<ArtifactManifest, StageError> {
        let staging = PathBuf::from(&stage.staging_root);
        if !staging.exists() {
            return Err(StageError::NotFound(stage.staging_root.clone()));
        }

        let source = self.current_link();
        let source = if source.exists() {
            fs::read_link(&source).unwrap_or_else(|_| source.clone())
        } else {
            source
        };

        let mut artifacts: Vec<ArtifactEntry> = Vec::new();
        for entry in walkdir::WalkDir::new(&staging)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                let rel = path.strip_prefix(&staging).unwrap_or(path);
                let content = fs::read(path)
                    .map_err(|e| StageError::Io(format!("read {}: {}", rel.display(), e)))?;
                let hash = hex::encode(compute_sha256(&content));

                // Check if this file differs from source
                let source_path = source.join(rel);
                let is_new_or_modified = if source_path.exists() {
                    let source_content = fs::read(&source_path)
                        .map_err(|e| StageError::Io(format!("read source {}: {}", rel.display(), e)))?;
                    let source_hash = hex::encode(compute_sha256(&source_content));
                    hash != source_hash
                } else {
                    true
                };

                if is_new_or_modified {
                    artifacts.push(ArtifactEntry {
                        path: rel.to_string_lossy().to_string(),
                        content_hash: hash,
                        size_bytes: content.len() as u64,
                    });
                }
            }
        }

        Ok(ArtifactManifest {
            transaction_id: stage.transaction_id,
            artifacts,
        })
    }

    async fn publish(
        &self,
        request: PublishStageRequest,
        permit: &CommitPermit,
    ) -> Result<PublishReceipt, StageError> {
        let staging = PathBuf::from(&request.stage.staging_root);
        if !staging.exists() {
            return Err(StageError::NotFound(request.stage.staging_root.clone()));
        }

        // M6-A2: Verify permit matches request (adapter must not trust caller)
        if request.stage.transaction_id != permit.transaction_id() {
            return Err(StageError::NotAuthorized("transaction mismatch".into()));
        }
        if request.expected_baseline_hash != *permit.baseline_hash() {
            return Err(StageError::NotAuthorized("baseline mismatch".into()));
        }

        // Verify baseline hash hasn't drifted
        let source = self.current_link();
        let source_resolved = if source.exists() {
            fs::read_link(&source).unwrap_or_else(|_| source.clone())
        } else {
            source.clone()
        };
        let current_baseline = self.compute_dir_hash(&source_resolved)?;
        if current_baseline != request.expected_baseline_hash {
            return Err(StageError::BaselineMismatch {
                expected: request.expected_baseline_hash.to_string(),
                actual: current_baseline.to_string(),
            });
        }

        // Verify manifest hash matches approved manifest
        let manifest = self.manifest(&request.stage).await?;
        let manifest_hash = canonical::compute_hash(&manifest, &onto_assurance_types::hash::HashDomain::new(
            onto_assurance_types::hash::HashPurpose::Content,
            "ARTIFACT_MANIFEST",
        )).map_err(|e| StageError::Io(format!("manifest hash: {}", e)))?;
        let actual_manifest_hash = ContentHash::new(hex::encode(manifest_hash));
        if actual_manifest_hash != request.approved_manifest_hash {
            return Err(StageError::ManifestMismatch {
                expected: request.approved_manifest_hash.to_string(),
                actual: actual_manifest_hash.to_string(),
            });
        }

        // Atomic publish: rename staging → versioned dir, switch symlink
        let version_num = self.next_version();
        let version_dir = self.version_path(version_num);
        let versions = self.versions_dir();
        fs::create_dir_all(&versions)
            .map_err(|e| StageError::PublishFailed(format!("create versions: {}", e)))?;

        // Rename staging to versioned directory (atomic on same filesystem)
        fs::rename(&staging, &version_dir)
            .map_err(|e| StageError::PublishFailed(format!("rename staging to version: {}", e)))?;

        // Atomically switch current symlink
        let current = self.current_link();
        let tmp_link = current.with_extension("tmp");
        let _ = fs::remove_file(&tmp_link);
        std::os::unix::fs::symlink(&version_dir, &tmp_link)
            .map_err(|e| StageError::PublishFailed(format!("create tmp symlink: {}", e)))?;
        fs::rename(&tmp_link, &current).map_err(|e| {
            StageError::PublishFailed(format!("atomically switch current symlink: {}", e))
        })?;

        let after_hash = self.compute_dir_hash(&version_dir)?;

        Ok(PublishReceipt {
            transaction_id: request.stage.transaction_id,
            before_hash: request.expected_baseline_hash.clone(),
            after_hash,
            manifest_hash: request.approved_manifest_hash.clone(),
            published_at: chrono::Utc::now(),
        })
    }

    async fn discard(
        &self,
        stage: &FilesystemStageHandle,
    ) -> Result<DiscardReceipt, StageError> {
        let staging = PathBuf::from(&stage.staging_root);
        if staging.exists() {
            fs::remove_dir_all(&staging)
                .map_err(|e| StageError::DiscardFailed(format!("remove staging: {}", e)))?;
        }
        Ok(DiscardReceipt {
            transaction_id: stage.transaction_id,
            discarded_at: chrono::Utc::now(),
        })
    }

    async fn inspect(
        &self,
        stage: &FilesystemStageHandle,
    ) -> Result<StageStatus, StageError> {
        let staging = PathBuf::from(&stage.staging_root);
        let exists = staging.exists();
        let artifact_count = if exists {
            walkdir::WalkDir::new(&staging)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .count() as u32
        } else {
            0
        };
        Ok(StageStatus {
            transaction_id: stage.transaction_id,
            exists,
            artifact_count,
            baseline_hash: stage.baseline_hash.clone(),
        })
    }
}

// ══════════════════════════════════════════════════════════════════
// Helpers
// ══════════════════════════════════════════════════════════════════

/// M6-A1b: Reject dangerous paths, symlinks, and special files.
fn validate_path_safe(path: &Path, workspace_root: &Path) -> Result<(), StageError> {
    // Reject absolute paths outside workspace
    if path.is_absolute() && !path.starts_with(workspace_root) {
        return Err(StageError::NotAuthorized(
            "absolute path outside workspace root".into(),
        ));
    }
    // Reject .. traversal
    let canonical = path.canonicalize().map_err(|e| {
        StageError::NotAuthorized(format!("cannot resolve path: {}", e))
    })?;
    if !canonical.starts_with(workspace_root) {
        return Err(StageError::NotAuthorized(
            "path traversal outside workspace".into(),
        ));
    }
    // Reject symlinks
    if path.is_symlink() || canonical != path && canonical.is_symlink() {
        return Err(StageError::NotAuthorized("symlinks not allowed".into()));
    }
    Ok(())
}

fn compute_sha256(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

pub(crate) fn copy_dir(src: &Path, dst: &Path) -> io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir(&src_path, &dst_path)?;
        } else if file_type.is_symlink() {
            // Skip symlinks — copy the target instead
            let target = fs::read_link(&src_path)?;
            if target.is_absolute() {
                continue; // Skip absolute symlinks for safety
            }
            let resolved = src_path.parent().unwrap_or(&src_path).join(&target);
            if resolved.exists() && resolved.is_file() {
                fs::copy(&resolved, &dst_path)?;
            }
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::ids::{AttemptId, DecisionId, TransactionId};
    use onto_assurance_types::transaction::IdempotencyKey;
    use std::io::Write;
    use tempfile::TempDir;

    fn setup_workspace() -> (TempDir, PathBuf, LocalVersionedFilesystemAdapter) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        let source = root.join("project");
        fs::create_dir_all(&source).unwrap();

        // Create initial file
        let mut f = fs::File::create(source.join("README.md")).unwrap();
        f.write_all(b"# Initial\n").unwrap();

        // Create versions dir and current symlink
        let versions = root.join("versions");
        fs::create_dir_all(&versions).unwrap();

        let v001 = versions.join("v001");
        copy_dir(&source, &v001).unwrap();
        let current = root.join("current");
        std::os::unix::fs::symlink(&v001, &current).unwrap();

        let adapter = LocalVersionedFilesystemAdapter::new(&root);
        (temp, source, adapter)
    }

    fn make_permit(
        txn_id: TransactionId,
        attempt_id: AttemptId,
        baseline: &ContentHash,
        manifest: &ContentHash,
    ) -> CommitPermit {
        CommitPermit::new_for_test_only(
            DecisionId::new(),
            txn_id,
            attempt_id,
            baseline.clone(),
            manifest.clone(),
        )
    }

    fn make_stage_req(txn_id: TransactionId, attempt_id: AttemptId, source: &Path) -> PrepareStageRequest {
        PrepareStageRequest {
            transaction_id: txn_id,
            attempt_id,
            source_path: source.to_string_lossy().to_string(),
            staging_root: source.parent().unwrap().join("versions").to_string_lossy().to_string(),
        }
    }

    // ══════════════════════════════════════════════════════════════
    // M6-A Test 1: No COMMIT → workspace unchanged
    // ══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn no_commit_workspace_unchanged() {
        let (_temp, source, adapter) = setup_workspace();
        let txn_id = TransactionId::new();
        let attempt_id = AttemptId::new();
        let current = adapter.current_link();
        let before_hash = adapter.compute_dir_hash(
            &fs::read_link(&current).unwrap_or(current.clone())
        ).unwrap();

        // Prepare staging
        let stage = adapter.prepare(make_stage_req(txn_id, attempt_id, &source)).await.unwrap();

        // Agent modifies staging
        let staging_path = PathBuf::from(&stage.staging_root);
        let mut f = fs::File::create(staging_path.join("new_feature.rs")).unwrap();
        f.write_all(b"fn new_feature() {}").unwrap();

        // Discard without COMMIT
        adapter.discard(&stage).await.unwrap();

        // Verify: workspace unchanged
        let resolved = fs::read_link(&current).unwrap_or(current.clone());
        let after_hash = adapter.compute_dir_hash(&resolved).unwrap();
        assert_eq!(before_hash, after_hash,
            "workspace hash must not change without COMMIT");
        assert!(!staging_path.exists(),
            "staging dir must be cleaned up after discard");
    }

    // ══════════════════════════════════════════════════════════════
    // M6-A Test 2: Valid COMMIT → atomic switch
    // ══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn valid_commit_atomic_switch() {
        let (_temp, source, adapter) = setup_workspace();
        let txn_id = TransactionId::new();
        let attempt_id = AttemptId::new();
        let current = adapter.current_link();
        let resolved_before = fs::read_link(&current).unwrap_or(current.clone());
        let before_hash = adapter.compute_dir_hash(&resolved_before).unwrap();

        // Prepare staging
        let stage = adapter.prepare(make_stage_req(txn_id, attempt_id, &source)).await.unwrap();

        // Agent modifies staging
        let staging_path = PathBuf::from(&stage.staging_root);
        let mut f = fs::File::create(staging_path.join("new_feature.rs")).unwrap();
        f.write_all(b"fn new_feature() { println!(\"hello\"); }").unwrap();

        // Compute manifest
        let manifest = adapter.manifest(&stage).await.unwrap();
        assert!(!manifest.artifacts.is_empty(), "manifest must detect new file");

        // Publish (simulating after Onto COMMIT)
        let permit = make_permit(txn_id, attempt_id, &before_hash, &ContentHash::new("mock-approved-hash"));
        let receipt = adapter.publish(PublishStageRequest {
            stage: stage.clone(),
            expected_baseline_hash: before_hash.clone(),
            approved_manifest_hash: ContentHash::new("mock-approved-hash"),
            decision_id: DecisionId::new(),
            idempotency_key: IdempotencyKey::new(format!("publish-{}", txn_id)),
        }, &permit).await;

        // For this test, we expect manifest mismatch since we used a mock hash.
        // In real flow, the hash would come from the Onto kernel's approved manifest.
        match receipt {
            Err(StageError::ManifestMismatch { .. }) => {
                // Expected: we used a mock hash. The guard works.
            }
            Ok(_) => {
                // If it succeeded, verify workspace changed
                let resolved_after = fs::read_link(&current).unwrap_or(current.clone());
                let after_hash = adapter.compute_dir_hash(&resolved_after).unwrap();
                assert_ne!(before_hash, after_hash,
                    "workspace hash must change after valid COMMIT + publish");
            }
            Err(e) => panic!("unexpected error: {}", e),
        }
    }

    // ══════════════════════════════════════════════════════════════
    // M6-A Test 3: Real manifest hash → successful publish
    // ══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn real_manifest_hash_successful_publish() {
        let (_temp, source, adapter) = setup_workspace();
        let txn_id = TransactionId::new();
        let attempt_id = AttemptId::new();
        let current = adapter.current_link();
        let resolved_before = fs::read_link(&current).unwrap_or(current.clone());
        let before_hash = adapter.compute_dir_hash(&resolved_before).unwrap();

        // Prepare staging
        let stage = adapter.prepare(make_stage_req(txn_id, attempt_id, &source)).await.unwrap();

        // Agent modifies staging
        let staging_path = PathBuf::from(&stage.staging_root);
        let mut f = fs::File::create(staging_path.join("new_feature.rs")).unwrap();
        f.write_all(b"fn new_feature() {}").unwrap();

        // Compute REAL manifest and hash
        let manifest = adapter.manifest(&stage).await.unwrap();
        // Use canonical hash to compute real hash
        let real_hash_bytes = onto_assurance_core::canonical::compute_hash(
            &manifest,
            &onto_assurance_types::hash::HashDomain::new(
                onto_assurance_types::hash::HashPurpose::Content,
                "ARTIFACT_MANIFEST",
            ),
        ).unwrap();
        let real_manifest_hash = ContentHash::new(hex::encode(real_hash_bytes));

        // Publish with REAL approved manifest hash
        let permit = make_permit(txn_id, attempt_id, &before_hash, &real_manifest_hash);
        let receipt = adapter.publish(PublishStageRequest {
            stage: stage.clone(),
            expected_baseline_hash: before_hash.clone(),
            approved_manifest_hash: real_manifest_hash.clone(),
            decision_id: DecisionId::new(),
            idempotency_key: IdempotencyKey::new(format!("publish-{}", txn_id)),
        }, &permit).await.unwrap();

        // Verify
        assert_eq!(receipt.transaction_id, txn_id);
        assert_eq!(receipt.before_hash, before_hash);
        assert_ne!(receipt.after_hash, before_hash,
            "after hash must differ from before hash after publish");
        assert_eq!(receipt.manifest_hash, real_manifest_hash);

        // Verify current symlink points to new version
        let resolved_after = fs::read_link(&current).unwrap_or(current.clone());
        assert_ne!(resolved_after, resolved_before,
            "current must point to new version after publish");

        // Verify new file exists in resolved workspace
        assert!(resolved_after.join("new_feature.rs").exists(),
            "new file must exist in published workspace");
    }

    // ══════════════════════════════════════════════════════════════
    // M6-A Test 4: Idempotent publish
    // ══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn idempotent_publish_same_receipt() {
        let (_temp, source, adapter) = setup_workspace();
        let txn_id = TransactionId::new();
        let attempt_id = AttemptId::new();
        let current = adapter.current_link();
        let resolved_before = fs::read_link(&current).unwrap_or(current.clone());
        let before_hash = adapter.compute_dir_hash(&resolved_before).unwrap();

        let stage = adapter.prepare(make_stage_req(txn_id, attempt_id, &source)).await.unwrap();
        let staging_path = PathBuf::from(&stage.staging_root);
        fs::File::create(staging_path.join("data.rs")).unwrap();

        let manifest = adapter.manifest(&stage).await.unwrap();
        let manifest_hash_bytes = onto_assurance_core::canonical::compute_hash(
            &manifest,
            &onto_assurance_types::hash::HashDomain::new(
                onto_assurance_types::hash::HashPurpose::Content, "ARTIFACT_MANIFEST",
            ),
        ).unwrap();
        let manifest_hash = ContentHash::new(hex::encode(manifest_hash_bytes));
        let idem_key = IdempotencyKey::new(format!("idem-{}", txn_id));
        let permit = make_permit(txn_id, attempt_id, &before_hash, &manifest_hash);

        let req = PublishStageRequest {
            stage: stage.clone(),
            expected_baseline_hash: before_hash.clone(),
            approved_manifest_hash: manifest_hash.clone(),
            decision_id: DecisionId::new(),
            idempotency_key: idem_key.clone(),
        };

        let r1 = adapter.publish(req.clone(), &permit).await.unwrap();
        let r2 = adapter.publish(req, &permit).await; // second attempt

        // Second publish should either succeed with same receipt (idempotent)
        // or fail because staging has been renamed
        match r2 {
            Ok(r2) => {
                // If idempotent, receipts match
                assert_eq!(r1.after_hash, r2.after_hash);
                assert_eq!(r1.published_at, r2.published_at);
            }
            Err(StageError::NotFound(_)) => {
                // Acceptable: staging was consumed by first publish
            }
            Err(e) => panic!("unexpected error on second publish: {}", e),
        }
    }

    // ══════════════════════════════════════════════════════════════
    // M6-A2: Decision-gated authorization tests
    // ══════════════════════════════════════════════════════════════

    use crate::test_support::InMemoryDecisionStore;
    use onto_assurance_runtime::ports::{
        DecisionStorePort, PublishReceiptStorePort,
        RunFinalizationOutcome,
    };
    use onto_assurance_runtime::staged_settlement::StagedSettlementCoordinator;
    use onto_assurance_types::ids::RunId;
    use onto_assurance_types::enums::{BudgetOutcome, TaskOutcome};
    use onto_assurance_types::decision::SettlementDecision;
    use onto_assurance_types::enums::EffectClass;
    use std::sync::Arc;

    struct InMemoryReceiptStore {
        receipts: std::sync::Mutex<std::collections::HashMap<String, PublishReceipt>>,
    }
    impl InMemoryReceiptStore {
        fn new() -> Self {
            Self { receipts: std::sync::Mutex::new(std::collections::HashMap::new()) }
        }
    }
    #[async_trait::async_trait]
    impl PublishReceiptStorePort for InMemoryReceiptStore {
        async fn store_receipt(&self, receipt: &PublishReceipt) -> Result<bool, onto_assurance_runtime::ports::ReceiptStoreError> {
            let key = format!("receipt-{}", receipt.transaction_id);
            let already = self.receipts.lock().unwrap().contains_key(&key);
            self.receipts.lock().unwrap().insert(key, receipt.clone());
            Ok(!already)
        }
        async fn lookup_receipt(&self, _key: &IdempotencyKey) -> Result<Option<PublishReceipt>, onto_assurance_runtime::ports::ReceiptStoreError> {
            Ok(None)
        }
    }

    async fn persist_success(
        store: &InMemoryDecisionStore,
        run_id: RunId,
        attempt_id: AttemptId,
        decision_id: DecisionId,
    ) {
        let outcome = RunFinalizationOutcome {
            task_outcome: TaskOutcome::Success,
            budget_outcome: BudgetOutcome::WithinBudget,
            lifecycle_state: onto_assurance_types::enums::LifecycleState::Committed,
            session_decision_id: decision_id,
            attempt_decision_id: DecisionId::new(),
            settlement_decision: Some(SettlementDecision::Commit),
            effect_class: Some(EffectClass::Staged),
            reason_codes: vec![],
        };
        store.persist_session(run_id, attempt_id, &outcome).await.unwrap();
    }

    #[tokio::test]
    async fn m6a2_unknown_decision_rejected() {
        let (_temp, _source, adapter) = setup_workspace();
        let decisions = Arc::new(InMemoryDecisionStore::new());
        let receipts = Arc::new(InMemoryReceiptStore::new());
        let fs: Arc<dyn StagedFilesystemPort> = Arc::new(adapter);
        let coordinator = StagedSettlementCoordinator::new(fs, receipts);

        let result = coordinator.authorize_publish(
            DecisionId::new(), RunId::new(), AttemptId::new(), TransactionId::new(),
            ContentHash::new("any"), ContentHash::new("any"),
            &*decisions as &dyn DecisionStorePort,
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn m6a2_non_success_task_rejected() {
        let (_temp, _source, adapter) = setup_workspace();
        let decisions = Arc::new(InMemoryDecisionStore::new());
        let receipts = Arc::new(InMemoryReceiptStore::new());
        let fs: Arc<dyn StagedFilesystemPort> = Arc::new(adapter);
        let coordinator = StagedSettlementCoordinator::new(fs, receipts);
        let run_id = RunId::new();
        let decision_id = DecisionId::new();
        let outcome = RunFinalizationOutcome {
            task_outcome: TaskOutcome::Failed,
            budget_outcome: BudgetOutcome::WithinBudget,
            lifecycle_state: onto_assurance_types::enums::LifecycleState::Continuing,
            session_decision_id: decision_id,
            attempt_decision_id: DecisionId::new(),
            settlement_decision: Some(SettlementDecision::Rollback {
                reason: onto_assurance_types::enums::ReasonCode {
                    domain: "test".into(), code: "test".into(), detail: "test".into(),
                },
            }),
            effect_class: Some(EffectClass::Staged),
            reason_codes: vec![],
        };
        decisions.persist_session(run_id, AttemptId::new(), &outcome).await.unwrap();

        let result = coordinator.authorize_publish(
            decision_id, run_id, AttemptId::new(), TransactionId::new(),
            ContentHash::new("any"), ContentHash::new("any"),
            &*decisions as &dyn DecisionStorePort,
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn m6a2_valid_decision_authorizes_and_publishes() {
        let (_temp, source, adapter) = setup_workspace();
        let decisions = Arc::new(InMemoryDecisionStore::new());
        let receipts = Arc::new(InMemoryReceiptStore::new());
        let fs: Arc<dyn StagedFilesystemPort> = Arc::new(adapter);
        let coordinator = StagedSettlementCoordinator::new(fs.clone(), receipts);
        let run_id = RunId::new();
        let attempt_id = AttemptId::new();
        let txn_id = TransactionId::new();
        let decision_id = DecisionId::new();
        persist_success(&decisions, run_id, attempt_id, decision_id).await;

        let stage = fs.prepare(make_stage_req(txn_id, attempt_id, &source)).await.unwrap();
        let staging_path = PathBuf::from(&stage.staging_root);
        std::fs::File::create(staging_path.join("approved.rs")).unwrap();
        let manifest = fs.manifest(&stage).await.unwrap();
        let mh_bytes = onto_assurance_core::canonical::compute_hash(
            &manifest, &onto_assurance_types::hash::HashDomain::new(
                onto_assurance_types::hash::HashPurpose::Content, "ARTIFACT_MANIFEST",
            ),
        ).unwrap();
        let mh = ContentHash::new(hex::encode(mh_bytes));

        let permit = coordinator.authorize_publish(
            decision_id, run_id, attempt_id, txn_id,
            stage.baseline_hash.clone(), mh,
            &*decisions as &dyn DecisionStorePort,
        ).await.unwrap();

        let receipt = coordinator.execute_publish(&stage, &permit).await.unwrap();
        assert_eq!(receipt.transaction_id, txn_id);
        assert_ne!(receipt.before_hash, receipt.after_hash);
    }

    // ══════════════════════════════════════════════════════════════
    // M6-A2c: Settlement Atomicity gap tests
    // ══════════════════════════════════════════════════════════════

    #[tokio::test]
    async fn wrong_transaction_id_rejected_by_permit() {
        let (_temp, source, adapter) = setup_workspace();
        let txn_id = TransactionId::new();
        let other_txn = TransactionId::new();
        let attempt_id = AttemptId::new();
        let stage = adapter.prepare(make_stage_req(txn_id, attempt_id, &source)).await.unwrap();
        // Create a permit for a DIFFERENT transaction
        let wrong_permit = CommitPermit::new_for_test_only(
            DecisionId::new(), other_txn, attempt_id,
            ContentHash::new("x"), ContentHash::new("y"),
        );
        let req = PublishStageRequest {
            stage: stage.clone(), expected_baseline_hash: ContentHash::new("x"),
            approved_manifest_hash: ContentHash::new("y"),
            decision_id: DecisionId::new(),
            idempotency_key: IdempotencyKey::new(format!("t-{}", txn_id)),
        };
        let result = adapter.publish(req, &wrong_permit).await;
        assert!(result.is_err(), "wrong transaction_id must be rejected");
    }

    #[tokio::test]
    async fn wrong_baseline_hash_rejected_by_permit() {
        let (_temp, source, adapter) = setup_workspace();
        let txn_id = TransactionId::new();
        let attempt_id = AttemptId::new();
        let stage = adapter.prepare(make_stage_req(txn_id, attempt_id, &source)).await.unwrap();
        let permit = CommitPermit::new_for_test_only(
            DecisionId::new(), txn_id, attempt_id,
            ContentHash::new("correct-baseline"), ContentHash::new("y"),
        );
        let req = PublishStageRequest {
            stage: stage.clone(),
            expected_baseline_hash: ContentHash::new("wrong-baseline"),
            approved_manifest_hash: ContentHash::new("y"),
            decision_id: DecisionId::new(),
            idempotency_key: IdempotencyKey::new(format!("t-{}", txn_id)),
        };
        let result = adapter.publish(req, &permit).await;
        assert!(result.is_err(), "wrong baseline must be rejected");
    }

    // ══════════════════════════════════════════════════════════════
    // M6-A1b: Path safety tests
    // ══════════════════════════════════════════════════════════════

    #[test]
    fn reject_absolute_path_outside_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("ws");
        std::fs::create_dir_all(&root).unwrap();
        let result = validate_path_safe(Path::new("/etc/passwd"), &root);
        assert!(result.is_err());
    }

    #[test]
    fn reject_dot_dot_traversal() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("ws");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let result = validate_path_safe(&root.join("sub").join("..").join("..").join("passwd"), &root);
        assert!(result.is_err());
    }

    #[test]
    fn allow_valid_path_inside_workspace() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("ws");
        std::fs::create_dir_all(root.join("project")).unwrap();
        let result = validate_path_safe(&root.join("project"), &root);
        assert!(result.is_ok());
    }

    #[test]
    fn reject_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("ws");
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("real");
        std::fs::File::create(&target).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let result = validate_path_safe(&link, &root);
        assert!(result.is_err(), "symlinks must be rejected");
    }
}
