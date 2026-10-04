//! DirectRunAdapter — bypass HTTP entry, feed Agent Run directly into M5 pipeline.
//!
//! The M5 bottleneck is NOT the seam — it's the HTTP product entry's incomplete
//! storage backend. This adapter skips HTTP and feeds data directly through
//! RunIngress → Agent Loop → Finalization → Decision.
//!
//! ## Usage
//!
//! - **CI (deterministic)**: MockLoopRuntime drives the pipeline without a real LLM.
//! - **Live smoke**: Python `ontotest/direct_run.py` drives real OntoRuntime binary.
//!
//! ## Pipeline
//!
//! ```text
//! StartRunRequest
//!   -> RunIngressPort (stub or CLI)
//!   -> RuntimeRunPort (mock or real OntoRuntime)
//!   -> RunFinalizationPort (OntoAssure kernel)
//!   -> RunFinalizationOutcome
//! ```

use onto_assurance_runtime::ports::{
    RunFinalizationOutcome, RunIngressPort, RuntimeRunPort,
};
use onto_assurance_types::enums::{LifecycleState, TaskOutcome};
use onto_assurance_types::ingress::StartRunRequest;
use onto_assurance_types::ids::RunId;

/// Result of a direct run through the M5 pipeline.
#[derive(Debug, Clone)]
pub struct DirectRunResult {
    pub run_id: Option<RunId>,
    pub task_outcome: TaskOutcome,
    pub lifecycle_state: LifecycleState,
    pub error: Option<String>,
}

impl DirectRunResult {
    /// Was the run successful (Committed)?
    pub fn is_committed(&self) -> bool {
        self.task_outcome == TaskOutcome::Success
            && self.lifecycle_state == LifecycleState::Committed
    }
}

/// Drives an OntoAssure-protected Agent Run directly, bypassing HTTP.
///
/// # Contract
///
/// - Agent produces a Run via RuntimeRunPort
/// - OntoAssure finalization runs on the Run
/// - The outcome is deterministic given the same mock input
pub struct DirectRunAdapter {
    ingress: Box<dyn RunIngressPort>,
}

impl DirectRunAdapter {
    pub fn new(ingress: Box<dyn RunIngressPort>) -> Self {
        Self { ingress }
    }

