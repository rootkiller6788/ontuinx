//! LocationResolution — 生产级双通道位置解析 (P0)。
//!
//! Hunk 和 File 独立运行，不互为 fallback：
//! - 都指向同一位置 → Corroborated
//! - 只有一个成功 → SingleSource
//! - 都成功但指向不同 → Ambiguous
//! - 都失败 → Unresolved
//! - 快照变化 → Stale
//!
//! Pure functions — 相同输入 → 相同输出。

use onto_assurance_types::evidence_location::{
    AnchorCandidate, ByteRange, LineRange, LocationProof, ResolutionMethod,
    ResolvedSourceAnchor, SourceLocationClaim, SourceLocationResolution, UnresolvedReason,
};
use onto_assurance_types::finding::FindingCandidate;

/// 主入口：生产级双通道位置解析。
pub fn resolve_production(
    claim: &SourceLocationClaim,
    file_hash: &str,
    snapshot_hash: &str,
    hunk_matches: &[HunkMatch],
    file_matches: &[FileMatch],
    old_snapshot_hash: Option<&str>,
) -> SourceLocationResolution {
    // Check stale snapshot
    if let Some(old) = old_snapshot_hash {
        if old != snapshot_hash {
            return SourceLocationResolution::StaleSnapshot {
                expected_hash: old.into(), actual_hash: snapshot_hash.into(),
            };
        }
    }

    let snippet = match &claim.snippet {
        Some(s) if s.lines().count() >= 2 => s,
        _ => return SourceLocationResolution::Unresolved(UnresolvedReason::SnippetTooShort {
            min_lines: 2, actual: claim.snippet.as_ref().map(|s| s.lines().count()).unwrap_or(0),
        }),
    };

    // Channel 1: Hunk match
    let hunk_result = best_hunk_match(snippet, hunk_matches);

    // Channel 2: File match
    let file_result = best_file_match(snippet, file_matches);

    match (hunk_result, file_result) {
        (Some(h), Some(f)) => {
            if h.line_range == f.line_range && h.file_path == f.file_path {
                // Corroborated — both channels agree
                SourceLocationResolution::Resolved(build_anchor(
                    &h.file_path, file_hash, snapshot_hash, &h.line_range, &h.byte_range,
                    ResolutionMethod::Corroborated,
                    vec![LocationProof::DiffNewSideMatch, LocationProof::FullFileSnippetMatch],
                ))
            } else {
                // Ambiguous — channels disagree
                SourceLocationResolution::Ambiguous(vec![
                    AnchorCandidate { file_path: h.file_path, line_range: h.line_range, byte_range: h.byte_range, snippet_hash: hash_snippet(snippet), confidence: h.confidence, proof: LocationProof::DiffNewSideMatch },
                    AnchorCandidate { file_path: f.file_path, line_range: f.line_range, byte_range: f.byte_range, snippet_hash: hash_snippet(snippet), confidence: f.confidence, proof: LocationProof::FullFileSnippetMatch },
                ])
            }
        }
        (Some(h), None) => {
            SourceLocationResolution::Resolved(build_anchor(
                &h.file_path, file_hash, snapshot_hash, &h.line_range, &h.byte_range,
                ResolutionMethod::SingleSource,
                vec![LocationProof::DiffNewSideMatch],
            ))
        }
        (None, Some(f)) => {
            SourceLocationResolution::Resolved(build_anchor(
                &f.file_path, file_hash, snapshot_hash, &f.line_range, &f.byte_range,
                ResolutionMethod::SingleSource,
                vec![LocationProof::FullFileSnippetMatch],
            ))
        }
        (None, None) => {
            SourceLocationResolution::Unresolved(UnresolvedReason::NoMatch {
                attempted_channels: vec!["hunk".into(), "file".into()],
            })
        }
    }
}

// ── Hunk/File match result types ──

#[derive(Debug, Clone)]
pub struct HunkMatch {
    pub file_path: String,
    pub line_range: LineRange,
    pub byte_range: ByteRange,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct FileMatch {
    pub file_path: String,
    pub line_range: LineRange,
    pub byte_range: ByteRange,
    pub confidence: f64,
}

// ── Matching functions ──

fn best_hunk_match(snippet: &str, matches: &[HunkMatch]) -> Option<HunkMatch> {
    matches.iter()
        .filter(|m| m.confidence > 0.5)
        .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
        .cloned()
}

fn best_file_match(snippet: &str, matches: &[FileMatch]) -> Option<FileMatch> {
    matches.iter()
        .filter(|m| m.confidence > 0.5)
        .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap())
        .cloned()
}

