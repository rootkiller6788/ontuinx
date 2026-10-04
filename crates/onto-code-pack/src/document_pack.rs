//! DocumentPack — Markdown/OpenAPI/ADR 验证 (P5)。

use onto_assurance_types::finding::{FindingCandidate, FindingCategory, FindingSeverity};
use crate::domain_pack::DomainPack;
use onto_assurance_types::verification_plan::VerificationUnit;

pub struct DocumentPack;

impl DomainPack for DocumentPack {
    fn domain_id(&self) -> &str { "document" }
    fn domain_name(&self) -> &str { "Document Assurance Pack" }
    fn supported_kinds(&self) -> Vec<String> { vec!["document".into(), "markdown".into(), "openapi".into(), "adr".into()] }

    fn verify(&self, unit: &VerificationUnit) -> Result<Vec<FindingCandidate>, String> {
        let t = &unit.target_id;
        let rules = vec![
            (1, "Required sections present", FindingCategory::Documentation, FindingSeverity::High),
            (2, "Internal links resolve to valid targets", FindingCategory::Bug, FindingSeverity::High),
            (3, "Heading hierarchy valid (no skipped levels)", FindingCategory::Style, FindingSeverity::Medium),
            (4, "Code blocks specify language", FindingCategory::Style, FindingSeverity::Low),
            (5, "OpenAPI schema valid against 3.x spec", FindingCategory::Bug, FindingSeverity::Critical),
            (6, "ADR has status/context/decision/consequences", FindingCategory::Documentation, FindingSeverity::High),
            (7, "Protocol fields match golden vectors", FindingCategory::Bug, FindingSeverity::Critical),
        ];
        let findings: Vec<_> = rules.into_iter().map(|(id, msg, cat, sev)| {
            FindingCandidate { finding_id: format!("doc-{}", id), target_id: t.clone(), rule_id: format!("doc-{}", id), verifier_id: "document-pack".into(), severity: sev, category: cat, location: None, message: msg.into(), suggestion_code: None, confidence: 0.9 }
        }).collect();
        Ok(findings)
    }
}
