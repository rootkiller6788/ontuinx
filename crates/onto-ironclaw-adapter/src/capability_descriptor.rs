//! CapabilityDescriptorPort adapter — Phase 3 stub.
//!
//! Production: wraps OntoRuntime's capability registry.
//! Stub: returns pre-registered descriptors for testing.

use std::collections::HashMap;
use std::sync::Mutex;

use onto_assurance_runtime::ports::{CapabilityDescriptorError, CapabilityDescriptorPort};
use onto_assurance_types::capability::{CapabilityDescriptor, ResourceType};
use onto_assurance_types::enums::{EffectClass, RiskLevel};
use onto_assurance_types::ids::CapabilityId;

/// Stub with pre-registered descriptors for common capabilities.
pub struct StubCapabilityRegistry {
    descriptors: Mutex<HashMap<String, CapabilityDescriptor>>,
}

impl StubCapabilityRegistry {
    pub fn new() -> Self {
        Self { descriptors: Mutex::new(HashMap::new()) }
    }

    /// Register a capability descriptor.
    pub fn register(&self, desc: CapabilityDescriptor) {
        self.descriptors.lock().unwrap().insert(desc.name.clone(), desc);
    }

    /// Pre-populate with common coding capabilities.
    pub fn with_common_capabilities() -> Self {
        let registry = Self::new();
        registry.register(CapabilityDescriptor {
            capability_id: CapabilityId::new(), name: "file_read".into(),
            effect_class: EffectClass::ReadOnly, requires_approval: false,
            resource_types: vec![ResourceType::Filesystem],
            max_risk_level: RiskLevel::Medium,
        });
        registry.register(CapabilityDescriptor {
            capability_id: CapabilityId::new(), name: "file_write".into(),
            effect_class: EffectClass::Staged, requires_approval: false,
            resource_types: vec![ResourceType::Filesystem],
            max_risk_level: RiskLevel::High,
        });
        registry.register(CapabilityDescriptor {
            capability_id: CapabilityId::new(), name: "http_get".into(),
            effect_class: EffectClass::ReadOnly, requires_approval: false,
            resource_types: vec![ResourceType::Network],
            max_risk_level: RiskLevel::Medium,
        });
        registry.register(CapabilityDescriptor {
            capability_id: CapabilityId::new(), name: "deploy".into(),
            effect_class: EffectClass::Irreversible, requires_approval: true,
            resource_types: vec![ResourceType::ExternalApi],
            max_risk_level: RiskLevel::Critical,
        });
        registry
    }
}

impl Default for StubCapabilityRegistry {
    fn default() -> Self { Self::with_common_capabilities() }
}

#[async_trait::async_trait]
impl CapabilityDescriptorPort for StubCapabilityRegistry {
    async fn resolve(
        &self,
        capability_name: &str,
    ) -> Result<CapabilityDescriptor, CapabilityDescriptorError> {
        self.descriptors
            .lock()
            .unwrap()
            .get(capability_name)
            .cloned()
            .ok_or_else(|| CapabilityDescriptorError::NotFound(capability_name.into()))
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_runtime::ports::CapabilityDescriptorPort;
    use onto_assurance_types::ids::RunId;

    #[tokio::test]
    async fn resolves_known_capability() {
        let registry = StubCapabilityRegistry::with_common_capabilities();
        let desc = registry.resolve("file_write").await.unwrap();
        assert_eq!(desc.effect_class, EffectClass::Staged);
        assert!(!desc.requires_approval);
    }

    #[tokio::test]
    async fn unknown_capability_returns_not_found() {
        let registry = StubCapabilityRegistry::new();
        let result = registry.resolve("exploit_tool").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            CapabilityDescriptorError::NotFound(_) => {}
            e => panic!("expected NotFound, got {:?}", e),
        }
    }

    #[tokio::test]
    async fn deploy_is_irreversible_and_requires_approval() {
        let registry = StubCapabilityRegistry::with_common_capabilities();
        let desc = registry.resolve("deploy").await.unwrap();
        assert_eq!(desc.effect_class, EffectClass::Irreversible);
        assert!(desc.requires_approval);
    }

    #[tokio::test]
    async fn create_envelope_uses_descriptor_not_agent_input() {
        let registry = StubCapabilityRegistry::with_common_capabilities();
        let envelope = registry
            .create_envelope(
                "file_write",
                RunId::new(),
                "agent-1".into(),
                "arg-hash-123".into(),
                vec![],
            )
            .await
            .unwrap();

        // Effect class comes from the descriptor (Staged), not from agent
        assert_eq!(envelope.effect_class, EffectClass::Staged);
        assert_eq!(envelope.capability_id, registry.resolve("file_write").await.unwrap().capability_id);
    }

    #[tokio::test]
    async fn downgrade_blocked_on_envelope() {
        let registry = StubCapabilityRegistry::with_common_capabilities();
        let mut envelope = registry
            .create_envelope("file_write", RunId::new(), "a".into(), "h".into(), vec![])
            .await
            .unwrap();

        assert_eq!(envelope.effect_class, EffectClass::Staged);
        // Agent tries to claim it's ReadOnly
        envelope.upgrade_effect_class(EffectClass::ReadOnly);
        // Must remain Staged
        assert_eq!(envelope.effect_class, EffectClass::Staged);
    }
}
