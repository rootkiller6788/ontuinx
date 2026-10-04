//! 双通道位置解析 (hunk + file scan) + 简单 offset 回退
use onto_assurance_types::finding::{FindingCandidate, SourceLocation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk { pub old_start: u32, pub old_count: u32, pub new_start: u32, pub new_count: u32, pub lines: Vec<HunkLine> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkLine { pub prefix: char, pub content: String, pub old_line: Option<u32>, pub new_line: Option<u32> }

pub fn resolve_location(f: &FindingCandidate, offset: i64) -> FindingCandidate {
    let mut r = f.clone();
    if let Some(ref loc) = f.location {
        r.location = Some(SourceLocation { start_line: (loc.start_line as i64 + offset).max(1) as u32, end_line: (loc.end_line as i64 + offset).max(1) as u32, ..loc.clone() });
    }
    r
}

pub fn resolve_from_hunks(code: &str, hunks: &[DiffHunk]) -> Option<(u32, u32)> {
    let nl = code.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>();
    if nl.len() < 2 { return None; }
    for h in hunks {
        for w in h.lines.windows(nl.len()) {
            if w.iter().zip(&nl).all(|(l, &c)| l.content.trim() == c) {
                let s = w.first()?.new_line.unwrap_or(w.first()?.old_line?);
                let e = w.last()?.new_line.unwrap_or(w.last()?.old_line?);
                return Some((s, e));
            }
        }
    }
    None
}

pub fn resolve_from_file(code: &str, content: &str) -> Option<(u32, u32)> {
    let nl: Vec<&str> = code.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if nl.len() < 2 || content.is_empty() { return None; }
    let fl: Vec<&str> = content.lines().collect();
    for i in 0..fl.len() {
        if i + nl.len() <= fl.len() && nl.iter().enumerate().all(|(j, &c)| fl[i+j].trim() == c) {
            return Some(((i+1) as u32, (i+nl.len()) as u32));
        }
    }
    None
}

pub fn is_valid_location(loc: &SourceLocation) -> bool { loc.start_line > 0 && loc.end_line >= loc.start_line }

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::finding::{FindingCategory, FindingSeverity};

    fn mf(code: &str) -> FindingCandidate {
        FindingCandidate { finding_id: "f".into(), target_id: "t".into(), rule_id: "r".into(), verifier_id: "v".into(), severity: FindingSeverity::High, category: FindingCategory::Bug, location: Some(SourceLocation { file_path: "f.rs".into(), start_line: 1, end_line: 1, start_column: None, end_column: None, existing_code: Some(code.into()), suggestion_code: None }), message: "m".into(), suggestion_code: None, confidence: 0.9 }
    }

    #[test]
    fn offset_resolve_works() { let f = mf(""); let r = resolve_location(&f, 5); assert_eq!(r.location.unwrap().start_line, 6); }

    #[test]
    fn file_scan_finds_code() {
        let r = resolve_from_file("let x = 1;\nprintln!(\"{}\", x);", "// header\nlet x = 1;\nprintln!(\"{}\", x);\n// footer");
        assert!(r.is_some());
        assert_eq!(r.unwrap(), (2, 3));
    }

    #[test]
    fn hunk_match_finds() {
        let hunks = vec![DiffHunk { old_start: 1, old_count: 2, new_start: 1, new_count: 2, lines: vec![HunkLine{prefix:' ',content:"old".into(),old_line:Some(1),new_line:Some(1)}, HunkLine{prefix:'-',content:"bad".into(),old_line:Some(2),new_line:None}] }];
        assert!(resolve_from_hunks("old\nbad", &hunks).is_some());
    }
}
