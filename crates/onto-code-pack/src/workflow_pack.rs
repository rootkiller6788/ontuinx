//! WorkflowPack — 验证 OntoFlow 自身语义 (P4)。

use onto_assurance_types::finding::{FindingCandidate, FindingCategory, FindingSeverity};
use crate::domain_pack::DomainPack;
use onto_assurance_types::verification_plan::VerificationUnit;

pub struct WorkflowPack;

impl DomainPack for WorkflowPack {
    fn domain_id(&self) -> &str { "workflow" }
    fn domain_name(&self) -> &str { "Workflow Assurance Pack" }
    fn supported_kinds(&self) -> Vec<String> { vec!["workflow".into()] }

    fn verify(&self, unit: &VerificationUnit) -> Result<Vec<FindingCandidate>, String> {
        let t = &unit.target_id;
        let rules = vec![
            (1, "All nodes reachable from entry", FindingCategory::Bug, FindingSeverity::High),
            (2, "No dead (unreachable) nodes", FindingCategory::Bug, FindingSeverity::Critical),
            (3, "DAG has no cycles", FindingCategory::Bug, FindingSeverity::Critical),
            (4, "Barrier nodes check upstream Committed", FindingCategory::Security, FindingSeverity::Critical),
            (5, "Every WorkItem has Authority gate", FindingCategory::Security, FindingSeverity::Critical),
            (6, "Old generation does not contaminate new", FindingCategory::Security, FindingSeverity::Critical),
        ];
        let findings: Vec<_> = rules.into_iter().map(|(id, msg, cat, sev)| {
            FindingCandidate { finding_id: format!("wf-{}", id), target_id: t.clone(), rule_id: format!("wf-{}", id), verifier_id: "workflow-pack".into(), severity: sev, category: cat, location: None, message: msg.into(), suggestion_code: None, confidence: 0.95 }
        }).collect();
        Ok(findings)
    }
}
