//! Industry Pack Definitions — v0.9
//!
//! Each pack defines the verification interface for a specific industry domain.
//! Code pack already exists in onto-code-pack. This module defines Ops, Data,
//! and Workflow pack trait requirements.
//!
//! ## Pack Architecture
//!
//! Every pack implements the `PackVerifier` trait (defined in onto-pack-sdk).
//! Each pack:
//! - Accepts an `ExecutionContract` + `EvidenceBundle`
//! - Returns a `PackVerdict` (passed/failed + per-criterion results)
//! - Is deterministic: same contract + same evidence = same verdict

use serde::{Deserialize, Serialize};

// ══════════════════════════════════════════════════════════════════
// Common Pack Types
// ══════════════════════════════════════════════════════════════════

/// A criterion specific to a pack domain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackCriterion {
    pub criterion_id: String,
    pub name: String,
    pub description: String,
    pub domain: PackDomain,
    pub is_blocking: bool,
    pub expected_kind: String,
}

/// Which industry domain a pack belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackDomain {
    Code,
    Ops,
    Data,
    Workflow,
    Robotics,
    Industrial,
}

/// Result of running a pack's verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackVerdict {
    pub domain: PackDomain,
    pub pack_name: String,
    pub passed: bool,
    pub total_criteria: u32,
    pub passed_criteria: u32,
    pub failed_criteria: Vec<String>,
    pub details: Vec<CriterionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CriterionResult {
    pub criterion_id: String,
    pub passed: bool,
    pub output: Option<String>,
    pub error: Option<String>,
}

// ══════════════════════════════════════════════════════════════════
// Ops Pack — infrastructure deployment verification
// ══════════════════════════════════════════════════════════════════

/// Ops pack criteria types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OpsCriterionKind {
    /// Kubernetes manifest validation.
    K8sManifestValid,
    /// K8s resource limits within bounds.
    K8sResourceLimits,
    /// Terraform plan is valid.
    TerraformPlanValid,
    /// Terraform plan doesn't destroy unexpected resources.
    TerraformNoDestroyUnexpected,
    /// Health check passes after deployment.
    HealthCheckPass,
    /// Deployment rollback plan exists.
    RollbackPlanExists,
    /// Configuration change is idempotent.
    ConfigIdempotent,
    /// No secrets in plain text.
    NoPlainTextSecrets,
    /// Network policy compliance.
    NetworkPolicyCompliant,
}

/// Ops pack verifier requirements.
pub trait OpsPackVerifier {
    /// Verify K8s manifests.
    fn verify_k8s_manifests(&self, manifests: &[String]) -> PackVerdict;

    /// Verify Terraform plan.
    fn verify_terraform_plan(&self, plan_json: &str, expected_no_destroy: &[String]) -> PackVerdict;

    /// Verify health check after deploy.
    fn verify_health_check(&self, endpoint: &str, expected_status: u16) -> PackVerdict;

    /// Verify rollback plan exists and is valid.
    fn verify_rollback_plan(&self, plan: &str) -> PackVerdict;

    /// Verify no plain-text secrets.
    fn verify_no_secrets(&self, config_files: &[String]) -> PackVerdict;
}

// ══════════════════════════════════════════════════════════════════
// Data Pack — data quality, schema, privacy
// ══════════════════════════════════════════════════════════════════

/// Data pack criteria types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DataCriterionKind {
    /// SQL migration is valid.
    SchemaMigrationValid,
    /// Migration is reversible (has DOWN).
    SchemaMigrationReversible,
    /// Migration doesn't break FK constraints.
    SchemaNoFkViolation,
    /// Data quality check passes.
    DataQualityCheck,
    /// No PII in output.
    NoPiiLeak,
    /// Data lineage traceable.
    DataLineageComplete,
    /// Privacy policy compliant.
    PrivacyPolicyCompliant,
    /// Row count within expected range.
    RowCountInRange,
}

/// Data pack verifier requirements.
pub trait DataPackVerifier {
    /// Verify SQL migration.
    fn verify_migration(&self, up_sql: &str, down_sql: &str, schema_ref: &str) -> PackVerdict;

    /// Verify data quality.
    fn verify_data_quality(&self, query: &str, expected_range: (u64, u64)) -> PackVerdict;

    /// Check for PII leaks.
    fn verify_no_pii(&self, output: &str, pii_patterns: &[String]) -> PackVerdict;

    /// Verify data lineage is traceable.
    fn verify_lineage(&self, source: &str, target: &str, transforms: &[String]) -> PackVerdict;
}

// ══════════════════════════════════════════════════════════════════
// Workflow Pack — business process verification
// ══════════════════════════════════════════════════════════════════

/// Workflow pack criteria types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowCriterionKind {
    /// Email template is valid.
    EmailTemplateValid,
    /// Email recipients are authorized.
    EmailRecipientsAuthorized,
    /// Approval chain is complete.
    ApprovalChainComplete,
    /// CRM record created correctly.
    CrmRecordValid,
    /// ERP transaction balanced.
    ErpTransactionBalanced,
    /// Order status transition is valid.
    OrderStateTransitionValid,
    /// API response matches expected schema.
    ApiResponseMatchesSchema,
    /// SLA timeline is feasible.
    SlaTimelineFeasible,
}

/// Workflow pack verifier requirements.
pub trait WorkflowPackVerifier {
    /// Verify email before send.
    fn verify_email(&self, template: &str, recipients: &[String], approved_list: &[String]) -> PackVerdict;

