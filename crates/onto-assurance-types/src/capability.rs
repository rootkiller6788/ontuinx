//! Capability types — trusted descriptors and invocation envelopes.
//!
//! Phase 3: Every capability invocation carries a unified envelope.
//! EffectClass comes from the trusted CapabilityDescriptor, not the Agent.

use serde::{Deserialize, Serialize};

use crate::enums::EffectClass;
use crate::ids::{CapabilityId, CorrelationId, InvocationId, RunId};

/// Trusted descriptor for a registered capability.
///
/// This is the system's authoritative knowledge about what a capability does.
/// The Agent CANNOT override the `effect_class` or `required_approval` fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    /// Unique capability identifier.
    pub capability_id: CapabilityId,

    /// Human-readable name (e.g. "file_write", "sql_execute").
    pub name: String,

    /// Trusted effect classification. Set by the system, NOT the Agent.
    pub effect_class: EffectClass,

    /// Whether this capability always requires human approval.
    pub requires_approval: bool,

    /// Resource types this capability may access.
    pub resource_types: Vec<ResourceType>,

    /// Maximum risk level permitted for this capability.
    pub max_risk_level: crate::enums::RiskLevel,
}

/// What kind of resource a capability targets.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    Filesystem,
    Network,
    Database,
    Process,
    Secret,
    ExternalApi,
    Device,
    Memory,
    Custom(String),
}

/// A reference to a specific resource instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceRef {
    pub resource_type: ResourceType,
    pub resource_id: String,
    pub scope: Option<String>,
}

/// Unified envelope carried by every capability invocation.
///
/// This is the boundary between "Agent wants to do X" and "the system
/// executes X with accountability." Every field that affects safety
/// comes from the trusted descriptor, not from Agent input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityInvocationEnvelope {
    /// Unique ID for this specific invocation.
    pub invocation_id: InvocationId,

    /// The run this invocation belongs to.
    pub run_id: RunId,

    /// The capability being invoked.
    pub capability_id: CapabilityId,

    /// The actor who initiated the invocation (from StartRunRequest).
    pub actor: String,

    /// Hash of the invocation arguments (for audit and replay).
    pub arguments_hash: String,

    /// Resources this invocation targets.
    pub resource_refs: Vec<ResourceRef>,

    /// Trusted effect classification — from CapabilityDescriptor, NOT Agent.
    /// Agent CANNOT downgrade this. Call chain can only upgrade.
    pub effect_class: EffectClass,

    /// Groups related invocations across attempts.
    pub correlation_id: CorrelationId,
}

impl CapabilityInvocationEnvelope {
    pub fn new(
        capability: &CapabilityDescriptor,
        run_id: RunId,
        actor: impl Into<String>,
        arguments_hash: String,
        resource_refs: Vec<ResourceRef>,
    ) -> Self {
        Self {
            invocation_id: InvocationId::new(),
            run_id,
            capability_id: capability.capability_id,
            actor: actor.into(),
            arguments_hash,
            resource_refs,
            effect_class: capability.effect_class,
            correlation_id: CorrelationId::new(),
        }
    }

    /// The Agent can NEVER call this. Only the trusted classifier or
    /// policy engine may upgrade the effect class.
    pub fn upgrade_effect_class(&mut self, new_class: EffectClass) {
        let current_rank = effect_class_rank(self.effect_class);
        let new_rank = effect_class_rank(new_class);
        if new_rank > current_rank {
            self.effect_class = new_class;
        }
    }
}

fn effect_class_rank(c: EffectClass) -> u8 {
    match c {
        EffectClass::Pure => 0,
        EffectClass::ReadOnly => 1,
        EffectClass::Staged => 2,
        EffectClass::Transactional => 3,
        EffectClass::Compensatable => 4,
        EffectClass::Irreversible => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::RiskLevel;

    #[test]
    fn agent_cannot_downgrade_effect_class() {
        let desc = CapabilityDescriptor {
            capability_id: CapabilityId::new(),
            name: "file_write".into(),
            effect_class: EffectClass::Staged,
            requires_approval: false,
            resource_types: vec![ResourceType::Filesystem],
            max_risk_level: RiskLevel::High,
        };

        let mut env = CapabilityInvocationEnvelope::new(
            &desc,
            RunId::new(),
            "agent-1",
            "hash123".into(),
            vec![],
        );

        assert_eq!(env.effect_class, EffectClass::Staged);

        // Attempt to downgrade (simulating Agent trying)
        env.upgrade_effect_class(EffectClass::ReadOnly);
        // Must still be Staged — downgrade rejected
        assert_eq!(env.effect_class, EffectClass::Staged);

        // Policy upgrade (simulating classifier escalation)
        env.upgrade_effect_class(EffectClass::Irreversible);
        // Upgrade allowed
        assert_eq!(env.effect_class, EffectClass::Irreversible);
    }

    #[test]
    fn envelope_derives_effect_class_from_descriptor() {
        let desc = CapabilityDescriptor {
            capability_id: CapabilityId::new(),
            name: "deploy".into(),
            effect_class: EffectClass::Irreversible,
            requires_approval: true,
            resource_types: vec![ResourceType::ExternalApi],
            max_risk_level: RiskLevel::Critical,
        };

        let env = CapabilityInvocationEnvelope::new(
            &desc, RunId::new(), "svc", "h".into(), vec![],
        );

        assert_eq!(env.effect_class, EffectClass::Irreversible);
        assert!(desc.requires_approval);
    }
}
