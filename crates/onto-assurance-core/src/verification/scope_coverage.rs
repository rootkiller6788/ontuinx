//! 覆盖检查 + ScopeManifest 不变量 SC-1/2/3
use onto_assurance_types::scope_manifest::ScopeManifest;
use onto_assurance_types::verification_target::{TargetKind, VerificationTarget};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageReport {
    pub total_targets: usize, pub covered: usize, pub excluded: usize,
    pub uncovered_required: Vec<String>, pub warnings: Vec<String>,
}

pub fn check_coverage(scope: &ScopeManifest, required: &[String], all_files: &[String]) -> CoverageReport {
    let mut r = CoverageReport { total_targets: all_files.len(), covered: scope.targets.len(), excluded: 0, uncovered_required: vec![], warnings: vec![] };
    for p in required {
        if all_files.iter().any(|f| f.contains(p)) && !scope.targets.iter().any(|t| t.target_ref.contains(p)) {
            r.uncovered_required.push(p.clone());
        }
    }
    for e in &scope.excluded_patterns {
        let c = all_files.iter().filter(|f| f.contains(e)).count();
        r.excluded += c;
        if c > all_files.len() / 2 { r.warnings.push(format!("broad exclusion '{}' covers {} of {}", e, c, all_files.len())); }
    }
    r
}

pub fn is_excluded(target: &VerificationTarget, patterns: &[String]) -> bool { patterns.iter().any(|p| target.target_ref.contains(p)) }

// SC-1/2/3 invariants
pub fn validate_no_duplicates(scope: &ScopeManifest) -> Result<(), String> {
    let mut seen = HashSet::new();
    for t in &scope.targets { if !seen.insert(&t.target_ref) { return Err(format!("duplicate: {}", t.target_ref)); } }
    Ok(())
}
pub fn validate_non_empty(scope: &ScopeManifest) -> Result<(), String> {
    for t in &scope.targets { if t.target_ref.is_empty() { return Err(format!("{}: empty target_ref", t.target_id)); } }
    Ok(())
}
pub fn validate_limit(scope: &ScopeManifest) -> Result<(), String> {
    if scope.targets.len() > scope.max_targets { return Err(format!("{} > max {}", scope.targets.len(), scope.max_targets)); }
    Ok(())
}
pub fn validate_scope(scope: &ScopeManifest) -> Result<(), Vec<String>> {
    let mut errs = vec![];
    if let Err(e) = validate_no_duplicates(scope) { errs.push(e); }
    if let Err(e) = validate_non_empty(scope) { errs.push(e); }
    if let Err(e) = validate_limit(scope) { errs.push(e); }
    if errs.is_empty() { Ok(()) } else { Err(errs) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vt(id: &str, path: &str) -> VerificationTarget { VerificationTarget::new(id, TargetKind::SourceFile, path, "h") }

    #[test]
    fn rejects_duplicates() {
        let s = ScopeManifest { targets: vec![vt("a","f.rs"), vt("b","f.rs")], excluded_patterns: vec![], max_targets: 10, generated_at: "n".into() };
        assert!(validate_no_duplicates(&s).is_err());
    }
    #[test]
    fn rejects_over_limit() {
        let s = ScopeManifest { targets: (0..5).map(|i| vt(&format!("t{}",i), &format!("f{}.rs",i))).collect(), excluded_patterns: vec![], max_targets: 3, generated_at: "n".into() };
        assert!(validate_limit(&s).is_err());
    }
    #[test]
    fn valid_scope_passes() {
        let s = ScopeManifest { targets: vec![vt("a","f.rs")], excluded_patterns: vec![], max_targets: 10, generated_at: "n".into() };
        assert!(validate_scope(&s).is_ok());
    }
}
