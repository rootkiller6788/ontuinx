//! Code domain configuration — explicit, version-controlled, auditable.
//!
//! Counterpart to system_rules.json (semantic rules).
//! This file: machine verifier profiles (what to run, what's required).
//!
//! Placed at repo root as `assurance.toml` or `.ontocode/assurance.toml`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ══════════════════════════════════════════════════════════════════
// Top-level config
// ══════════════════════════════════════════════════════════════════

/// Project-level assurance configuration.
/// Lives at: `<repo_root>/assurance.toml`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssuranceConfig {
    /// Active domains. Only these will be loaded.
    /// Example: ["code", "document"]
    pub domains: Vec<String>,

    /// Domain-specific configuration. Key = domain_id.
    #[serde(default)]
    pub profiles: BTreeMap<String, DomainConfig>,
}

// ══════════════════════════════════════════════════════════════════
// Per-domain config
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainConfig {
    pub name: String,
    #[serde(default)]
    pub languages: BTreeMap<String, LanguageMachineConfig>,
    #[serde(default = "ScopeConfig::diff")]
    pub scope: ScopeConfig,
    #[serde(default = "EnforcementConfig::strict")]
    pub enforcement: EnforcementConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageMachineConfig {
    /// Machine verifier profile to use.
    /// Example: "onto.code.rust.v1"
    pub profile: String,

    /// Which verification levels are required (blocking).
    ///   "all" = all 4 levels
    ///   "lint+test" = level 1 + level 2
    ///   "advisory" = none blocking
    #[serde(default = "default_required_levels")]
    pub required_levels: String,

    /// Extra verifiers beyond the profile defaults.
    #[serde(default)]
    pub extra_verifiers: Vec<String>,
}

fn default_required_levels() -> String { "all".to_string() }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeConfig {
    /// How to discover targets.
    /// "diff" = git diff (default)
    /// "scan" = full repository scan
    /// "paths" = explicit list
    #[serde(default = "default_scope_mode")]
    pub mode: String,

    /// Git diff refs (mode = "diff")
    #[serde(default)]
    pub diff_from: Option<String>,
    #[serde(default)]
    pub diff_to: Option<String>,

    /// Explicit paths (mode = "paths")
    #[serde(default)]
    pub paths: Vec<String>,

    /// Exclude patterns (glob)
    #[serde(default)]
    pub exclude: Vec<String>,
}

fn default_scope_mode() -> String { "diff".to_string() }

impl ScopeConfig {
    fn diff() -> Self {
        Self { mode: "diff".into(), diff_from: None, diff_to: None, paths: vec![], exclude: vec![] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnforcementConfig {
    /// Maximum severity that can be advisory (non-blocking).
    /// "low" = only low-severity findings are advisory
    /// "medium" = low+medium are advisory
    /// "none" = all findings block (strictest)
    #[serde(default = "default_advisory_ceiling")]
    pub advisory_ceiling: String,

    /// Fail closed: if a required verifier is unavailable, fail the verification.
    #[serde(default = "default_true")]
    pub fail_on_verifier_unavailable: bool,

    /// If IronClaw is unavailable, fail (don't silently fall back to Generic LLM).
    #[serde(default = "default_true")]
    pub require_ironclaw: bool,
}

fn default_advisory_ceiling() -> String { "low".to_string() }
fn default_true() -> bool { true }

impl EnforcementConfig {
    fn strict() -> Self {
        Self { advisory_ceiling: "low".into(), fail_on_verifier_unavailable: true, require_ironclaw: true }
    }
}

// ══════════════════════════════════════════════════════════════════
// Default config (used when no assurance.toml exists)
// ══════════════════════════════════════════════════════════════════

impl Default for AssuranceConfig {
    fn default() -> Self {
        let mut languages = BTreeMap::new();
        for (lang, profile) in &[
            ("rust", "onto.code.rust.v1"),
            ("go", "onto.code.go.v1"),
            ("python", "onto.code.python.v1"),
            ("typescript", "onto.code.typescript.v1"),
            ("java", "onto.code.java.v1"),
            ("kotlin", "onto.code.kotlin.v1"),
            ("cpp", "onto.code.cpp.v1"),
        ] {
            languages.insert(lang.to_string(), LanguageMachineConfig {
                profile: profile.to_string(),
                required_levels: "all".to_string(),
                extra_verifiers: vec![],
            });
        }

        Self {
            domains: vec!["code".to_string()],
            profiles: BTreeMap::from([(
                "code".to_string(),
                DomainConfig {
                    name: "Code Assurance".to_string(),
                    languages,
                    scope: ScopeConfig {
                        mode: "diff".to_string(),
                        diff_from: None,
                        diff_to: None,
                        paths: vec![],
                        exclude: vec![],
                    },
                    enforcement: EnforcementConfig {
                        advisory_ceiling: "low".to_string(),
                        fail_on_verifier_unavailable: true,
                        require_ironclaw: true,
                    },
                },
            )]),
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// Loader
// ══════════════════════════════════════════════════════════════════

impl AssuranceConfig {
    /// Load from a project root. Falls back to default if no file found.
    pub fn load(repo_root: &str) -> Result<Self, ConfigError> {
        let path = std::path::Path::new(repo_root).join("assurance.toml");
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                toml::from_str(&content)
                    .map_err(|e| ConfigError::Parse(e.to_string()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // "No assurance.toml found at {}, using defaults", path.display());
                Ok(Self::default())
            }
            Err(e) => Err(ConfigError::Io(e.to_string())),
        }
    }

    /// Check if a domain is active.
    pub fn has_domain(&self, domain_id: &str) -> bool {
        self.domains.iter().any(|d| d == domain_id)
    }

    /// Get config for a specific domain.
    pub fn domain_config(&self, domain_id: &str) -> Option<&DomainConfig> {
        self.profiles.get(domain_id)
    }

    /// Get the machine verifier profile for a language.
    pub fn language_profile(&self, domain_id: &str, language: &str) -> Option<&LanguageMachineConfig> {
        self.domain_config(domain_id)?
            .languages
            .get(language)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("io error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_has_code_domain() {
        let cfg = AssuranceConfig::default();
        assert!(cfg.has_domain("code"));
        assert!(!cfg.has_domain("chip"));
    }

    #[test]
    fn default_has_7_languages() {
        let cfg = AssuranceConfig::default();
        let code = cfg.domain_config("code").unwrap();
        assert_eq!(code.languages.len(), 7);
    }

    #[test]
    fn resolve_rust_profile() {
        let cfg = AssuranceConfig::default();
        let p = cfg.language_profile("code", "rust").unwrap();
        assert_eq!(p.profile, "onto.code.rust.v1");
        assert_eq!(p.required_levels, "all");
    }

    #[test]
    fn fail_on_verifier_unavailable_by_default() {
        let cfg = AssuranceConfig::default();
        assert!(cfg.domain_config("code").unwrap().enforcement.fail_on_verifier_unavailable);
    }

    #[test]
    fn roundtrip_toml() {
        let cfg = AssuranceConfig::default();
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: AssuranceConfig = toml::from_str(&toml_str).unwrap();
        assert!(parsed.has_domain("code"));
    }
}
