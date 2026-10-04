//! LanguageProfile — 语言配置。规则路由依据。

use serde::{Deserialize, Serialize};

/// 编程语言 / 目标类型 Profile。用于规则路由。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LanguageProfile {
    pub profile_id: String,
    pub name: String,
    pub file_extensions: Vec<String>,
    pub default_rule_sets: Vec<String>,
    pub linter_tools: Vec<String>,
    pub formatter_tools: Vec<String>,
    pub test_frameworks: Vec<String>,
    pub build_systems: Vec<String>,
}

impl LanguageProfile {
    pub fn matches_extension(&self, ext: &str) -> bool {
        self.file_extensions.iter().any(|e| e == ext)
    }

    pub fn rust() -> Self {
        Self {
            profile_id: "rust".into(), name: "Rust".into(),
            file_extensions: vec!["rs".into()],
            default_rule_sets: vec!["rust-clippy".into(), "rust-security".into()],
            linter_tools: vec!["clippy".into()], formatter_tools: vec!["rustfmt".into()],
            test_frameworks: vec!["cargo-test".into()], build_systems: vec!["cargo".into()],
        }
    }

    pub fn go() -> Self {
        Self {
            profile_id: "go".into(), name: "Go".into(),
            file_extensions: vec!["go".into()],
            default_rule_sets: vec!["go-vet".into(), "go-security".into()],
            linter_tools: vec!["golangci-lint".into()], formatter_tools: vec!["gofmt".into()],
            test_frameworks: vec!["go-test".into()], build_systems: vec!["go-build".into()],
        }
    }

    pub fn python() -> Self {
        Self {
            profile_id: "python".into(), name: "Python".into(),
            file_extensions: vec!["py".into(), "pyi".into()],
            default_rule_sets: vec!["python-ruff".into(), "python-security".into()],
            linter_tools: vec!["ruff".into()], formatter_tools: vec!["black".into()],
            test_frameworks: vec!["pytest".into()], build_systems: vec![],
        }
    }
}
