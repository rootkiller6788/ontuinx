//! 调度规则 — 纯函数，无 I/O。
//!
//! classifies changed file lists into ChangeType,
//! then maps ChangeType → EnforcementLevel for Integrity and Risk.

/// What kind of code was changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeType {
    Documentation,
    Config,
    TestCode,
    NormalCode,
    PublicApi,
    SecurityCritical,
    Unknown,
}

/// How strictly a verifier must be enforced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementLevel {
    Skip,
    Advisory,
    Required,
    StrengthenedRequired,
}

impl EnforcementLevel {
    pub fn should_invoke(self) -> bool { !matches!(self, Self::Skip) }
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Required | Self::StrengthenedRequired)
    }
}

/// Classify a list of changed file paths into the highest-risk ChangeType.
pub fn classify_change(changed_files: &[String]) -> ChangeType {
    if changed_files.is_empty() { return ChangeType::Unknown; }
    let mut highest = ChangeType::Documentation;
    for f in changed_files {
        let ct = classify_single(f);
        if ct > highest { highest = ct; }
    }
    highest
}

fn classify_single(path: &str) -> ChangeType {
    let lower = path.to_lowercase();

    // security-critical paths
    if lower.contains("auth") || lower.contains("payment") || lower.contains("crypto")
        || lower.contains("permission") || lower.contains("credential")
        || lower.contains("token") || lower.contains("secret") || lower.contains("key")
    {
        return ChangeType::SecurityCritical;
    }

    // documentation
    if lower.ends_with(".md") || lower.ends_with(".rst") || lower.ends_with(".txt")
        || lower.starts_with("docs/") || lower.starts_with("doc/")
        || lower.contains("readme") || lower.contains("changelog")
    {
        return ChangeType::Documentation;
    }

    // configuration
    if lower.ends_with(".toml") || lower.ends_with(".yaml") || lower.ends_with(".yml")
        || lower.ends_with(".json") || lower.ends_with(".cfg") || lower.ends_with(".ini")
        || lower.ends_with(".env.example") || lower.ends_with("makefile")
        || lower.contains("dockerfile") || lower.contains(".docker")
    {
        return ChangeType::Config;
    }

    // test code
    if lower.contains("test") || lower.contains("spec") || lower.contains("__test__")
        || lower.starts_with("tests/") || lower.starts_with("test/")
        || lower.ends_with("_test.rs") || lower.ends_with("_test.py") || lower.ends_with("_test.go")
        || lower.ends_with(".test.ts") || lower.ends_with(".spec.ts")
        || lower.ends_with("test.cpp") || lower.ends_with("test.c")
    {
        return ChangeType::TestCode;
    }

    // public api markers
    if lower.contains("__init__") || lower.contains("index.")
        || lower.ends_with("lib.rs") || lower.ends_with("mod.rs")
        || lower.starts_with("pkg/") || lower.starts_with("export")
    {
        return ChangeType::PublicApi;
    }

    // remaining → normal code
    ChangeType::NormalCode
}

/// GraphIntegrity enforcement policy.
pub fn integrity_policy(ct: ChangeType) -> EnforcementLevel {
    match ct {
        ChangeType::Documentation    => EnforcementLevel::Skip,
        ChangeType::Config           => EnforcementLevel::Advisory,
        ChangeType::TestCode         => EnforcementLevel::Required,
        ChangeType::NormalCode       => EnforcementLevel::Required,
        ChangeType::PublicApi        => EnforcementLevel::Required,
        ChangeType::SecurityCritical => EnforcementLevel::StrengthenedRequired,
        ChangeType::Unknown          => EnforcementLevel::Required,
    }
}

/// GraphRisk enforcement policy.
pub fn risk_policy(ct: ChangeType) -> EnforcementLevel {
    match ct {
        ChangeType::Documentation    => EnforcementLevel::Skip,
        ChangeType::Config           => EnforcementLevel::Advisory,
        ChangeType::TestCode         => EnforcementLevel::Advisory,
        ChangeType::NormalCode       => EnforcementLevel::Required,
        ChangeType::PublicApi        => EnforcementLevel::StrengthenedRequired,
        ChangeType::SecurityCritical => EnforcementLevel::StrengthenedRequired,
        ChangeType::Unknown          => EnforcementLevel::Required,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_readme_is_docs() {
        assert_eq!(classify_change(&["README.md".into()]), ChangeType::Documentation);
        assert_eq!(classify_change(&["docs/guide.md".into()]), ChangeType::Documentation);
    }

    #[test]
    fn test_auth_is_security() {
        assert_eq!(classify_change(&["src/auth/login.rs".into()]), ChangeType::SecurityCritical);
        assert_eq!(classify_change(&["internal/payment/handler.go".into()]), ChangeType::SecurityCritical);
    }

    #[test]
    fn test_config_is_config() {
        assert_eq!(classify_change(&["Cargo.toml".into()]), ChangeType::Config);
        assert_eq!(classify_change(&[".github/workflows/ci.yml".into()]), ChangeType::Config);
    }

    #[test]
    fn test_mixed_chooses_highest() {
        assert_eq!(
            classify_change(&["README.md".into(), "src/auth/payment.rs".into()]),
            ChangeType::SecurityCritical
        );
        assert_eq!(
            classify_change(&["README.md".into(), "src/main.rs".into()]),
            ChangeType::NormalCode
        );
    }

    #[test]
    fn test_docs_policy_is_skip() {
        assert_eq!(integrity_policy(ChangeType::Documentation), EnforcementLevel::Skip);
        assert_eq!(risk_policy(ChangeType::Documentation), EnforcementLevel::Skip);
    }

    #[test]
    fn test_security_policy_is_strengthened() {
        assert_eq!(integrity_policy(ChangeType::SecurityCritical), EnforcementLevel::StrengthenedRequired);
        assert_eq!(risk_policy(ChangeType::SecurityCritical), EnforcementLevel::StrengthenedRequired);
    }

    #[test]
    fn test_test_code_is_required_advisory() {
        assert_eq!(integrity_policy(ChangeType::TestCode), EnforcementLevel::Required);
        assert_eq!(risk_policy(ChangeType::TestCode), EnforcementLevel::Advisory);
    }

    #[test]
    fn test_unknown_is_required_both() {
        assert_eq!(integrity_policy(ChangeType::Unknown), EnforcementLevel::Required);
        assert_eq!(risk_policy(ChangeType::Unknown), EnforcementLevel::Required);
    }

    #[test]
    fn test_enforcement_level_methods() {
        assert!(!EnforcementLevel::Skip.should_invoke());
        assert!(EnforcementLevel::Advisory.should_invoke());
        assert!(!EnforcementLevel::Advisory.is_blocking());
        assert!(EnforcementLevel::Required.is_blocking());
        assert!(EnforcementLevel::StrengthenedRequired.is_blocking());
    }
}
