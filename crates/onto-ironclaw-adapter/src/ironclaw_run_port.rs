//! IronClaw Internal Rust Port — 生产级 AttemptRunPort (S1.2)。
//!
//! 绑定: loop_id + attempt_id + ironclaw_run_id + execution_generation
//!       + contract_hash + candidate_checkpoint_hash
//!
//! 不变量: 1 Attempt = 1 IronClaw Run, 不允许跨 Attempt 复用 Run。
//! 所有退出路径必须经过 AfterLoopExit。

use async_trait::async_trait;
use onto_assurance_types::ids::{AttemptId, RunId};

/// 完整绑定 — 生产级 Attempt ↔ IronClaw Run 映射。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptRunBinding {
    pub loop_id: String,
    pub attempt_id: AttemptId,
    pub ironclaw_run_id: RunId,
    pub execution_generation: u64,
    pub contract_hash: String,
    pub candidate_checkpoint_hash: String,
    pub started_at: String,
}

impl AttemptRunBinding {
    pub fn binding_key(&self) -> String {
        format!("{}:{}:{}:{}",
            self.loop_id, self.attempt_id, self.ironclaw_run_id, self.execution_generation)
    }

    /// 验证绑定完整性。
    pub fn validate(&self) -> Result<(), String> {
        if self.loop_id.is_empty() { return Err("loop_id empty".into()); }
        if self.contract_hash.is_empty() { return Err("contract_hash empty".into()); }
        if self.candidate_checkpoint_hash.is_empty() { return Err("checkpoint_hash empty".into()); }
        Ok(())
    }
}

/// AfterLoopExit 报告 — 所有退出路径的统一结构。
#[derive(Debug, Clone)]
pub struct AfterLoopExitReport {
    pub binding: AttemptRunBinding,
    pub exit_path: ExitPath,
    pub candidate_checkpoint_hash: Option<String>,
    pub side_effect_manifest_hash: Option<String>,
    pub observations: Vec<String>,
    pub error: Option<String>,
}

/// 所有可能的退出路径 — 不能有"异常退出直接写 Completed"的旁路。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitPath {
    /// Agent 正常结束（模型返回 finish reason）。
    AgentFinished,
    /// 模型声明任务完成。
    AgentDeclaredComplete,
    /// 达到 Turn 上限。
    MaxTurnsReached,
    /// 用户取消。
    UserCancelled,
    /// 工具调用失败。
    ToolFailure,
    /// LLM Provider 错误。
    ProviderError,
    /// 进程崩溃（OOM/SIGKILL 等）。
    ProcessCrashed,
    /// 预算耗尽。
    BudgetExhausted,
    /// 执行超时。
    Timeout,
}

impl ExitPath {
    /// 是否应该触发重试。
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::ProviderError | Self::Timeout)
    }
    /// 是否表示 Agent 执行了（不论结果）。
    pub fn agent_did_execute(&self) -> bool {
        !matches!(self, Self::ProcessCrashed | Self::UserCancelled)
    }
    /// 是否应该进入 Verification。
    pub fn should_verify(&self) -> bool {
        matches!(self, Self::AgentFinished | Self::AgentDeclaredComplete | Self::MaxTurnsReached | Self::ToolFailure)
    }
}

/// ★ 生产级 AttemptRunPort — 1 Attempt = 1 IronClaw Run。
#[async_trait]
pub trait ProductionAttemptRunPort: Send + Sync {
    /// 启动 Attempt，创建 IronClaw Run。返回完整绑定。
    async fn start_attempt(&self, request: AttemptRunRequest) -> Result<AttemptRunBinding, AttemptRunError>;

    /// 等待 Agent Loop 退出，接收 AfterLoopExit 报告。
    /// ★ 所有退出路径必须经过此方法 — 不能有旁路。
    async fn await_after_loop_exit(&self, binding: &AttemptRunBinding) -> Result<AfterLoopExitReport, AttemptRunError>;

    /// 取消正在运行的 Attempt。
    async fn cancel(&self, binding: &AttemptRunBinding) -> Result<(), AttemptRunError>;
}

