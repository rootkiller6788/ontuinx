use async_trait::async_trait;
use onto_assurance_types::verification_target::VerificationTarget;

#[async_trait]
pub trait ScopeProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn supported_kinds(&self) -> Vec<String>;
    async fn enumerate(&self, root: &str, spec: &serde_json::Value) -> Result<Vec<VerificationTarget>, String>;
}
