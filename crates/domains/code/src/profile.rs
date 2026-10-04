//! Code-specific profile resolution — depends on RuleRouter (rules) and
//! LanguageVerifierSet (machine_verifiers), both code-domain types.

use sha2::{Sha256, Digest};
use onto_pack::profile::DualPlaneProfile;
use crate::rules::router::RuleRouter;

/// 从 Router 解析双平面 Profile（代码领域）。
pub fn resolve_dual_plane(language: &str, router: &RuleRouter) -> Option<DualPlaneProfile> {
    let sample_path = match language {
        "rust" => "dummy.rs", "go" => "dummy.go", "python" => "dummy.py",
        "typescript" => "dummy.ts", "java" => "dummy.java", "kotlin" => "dummy.kt",
        "cpp" | "c" => "dummy.cpp",
        _ => return None,
    };
    let semantic_rule = router.resolve(sample_path);
    let sem_hash = hex::encode(&Sha256::digest(semantic_rule.as_bytes())[..8]);

    let combined = format!("{}:{}", language, sem_hash);
    let eff_hash = hex::encode(&Sha256::digest(combined.as_bytes())[..8]);

    Some(DualPlaneProfile {
        language: language.into(),
        semantic_rule_hash: sem_hash,
        machine_verifier_count: 0,
        effective_rule_hash: eff_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_profile_resolves() {
        let router = RuleRouter::from_embedded();
        let p = resolve_dual_plane("rust", &router).unwrap();
        assert_eq!(p.language, "rust");
    }

    #[test]
    fn go_profile_resolves() {
        let router = RuleRouter::from_embedded();
        let p = resolve_dual_plane("go", &router).unwrap();
        assert!(!p.semantic_rule_hash.is_empty());
    }

    #[test]
    fn all_seven_languages_resolve() {
        let router = RuleRouter::from_embedded();
        for lang in &["rust", "go", "python", "typescript", "java", "kotlin", "cpp"] {
            assert!(resolve_dual_plane(lang, &router).is_some(), "missing: {}", lang);
        }
    }

    #[test]
    fn effective_hash_changes_with_language() {
        let router = RuleRouter::from_embedded();
        let rust = resolve_dual_plane("rust", &router).unwrap();
        let go = resolve_dual_plane("go", &router).unwrap();
        assert_ne!(rust.effective_rule_hash, go.effective_rule_hash);
    }
}
