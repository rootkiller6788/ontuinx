use async_trait::async_trait;
use onto_assurance_types::finding::FindingCandidate;
use onto_assurance_types::contract::AcceptanceCriterion;

#[derive(Debug, Clone)]
pub struct CriterionDraft { pub kind: String, pub description: String }

#[async_trait]
pub trait CriterionMapper: Send + Sync {
    fn mapper_id(&self) -> &str;
    fn supported_categories(&self) -> Vec<String>;
    fn map(&self, finding: &FindingCandidate, available: &[AcceptanceCriterion]) -> Result<Vec<CriterionDraft>, String>;
}
