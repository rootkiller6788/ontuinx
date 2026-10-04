use async_trait::async_trait; use onto_assurance_types::verification_session::{SessionState,VerificationSession}; use onto_assurance_types::verification_plan::VerificationPlan; use std::sync::Mutex; use std::collections::HashMap;
#[derive(Debug,thiserror::Error)] pub enum SessionError { #[error("not found: {0}")] NotFound(String), #[error("persistence: {0}")] Persistence(String) }
#[async_trait] pub trait SessionManager: Send+Sync { async fn create(&self, plan: VerificationPlan) -> Result<VerificationSession, SessionError>; async fn save(&self, s: &VerificationSession) -> Result<(), SessionError>; async fn load(&self, id: &str) -> Result<VerificationSession, SessionError>; async fn complete(&self, id: &str) -> Result<(), SessionError>; }
pub struct InMemorySessionManager { sessions: Mutex<HashMap<String, VerificationSession>> }
impl InMemorySessionManager { pub fn new() -> Self { Self { sessions: Mutex::new(HashMap::new()) } } }
#[async_trait] impl SessionManager for InMemorySessionManager {
    async fn create(&self, plan: VerificationPlan) -> Result<VerificationSession, SessionError> { let s = VerificationSession{session_id:plan.plan_id.clone(),plan,state:SessionState::Created,completed_units:vec![],failed_units:vec![],findings:vec![],tokens_used:0,started_at:chrono::Utc::now().to_rfc3339(),updated_at:chrono::Utc::now().to_rfc3339()}; self.sessions.lock().unwrap().insert(s.session_id.clone(), s.clone()); Ok(s) }
    async fn save(&self, s: &VerificationSession) -> Result<(), SessionError> { self.sessions.lock().unwrap().insert(s.session_id.clone(), s.clone()); Ok(()) }
    async fn load(&self, id: &str) -> Result<VerificationSession, SessionError> { self.sessions.lock().unwrap().get(id).cloned().ok_or_else(|| SessionError::NotFound(id.into())) }
    async fn complete(&self, id: &str) -> Result<(), SessionError> { if let Some(s) = self.sessions.lock().unwrap().get_mut(id) { s.state = SessionState::Completed; } Ok(()) }
}
#[cfg(test)] mod tests { use super::*; use onto_assurance_types::verification_target::VerificationTarget;
    fn mp() -> VerificationPlan { VerificationPlan{plan_id:"tp".into(),units:vec![],scope:onto_assurance_types::scope_manifest::ScopeManifest{targets:vec![VerificationTarget::new("t", onto_assurance_types::verification_target::TargetKind::SourceFile, "f.rs", "h")],excluded_patterns:vec![],max_targets:10,generated_at:"n".into()},rules:vec![],estimated_tokens:0,max_parallel_units:1} }
    #[tokio::test] async fn create_recover() { let m = InMemorySessionManager::new(); let s = m.create(mp()).await.unwrap(); assert_eq!(s.state, SessionState::Created); let r = m.load(&s.session_id).await.unwrap(); assert_eq!(r.session_id, s.session_id); }
}
