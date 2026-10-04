//! Runtime Profiles — decouple execution mode from personal-assistant default.
//!
//! Phase 8: Each profile selects its entry point, capabilities, resource scope,
//! policy, and OntoAssure pack. The system no longer assumes a Conversation is
//! the only way to start an agent run.

use serde::{Deserialize, Serialize};

/// A runtime profile defines the execution mode for an agent.
///
/// Profiles replace hardcoded "personal assistant" assumptions with explicit
/// configuration. Each profile is a self-contained description of what the
/// agent can do, how it's invoked, and what resources it can access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeProfile {
    /// Unique profile identifier.
    pub profile_id: String,
    /// Human-readable name.
    pub name: String,
    /// What kind of agent this profile is for.
    pub kind: ProfileKind,
    /// Allowed entry points (sources).
    pub allowed_sources: Vec<String>,
    /// Required capabilities.
    pub capabilities: Vec<String>,
    /// Optional skills (knowledge/prompt packs).
    pub skills: Vec<String>,
    /// Resource scope: what this profile can access.
    pub resource_scope: ResourceScope,
    /// OntoAssure pack to use for verification.
    pub assurance_pack: String,
    /// Maximum risk level accepted.
    pub max_risk_level: RiskTolerance,
    /// Does this profile require a Conversation?
    pub requires_conversation: bool,
}

/// What kind of agent profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileKind {
    /// Traditional chat-based assistant.
    PersonalAssistant,
    /// Autonomous coding agent.
    CodingAgent,
    /// Infrastructure operations agent.
    OpsAgent,
    /// Workflow/business process agent.
    WorkflowAgent,
    /// Robotics/physical-world agent.
    RobotAgent,
    /// Industrial control agent (PLC/SCADA).
    IndustrialAgent,
    /// Custom profile.
    Custom,
}

/// What resources this profile can access.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceScope {
    /// Allowed filesystem paths (glob patterns).
    pub allowed_paths: Vec<String>,
    /// Allowed network domains.
    pub allowed_domains: Vec<String>,
    /// Allowed database resources.
    pub allowed_databases: Vec<String>,
    /// Maximum resource quota.
    pub max_memory_mb: Option<u64>,
    pub max_cpu_seconds: Option<u64>,
    pub max_disk_mb: Option<u64>,
}

impl Default for ResourceScope {
    fn default() -> Self {
        Self {
            allowed_paths: vec![],
            allowed_domains: vec![],
            allowed_databases: vec![],
            max_memory_mb: None,
            max_cpu_seconds: None,
            max_disk_mb: None,
        }
    }
}

/// Risk tolerance level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RiskTolerance {
    /// Only pure/read-only operations.
    None,
    /// Staged + transactional allowed.
    Low,
    /// Compensatable allowed.
    Medium,
    /// Irreversible allowed (with strong approval).
    High,
}

/// Registry of available profiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileRegistry {
    pub profiles: Vec<RuntimeProfile>,
    pub default_profile: String,
}

impl ProfileRegistry {
    /// Find a profile by ID.
    pub fn find(&self, profile_id: &str) -> Option<&RuntimeProfile> {
        self.profiles.iter().find(|p| p.profile_id == profile_id)
    }

    /// Get the default profile.
    pub fn default_profile(&self) -> Option<&RuntimeProfile> {
        self.find(&self.default_profile)
    }

    /// Check if a source is allowed for a profile.
    pub fn is_source_allowed(&self, profile_id: &str, source: &str) -> bool {
        self.find(profile_id)
            .map(|p| p.allowed_sources.iter().any(|s| s == source))
            .unwrap_or(false)
    }

    /// Check if a profile requires a Conversation.
    pub fn requires_conversation(&self, profile_id: &str) -> bool {
        self.find(profile_id)
            .map(|p| p.requires_conversation)
            .unwrap_or(true) // safe default: require conversation if unknown
    }
}

/// Built-in profiles.
impl RuntimeProfile {
    pub fn personal_assistant() -> Self {
        Self {
            profile_id: "personal-assistant".into(),
            name: "Personal Assistant".into(),
            kind: ProfileKind::PersonalAssistant,
            allowed_sources: vec!["conversation".into(), "cli".into()],
            capabilities: vec!["file_read".into(), "file_write_staged".into(), "web_search".into()],
            skills: vec!["general-reasoning".into(), "code-assist".into()],
            resource_scope: ResourceScope::default(),
            assurance_pack: "code-pack".into(),
            max_risk_level: RiskTolerance::Low,
            requires_conversation: true,
        }
    }