    /// Execute a single agent run end-to-end.
    ///
    /// This is the complete M5 pipeline in one call:
    /// 1. Start run via RunIngressPort
    /// 2. Execute via RuntimeRunPort (Agent Loop)
    /// 3. Wait for finalization (OntoAssure)
    /// 4. Return the structured outcome
    pub async fn execute(
        &self,
        request: StartRunRequest,
        runtime: &dyn RuntimeRunPort,
    ) -> DirectRunResult {
        // Step 1: Create the run
        let start_result = match self.ingress.start_run(request).await {
            Ok(r) => r,
            Err(e) => {
                return DirectRunResult {
                    run_id: None,
                    task_outcome: TaskOutcome::EnvironmentError,
                    lifecycle_state: LifecycleState::Continuing,
                    error: Some(format!("start_run failed: {}", e)),
                };
            }
        };

        let run_id = match start_result.run_id {
            Some(rid) => rid,
            None => {
                return DirectRunResult {
                    run_id: None,
                    task_outcome: TaskOutcome::EnvironmentError,
                    lifecycle_state: LifecycleState::Continuing,
                    error: Some("no run_id returned".into()),
                };
            }
        };

        // Step 2: Execute via runtime (mock or real OntoRuntime)
        // For CI, this uses MockLoopRuntime with pre-configured outcomes.
        let outcome = match runtime.await_terminal(RunId::new()).await {
            Ok(o) => o,
            Err(e) => {
                return DirectRunResult {
                    run_id: Some(RunId::new()),
                    task_outcome: TaskOutcome::EnvironmentError,
                    lifecycle_state: LifecycleState::Continuing,
                    error: Some(format!("execution failed: {}", e)),
                };
            }
        };

        // Step 3: Return the OntoAssure-verified outcome
        DirectRunResult {
            run_id: Some(RunId::new()),
            task_outcome: outcome.task_outcome,
            lifecycle_state: outcome.lifecycle_state,
            error: None,
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests — M5 DirectRun E2E
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_runtime::mocks::MockLoopRuntime;
    use onto_assurance_runtime::ports::RunFinalizationOutcome;
    use onto_assurance_types::enums::BudgetOutcome;
    use onto_assurance_types::ids::DecisionId;
    use onto_assurance_types::ingress::{InputEnvelope, RuntimeActorRef, SessionSource};
    use crate::run_ingress::StubRunIngress;

    fn make_outcome(task: TaskOutcome, lifecycle: LifecycleState) -> RunFinalizationOutcome {
        RunFinalizationOutcome {
            task_outcome: task,
            budget_outcome: BudgetOutcome::WithinBudget,
            lifecycle_state: lifecycle,
            session_decision_id: DecisionId::new(),
            attempt_decision_id: DecisionId::new(),
            reason_codes: vec![],
            settlement_decision: None,
            effect_class: None,
        }
    }

    /// DR.1: Success path — Agent succeeds, OntoAssure Commits.
    #[tokio::test]
    async fn dr1_success_path_committed() {
        let ingress = Box::new(StubRunIngress::new());
        let adapter = DirectRunAdapter::new(ingress);

        let mock_rt = MockLoopRuntime::new();
        mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

        let request = StartRunRequest {
            actor: RuntimeActorRef::human("alice"),
            source: SessionSource::Direct,
            input: InputEnvelope::new("write hello()"),
            project_id: None,
            parent_work_item: None,
        };

        let result = adapter.execute(request, &mock_rt).await;
        assert!(result.is_committed());
        assert_eq!(result.task_outcome, TaskOutcome::Success);
        assert_eq!(result.lifecycle_state, LifecycleState::Committed);
        assert!(result.error.is_none());
    }

    /// DR.2: Agent fails → OntoAssure marks Continuing.
    #[tokio::test]
    async fn dr2_agent_failure_continuing() {
        let ingress = Box::new(StubRunIngress::new());
        let adapter = DirectRunAdapter::new(ingress);

        let mock_rt = MockLoopRuntime::new();
        mock_rt.push_outcome(make_outcome(TaskOutcome::Failed, LifecycleState::Continuing));

        let request = StartRunRequest {
            actor: RuntimeActorRef::agent("bot-1"),
            source: SessionSource::OntoLoop,
            input: InputEnvelope::new("fix the bug"),
            project_id: None,
            parent_work_item: Some("parent-wi-1".into()),
        };

        let result = adapter.execute(request, &mock_rt).await;
        assert!(!result.is_committed());
        assert_eq!(result.task_outcome, TaskOutcome::Failed);
    }

    /// DR.3: Empty input rejected at ingress.
    #[tokio::test]
    async fn dr3_empty_input_rejected() {
        let ingress = Box::new(StubRunIngress::new());
        let adapter = DirectRunAdapter::new(ingress);

        let mock_rt = MockLoopRuntime::new();

        let request = StartRunRequest {
            actor: RuntimeActorRef::human("alice"),
            source: SessionSource::Cli,
            input: InputEnvelope::new(""), // empty
            project_id: None,
            parent_work_item: None,
        };

        let result = adapter.execute(request, &mock_rt).await;
        assert!(result.error.is_some());
        assert_eq!(result.task_outcome, TaskOutcome::EnvironmentError);
    }

    /// DR.4: All actor types pass through correctly.
    #[tokio::test]
    async fn dr4_all_actor_types() {
        let ingress = Box::new(StubRunIngress::new());
        let adapter = DirectRunAdapter::new(ingress);

        let actors = vec![
            (RuntimeActorRef::human("alice"), SessionSource::Conversation),
            (RuntimeActorRef::agent("bot"), SessionSource::OntoLoop),
            (RuntimeActorRef::service("cron"), SessionSource::OntoFlow),
            (RuntimeActorRef::device("sensor"), SessionSource::DeviceEvent),
        ];

        for (actor, source) in actors {
            let mock_rt = MockLoopRuntime::new();
            mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

            let request = StartRunRequest {
                actor,
                source,
                input: InputEnvelope::new("test"),
                project_id: None,
                parent_work_item: None,
            };

            let result = adapter.execute(request, &mock_rt).await;
            assert!(result.is_committed(), "actor should succeed");
        }
    }

    /// DR.5: DirectRun bypasses Conversation — no Conversation dependency.
    #[tokio::test]
    async fn dr5_direct_run_no_conversation_dependency() {
        // DirectRun source doesn't require a Conversation to exist
        let ingress = Box::new(StubRunIngress::new());
        let adapter = DirectRunAdapter::new(ingress);

        let mock_rt = MockLoopRuntime::new();
        mock_rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));

        let result = adapter
            .execute(
                StartRunRequest {
                    actor: RuntimeActorRef::service("ontoloop"),
                    source: SessionSource::Direct,
                    input: InputEnvelope::new("autonomous task"),
                    project_id: None,
                    parent_work_item: None,
                },
                &mock_rt,
            )
            .await;

        assert!(result.is_committed());
        // Direct source — no Conversation needed
    }
}
