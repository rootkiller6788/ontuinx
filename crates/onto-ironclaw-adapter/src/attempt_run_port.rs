//! AttemptRunPort — 一次 Attempt = 一次 OntoRuntime Run (P2)。
//!
//! IronClaw HTTP 入口未完成，内部集成优先使用 Rust Port。
//! HTTP 只是产品入口，不应成为内核依赖。

use async_trait::async_trait;
use onto_assurance_types::ids::{AttemptId, RunId};
use onto_assurance_types::ingress::{InputEnvelope, RuntimeActorRef, SessionSource, StartRunRequest};

/// 一次 Attempt 执行的结果。
#[derive(Debug, Clone)]
pub struct AttemptRunHandle {
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub loop_id: String,
    pub execution_generation: u64,
}

/// Agent Loop 退出后封存的报告。
#[derive(Debug, Clone)]
pub struct AttemptExitReport {
    pub handle: AttemptRunHandle,
    pub exit_reason: ExitReason,
    pub candidate_checkpoint_hash: Option<String>,
    pub observations: Vec<String>, // RuntimeObservation IDs
    pub side_effect_manifest_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ExitReason {
    AgentFinished,
    AgentDeclaredComplete,
    MaxTurnsReached,
    UserCancelled,
    ToolFailure(String),
    ProviderError(String),
    ProcessCrashed,
    BudgetExhausted,
    Timeout,
}

#[derive(Debug, thiserror::Error)]
pub enum RunIngressError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("run not found: {0}")]
    NotFound(String),
    #[error("internal: {0}")]
    Internal(String),
    #[error("attempt already executed: {0}")]
    AlreadyExecuted(String),
}

/// ★ P2 核心接口：定义一个 Attempt 的完整执行生命周期。
///
/// 一个 Attempt 严格对应一个 IronClaw Run。
/// 所有退出路径必须经过 await_exit()。
#[async_trait]
pub trait AttemptRunPort: Send + Sync {
    /// 启动一个 Attempt（创建 IronClaw Run）。
    async fn start_attempt(&self, request: AttemptRunRequest) -> Result<AttemptRunHandle, RunIngressError>;

    /// 等待 Agent Loop 退出并封存报告。
    async fn await_exit(&self, handle: AttemptRunHandle) -> Result<AttemptExitReport, RunIngressError>;

    /// 取消正在运行的 Attempt。
    async fn cancel(&self, handle: AttemptRunHandle) -> Result<(), RunIngressError>;
}

/// 启动 Attempt 的请求。
#[derive(Debug, Clone)]
pub struct AttemptRunRequest {
    pub attempt_id: AttemptId,
    pub loop_id: String,
    pub execution_generation: u64,
    pub objective: String,
    pub contract_ref: String,
    pub max_iterations: Option<u32>,
    pub budget_grant_ref: String,
}

impl AttemptRunRequest {
    /// 转换为 IronClaw StartRunRequest（通过 RunIngressPort）。
    pub fn to_start_run_request(&self) -> StartRunRequest {
        StartRunRequest {
            actor: RuntimeActorRef::service("ontoloop"),
            source: SessionSource::OntoLoop,
            input: InputEnvelope {
                objective: self.objective.clone(),
                requirements: vec![],
                context: Some(format!(
                    "attempt_id={} loop_id={} generation={}",
                    self.attempt_id, self.loop_id, self.execution_generation
                )),
                model: None,
                max_iterations: self.max_iterations,
            },
            project_id: None,
            parent_work_item: Some(self.loop_id.clone()),
        }
    }
}

// ── Stub 实现 (用于 CI) ──

use std::sync::Mutex;
use onto_assurance_types::ids::DecisionId;
use onto_assurance_runtime::ports::RunFinalizationOutcome;
use onto_assurance_types::enums::{BudgetOutcome, LifecycleState, TaskOutcome};

/// CI 测试用的 Stub。Production 替换为真实 IronClaw Adapter。
pub struct StubAttemptRunPort {
    outcomes: Mutex<Vec<RunFinalizationOutcome>>,
    started: Mutex<Vec<AttemptId>>,
}

impl StubAttemptRunPort {
    pub fn new() -> Self { Self { outcomes: Mutex::new(vec![]), started: Mutex::new(vec![]) } }

    /// 注入预定义结果（模拟 IronClaw 多次 Attempt）。
    pub fn push_outcome(&self, o: RunFinalizationOutcome) { self.outcomes.lock().unwrap().push(o); }

