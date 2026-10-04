use async_trait::async_trait;
use onto_assurance_types::verification_target::VerificationTarget;

#[derive(Debug, Clone)]
pub struct RuleBinding { pub rule_id: String, pub target_ref: String, pub rule_content: String, pub rule_hash: String }

#[async_trait]
pub trait RuleSelector: Send + Sync {
    fn selector_id(&self) -> &str;
    fn select(&self, target: &VerificationTarget) -> Result<Vec<RuleBinding>, String>;
}
