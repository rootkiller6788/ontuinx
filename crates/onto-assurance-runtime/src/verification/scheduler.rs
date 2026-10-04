use async_trait::async_trait; use onto_assurance_types::verification_plan::VerificationUnit; use onto_assurance_types::finding::FindingCandidate;
pub type ScheduleResult = Result<Vec<FindingCandidate>, ScheduleError>;
#[derive(Debug,thiserror::Error)] pub enum ScheduleError { #[error("no verifier for {0}")] NoVerifier(String), #[error("internal: {0}")] Internal(String) }
#[async_trait] pub trait UnitScheduler: Send+Sync { async fn schedule(&self, unit: &VerificationUnit) -> ScheduleResult; }
