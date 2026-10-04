//! Verification Profiles — harness-level profile types.
//!
//! Domain-specific profile resolution (e.g. code language routing)
//! lives in domain crates (onto-assurance-domain-code).

use sha2::{Sha256, Digest};

/// 完整双平面 Profile — 语义规则 + 机器验证器集合。
pub struct DualPlaneProfile {
    pub language: String,
    pub semantic_rule_hash: String,
    pub machine_verifier_count: usize,
    pub effective_rule_hash: String,
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
    fn rule_change_detected() {
        assert!(has_rules_changed("abc", "def"));
        assert!(!has_rules_changed("abc", "abc"));
    }

    #[test]
    fn template_injection() {
        let result = inject_semantic_rule("Review: {{system_rule}}", "Check for bugs");
        assert_eq!(result, "Review: Check for bugs");
    }
}
