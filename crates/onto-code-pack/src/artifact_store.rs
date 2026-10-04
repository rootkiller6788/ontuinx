//! Artifact Store — 大对象本地存储 (S2.2)。
//!
//! PG 只存 artifact_ref + content_hash + size + media_type。
//! 读取时重新校验 Hash。大对象 (stdout/stderr/规则文本/文件内容/Diff/Evidence Bundle) → 本地文件系统。

use sha2::{Sha256, Digest};
use std::fs;
use std::path::PathBuf;

/// Artifact 元数据 — PG 存储的引用。
#[derive(Debug, Clone)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub content_hash: String,
    pub size_bytes: u64,
    pub media_type: String,
    pub storage_path: String,
}

/// 本地文件系统 Artifact Store。
pub struct LocalArtifactStore {
    base_path: PathBuf,
}

impl LocalArtifactStore {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        let path = base.into();
        fs::create_dir_all(&path).ok();
        Self { base_path: path }
    }

    /// 存储一个 artifact，返回引用。
    pub fn store(&self, id: &str, content: &[u8], media_type: &str) -> Result<ArtifactRef, String> {
        let hash = hex::encode(Sha256::digest(content));
        let dir = &hash[..2];
        let dir_path = self.base_path.join(dir);
        fs::create_dir_all(&dir_path).map_err(|e| e.to_string())?;

        let file_path = dir_path.join(&hash[2..]);
        fs::write(&file_path, content).map_err(|e| e.to_string())?;

        Ok(ArtifactRef {
            artifact_id: id.into(),
            content_hash: hash,
            size_bytes: content.len() as u64,
            media_type: media_type.into(),
            storage_path: file_path.to_string_lossy().into(),
        })
    }

    /// 读取 artifact 并校验 Hash。
    pub fn load(&self, r: &ArtifactRef) -> Result<Vec<u8>, String> {
        let content = fs::read(&r.storage_path).map_err(|e| e.to_string())?;
        let actual_hash = hex::encode(Sha256::digest(&content));
        if actual_hash != r.content_hash {
            return Err(format!("hash mismatch: expected {} got {}", r.content_hash, actual_hash));
        }
        Ok(content)
    }

    /// 校验 artifact 是否存在且 Hash 一致。
    pub fn verify(&self, r: &ArtifactRef) -> Result<bool, String> {
        match fs::read(&r.storage_path) {
            Ok(content) => {
                let actual = hex::encode(Sha256::digest(&content));
                Ok(actual == r.content_hash)
            }
            Err(_) => Ok(false),
        }
    }
}

// ── Session Recovery Bindings (S2.3) ──

/// Session 恢复绑定 — 决定哪些 Unit 可以复用，哪些必须重跑。
#[derive(Debug, Clone)]
pub struct SessionRecoveryBinding {
    pub session_id: String,
    pub checkpoint_hash: String,
    pub contract_hash: String,
    pub scope_manifest_hash: String,
    pub effective_rule_hash: String,
    pub verification_profile_hash: String,
    pub verifier_version_hash: String,
    pub planner_version_hash: String,
    pub unit_fingerprint: String,
}

impl SessionRecoveryBinding {
    /// 完全匹配 → 可复用。
    pub fn can_reuse(&self, other: &Self) -> bool {
        self.checkpoint_hash == other.checkpoint_hash
            && self.contract_hash == other.contract_hash
            && self.effective_rule_hash == other.effective_rule_hash
    }

    /// 规则变化 → 仅失效受影响 Unit。
    pub fn only_rules_changed(&self, other: &Self) -> bool {
        self.effective_rule_hash != other.effective_rule_hash
            && self.checkpoint_hash == other.checkpoint_hash
    }

    /// Verifier 版本变化 → 重跑依赖该 Verifier 的 Unit。
    pub fn verifier_changed(&self, other: &Self) -> bool {
        self.verifier_version_hash != other.verifier_version_hash
    }

    /// Checkpoint 变化 → 全部失效。
    pub fn checkpoint_changed(&self, other: &Self) -> bool {
        self.checkpoint_hash != other.checkpoint_hash
    }
}

// ── Budget 多维持久化 (S2.4) ──

/// 多维持久化预算。
#[derive(Debug, Clone, Default)]
pub struct PersistentBudget {
    pub tokens_used: u64,
    pub model_cost_cents: u64,
    pub wall_time_millis: u64,
    pub concurrent_units: u32,
    pub semantic_calls: u32,
    pub cpu_millis: u64,
    pub memory_kb: u64,
    pub subprocess_count: u32,
    pub max_target_size_bytes: u64,
    pub artifact_read_bytes: u64,
}

