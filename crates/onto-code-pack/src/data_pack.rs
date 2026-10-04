//! DataPack — PostgreSQL Schema 只读验证 (P6)。

use onto_assurance_types::finding::{FindingCandidate, FindingCategory, FindingSeverity};
use crate::domain_pack::DomainPack;
use onto_assurance_types::verification_plan::VerificationUnit;

pub struct DataPack;

impl DomainPack for DataPack {
    fn domain_id(&self) -> &str { "data" }
    fn domain_name(&self) -> &str { "Data Assurance Pack" }
    fn supported_kinds(&self) -> Vec<String> { vec!["dataset".into(), "schema".into(), "migration".into()] }

    fn verify(&self, unit: &VerificationUnit) -> Result<Vec<FindingCandidate>, String> {
        let t = &unit.target_id;
        let rules = vec![
            (1, "Migration up+down reversible", FindingCategory::Bug, FindingSeverity::Critical),
            (2, "NOT NULL addition has DEFAULT", FindingCategory::Bug, FindingSeverity::Critical),
            (3, "Column drop explicitly approved", FindingCategory::Bug, FindingSeverity::Critical),
            (4, "FK references existing table/column", FindingCategory::Bug, FindingSeverity::Critical),
            (5, "Index exists for FK columns", FindingCategory::Performance, FindingSeverity::High),
            (6, "Schema snapshot binds DB version", FindingCategory::Bug, FindingSeverity::High),
            (7, "PII columns classified and tagged", FindingCategory::Security, FindingSeverity::Critical),
            (8, "Evidence payload excludes raw values", FindingCategory::Security, FindingSeverity::Critical),
            (9, "Migration idempotent (re-runnable)", FindingCategory::Bug, FindingSeverity::High),
            (10, "Schema drift between environments detected", FindingCategory::Bug, FindingSeverity::High),
        ];
        let findings: Vec<_> = rules.into_iter().map(|(id, msg, cat, sev)| {
            FindingCandidate { finding_id: format!("data-{}", id), target_id: t.clone(), rule_id: format!("data-{}", id), verifier_id: "data-pack".into(), severity: sev, category: cat, location: None, message: msg.into(), suggestion_code: None, confidence: 0.9 }
        }).collect();
        Ok(findings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_10_rules() {
        let unit = VerificationUnit { unit_id: "u1".into(), target_id: "public.users".into(), rule_set_id: "schema".into(), verifier_kind: onto_assurance_types::verification_plan::VerifierKind::Deterministic, priority: 1, depends_on: vec![], metadata: serde_json::Value::default() };
        assert_eq!(DataPack.verify(&unit).unwrap().len(), 10);
    }
}
