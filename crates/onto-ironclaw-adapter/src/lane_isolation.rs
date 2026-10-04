//! Lane Isolation — Agent Execution Lane A vs Semantic Verification Lane B (P2)。
//!
//! 同一个 IronClaw 代码库，两个调用模式，权限完全不同：
//!
//! Lane A (Agent Execution):
//!   ✅ 可写 staging → 生成 Candidate
//!   ❌ 不能给自己的修改生成强 Evidence
//!
//! Lane B (Semantic Verification):
//!   ✅ 只读 Candidate Snapshot
//!   ❌ 无 Publish 权限
//!   ❌ 无 Decision 权限
//!   ❌ 只返回 FindingCandidate[]

/// 执行 Lane 标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionLane {
    /// Lane A: Agent Execution — 产生候选产物。
    AgentExecution,
    /// Lane B: Semantic Verification — 被 OntoAssure 调用，只做分析。
    SemanticVerification,
}

/// Lane B 权限守卫。
///
/// 在 Semantic Verification 模式下，任何写操作、Publish 尝试、
/// Decision 声明都必须被阻止。
pub struct LaneGuard {
    pub lane: ExecutionLane,
}

impl LaneGuard {
    pub fn new(lane: ExecutionLane) -> Self { Self { lane } }

    /// Lane B 能否写入 staging？
    pub fn can_write(&self) -> bool { matches!(self.lane, ExecutionLane::AgentExecution) }

    /// Lane B 能否 Publish？
    pub fn can_publish(&self) -> bool { false } // P2: 只有 Settlement Authority 能 Publish

    /// Lane B 能否声明 Decision？
    pub fn can_decide(&self) -> bool { false } // 只有 OntoAssure 能 Decision

    /// Lane B 能否返回 FindingCandidate？
    pub fn can_report_findings(&self) -> bool { true } // Lane B 的唯一输出

    /// 检查操作是否在当前 Lane 允许。
    pub fn guard_write(&self, operation: &str) -> Result<(), LaneViolation> {
        if !self.can_write() {
            return Err(LaneViolation {
                lane: self.lane,
                operation: operation.into(),
                reason: "Lane B (Semantic Verification) cannot write to staging".into(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Lane violation: {lane:?} cannot perform '{operation}': {reason}")]
pub struct LaneViolation {
    pub lane: ExecutionLane,
    pub operation: String,
    pub reason: String,
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_a_can_write() {
        let guard = LaneGuard::new(ExecutionLane::AgentExecution);
        assert!(guard.can_write());
        assert!(guard.guard_write("file_write").is_ok());
    }

    #[test]
    fn lane_b_cannot_write() {
        let guard = LaneGuard::new(ExecutionLane::SemanticVerification);
        assert!(!guard.can_write());
        assert!(guard.guard_write("file_write").is_err());
    }

    #[test]
    fn lane_b_cannot_publish() {
        let guard = LaneGuard::new(ExecutionLane::SemanticVerification);
        assert!(!guard.can_publish());
    }

    #[test]
    fn lane_b_cannot_decide() {
        let guard = LaneGuard::new(ExecutionLane::SemanticVerification);
        assert!(!guard.can_decide());
    }

    #[test]
    fn lane_b_can_report_findings() {
        let guard = LaneGuard::new(ExecutionLane::SemanticVerification);
        assert!(guard.can_report_findings());
    }

    #[test]
    fn violation_message_clear() {
        let guard = LaneGuard::new(ExecutionLane::SemanticVerification);
        let err = guard.guard_write("publish").unwrap_err();
        assert!(err.to_string().contains("Lane B"));
        assert!(err.to_string().contains("publish"));
    }
}
