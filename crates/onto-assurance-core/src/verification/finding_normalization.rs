//! Finding 去重、排序、过滤
use std::collections::HashSet;
use onto_assurance_types::finding::{FindingCandidate, FindingSeverity};

pub fn deduplicate(fs: &[FindingCandidate]) -> Vec<FindingCandidate> {
    let mut seen = HashSet::new(); let mut r = vec![];
    for f in fs {
        let k = format!("{}:{}:{}", f.target_id, f.rule_id, f.location.as_ref().map(|l| l.start_line).unwrap_or(0));
        if seen.insert(k) { r.push(f.clone()); }
    }
    r
}

pub fn sort_by_severity(fs: &mut [FindingCandidate]) {
    fs.sort_by(|a,b| rank(b.severity).cmp(&rank(a.severity)));
}
fn rank(s: FindingSeverity) -> u8 { match s { FindingSeverity::Critical=>4, FindingSeverity::High=>3, FindingSeverity::Medium=>2, FindingSeverity::Low=>1 } }

pub fn filter_by_confidence(fs: &[FindingCandidate], t: f64) -> Vec<FindingCandidate> { fs.iter().filter(|f| f.confidence >= t).cloned().collect() }

#[derive(Debug,Default,Clone,Copy)] pub struct FindingSummary { pub total: usize, pub blocking: usize }
impl FindingSummary { pub fn all_clear(&self) -> bool { self.blocking == 0 } }
pub fn summary(fs: &[FindingCandidate]) -> FindingSummary {
    let mut s = FindingSummary::default();
    for f in fs { s.total += 1; if f.is_blocking() { s.blocking += 1; } }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::finding::{FindingCategory, SourceLocation};
    fn mf(id: &str, line: u32, sev: FindingSeverity) -> FindingCandidate {
        FindingCandidate { finding_id: id.into(), target_id: "t".into(), rule_id: "r".into(), verifier_id: "v".into(), severity: sev, category: FindingCategory::Other, location: Some(SourceLocation{file_path:"f.rs".into(),start_line:line,end_line:line,start_column:None,end_column:None,existing_code:None,suggestion_code:None}), message: "m".into(), suggestion_code: None, confidence: 0.9 }
    }
    #[test] fn dedup_same_line() { let r = deduplicate(&[mf("a",10,FindingSeverity::High), mf("b",10,FindingSeverity::High)]); assert_eq!(r.len(), 1); }
    #[test] fn sort_critical_first() { let mut v = vec![mf("a",1,FindingSeverity::Medium), mf("b",2,FindingSeverity::Critical)]; sort_by_severity(&mut v); assert_eq!(v[0].severity, FindingSeverity::Critical); }
    #[test] fn filter_low_conf() { let v = vec![FindingCandidate{confidence:0.95,..mf("a",1,FindingSeverity::High)}, FindingCandidate{confidence:0.3,..mf("b",2,FindingSeverity::High)}]; assert_eq!(filter_by_confidence(&v, 0.8).len(), 1); }
}
