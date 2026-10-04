use onto_assurance_types::finding::FindingCandidate;
use onto_assurance_types::verification_plan::VerificationUnit;
pub trait DomainPack: Send+Sync { fn domain_id(&self) -> &str; fn domain_name(&self) -> &str; fn supported_kinds(&self) -> Vec<String>; fn verify(&self, unit: &VerificationUnit) -> Result<Vec<FindingCandidate>, String>; }
