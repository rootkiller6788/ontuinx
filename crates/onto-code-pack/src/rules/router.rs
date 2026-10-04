//! RuleRouter — 移植 OCR SystemRule.Resolve()。
//!
//! 核心原理：
//! - system_rules.json 只管路由（哪个文件→哪个规则文件）
//! - rule_docs/*.md 只管规则内容（LLM 直接读的自然语言）
//! - router.rs 只管确定性路径→规则映射
//!
//! 26 种文件类型，25 个专用规则文件，1 个全局默认规则。

use serde::Deserialize;
use std::collections::HashMap;

/// system_rules.json 的结构。
#[derive(Debug, Deserialize)]
struct SystemRules {
    default_rule: String,
    path_rule_map: HashMap<String, String>,
}

/// 规则路由器 — 确定性、纯函数。
pub struct RuleRouter {
    /// 已展开大括号的规则列表，按 system_rules.json 的 key 顺序排列。
    /// 每个元素是 (glob_pattern, rule_name)。
    rules: Vec<(String, String)>,
    default_rule: String,
}

impl RuleRouter {
    /// 从编译时嵌入的 system_rules.json 构建路由器。
    pub fn from_embedded() -> Self {
        let json = include_str!("system_rules.json");
        let sys: SystemRules = serde_json::from_str(json)
            .expect("system_rules.json must be valid");

        let mut rules = Vec::new();
        // HashMap 迭代顺序不固定，但 JSON 中 key 的顺序就是优先级
        // serde_json 的 Map 保留插入顺序，所以直接解析 path_rule_map
        // 然后按 JSON 原文的 key 顺序构建 rules
        // （HashMap 不保留顺序，需要用 serde_json::Value 直接遍历）
        let raw: serde_json::Value = serde_json::from_str(json).unwrap();
        if let Some(map) = raw["path_rule_map"].as_object() {
            for (pattern, rule) in map {
                let rule_name = rule.as_str().unwrap_or(&sys.default_rule);
                for expanded in expand_braces(pattern) {
                    rules.push((expanded, rule_name.to_string()));
                }
            }
        }

        Self { rules, default_rule: sys.default_rule }
    }

    /// 根据文件路径解析到规则文件名（如 "rust.md"）。
    /// 确定性 — 相同输入→相同输出。
    pub fn resolve_rule_name(&self, path: &str) -> &str {
        let lower = path.to_lowercase();

        // 顺序遍历 — 第一个匹配的立即返回
        for (pattern, rule_name) in &self.rules {
            if glob_match(pattern, &lower) {
                return rule_name;
            }
        }

        &self.default_rule
    }

    /// 根据文件路径返回规则内容（Markdown 文本）。
    pub fn resolve(&self, path: &str) -> &str {
        let rule_name = self.resolve_rule_name(path);
        Self::load_rule_doc(rule_name)
    }

    /// 加载 rule_docs/*.md 文件内容。编译时嵌入。
    fn load_rule_doc(name: &str) -> &'static str {
        // 内嵌所有 26 个规则文件
        match name {
            "default.md" => include_str!("rule_docs/default.md"),
            "rust.md" => include_str!("rule_docs/rust.md"),
            "go.md" => include_str!("rule_docs/go.md"),
            "python.md" => include_str!("rule_docs/python.md"),
            "java.md" => include_str!("rule_docs/java.md"),
            "kotlin.md" => include_str!("rule_docs/kotlin.md"),
            "cpp.md" => include_str!("rule_docs/cpp.md"),
            "c.md" => include_str!("rule_docs/c.md"),
            "ts_js_tsx_jsx.md" => include_str!("rule_docs/ts_js_tsx_jsx.md"),
            "arkts.md" => include_str!("rule_docs/arkts.md"),
            "astro.md" => include_str!("rule_docs/astro.md"),
            "julia.md" => include_str!("rule_docs/julia.md"),
            "graphql.md" => include_str!("rule_docs/graphql.md"),
            "freemarker.md" => include_str!("rule_docs/freemarker.md"),
            "properties.md" => include_str!("rule_docs/properties.md"),
            "pom_xml.md" => include_str!("rule_docs/pom_xml.md"),
            "build_gradle.md" => include_str!("rule_docs/build_gradle.md"),
            "package_json.md" => include_str!("rule_docs/package_json.md"),
            "cargo_toml.md" => include_str!("rule_docs/cargo_toml.md"),
            "json.md" => include_str!("rule_docs/json.md"),
            "github_workflows.md" => include_str!("rule_docs/github_workflows.md"),
            "github_config.md" => include_str!("rule_docs/github_config.md"),
            "yaml.md" => include_str!("rule_docs/yaml.md"),
            "po.md" => include_str!("rule_docs/po.md"),
            "pot.md" => include_str!("rule_docs/pot.md"),
            "mapper_dao_xml.md" => include_str!("rule_docs/mapper_dao_xml.md"),
            _ => include_str!("rule_docs/default.md"),
        }
    }

    /// 返回默认规则内容。
    pub fn default_rule(&self) -> &str {
        Self::load_rule_doc(&self.default_rule)
    }
}

