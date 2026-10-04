//! Contract types — what the Agent intends to do, and what constraints bind it.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::enums::{EffectClass, RiskLevel};
use crate::ids::{ContractId, CriterionId, IntentId};

// ══════════════════════════════════════════════════════════════════
// ExecutionIntent — what the Agent proposes to do
// ══════════════════════════════════════════════════════════════════

/// Constructed BEFORE OntoRuntime authorization, so the auth system sees
/// task-level semantics (not just raw tool calls).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionIntent {
    pub intent_id: IntentId,
    pub objective: String,
    pub effect_class: EffectClass,
    pub risk_level: RiskLevel,
    pub resource_scope: ResourceScope,
    pub constraints: Vec<Constraint>,
    pub created_at: DateTime<Utc>,
}

/// What resources (files, network, databases) the intent targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceScope {
    pub files: BTreeSet<String>,
    pub allow_network: bool,
    pub target_hosts: BTreeSet<String>,
}

/// A single constraint on execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Constraint {
    pub kind: ConstraintKind,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintKind {
    MaxFiles,
    MaxCommands,
    MaxAttempts,
    TimeLimitSeconds,
    ForbiddenPattern,
    RequiredPattern,
}

// ══════════════════════════════════════════════════════════════════
// ExecutionContract — immutable, bound to a run
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionContract {
    pub contract_id: ContractId,
    pub intent_id: IntentId,
    pub criteria: Vec<AcceptanceCriterion>,
    pub evidence_required: Vec<EvidenceRequirement>,
    pub approval_mode: ApprovalMode,
    pub max_attempts: u32,
    pub created_at: DateTime<Utc>,
}

/// One thing that must be true for the task to be considered successful.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub criterion_id: CriterionId,
    pub name: String,
    pub kind: CriterionKind,
    pub description: String,
    pub is_blocking: bool, // unsatisfied blocking criterion → cannot SUCCEED
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionKind {
    TestPass,
    LintPass,
    TypeCheckPass,
    BenchmarkOk,
    NoRegression,
    Custom(String),
}

/// Evidence the Agent must provide before the task can be finalized.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRequirement {
    pub kind: EvidenceKind,
    pub is_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Diff,
    TestResult,
    LintResult,
    Verdict,
    HumanApproval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalMode {
    Never,
    OnFailure,
    OnHighRisk,
    Always,
}

// ══════════════════════════════════════════════════════════════════
// EffectClassification — output of the trusted EffectClassifier
// ══════════════════════════════════════════════════════════════════

/// Agent CANNOT set these fields.  Only the trusted EffectClassifier can.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectClassification {
    pub effect_class: EffectClass,
    pub rationale: String,
    /// If policy escalated the class (e.g. Staged → Irreversible), record
    /// what the original was.  None if no escalation occurred.
    pub upgraded_from: Option<EffectClass>,
    pub requires_approval: bool,
    pub pre_verification_required: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_serde() {
        let intent = ExecutionIntent {
            intent_id: IntentId::new(),
            objective: "Fix null pointer in payment service".into(),
            effect_class: EffectClass::Staged,
            risk_level: RiskLevel::High,
            resource_scope: ResourceScope {
                files: ["src/**/*.rs".into()].into(),
                allow_network: false,
                target_hosts: BTreeSet::new(),
            },
            constraints: vec![Constraint {
                kind: ConstraintKind::MaxFiles,
                value: "3".into(),
            }],
            created_at: Utc::now(),
        };
        let json = serde_json::to_string_pretty(&intent).unwrap();
        let back: ExecutionIntent = serde_json::from_str(&json).unwrap();
        assert_eq!(intent.objective, back.objective);
        assert_eq!(intent.effect_class, back.effect_class);
    }
}
