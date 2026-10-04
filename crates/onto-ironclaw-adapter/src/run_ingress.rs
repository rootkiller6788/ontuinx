//! RunIngressPort adapter — unified Run entry point.
//!
//! Phase 2: Provides a stub that records the request. When OntoRuntime's
//! run creation is wired, this adapter will delegate to the real
//! RunIngressPort implementation inside OntoRuntime's composition layer.

use onto_assurance_runtime::ports::RunIngressPort;
use onto_assurance_types::ingress::{
    RunIngressError, RunIngressStatus, SessionSource, StartRunRequest, StartRunResult,
};

/// Stub implementation that accepts all valid requests.
/// Production: replace with OntoRuntime composition delegate.
pub struct StubRunIngress {
    /// If set, reject requests from these sources.
    pub blocked_sources: Vec<SessionSource>,
}

impl StubRunIngress {
    pub fn new() -> Self {
        Self { blocked_sources: vec![] }
    }
}

impl Default for StubRunIngress {
    fn default() -> Self { Self::new() }
}

#[async_trait::async_trait]
impl RunIngressPort for StubRunIngress {
    async fn start_run(
        &self,
        request: StartRunRequest,
    ) -> Result<StartRunResult, RunIngressError> {
        // Validate input
        if request.input.objective.trim().is_empty() {
            return Err(RunIngressError::InvalidInput(
                "objective must not be empty".into(),
            ));
        }

        // Check blocked sources
        if self.blocked_sources.contains(&request.source) {
            return Err(RunIngressError::UnsupportedSource(request.source));
        }

        // In stub mode, accept and return a synthetic run ID.
        // Production will delegate to OntoRuntime's actual run creation.
        let run_id = format!(
            "run-{}-{}",
            request.source_label(),
            chrono::Utc::now().timestamp_millis()
        );

        Ok(StartRunResult {
            run_id: Some(run_id),
            source: request.source.clone(),
            status: RunIngressStatus::Started,
        })
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::ingress::{InputEnvelope, RuntimeActorRef};

    #[tokio::test]
    async fn stub_accepts_valid_request() {
        let ingress = StubRunIngress::new();
        let result = ingress
            .start_run(StartRunRequest {
                actor: RuntimeActorRef::human("alice"),
                source: SessionSource::Cli,
                input: InputEnvelope::new("say hello"),
                project_id: None,
                parent_work_item: None,
            })
            .await
            .unwrap();

        assert_eq!(result.status, RunIngressStatus::Started);
        assert!(result.run_id.is_some());
        let rid = result.run_id.unwrap();
        assert!(rid.starts_with("run-cli-"), "run_id should start with run-cli-, got {}", rid);
    }

    #[tokio::test]
    async fn stub_rejects_empty_objective() {
        let ingress = StubRunIngress::new();
        let result = ingress
            .start_run(StartRunRequest {
                actor: RuntimeActorRef::agent("bot"),
                source: SessionSource::Direct,
                input: InputEnvelope::new(""),
                project_id: None,
                parent_work_item: None,
            })
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            RunIngressError::InvalidInput(_) => {}
            e => panic!("expected InvalidInput, got {:?}", e),
        }
    }

    #[tokio::test]
    async fn stub_blocks_disallowed_sources() {
        let mut ingress = StubRunIngress::new();
        ingress.blocked_sources = vec![SessionSource::OntoFlow];

        let result = ingress
            .start_run(StartRunRequest {
                actor: RuntimeActorRef::service("cron"),
                source: SessionSource::OntoFlow,
                input: InputEnvelope::new("daily report"),
                project_id: None,
                parent_work_item: None,
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn all_actor_types_accepted() {
        let ingress = StubRunIngress::new();
        let actors = vec![
            RuntimeActorRef::human("alice"),
            RuntimeActorRef::agent("bot-42"),
            RuntimeActorRef::service("ontoloop"),
            RuntimeActorRef::device("sensor-1"),
        ];

        for actor in actors {
            let result = ingress
                .start_run(StartRunRequest {
                    actor: actor.clone(),
                    source: SessionSource::Direct,
                    input: InputEnvelope::new("test"),
                    project_id: None,
                    parent_work_item: None,
                })
                .await;
            assert!(result.is_ok(), "actor {:?} should be accepted", actor);
        }
    }

    #[tokio::test]
    async fn conversation_still_works() {
        // Critical: Conversation must remain a first-class source
        let ingress = StubRunIngress::new();
        let result = ingress
            .start_run(StartRunRequest {
                actor: RuntimeActorRef::human("bob"),
                source: SessionSource::Conversation,
                input: InputEnvelope::new("help me with code"),
                project_id: Some("my-project".into()),
                parent_work_item: None,
            })
            .await
            .unwrap();

        assert_eq!(result.source, SessionSource::Conversation);
        assert_eq!(result.status, RunIngressStatus::Started);
    }
}