fn build_anchor(
    file_path: &str, file_hash: &str, snapshot_hash: &str,
    line_range: &LineRange, byte_range: &ByteRange,
    method: ResolutionMethod, proofs: Vec<LocationProof>,
) -> ResolvedSourceAnchor {
    let fp = format!("{}:{}:{}-{}", file_path, file_hash, line_range.start, line_range.end);
    ResolvedSourceAnchor {
        repository_snapshot_hash: snapshot_hash.into(),
        file_path: file_path.into(), file_hash: file_hash.into(),
        byte_range: *byte_range, line_range: *line_range,
        symbol_id: None, ast_node_kind: None,
        snippet_hash: String::new(), context_hash: String::new(),
        anchor_fingerprint: fp, resolution_proofs: proofs,
        resolution_method: method,
    }
}

// ── Entry from FindingCandidate ──

/// 从 FindingCandidate 提取 SourceLocationClaim。
pub fn claim_from_finding(finding: &FindingCandidate) -> Option<SourceLocationClaim> {
    let loc = finding.location.as_ref()?;
    Some(SourceLocationClaim {
        target_ref: loc.file_path.clone(),
        claimed_lines: Some(LineRange { start: loc.start_line, end: loc.end_line }),
        snippet: loc.existing_code.clone(),
        symbol_name: None,
        byte_range: None,
    })
}

fn hash_snippet(s: &str) -> String {
    use sha2::{Sha256, Digest};
    let h = Sha256::digest(s.as_bytes());
    hex::encode(&h[..8])
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corroborated_when_both_agree() {
        let claim = SourceLocationClaim {
            target_ref: "src/main.rs".into(),
            claimed_lines: Some(LineRange{start:5,end:7}),
            snippet: Some("let x = 1;\nprintln!(\"{}\", x);".into()),
            symbol_name: None, byte_range: None,
        };
        let hunk = vec![HunkMatch{file_path:"src/main.rs".into(),line_range:LineRange{start:5,end:7},byte_range:ByteRange{start:100,end:150},confidence:0.95}];
        let file = vec![FileMatch{file_path:"src/main.rs".into(),line_range:LineRange{start:5,end:7},byte_range:ByteRange{start:100,end:150},confidence:0.92}];

        let r = resolve_production(&claim, "fh", "sh", &hunk, &file, None);
        assert!(r.is_resolved());
        match r {
            SourceLocationResolution::Resolved(a) => {
                assert_eq!(a.resolution_method, ResolutionMethod::Corroborated);
                assert_eq!(a.resolution_proofs.len(), 2);
            }
            _ => panic!("expected Resolved"),
        }
    }

    #[test]
    fn ambiguous_when_channels_disagree() {
        let claim = SourceLocationClaim {
            target_ref: "src/main.rs".into(),
            claimed_lines: Some(LineRange{start:5,end:7}),
            snippet: Some("let x = 1;\nprintln!(\"{}\", x);".into()),
            symbol_name: None, byte_range: None,
        };
        let hunk = vec![HunkMatch{file_path:"src/main.rs".into(),line_range:LineRange{start:5,end:7},byte_range:ByteRange{start:100,end:150},confidence:0.9}];
        let file = vec![FileMatch{file_path:"src/main.rs".into(),line_range:LineRange{start:20,end:22},byte_range:ByteRange{start:500,end:550},confidence:0.85}];

        let r = resolve_production(&claim, "fh", "sh", &hunk, &file, None);
        assert!(r.is_ambiguous());
    }

    #[test]
    fn unresolved_when_both_fail() {
        let claim = SourceLocationClaim {
            target_ref: "src/main.rs".into(),
            claimed_lines: None,
            snippet: Some("unique_code".into()),
            symbol_name: None, byte_range: None,
        };
        let r = resolve_production(&claim, "fh", "sh", &[], &[], None);
        assert!(!r.is_resolved());
    }

    #[test]
    fn stale_snapshot_rejected() {
        let claim = SourceLocationClaim {
            target_ref: "src/main.rs".into(),
            claimed_lines: None, snippet: Some("code".into()),
            symbol_name: None, byte_range: None,
        };
        let r = resolve_production(&claim, "fh", "new_sh", &[], &[], Some("old_sh"));
        match r {
            SourceLocationResolution::StaleSnapshot { .. } => {},
            _ => panic!("expected StaleSnapshot"),
        }
    }

    #[test]
    fn snippet_too_short_rejected() {
        let claim = SourceLocationClaim {
            target_ref: "f.rs".into(), claimed_lines: None,
            snippet: Some("x".into()), symbol_name: None, byte_range: None,
        };
        let r = resolve_production(&claim, "fh", "sh", &[], &[], None);
        assert!(matches!(r, SourceLocationResolution::Unresolved(UnresolvedReason::SnippetTooShort{..})));
    }
}