    pub fn coding_agent() -> Self {
        Self {
            profile_id: "coding-agent".into(),
            name: "Coding Agent".into(),
            kind: ProfileKind::CodingAgent,
            allowed_sources: vec!["ontoloop".into(), "temporal".into(), "cli".into(), "direct".into()],
            capabilities: vec![
                "file_read".into(), "file_write_staged".into(),
                "shell_exec_staged".into(), "git_commit".into(),
            ],
            skills: vec!["code-generation".into(), "test-generation".into(), "refactoring".into()],
            resource_scope: ResourceScope {
                allowed_paths: vec!["**/*.rs".into(), "**/*.py".into(), "**/*.toml".into()],
                ..Default::default()
            },
            assurance_pack: "code-pack".into(),
            max_risk_level: RiskTolerance::Medium,
            requires_conversation: false,
        }
    }

    pub fn ops_agent() -> Self {
        Self {
            profile_id: "ops-agent".into(),
            name: "Ops Agent".into(),
            kind: ProfileKind::OpsAgent,
            allowed_sources: vec!["temporal".into(), "cli".into()],
            capabilities: vec![
                "k8s_get".into(), "k8s_apply".into(),
                "terraform_plan".into(), "terraform_apply".into(),
            ],
            skills: vec!["infrastructure".into(), "incident-response".into()],
            resource_scope: ResourceScope {
                allowed_domains: vec!["k8s.internal".into(), "terraform.internal".into()],
                ..Default::default()
            },
            assurance_pack: "ops-pack".into(),
            max_risk_level: RiskTolerance::High,
            requires_conversation: false,
        }
    }

    pub fn all_builtins() -> Vec<Self> {
        vec![
            Self::personal_assistant(),
            Self::coding_agent(),
            Self::ops_agent(),
        ]
    }

    /// Build the default registry.
    pub fn default_registry() -> ProfileRegistry {
        ProfileRegistry {
            profiles: Self::all_builtins(),
            default_profile: "coding-agent".into(),
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn personal_assistant_requires_conversation() {
        let pa = RuntimeProfile::personal_assistant();
        assert!(pa.requires_conversation);
        assert!(pa.allowed_sources.contains(&"conversation".to_string()));
    }

    #[test]
    fn coding_agent_no_conversation_required() {
        let ca = RuntimeProfile::coding_agent();
        assert!(!ca.requires_conversation);
        assert!(ca.allowed_sources.contains(&"ontoloop".to_string()));
        assert!(ca.allowed_sources.contains(&"temporal".to_string()));
    }

    #[test]
    fn registry_finds_profile() {
        let registry = ProfileRegistry {
            profiles: RuntimeProfile::all_builtins(),
            default_profile: "coding-agent".into(),
        };

        let found = registry.find("ops-agent");
        assert!(found.is_some());
        assert_eq!(found.unwrap().kind, ProfileKind::OpsAgent);

        let not_found = registry.find("nonexistent");
        assert!(not_found.is_none());
    }

    #[test]
    fn ops_agent_allows_temporal_not_conversation() {
        let ops = RuntimeProfile::ops_agent();
        assert!(ops.allowed_sources.contains(&"temporal".to_string()));
        assert!(!ops.allowed_sources.contains(&"conversation".to_string()));
    }

    #[test]
    fn registry_source_check() {
        let registry = RuntimeProfile::default_registry();

        assert!(registry.is_source_allowed("coding-agent", "ontoloop"));
        assert!(registry.is_source_allowed("coding-agent", "temporal"));
        assert!(!registry.is_source_allowed("coding-agent", "conversation"));
        assert!(registry.is_source_allowed("personal-assistant", "conversation"));
    }

    #[test]
    fn coding_agent_has_staged_capabilities() {
        let ca = RuntimeProfile::coding_agent();
        assert!(ca.capabilities.iter().any(|c| c.contains("staged")));
    }

    #[test]
    fn profiles_json_roundtrip() {
        let registry = RuntimeProfile::default_registry();
        let json = serde_json::to_string_pretty(&registry).unwrap();
        let back: ProfileRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.profiles.len(), 3);
        assert_eq!(back.default_profile, "coding-agent");
    }

    #[test]
    fn risk_tolerance_ordering() {
        assert!(RiskTolerance::Low > RiskTolerance::None);
        assert!(RiskTolerance::High > RiskTolerance::Medium);
        assert!(RiskTolerance::None < RiskTolerance::Low);
    }

    #[test]
    fn resource_scope_default_is_restrictive() {
        let scope = ResourceScope::default();
        assert!(scope.allowed_paths.is_empty());
        assert!(scope.allowed_domains.is_empty());
        assert!(scope.max_memory_mb.is_none());
    }
}