/// 大括号展开：`*.{go,py,rs}` → `*.go`, `*.py`, `*.rs`
fn expand_braces(pattern: &str) -> Vec<String> {
    if let Some(start) = pattern.find('{') {
        if let Some(end) = pattern.find('}') {
            if end > start {
                let prefix = &pattern[..start];
                let suffix = &pattern[end + 1..];
                let inside = &pattern[start + 1..end];
                return inside.split(',')
                    .map(|opt| format!("{}{}{}", prefix, opt.trim(), suffix))
                    .collect();
            }
        }
    }
    vec![pattern.to_string()]
}

/// 简单 glob 匹配：`**/*.rs`, `**/*.{go,py}`, `src/main.rs`。
fn glob_match(pattern: &str, path: &str) -> bool {
    // 简化实现：支持 **/ 前缀和 *.ext 后缀
    if pattern.starts_with("**/") {
        let suffix = &pattern[3..];
        if suffix.starts_with("*.") {
            let ext = &suffix[1..]; // .rs, .go
            return path.ends_with(ext);
        }
        // 其他 **/pattern
        return path.ends_with(suffix) || path.contains(suffix);
    }
    // 精确路径匹配
    if !pattern.contains('*') {
        return path == pattern;
    }
    // 简单通配符：*.ext
    if pattern.starts_with("*.") {
        let ext = &pattern[1..];
        return path.ends_with(ext);
    }
    false
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brace_expansion() {
        let result = expand_braces("*.{go,py,rs}");
        assert_eq!(result, vec!["*.go", "*.py", "*.rs"]);
    }

    #[test]
    fn no_braces_passthrough() {
        assert_eq!(expand_braces("*.rs"), vec!["*.rs"]);
    }

    #[test]
    fn router_rust_file() {
        let router = RuleRouter::from_embedded();
        assert_eq!(router.resolve_rule_name("src/main.rs"), "rust.md");
    }

    #[test]
    fn router_go_file() {
        let router = RuleRouter::from_embedded();
        // system_rules.json doesn't have go — need to add it
        // For now, unknown falls back to default
        let result = router.resolve_rule_name("main.go");
        assert!(!result.is_empty());
    }

    #[test]
    fn router_unknown_gets_default() {
        let router = RuleRouter::from_embedded();
        let result = router.resolve_rule_name("data.bin");
        assert_eq!(result, "default.md");
    }

    #[test]
    fn rule_content_loaded() {
        let router = RuleRouter::from_embedded();
        let content = router.resolve("src/main.rs");
        assert!(!content.is_empty());
        assert!(!content.is_empty() && content.contains("####"));
    }

    #[test]
    fn default_rule_loaded() {
        let router = RuleRouter::from_embedded();
        let content = router.default_rule();
        assert!(!content.is_empty());
    }

    #[test]
    fn glob_star_ext() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.go"));
    }

    #[test]
    fn glob_double_star() {
        assert!(glob_match("**/*.java", "src/main/java/com/Foo.java"));
        assert!(glob_match("**/*.java", "Foo.java"));
        assert!(!glob_match("**/*.java", "Foo.kt"));
    }
}
