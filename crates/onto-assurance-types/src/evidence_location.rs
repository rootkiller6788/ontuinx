//! EvidenceLocation — 生产级证据锚定系统 (P0)。
//!
//! 核心原则：
//! - 唯一确定时返回唯一位置
//! - 存在歧义时明确返回 Ambiguous，绝不猜测
//! - 快照变化时返回 Stale，绝不绑定旧位置

use serde::{Deserialize, Serialize};

// ── 不可信声明（Verifier/LLM 原始输出） ──

/// Verifier 声称的位置。不可信 — 必须经过解析。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLocationClaim {
    pub target_ref: String,
    pub claimed_lines: Option<LineRange>,
    pub snippet: Option<String>,
    pub symbol_name: Option<String>,
    pub byte_range: Option<ByteRange>,
}

// ── 权威解析结果 ──

/// 位置解析的最终结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SourceLocationResolution {
    /// 唯一确定的位置。
    Resolved(ResolvedSourceAnchor),
    /// 存在多个候选位置，无法唯一确定。
    Ambiguous(Vec<AnchorCandidate>),
    /// 无法解析。
    Unresolved(UnresolvedReason),
    /// 快照已变化，旧位置不再适用。
    StaleSnapshot {
        expected_hash: String,
        actual_hash: String,
    },
}

impl SourceLocationResolution {
    pub fn is_resolved(&self) -> bool { matches!(self, Self::Resolved(_)) }
    pub fn is_ambiguous(&self) -> bool { matches!(self, Self::Ambiguous(_)) }
    pub fn can_become_evidence(&self) -> bool { self.is_resolved() }
}

// ── 权威锚点 ──

/// 唯一确定的位置锚点。可绑定到 Evidence。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedSourceAnchor {
    pub repository_snapshot_hash: String,
    pub file_path: String,
    pub file_hash: String,
    pub byte_range: ByteRange,
    pub line_range: LineRange,
    pub symbol_id: Option<String>,
    pub ast_node_kind: Option<String>,
    pub snippet_hash: String,
    pub context_hash: String,
    pub anchor_fingerprint: String,
    /// 解析证明列表 — 记录通过哪些通道确认了位置。
    pub resolution_proofs: Vec<LocationProof>,
    /// 解析方式。
    pub resolution_method: ResolutionMethod,
}

// ── 候选锚点 ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnchorCandidate {
    pub file_path: String,
    pub line_range: LineRange,
    pub byte_range: ByteRange,
    pub snippet_hash: String,
    pub confidence: f64,
    pub proof: LocationProof,
}

// ── 解析方式 ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionMethod {
    /// Hunk 和 File 都指向同一位置。
    Corroborated,
    /// 只有一个通道成功。
    SingleSource,
    /// 简单偏移量修正（不可作为强 Evidence）。
    OffsetOnly,
}

// ── 位置证明 ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocationProof {
    ExactByteRange,
    ExactLineRange,
    DiffNewSideMatch,
    FullFileSnippetMatch,
    SymbolMatch,
    AstNodeMatch,
    ContextWindowMatch,
}

// ── 基础类型 ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UnresolvedReason {
    NoSnippet,
    SnippetTooShort { min_lines: usize, actual: usize },
    FileNotFound,
    SnapshotMismatch,
    NoMatch { attempted_channels: Vec<String> },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_can_become_evidence() {
        let anchor = ResolvedSourceAnchor {
            repository_snapshot_hash: "s1".into(), file_path: "f.rs".into(),
            file_hash: "h1".into(), byte_range: ByteRange{start:0,end:10},
            line_range: LineRange{start:1,end:3}, symbol_id: None,
            ast_node_kind: None, snippet_hash: "sh".into(), context_hash: "ch".into(),
            anchor_fingerprint: "fp".into(), resolution_proofs: vec![LocationProof::ExactLineRange],
            resolution_method: ResolutionMethod::SingleSource,
        };
        let r = SourceLocationResolution::Resolved(anchor);
        assert!(r.can_become_evidence());
    }

    #[test]
    fn ambiguous_cannot_become_evidence() {
        let r = SourceLocationResolution::Ambiguous(vec![]);
        assert!(!r.can_become_evidence());
    }

    #[test]
    fn stale_rejected() {
        let r = SourceLocationResolution::StaleSnapshot {
            expected_hash: "old".into(), actual_hash: "new".into(),
        };
        assert!(!r.is_resolved());
    }
}
