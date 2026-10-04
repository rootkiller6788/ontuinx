//! Real RunIngressPort implementation — spawns ironclaw binary.
//!
//! Phase 2 production wiring: delegates to `ironclaw run --input-json`.

use std::process::Command;

use onto_assurance_runtime::ports::RunIngressPort;
use onto_assurance_types::ingress::{RunIngressError, RunIngressStatus, StartRunRequest, StartRunResult};

pub struct CliRunIngress {
    binary: String,
}

impl CliRunIngress {
    pub fn new(binary: impl Into<String>) -> Self { Self { binary: binary.into() } }

    fn request_to_json(&self, req: &StartRunRequest) -> serde_json::Value {
        serde_json::json!({
            "objective": req.input.objective,
            "requirements": req.input.requirements,
            "actor": req.actor.to_string(),
            "source": format!("{:?}", req.source).to_lowercase(),
            "model": req.input.model,
            "max_iterations": req.input.max_iterations,
        })
    }
}

#[async_trait::async_trait]
impl RunIngressPort for CliRunIngress {
    async fn start_run(&self, request: StartRunRequest) -> Result<StartRunResult, RunIngressError> {
        if request.input.objective.trim().is_empty() {
            return Err(RunIngressError::InvalidInput("empty objective".into()));
        }

        let json = self.request_to_json(&request);
        let json_str = serde_json::to_string(&json).map_err(|e| RunIngressError::Internal(e.to_string()))?;

        let output = Command::new(&self.binary)
            .args(["run", "--input-json", &json_str])
            .env("OPENAI_API_KEY", std::env::var("OPENAI_API_KEY").unwrap_or_default())
            .env("OPENAI_BASE_URL", std::env::var("OPENAI_BASE_URL").unwrap_or_default())
            .env("OPENAI_MODEL", std::env::var("OPENAI_MODEL").unwrap_or_default())
            .output()
            .map_err(|e| RunIngressError::Internal(format!("spawn failed: {}", e)))?;

        if output.status.success() {
            Ok(StartRunResult {
                run_id: Some(format!("cli-{}", chrono::Utc::now().timestamp_millis())),
                source: request.source.clone(),
                status: RunIngressStatus::Started,
            })
        } else {
            let _stderr = String::from_utf8_lossy(&output.stderr);
            Ok(StartRunResult {
                run_id: None,
                source: request.source.clone(),
                status: RunIngressStatus::Failed,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::ingress::{InputEnvelope, RuntimeActorRef, SessionSource};

    #[tokio::test]
    async fn rejects_empty_objective() {
        let ingress = CliRunIngress::new("echo"); // use echo as fake binary
        let result = ingress.start_run(StartRunRequest {
            actor: RuntimeActorRef::human("test"),
            source: SessionSource::Direct,
            input: InputEnvelope::new(""),
            project_id: None,
            parent_work_item: None,
        }).await;
        assert!(result.is_err());
    }

    #[test]
    fn json_generation_includes_all_fields() {
        let ingress = CliRunIngress::new("echo");
        let json = ingress.request_to_json(&StartRunRequest {
            actor: RuntimeActorRef::agent("bot-1"),
            source: SessionSource::OntoLoop,
            input: InputEnvelope::new("write hello()")
                .with_requirements(vec!["test".into()]),
            project_id: Some("proj".into()),
            parent_work_item: None,
        });
        assert_eq!(json["actor"], "agent:bot-1");
        assert_eq!(json["objective"], "write hello()");
        assert!(!json["requirements"].as_array().unwrap().is_empty());
    }
}
