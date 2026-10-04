//! VerifierRunResult — 统一 Verifier 执行结果 (S0.1)。
//!
//! 根不变量: execution_status = Completed ≠ verifier_verdict = Passed。
//! 当前 ScheduleResult = Result<Vec<FindingCandidate>> 无法区分"无问题"和"未运行"。

use serde::{Deserialize, Serialize};
use crate::finding::FindingCandidate;

/// Verifier 单次执行的完整结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierRunResult {
    pub run_id: String,
    pub unit_id: String,
    pub verifier_id: String,
    pub verifier_version_hash: String,

    /// 执行状态 — 工具是否真的跑了
    pub execution_status: VerifierExecutionStatus,
    /// Verifier 自身的判定（如有）
    pub verifier_verdict: Option<VerifierVerdict>,

    /// 发现的问题
    pub findings: Vec<FindingCandidate>,
    /// 正向观察（如"所有测试通过"）
    pub positive_observations: Vec<PositiveObservation>,

    pub checkpoint_hash: String,
    pub unit_fingerprint: String,
    pub configuration_hash: String,
    pub environment_hash: String,

    pub stdout_ref: Option<String>,
    pub stderr_ref: Option<String>,
    pub exit_code: Option<i32>,

    pub started_at: String,
    pub finished_at: String,
    pub resource_usage: VerifierResourceUsage,
}

impl VerifierRunResult {
    /// 工具真正完成执行（不论结果）。
    pub fn did_execute(&self) -> bool {
        matches!(self.execution_status, VerifierExecutionStatus::Completed)
    }

    /// 工具完成执行 AND 自身判定通过。
    pub fn passed(&self) -> bool {
        self.did_execute() && self.verifier_verdict == Some(VerifierVerdict::Passed)
    }

    /// 有 findings 且工具完成执行。
    pub fn has_findings(&self) -> bool {
        self.did_execute() && !self.findings.is_empty()
    }

    /// 工具不可用 — 不能降级为"通过"。
    pub fn is_unavailable(&self) -> bool {
        matches!(self.execution_status, VerifierExecutionStatus::Unavailable)
    }

    /// 工具执行失败 — 不能降级为"无问题"。
    pub fn is_failed(&self) -> bool {
        matches!(self.execution_status,
            VerifierExecutionStatus::ExecutionFailed |
            VerifierExecutionStatus::TimedOut |
            VerifierExecutionStatus::InvalidOutput
        )
    }
}

/// Verifier 执行状态 — 必须区分"跑了"和"没跑"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierExecutionStatus {
    /// 正常完成。
    Completed,
    /// 工具未安装或不可用。
    Unavailable,
    /// 执行超时。
    TimedOut,
    /// 运行但非零退出码。
    ExecutionFailed,
    /// 输出无法解析。
    InvalidOutput,
    /// 被取消。
    Cancelled,
    /// 预算不足未启动。
    BudgetBlocked,
    /// 语言/目标类型不支持。
    Unsupported,
}

impl VerifierExecutionStatus {
    /// 是否代表工具真正执行了（不论结果）。
    pub fn did_execute(&self) -> bool { matches!(self, Self::Completed) }
    /// 是否应该触发重试。
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::TimedOut | Self::Unavailable)
    }
    /// 是否应该停止流水线。
    pub fn is_blocking_failure(&self) -> bool {
        matches!(self, Self::ExecutionFailed | Self::InvalidOutput | Self::BudgetBlocked)
    }
}

/// Verifier 自身的判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierVerdict {
    Passed,
    Failed,
    WarningOnly,
    NeedsReview,
}

/// 正向观察 — Verifier 确认"好的状态"。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositiveObservation {
    pub kind: String,
    pub description: String,
    pub artifact_ref: Option<String>,
}

/// Verifier 资源消耗。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct VerifierResourceUsage {
    pub cpu_millis: u64,
    pub memory_kb: u64,
    pub tokens_used: u64,
    pub duration_millis: u64,
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    fn completed_passed() -> VerifierRunResult {
        VerifierRunResult {
            run_id: "r1".into(), unit_id: "u1".into(), verifier_id: "v1".into(),
            verifier_version_hash: "h1".into(),
            execution_status: VerifierExecutionStatus::Completed,
            verifier_verdict: Some(VerifierVerdict::Passed),
            findings: vec![], positive_observations: vec![],
            checkpoint_hash: "c".into(), unit_fingerprint: "f".into(),
            configuration_hash: "cfg".into(), environment_hash: "env".into(),
            stdout_ref: None, stderr_ref: None, exit_code: Some(0),
            started_at: "t1".into(), finished_at: "t2".into(),
            resource_usage: VerifierResourceUsage::default(),
        }
    }

    #[test]
    fn completed_neq_passed() {
        // Completed ≠ Passed — 根不变量
        let r = VerifierRunResult {
            execution_status: VerifierExecutionStatus::Completed,
            verifier_verdict: None,
            ..completed_passed()
        };
        assert!(r.did_execute());
        assert!(!r.passed()); // Completed but no verdict → not passed
    }

    #[test]
    fn unavailable_must_not_pass() {
        let r = VerifierRunResult {
            execution_status: VerifierExecutionStatus::Unavailable,
            ..completed_passed()
        };
        assert!(!r.did_execute());
        assert!(!r.passed());
        assert!(r.is_unavailable());
    }

    #[test]
    fn execution_failed_is_not_no_issues() {
        let r = VerifierRunResult {
            execution_status: VerifierExecutionStatus::ExecutionFailed,
            findings: vec![], // no findings but tool failed
            ..completed_passed()
        };
        assert!(!r.did_execute());
        assert!(r.is_failed());
        // Empty findings + ExecutionFailed ≠ "no issues"
    }

    #[test]
    fn retryable_statuses() {
        assert!(VerifierExecutionStatus::TimedOut.is_retryable());
        assert!(VerifierExecutionStatus::Unavailable.is_retryable());
        assert!(!VerifierExecutionStatus::ExecutionFailed.is_retryable());
    }

    #[test]
    fn blocking_failures() {
        assert!(VerifierExecutionStatus::ExecutionFailed.is_blocking_failure());
        assert!(VerifierExecutionStatus::InvalidOutput.is_blocking_failure());
        assert!(!VerifierExecutionStatus::TimedOut.is_blocking_failure());
    }
}