#[derive(Debug, Clone)]
pub struct AttemptRunRequest {
    pub attempt_id: AttemptId,
    pub loop_id: String,
    pub execution_generation: u64,
    pub objective: String,
    pub contract_ref: String,
    pub contract_hash: String,
    pub max_iterations: Option<u32>,
    pub budget_grant_ref: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AttemptRunError {
    #[error("already executed: {0}")]
    AlreadyExecuted(String),
    #[error("binding invalid: {0}")]
    InvalidBinding(String),
    #[error("run not found: {0}")]
    NotFound(String),
    #[error("internal: {0}")]
    Internal(String),
}

// ── Stub for CI ──

use std::sync::Mutex;
use std::collections::HashMap;

pub struct StubProductionRunPort {
    bindings: Mutex<HashMap<AttemptId, AttemptRunBinding>>,
    outcomes: Mutex<Vec<(AttemptRunBinding, ExitPath)>>,
}

impl StubProductionRunPort {
    pub fn new() -> Self { Self { bindings: Mutex::new(HashMap::new()), outcomes: Mutex::new(vec![]) } }
    pub fn push_outcome(&self, binding: AttemptRunBinding, exit: ExitPath) {
        self.outcomes.lock().unwrap().push((binding, exit));
    }
}

#[async_trait]
impl ProductionAttemptRunPort for StubProductionRunPort {
    async fn start_attempt(&self, req: AttemptRunRequest) -> Result<AttemptRunBinding, AttemptRunError> {
        let mut bindings = self.bindings.lock().unwrap();
        if bindings.contains_key(&req.attempt_id) {
            return Err(AttemptRunError::AlreadyExecuted(req.attempt_id.to_string()));
        }
        let binding = AttemptRunBinding {
            loop_id: req.loop_id.clone(), attempt_id: req.attempt_id,
            ironclaw_run_id: RunId::new(), execution_generation: req.execution_generation,
            contract_hash: req.contract_hash.clone(),
            candidate_checkpoint_hash: format!("ckpt-{}", req.attempt_id),
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        binding.validate().map_err(|e| AttemptRunError::InvalidBinding(e))?;
        bindings.insert(req.attempt_id, binding.clone());
        Ok(binding)
    }

    async fn await_after_loop_exit(&self, binding: &AttemptRunBinding) -> Result<AfterLoopExitReport, AttemptRunError> {
        let outcome = self.outcomes.lock().unwrap().pop()
            .unwrap_or_else(|| (binding.clone(), ExitPath::AgentFinished));
        Ok(AfterLoopExitReport {
            binding: outcome.0,
            exit_path: outcome.1,
            candidate_checkpoint_hash: Some(binding.candidate_checkpoint_hash.clone()),
            side_effect_manifest_hash: None,
            observations: vec![],
            error: None,
        })
    }

    async fn cancel(&self, binding: &AttemptRunBinding) -> Result<(), AttemptRunError> {
        self.bindings.lock().unwrap().remove(&binding.attempt_id);
        Ok(())
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    fn req(id: &str) -> AttemptRunRequest {
        AttemptRunRequest { attempt_id: AttemptId::new(), loop_id: format!("L-{}", id), execution_generation: 1, objective: "task".into(), contract_ref: "c".into(), contract_hash: "ch".into(), max_iterations: Some(10), budget_grant_ref: "g".into() }
    }

    #[tokio::test]
    async fn one_attempt_one_run() {
        let port = StubProductionRunPort::new();
        let r = req("a1");
        let aid = r.attempt_id;
        let binding = port.start_attempt(r).await.unwrap();
        assert_eq!(binding.attempt_id, aid);
        assert!(!binding.ironclaw_run_id.to_string().is_empty());
        // Same attempt_id → rejected
        let r2 = AttemptRunRequest { attempt_id: aid, ..req("a1") };
        assert!(port.start_attempt(r2).await.is_err());
    }

    #[tokio::test]
    async fn binding_validation() {
        let bad = AttemptRunBinding {
            loop_id: "".into(), attempt_id: AttemptId::new(), ironclaw_run_id: RunId::new(),
            execution_generation: 1, contract_hash: "ch".into(),
            candidate_checkpoint_hash: "ckpt".into(), started_at: "t".into(),
        };
        assert!(bad.validate().is_err());
    }

    #[tokio::test]
    async fn all_exit_paths_covered() {
        let paths = [
            ExitPath::AgentFinished, ExitPath::AgentDeclaredComplete,
            ExitPath::MaxTurnsReached, ExitPath::UserCancelled,
            ExitPath::ToolFailure, ExitPath::ProviderError,
            ExitPath::ProcessCrashed, ExitPath::BudgetExhausted, ExitPath::Timeout,
        ];
        assert_eq!(paths.len(), 9, "9 exit paths — no silent bypass");

        let should_verify: Vec<_> = paths.iter().filter(|p| p.should_verify()).collect();
        assert_eq!(should_verify.len(), 4, "4 paths go to verification");

        let retryable: Vec<_> = paths.iter().filter(|p| p.is_retryable()).collect();
        assert_eq!(retryable.len(), 2, "2 paths trigger retry");
    }

    #[tokio::test]
    async fn after_loop_exit_report() {
        let port = StubProductionRunPort::new();
        let binding = port.start_attempt(req("a2")).await.unwrap();
        let report = port.await_after_loop_exit(&binding).await.unwrap();
        assert_eq!(report.exit_path, ExitPath::AgentFinished);
        assert!(report.candidate_checkpoint_hash.is_some());
    }

    #[tokio::test]
    async fn cancel_removes_binding() {
        let port = StubProductionRunPort::new();
        let binding = port.start_attempt(req("a3")).await.unwrap();
        port.cancel(&binding).await.unwrap();
        // After cancel, re-start same attempt_id is allowed (binding removed)
        let r = AttemptRunRequest { attempt_id: binding.attempt_id, ..req("a3") };
        assert!(port.start_attempt(r).await.is_ok());
    }
}
