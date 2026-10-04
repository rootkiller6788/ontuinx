//! Staged Rollout — Shadow → Gated → Enforced (P2)。

use serde::{Deserialize, Serialize};

/// P2 三阶段上线模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutStage {
    /// 真实 IronClaw 执行 + Mock 比对，真实结果不控制 Commit。
    Shadow,
    /// 真实 IronClaw 执行 + 真实 Verification Fabric 裁决，Commit 需人工批准。
    Gated,
    /// 真实 IronClaw 执行 + OntoAssure Decision 唯一控制 M6，取消人工常态审批。
    Enforced,
}

impl RolloutStage {
    pub fn allows_auto_commit(&self) -> bool { matches!(self, Self::Enforced) }
    pub fn requires_human_approval(&self) -> bool { matches!(self, Self::Gated) }
    pub fn is_shadow(&self) -> bool { matches!(self, Self::Shadow) }
}

/// 当前部署的 rollout 配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolloutConfig {
    pub stage: RolloutStage,
    /// Shadow 模式下保留多少百分比的执行用于 Mock 比对。
    pub shadow_compare_percent: u8, // 0-100
    /// Gated 模式下最大自动审批的风险等级。
    pub max_auto_risk_level: String,
}

impl Default for RolloutConfig {
    fn default() -> Self {
        Self { stage: RolloutStage::Shadow, shadow_compare_percent: 100, max_auto_risk_level: "low".into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shadow_no_auto_commit() { assert!(!RolloutStage::Shadow.allows_auto_commit()); }
    #[test] fn gated_requires_approval() { assert!(RolloutStage::Gated.requires_human_approval()); }
    #[test] fn enforced_allows_auto() { assert!(RolloutStage::Enforced.allows_auto_commit()); }
    #[test] fn default_is_shadow() { assert!(RolloutConfig::default().stage.is_shadow()); }
}