    pub fn start_count(&self) -> usize { self.started.lock().unwrap().len() }
}

#[async_trait]
impl AttemptRunPort for StubAttemptRunPort {
    async fn start_attempt(&self, request: AttemptRunRequest) -> Result<AttemptRunHandle, RunIngressError> {
        // P2 不变量：一个 Attempt 只能启动一次
        let mut started = self.started.lock().unwrap();
        if started.contains(&request.attempt_id) {
            return Err(RunIngressError::AlreadyExecuted(request.attempt_id.to_string()));
        }
        started.push(request.attempt_id);
        Ok(AttemptRunHandle {
            run_id: RunId::new(),
            attempt_id: request.attempt_id,
            loop_id: request.loop_id,
            execution_generation: request.execution_generation,
        })
    }

    async fn await_exit(&self, handle: AttemptRunHandle) -> Result<AttemptExitReport, RunIngressError> {
        let outcome = self.outcomes.lock().unwrap().pop()
            .unwrap_or(RunFinalizationOutcome {
                task_outcome: TaskOutcome::Success,
                budget_outcome: BudgetOutcome::WithinBudget,
                lifecycle_state: LifecycleState::Committed,
                session_decision_id: DecisionId::new(),
                attempt_decision_id: DecisionId::new(),
                reason_codes: vec![],
                settlement_decision: None,
                effect_class: None,
            });

        let ckpt = format!("ckpt-{}", handle.attempt_id);
        Ok(AttemptExitReport {
            handle,
            exit_reason: match outcome.task_outcome {
                TaskOutcome::Success => ExitReason::AgentFinished,
                TaskOutcome::Failed => ExitReason::ToolFailure("verifier failed".into()),
                _ => ExitReason::AgentFinished,
            },
            candidate_checkpoint_hash: Some(ckpt),
            observations: vec![],
            side_effect_manifest_hash: None,
        })
    }

    async fn cancel(&self, handle: AttemptRunHandle) -> Result<(), RunIngressError> {
        // Remove from started list
        self.started.lock().unwrap().retain(|id| *id != handle.attempt_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn one_attempt_one_run() {
        let port = StubAttemptRunPort::new();
        let req = AttemptRunRequest {
            attempt_id: AttemptId::new(), loop_id: "L1".into(),
            execution_generation: 1, objective: "write hello.py".into(),
            contract_ref: "c".into(), max_iterations: Some(10), budget_grant_ref: "g".into(),
        };
        let handle = port.start_attempt(req.clone()).await.unwrap();
        assert_eq!(handle.loop_id, "L1");

        // Same attempt_id → rejected
        let dup = port.start_attempt(req).await;
        assert!(dup.is_err());
    }

    #[tokio::test]
    async fn all_exit_paths_go_through_await_exit() {
        let port = StubAttemptRunPort::new();
        port.push_outcome(RunFinalizationOutcome {
            task_outcome: TaskOutcome::Failed, budget_outcome: BudgetOutcome::WithinBudget,
            lifecycle_state: LifecycleState::Continuing, session_decision_id: DecisionId::new(),
            attempt_decision_id: DecisionId::new(), reason_codes: vec![],
            settlement_decision: None, effect_class: None,
        });

        let req = AttemptRunRequest {
            attempt_id: AttemptId::new(), loop_id: "L2".into(),
            execution_generation: 1, objective: "fix bug".into(),
            contract_ref: "c".into(), max_iterations: None, budget_grant_ref: "g".into(),
        };
        let handle = port.start_attempt(req).await.unwrap();
        let report = port.await_exit(handle).await.unwrap();

        // All exit paths produce a report — no silent bypass
        assert!(report.candidate_checkpoint_hash.is_some());
    }

    #[tokio::test]
    async fn cancel_stops_run() {
        let port = StubAttemptRunPort::new();
        let req = AttemptRunRequest {
            attempt_id: AttemptId::new(), loop_id: "L3".into(),
            execution_generation: 1, objective: "task".into(),
            contract_ref: "c".into(), max_iterations: None, budget_grant_ref: "g".into(),
        };
        let handle = port.start_attempt(req).await.unwrap();
        assert_eq!(port.start_count(), 1);

        port.cancel(handle).await.unwrap();
        assert_eq!(port.start_count(), 0, "cancel removes from active set");
    }
}
