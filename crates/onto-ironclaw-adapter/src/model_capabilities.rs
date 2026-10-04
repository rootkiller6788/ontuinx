//! Model Capability Registry — Phase 3 provider capability gating.
//!
//! Maps provider capabilities (tool_calling, reasoning, reasoning_roundtrip)
//! to OntoAssure constraints. Tasks requiring tool_calling can be filtered
//! to only use models that support it.

use std::collections::HashMap;
use std::sync::Mutex;

/// Capabilities a model+provider combination supports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub supports_tool_calling: bool,
    pub supports_reasoning: bool,
    pub supports_reasoning_roundtrip: bool,
}

/// Registry mapping model→provider→capabilities, backed by providers.json data.
pub struct ModelCapabilityRegistry {
    capabilities: Mutex<HashMap<String, ModelCapabilities>>,
}

impl ModelCapabilityRegistry {
    pub fn new() -> Self {
        Self { capabilities: Mutex::new(HashMap::new()) }
    }

    /// Register a model with its known capabilities.
    pub fn register(&self, model: &str, caps: ModelCapabilities) {
        self.capabilities.lock().unwrap().insert(model.to_string(), caps);
    }

    /// Pre-populate with known providers from providers.json capability data.
    pub fn with_known_providers() -> Self {
        let registry = Self::new();
        let known: Vec<(&str, ModelCapabilities)> = vec![
            ("deepseek-v4-flash", ModelCapabilities { supports_tool_calling: true, supports_reasoning: true, supports_reasoning_roundtrip: true }),
            ("deepseek-v4-pro", ModelCapabilities { supports_tool_calling: true, supports_reasoning: true, supports_reasoning_roundtrip: true }),
            ("deepseek-chat", ModelCapabilities { supports_tool_calling: true, supports_reasoning: false, supports_reasoning_roundtrip: false }),
            ("deepseek-reasoner", ModelCapabilities { supports_tool_calling: false, supports_reasoning: true, supports_reasoning_roundtrip: false }),
            ("claude-sonnet-4-20250514", ModelCapabilities { supports_tool_calling: true, supports_reasoning: true, supports_reasoning_roundtrip: true }),
            ("claude-opus-4-20250514", ModelCapabilities { supports_tool_calling: true, supports_reasoning: true, supports_reasoning_roundtrip: true }),
            ("gpt-5.5", ModelCapabilities { supports_tool_calling: true, supports_reasoning: true, supports_reasoning_roundtrip: false }),
            ("gpt-5-mini", ModelCapabilities { supports_tool_calling: true, supports_reasoning: true, supports_reasoning_roundtrip: false }),
            ("gemini-2.5-flash", ModelCapabilities { supports_tool_calling: true, supports_reasoning: true, supports_reasoning_roundtrip: true }),
            ("gemini-2.5-pro", ModelCapabilities { supports_tool_calling: true, supports_reasoning: true, supports_reasoning_roundtrip: true }),
        ];
        for (model, caps) in known {
            registry.register(model, caps);
        }
        registry
    }

    /// Look up capabilities for a model. Returns None if unknown.
    pub fn lookup(&self, model: &str) -> Option<ModelCapabilities> {
        self.capabilities.lock().unwrap().get(model).cloned()
    }

    /// Check if a model satisfies the required capabilities for a task.
    /// Returns Ok if all required caps are met, Err with missing caps otherwise.
    pub fn validate_for_task(
        &self,
        model: &str,
        requires_tool_calling: bool,
        requires_reasoning: bool,
    ) -> Result<(), Vec<String>> {
        let caps = self.lookup(model).unwrap_or_else(|| ModelCapabilities {
            supports_tool_calling: false,
            supports_reasoning: false,
            supports_reasoning_roundtrip: false,
        });

        let mut missing = Vec::new();
        if requires_tool_calling && !caps.supports_tool_calling {
            missing.push("tool_calling".into());
        }
        if requires_reasoning && !caps.supports_reasoning {
            missing.push("reasoning".into());
        }
        if missing.is_empty() { Ok(()) } else { Err(missing) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepseek_v4_flash_supports_tool_calling() {
        let registry = ModelCapabilityRegistry::with_known_providers();
        let caps = registry.lookup("deepseek-v4-flash").unwrap();
        assert!(caps.supports_tool_calling);
        assert!(caps.supports_reasoning_roundtrip);
    }

    #[test]
    fn validates_task_requirements() {
        let registry = ModelCapabilityRegistry::with_known_providers();
        // deepseek-v4-flash can do tool calling + reasoning
        assert!(registry.validate_for_task("deepseek-v4-flash", true, true).is_ok());
        // deepseek-chat can't do reasoning roundtrip
        assert!(registry.validate_for_task("deepseek-reasoner", true, false).is_err());
    }

    #[test]
    fn unknown_model_defaults_to_no_capabilities() {
        let registry = ModelCapabilityRegistry::new();
        assert!(registry.lookup("unknown-model-42").is_none());
        assert!(registry.validate_for_task("unknown-model-42", true, false).is_err());
    }
}
