use onto_assurance_types::scope_manifest::ScopeManifest; use onto_assurance_types::verification_target::VerificationTarget;
pub fn scope_from_diff(files: &[String], excluded: Vec<String>, max: usize) -> ScopeManifest {
    let targets: Vec<_> = files.iter().take(max).map(|f| {
        let id = format!("diff-{}", f.replace("/", "-"));
        VerificationTarget::new(&id, onto_assurance_types::verification_target::TargetKind::SourceFile, f.as_str(), "")
    }).collect();
    ScopeManifest { targets, excluded_patterns: excluded, max_targets: max, generated_at: chrono::Utc::now().to_rfc3339() }
}
#[cfg(test)] mod tests { use super::*;
    #[test] fn diff_generates() { let s = scope_from_diff(&["src/main.rs".into(),"src/lib.rs".into()], vec![], 10); assert_eq!(s.targets.len(), 2); }
}
