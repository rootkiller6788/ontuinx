//! OntoEnvelopeCapabilityPort — Phase 3 real OntoRuntime wiring.
//!
//! Wraps OntoRuntime's LoopCapabilityPort and stamps a CapabilityInvocationEnvelope
//! on every invocation. EffectClass comes from the trusted CapabilityDescriptor,
//! not from the Agent.

use std::sync::Arc;

use onto_assurance_runtime::ports::CapabilityDescriptorPort;
use onto_assurance_types::capability::CapabilityInvocationEnvelope;
use onto_assurance_types::ids::RunId;

/// Decorator that stamps a CapabilityInvocationEnvelope on each capability call.
///
/// This is the Phase 3 production wiring: every capability invocation through
/// OntoRuntime's CapabilityHost gets an envelope with the trusted EffectClass.
pub struct OntoEnvelopeCapabilityPort {
    descriptor_port: Arc<dyn CapabilityDescriptorPort>,
}

impl OntoEnvelopeCapabilityPort {
    pub fn new(descriptor_port: Arc<dyn CapabilityDescriptorPort>) -> Self {
        Self { descriptor_port }
    }

    /// Create an envelope for a capability invocation.
    /// The EffectClass comes from the trusted descriptor, never from caller input.
    pub async fn create_envelope(
        &self,
        capability_name: &str,
        run_id: RunId,
        actor: &str,
        arguments_hash: &str,
    ) -> Result<CapabilityInvocationEnvelope, String> {
        let descriptor = self.descriptor_port.resolve(capability_name).await
            .map_err(|e| format!("descriptor not found: {}", e))?;

        Ok(CapabilityInvocationEnvelope::new(
            &descriptor,
            run_id,
            actor,
            arguments_hash.to_string(),
            vec![],
        ))
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests — verify EffectClass from descriptor, not from agent input
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_descriptor::StubCapabilityRegistry;
    use onto_assurance_runtime::ports::CapabilityDescriptorPort;

    #[tokio::test]
    async fn envelope_uses_trusted_effect_class() {
        let registry = Arc::new(StubCapabilityRegistry::with_common_capabilities());
        let port = OntoEnvelopeCapabilityPort::new(registry as Arc<dyn CapabilityDescriptorPort>);

        let env = port.create_envelope(
            "deploy",
            RunId::new(),
            "agent-1",
            "hash123",
        ).await.unwrap();

        // Deploy is Irreversible — from the trusted descriptor, NOT from agent
        assert_eq!(env.effect_class, onto_assurance_types::enums::EffectClass::Irreversible);
    }

    #[tokio::test]
    async fn unknown_capability_fails_closed() {
        let registry = Arc::new(StubCapabilityRegistry::new());
        let port = OntoEnvelopeCapabilityPort::new(registry as Arc<dyn CapabilityDescriptorPort>);

        let result = port.create_envelope("exploit_tool", RunId::new(), "agent-2", "h").await;
        assert!(result.is_err(), "unknown capability must fail-closed");
    }

    #[tokio::test]
    async fn file_write_is_staged_not_agent_controlled() {
        let registry = Arc::new(StubCapabilityRegistry::with_common_capabilities());
        let port = OntoEnvelopeCapabilityPort::new(registry as Arc<dyn CapabilityDescriptorPort>);

        let env = port.create_envelope("file_write", RunId::new(), "agent-3", "h").await.unwrap();

        // Agent might think file_write is harmless, but descriptor says Staged
        assert_eq!(env.effect_class, onto_assurance_types::enums::EffectClass::Staged);
    }
}
