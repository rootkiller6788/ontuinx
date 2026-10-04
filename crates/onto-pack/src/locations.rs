use async_trait::async_trait;
use onto_assurance_types::verification_target::VerificationTarget;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocationHint { pub scheme: String, pub payload: serde_json::Value }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LocationResolution {
    Resolved { scheme: String, anchor: serde_json::Value, confidence: f64 },
    Ambiguous { candidates: Vec<serde_json::Value>, reason: String },
    Unresolved { reason: String },
    Unsupported { scheme: String, reason: String },
}

#[async_trait]
pub trait LocationResolver: Send + Sync {
    fn resolver_id(&self) -> &str;
    fn supported_schemes(&self) -> Vec<String>;
    fn priority(&self) -> i32 { 0 }
    async fn resolve(&self, target: &VerificationTarget, hint: &LocationHint) -> Result<LocationResolution, String>;
}
