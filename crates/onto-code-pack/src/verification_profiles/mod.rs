//! Verification Profiles — 双平面路由 + Rule Bundle Hash (S0.5)。
//!
//! 语义平面: system_rules.json + rule_docs/*.md
//! 机器平面: LanguageVerifierSet (7 languages × 4 levels)

use sha2::{Sha256, Digest};
use crate::rules::router::RuleRouter;
use crate::machine_verifiers::LanguageVerifierSet;

/// 完整双平面 Profile — 语义规则 + 机器验证器集合。
pub struct DualPlaneProfile {
    pub language: String,
    pub semantic_rule_hash: String,
    pub machine_verifier_count: usize,
    pub effective_rule_hash: String,
}

/// 从 Router 解析双平面 Profile。
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
        machine_verifier_count: 0, // resolved at runtime
        effective_rule_hash: eff_hash,
    })
}

/// 规则变化检测。
pub fn has_rules_changed(old: &str, new: &str) -> bool { old != new }

/// 语义规则内容 → Verifier 注入模板。
pub fn inject_semantic_rule(template: &str, rule_content: &str) -> String {
    template.replace("{{system_rule}}", rule_content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_profile_resolves() {
        let router = RuleRouter::from_embedded();
        let p = resolve_dual_plane("rust", &router).unwrap();
        assert_eq!(p.language, "rust");
        assert!(!p.effective_rule_hash.is_empty());
    }

    #[test]
    fn go_profile_resolves() {
        let router = RuleRouter::from_embedded();
        let p = resolve_dual_plane("go", &router).unwrap();
        assert!(!p.semantic_rule_hash.is_empty());
    }

    #[test]
    fn rule_change_detected() {
        assert!(has_rules_changed("abc", "def"));
        assert!(!has_rules_changed("abc", "abc"));
    }

    #[test]
    fn template_injection() {
        let result = inject_semantic_rule("Review: {{system_rule}}", "Check for bugs");
        assert_eq!(result, "Review: Check for bugs");
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
