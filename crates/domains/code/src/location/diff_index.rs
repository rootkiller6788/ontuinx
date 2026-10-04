//! DiffIndex — 从 git diff hunk 构建位置索引。
//! 用于通道 1：Hunk 匹配。

use onto_assurance_core::location_resolution::HunkMatch;
use onto_assurance_types::evidence_location::{ByteRange, LineRange};

/// 从 diff hunk 行列表中搜索 snippet。
pub fn search_hunks(snippet: &str, hunks: &[DiffHunkEntry]) -> Vec<HunkMatch> {
    let nl: Vec<&str> = snippet.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if nl.len() < 2 { return vec![]; }

    let mut matches = vec![];
    for hunk in hunks {
        for w in hunk.lines.windows(nl.len()) {
            if w.iter().zip(&nl).all(|(l, &c)| l.content.trim() == c) {
                let first = &w[0];
                let last = &w[w.len() - 1];
                // Prefer new-side line numbers (added/context lines)
                if let (Some(s), Some(e)) = (first.new_line, last.new_line) {
                    matches.push(HunkMatch {
                        file_path: hunk.file_path.clone(),
                        line_range: LineRange { start: s, end: e },
                        byte_range: ByteRange { start: first.byte_offset, end: last.byte_offset + last.content.len() },
                        confidence: if w.len() == nl.len() { 0.95 } else { 0.7 },
                    });
                }
            }
        }
    }
    matches
}

#[derive(Debug, Clone)]
pub struct DiffHunkEntry {
    pub file_path: String,
    pub lines: Vec<DiffLineEntry>,
}

#[derive(Debug, Clone)]
pub struct DiffLineEntry {
    pub content: String,
    pub prefix: char,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub byte_offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_snippet_in_hunk() {
        let hunks = vec![DiffHunkEntry {
            file_path: "src/main.rs".into(),
            lines: vec![
                DiffLineEntry{content:"fn main() {".into(),prefix:' ',old_line:Some(1),new_line:Some(1),byte_offset:0},
                DiffLineEntry{content:"    let x = 1;".into(),prefix:'+',old_line:None,new_line:Some(2),byte_offset:12},
                DiffLineEntry{content:"    println!(\"{}\", x);".into(),prefix:'+',old_line:None,new_line:Some(3),byte_offset:40},
                DiffLineEntry{content:"}".into(),prefix:' ',old_line:Some(2),new_line:Some(4),byte_offset:70},
            ],
        }];
        let m = search_hunks("let x = 1;\nprintln!(\"{}\", x);", &hunks);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].line_range.start, 2);
        assert_eq!(m[0].line_range.end, 3);
    }
}
