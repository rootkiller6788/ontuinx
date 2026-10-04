use onto_assurance_types::scope_manifest::ScopeManifest; use onto_assurance_types::verification_target::VerificationTarget;
pub fn scope_from_repository(files: &[String], excluded: Vec<String>, max: usize) -> ScopeManifest {
    let targets: Vec<_> = files.iter().filter(|f| !excluded.iter().any(|e| f.contains(e))).take(max).map(|f| {
        let id = format!("scan-{}", f.replace("/", "-"));
        VerificationTarget::new(&id, onto_assurance_types::verification_target::TargetKind::SourceFile, f.as_str(), "")
    }).collect();
    ScopeManifest { targets, excluded_patterns: excluded, max_targets: max, generated_at: chrono::Utc::now().to_rfc3339() }
}
#[cfg(test)] mod tests { use super::*;
    #[test] fn scan_excludes() { let s = scope_from_repository(&["src/a.rs".into(),"node_modules/x.js".into(),"target/b.rs".into()], vec!["node_modules".into(),"target".into()], 10); assert_eq!(s.targets.len(), 1); }
}
