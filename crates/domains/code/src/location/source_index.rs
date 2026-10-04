//! SourceIndex — 全文件内容索引。
//! 用于通道 2：File Scan 匹配。

use onto_assurance_core::location_resolution::FileMatch;
use onto_assurance_types::evidence_location::{ByteRange, LineRange};

/// 在全文件中搜索 snippet。
pub fn search_file(snippet: &str, file_path: &str, content: &str) -> Vec<FileMatch> {
    let nl: Vec<&str> = snippet.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if nl.len() < 2 || content.is_empty() { return vec![]; }

    let fl: Vec<&str> = content.lines().collect();
    let mut matches = vec![];
    let mut byte_offset = 0usize;

    for (line_idx, line) in fl.iter().enumerate() {
        if line_idx + nl.len() <= fl.len() {
            let all_match = nl.iter().enumerate().all(|(j, &c)| fl[line_idx + j].trim() == c);
            if all_match {
                let end_line = line_idx + nl.len();
                let end_byte = byte_offset + fl[line_idx..end_line].iter().map(|l| l.len() + 1).sum::<usize>();
                matches.push(FileMatch {
                    file_path: file_path.into(),
                    line_range: LineRange { start: (line_idx + 1) as u32, end: end_line as u32 },
                    byte_range: ByteRange { start: byte_offset, end: end_byte },
                    confidence: 0.92,
                });
            }
        }
        byte_offset += line.len() + 1; // +1 for newline
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_in_file() {
        let content = "// header\nfn main() {\n    let x = 1;\n    println!(\"{}\", x);\n}\n";
        let m = search_file("let x = 1;\nprintln!(\"{}\", x);", "src/main.rs", content);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].line_range.start, 3);
        assert_eq!(m[0].line_range.end, 4);
    }

    #[test]
    fn no_match_for_absent_code() {
        let m = search_file("nonexistent", "f.rs", "some content");
        assert!(m.is_empty());
    }

    #[test]
    fn empty_file_returns_empty() {
        let m = search_file("code", "f.rs", "");
        assert!(m.is_empty());
    }
}
