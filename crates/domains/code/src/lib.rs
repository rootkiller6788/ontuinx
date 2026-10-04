//! onto-assurance-domain-code — Code Assurance Domain Plugin.
//!
//! Implements onto-pack SPI (DomainPack, MachineVerifier) for software code.
//!
//! Scope: Git diff + full repository scan (7 languages).
//! Rules: 26 language-specific semantic rule files (OCR system ported from Alibaba).
//! Location: Dual-channel resolution (diff hunk + file content scan).
//! Verifiers: Build, test, lint, format, SAST, dependency audit.
//!
//! This is the reference domain plugin. Other domains (document, dataset, chip, etc.)
//! follow the same pattern.

pub mod scope;
pub mod location;
pub mod rules;
pub mod verifiers;
pub mod build_verifier;
pub mod test_verifier;
pub mod lint_verifier;
pub mod machine_verifiers;
pub mod file_verifiers;
pub mod artifact_collector;
pub mod project_profiler;
pub mod pg_stores;
pub mod artifact_store;
pub mod observability;
pub mod profile;
pub mod config;

use onto_pack::DomainPack;
use onto_assurance_types::finding::FindingCandidate;
use onto_assurance_types::verification_plan::VerificationUnit;

/// Code domain pack — implements DomainPack for software code verification.
pub struct CodeDomainPack;

impl DomainPack for CodeDomainPack {
    fn domain_id(&self) -> &str { "code" }
    fn domain_name(&self) -> &str { "Code Assurance Pack" }
    fn supported_kinds(&self) -> Vec<String> {
        vec!["source_file".into(), "diff".into(), "repository".into()]
    }
    fn verify(&self, _unit: &VerificationUnit) -> Result<Vec<FindingCandidate>, String> {
        Ok(vec![])
    }
}