impl PersistentBudget {
    pub fn is_exhausted(&self, limits: &BudgetLimits) -> Option<BudgetExhaustionReason> {
        if self.tokens_used >= limits.max_tokens { return Some(BudgetExhaustionReason::Tokens); }
        if self.model_cost_cents >= limits.max_cost_cents { return Some(BudgetExhaustionReason::Cost); }
        if self.wall_time_millis >= limits.max_wall_time_ms { return Some(BudgetExhaustionReason::WallTime); }
        if self.semantic_calls >= limits.max_semantic_calls { return Some(BudgetExhaustionReason::SemanticCalls); }
        None
    }
}

#[derive(Debug, Clone)]
pub struct BudgetLimits {
    pub max_tokens: u64, pub max_cost_cents: u64, pub max_wall_time_ms: u64,
    pub max_semantic_calls: u32, pub max_cpu_ms: u64, pub max_memory_kb: u64,
}

impl Default for BudgetLimits {
    fn default() -> Self {
        Self { max_tokens: 100000, max_cost_cents: 500, max_wall_time_ms: 3600000, max_semantic_calls: 50, max_cpu_ms: 300000, max_memory_kb: 2097152 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetExhaustionReason { Tokens, Cost, WallTime, SemanticCalls }

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_and_load_artifact() {
        let store = LocalArtifactStore::new("/tmp/vf-artifact-test");
        let r = store.store("test-1", b"hello world", "text/plain").unwrap();
        assert_eq!(r.size_bytes, 11);
        let content = store.load(&r).unwrap();
        assert_eq!(content, b"hello world");
    }

    #[test]
    fn hash_mismatch_rejected() {
        let store = LocalArtifactStore::new("/tmp/vf-artifact-test");
        let r = store.store("test-2", b"original", "text/plain").unwrap();
        // Tamper with the file
        std::fs::write(&r.storage_path, b"tampered").unwrap();
        assert!(store.load(&r).is_err());
    }

    #[test]
    fn session_can_reuse() {
        let b1 = SessionRecoveryBinding {
            session_id: "s1".into(), checkpoint_hash: "c1".into(), contract_hash: "ct1".into(),
            scope_manifest_hash: "s1".into(), effective_rule_hash: "r1".into(),
            verification_profile_hash: "p1".into(), verifier_version_hash: "v1".into(),
            planner_version_hash: "pl1".into(), unit_fingerprint: "u1".into(),
        };
        let b2 = b1.clone();
        assert!(b1.can_reuse(&b2));
    }

    #[test]
    fn checkpoint_change_full_invalidation() {
        let b1 = SessionRecoveryBinding {
            session_id: "s1".into(), checkpoint_hash: "c1".into(), contract_hash: "ct1".into(),
            scope_manifest_hash: "s1".into(), effective_rule_hash: "r1".into(),
            verification_profile_hash: "p1".into(), verifier_version_hash: "v1".into(),
            planner_version_hash: "pl1".into(), unit_fingerprint: "u1".into(),
        };
        let b2 = SessionRecoveryBinding { checkpoint_hash: "c2".into(), ..b1.clone() };
        assert!(!b1.can_reuse(&b2));
        assert!(b1.checkpoint_changed(&b2));
    }

    #[test]
    fn rule_change_only_affects_units() {
        let b1 = SessionRecoveryBinding {
            session_id: "s1".into(), checkpoint_hash: "c1".into(), contract_hash: "ct1".into(),
            scope_manifest_hash: "s1".into(), effective_rule_hash: "r1".into(),
            verification_profile_hash: "p1".into(), verifier_version_hash: "v1".into(),
            planner_version_hash: "pl1".into(), unit_fingerprint: "u1".into(),
        };
        let b2 = SessionRecoveryBinding { effective_rule_hash: "r2".into(), ..b1.clone() };
        assert!(b1.only_rules_changed(&b2));
    }

    #[test]
    fn budget_exhausted_by_tokens() {
        let b = PersistentBudget { tokens_used: 100001, ..Default::default() };
        assert_eq!(b.is_exhausted(&BudgetLimits::default()), Some(BudgetExhaustionReason::Tokens));
    }

    #[test]
    fn budget_not_exhausted() {
        let b = PersistentBudget::default();
        assert_eq!(b.is_exhausted(&BudgetLimits::default()), None);
    }
}