    /// Verify CRM operation.
    fn verify_crm_operation(&self, entity: &str, fields: &serde_json::Value, schema: &str) -> PackVerdict;

    /// Verify order state transition.
    fn verify_order_transition(&self, from_state: &str, to_state: &str, allowed: &[String]) -> PackVerdict;

    /// Verify API response.
    fn verify_api_response(&self, response: &str, json_schema: &str) -> PackVerdict;
}

// ══════════════════════════════════════════════════════════════════
// Pack Registry
// ══════════════════════════════════════════════════════════════════

/// Registry of available industry packs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackRegistry {
    pub packs: Vec<PackInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackInfo {
    pub domain: PackDomain,
    pub name: String,
    pub version: String,
    pub criteria: Vec<String>,
    pub description: String,
}

impl PackRegistry {
    pub fn available_packs() -> Self {
        Self {
            packs: vec![
                PackInfo {
                    domain: PackDomain::Code,
                    name: "code-pack".into(),
                    version: "0.1.0".into(),
                    criteria: vec![
                        "build_pass".into(), "test_pass".into(), "lint_pass".into(),
                        "type_check_pass".into(), "no_regression".into(),
                    ],
                    description: "Software build, test, and lint verification".into(),
                },
                PackInfo {
                    domain: PackDomain::Ops,
                    name: "ops-pack".into(),
                    version: "0.1.0".into(),
                    criteria: vec![
                        "k8s_manifest_valid".into(), "terraform_plan_valid".into(),
                        "health_check_pass".into(), "rollback_plan_exists".into(),
                        "no_plain_text_secrets".into(),
                    ],
                    description: "Infrastructure deployment safety verification".into(),
                },
                PackInfo {
                    domain: PackDomain::Data,
                    name: "data-pack".into(),
                    version: "0.1.0".into(),
                    criteria: vec![
                        "schema_migration_valid".into(), "schema_migration_reversible".into(),
                        "data_quality_check".into(), "no_pii_leak".into(),
                    ],
                    description: "Database migration and data quality verification".into(),
                },
                PackInfo {
                    domain: PackDomain::Workflow,
                    name: "workflow-pack".into(),
                    version: "0.1.0".into(),
                    criteria: vec![
                        "email_template_valid".into(), "email_recipients_authorized".into(),
                        "approval_chain_complete".into(), "crm_record_valid".into(),
                    ],
                    description: "Business process and workflow verification".into(),
                },
            ],
        }
    }

    pub fn find_by_domain(&self, domain: PackDomain) -> Option<&PackInfo> {
        self.packs.iter().find(|p| p.domain == domain)
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_registry_has_four_packs() {
        let registry = PackRegistry::available_packs();
        assert_eq!(registry.packs.len(), 4);
    }

    #[test]
    fn code_pack_is_registered() {
        let registry = PackRegistry::available_packs();
        let code = registry.find_by_domain(PackDomain::Code).unwrap();
        assert_eq!(code.name, "code-pack");
        assert!(code.criteria.contains(&"build_pass".to_string()));
    }

    #[test]
    fn ops_pack_has_safety_criteria() {
        let registry = PackRegistry::available_packs();
        let ops = registry.find_by_domain(PackDomain::Ops).unwrap();
        assert!(ops.criteria.contains(&"rollback_plan_exists".to_string()));
        assert!(ops.criteria.contains(&"no_plain_text_secrets".to_string()));
    }

    #[test]
    fn data_pack_has_reversible_migration() {
        let registry = PackRegistry::available_packs();
        let data = registry.find_by_domain(PackDomain::Data).unwrap();
        assert!(data.criteria.contains(&"schema_migration_reversible".to_string()));
        assert!(data.criteria.contains(&"no_pii_leak".to_string()));
    }

    #[test]
    fn workflow_pack_has_approval_chain() {
        let registry = PackRegistry::available_packs();
        let wf = registry.find_by_domain(PackDomain::Workflow).unwrap();
        assert!(wf.criteria.contains(&"approval_chain_complete".to_string()));
    }

    #[test]
    fn pack_info_json_roundtrip() {
        let registry = PackRegistry::available_packs();
        let json = serde_json::to_string_pretty(&registry).unwrap();
        let back: PackRegistry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.packs.len(), 4);
    }

    #[test]
    fn ops_criterion_kind_serialization() {
        let kinds = vec![
            OpsCriterionKind::K8sManifestValid,
            OpsCriterionKind::NoPlainTextSecrets,
        ];
        let json = serde_json::to_string(&kinds).unwrap();
        let back: Vec<OpsCriterionKind> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), 2);
    }

    #[test]
    fn pack_verdict_tracks_failures() {
        let verdict = PackVerdict {
            domain: PackDomain::Code,
            pack_name: "test-pack".into(),
            passed: false,
            total_criteria: 3,
            passed_criteria: 2,
            failed_criteria: vec!["lint_check".into()],
            details: vec![
                CriterionResult { criterion_id: "build".into(), passed: true, output: None, error: None },
                CriterionResult { criterion_id: "test".into(), passed: true, output: None, error: None },
                CriterionResult { criterion_id: "lint".into(), passed: false, output: None, error: Some("unused import".into()) },
            ],
        };
        assert!(!verdict.passed);
        assert_eq!(verdict.failed_criteria.len(), 1);
        assert_eq!(verdict.details.len(), 3);
    }
}
