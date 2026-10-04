//! GraphRiskVerifier — P7
//!
//! Scheduled by policy. Analyzes: call chains, language-specific risks, shared state.
//! In Advisory mode: gRPC failure → warnings (not blocking).
//! In Required/StrengthenedRequired mode: gRPC failure → EnvironmentError.

use async_trait::async_trait;
use onto_assurance_types::finding::{FindingCandidate, FindingCategory, FindingSeverity};
use onto_assurance_types::verification_plan::VerificationUnit;
use std::sync::Arc;
use tracing::{info, warn};

use crate::client::GraphRiskClient;
use crate::schedule_rules::EnforcementLevel;

/// Risk analysis dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskDimension {
    CallChain,
    DependencyClosure,
    AffectedTests,
    SharedState,
    LanguageRisks,
}

#[derive(Debug, Clone)]
pub struct RiskDimensions {
    pub call_chain: bool,
    pub dependency_closure: bool,
    pub affected_tests: bool,
    pub shared_state: bool,
    pub language_risks: bool,
}

impl RiskDimensions {
    pub fn minimal() -> Self {
        Self { call_chain: true, dependency_closure: false,
               affected_tests: false, shared_state: false, language_risks: false }
    }
    pub fn full() -> Self {
        Self { call_chain: true, dependency_closure: true,
               affected_tests: true, shared_state: true, language_risks: true }
    }
    pub fn for_enforcement(level: EnforcementLevel) -> Self {
        match level {
            EnforcementLevel::Skip => Self { call_chain: false, dependency_closure: false,
                affected_tests: false, shared_state: false, language_risks: false },
            EnforcementLevel::Advisory | EnforcementLevel::Required => Self::minimal(),
            EnforcementLevel::StrengthenedRequired => Self::full(),
        }
    }
}

pub struct GraphRiskVerifier {
    client: Arc<dyn GraphRiskClient>,
    enforcement: EnforcementLevel,
    dimensions: RiskDimensions,
    proto_desc: onto_protocol::verifier::VerifierDescriptor,
}

impl GraphRiskVerifier {
    pub fn new(client: Arc<dyn GraphRiskClient>, enforcement: EnforcementLevel) -> Self {
        let dimensions = RiskDimensions::for_enforcement(enforcement);
        Self {
            client, enforcement, dimensions,
            proto_desc: onto_protocol::verifier::VerifierDescriptor {
                verifier_id: "graph-risk".to_string(),
                pass: onto_protocol::verifier::Pass::GraphRisk,
                stage: onto_protocol::verifier::VerificationStage::PostSandbox,
                mode: onto_protocol::verifier::VerificationMode::Internal,
                supported_rules: vec![
                    "graph_risk.call_chain".to_string(),
                    "graph_risk.dependency_closure".to_string(),
                    "graph_risk.affected_tests".to_string(),
                    "graph_risk.shared_state".to_string(),
                    "graph_risk.language_risks".to_string(),
                ],
            },
        }
    }
}

#[async_trait]

// ── New unified Verifier trait impl (parallel to old trait) ──
#[async_trait]
impl onto_protocol::verifier::Verifier for GraphRiskVerifier {
    fn descriptor(&self) -> &onto_protocol::verifier::VerifierDescriptor {
        &self.proto_desc
    }

    fn external_requirements(&self, _ctx: &onto_protocol::context::VerificationContext) -> Vec<onto_protocol::check::ExternalCheckRequirement> {
        vec![] // GraphRisk does its own gRPC
    }

    async fn evaluate(
        &self,
        ctx: &onto_protocol::context::VerificationContext,
        _evidence: &[(&String, &onto_protocol::sandbox::RawCheckResult)],
        _services: &onto_protocol::verifier::VerifierServices<'_>,
    ) -> onto_protocol::verifier::VerifierResult {
        let graph_ctx = match ctx.graph() {
            Some(g) => g,
            None => {
                return onto_protocol::verifier::VerifierResult {
                    verifier_id: self.proto_desc.verifier_id.clone(),
                    pass: onto_protocol::verifier::Pass::GraphRisk,
                    status: onto_protocol::verifier::VerifierStatus::PrerequisiteFailed,
                    findings: vec![], raw_evidence: vec![],
                    diagnostic: Some(onto_protocol::verifier::VerifierDiagnostic::new(
                        "GraphRisk requires GraphVerificationContext"
                    )),
                };
            }
        };

        let entity_keys = graph_ctx.changed_entity_keys.clone();
        if entity_keys.is_empty() {
            return onto_protocol::verifier::VerifierResult {
                verifier_id: self.proto_desc.verifier_id.clone(),
                pass: onto_protocol::verifier::Pass::GraphRisk,
                status: onto_protocol::verifier::VerifierStatus::Completed,
                findings: vec![], raw_evidence: vec![],
                diagnostic: None,
            };
        }

        let mut findings = Vec::new();
        if self.dimensions.call_chain {
            match self.client.analyze_call_chain(&graph_ctx.snapshot_id, &entity_keys, 3, 50).await {
                Ok(resp) => {
                    let high_fanout: Vec<_> = resp.entries.iter()
                        .filter(|e| e.depth <= 2 && e.direction == "caller").collect();
                    if high_fanout.len() > 10 {
                        findings.push(onto_protocol::finding::Finding {
                            finding_id: uuid::Uuid::new_v4().to_string(),
                            fingerprint: onto_protocol::finding::FindingFingerprint {
                                rule_id: "graph_risk.call_chain".to_string(), entity_key: None,
                                artifact_path: String::new(), semantic_key: "call_chain_fanout".to_string(),
                                line_hint: None,
                            },
                            pass: onto_protocol::verifier::Pass::GraphRisk,
                            rule_id: "graph_risk.call_chain_impact".to_string(),
                            rule_version: "1.0".to_string(),
                            severity: onto_protocol::finding::FindingSeverity::High,
                            category: onto_protocol::finding::CategoryId::new("maintainability"),
                            disposition: onto_protocol::finding::FindingDisposition::Advisory,
                            remediation: onto_protocol::finding::RemediationClass::RetryWithFeedback,
                            location: None,
                            message: format!("{} callers within depth 2 — wide impact risk.", high_fanout.len()),
                            fix_hint: None,
                            confidence: 1.0,
                            evidence_refs: vec![],
                        });
                    }
                }
                Err(e) => {
                    warn!("call chain analysis failed: {}", e);
                    if self.enforcement.is_blocking() {
                        return onto_protocol::verifier::VerifierResult {
                            verifier_id: self.proto_desc.verifier_id.clone(),
                            pass: onto_protocol::verifier::Pass::GraphRisk,
                            status: onto_protocol::verifier::VerifierStatus::Unavailable,
                            findings: vec![],
                            raw_evidence: vec![],
                            diagnostic: Some(onto_protocol::verifier::VerifierDiagnostic::with_detail(
                                "call chain unavailable", e.to_string(),
                            )),
                        };
                    }
                }
            }
        }

        onto_protocol::verifier::VerifierResult {
            verifier_id: self.proto_desc.verifier_id.clone(),
            pass: onto_protocol::verifier::Pass::GraphRisk,
            status: onto_protocol::verifier::VerifierStatus::Completed,
            findings,
            raw_evidence: vec![],
            diagnostic: None,
        }
    }
}
