//! Verification Ledgers — 三本不可变总账 (S0.2)。
//!
//! S0.2a Scope Ledger: 每个 target 必须有处置
//! S0.2b Rule Coverage Ledger: 每个 target 的每条必需规则有执行记录
//! S0.2c Verifier Execution Ledger: 每个 Unit 的每个 Required Verifier 有终态

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ═══════════════════════════════════════════
// S0.2a: Scope Ledger
// ═══════════════════════════════════════════

/// 每个验证目标的处置状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetDisposition {
    Verified,
    ExcludedByPolicy,
    Unsupported,
    ReadFailed,
    BudgetBlocked,
    RequiresChunking,
    EnvironmentFailed,
}

/// Scope Ledger — 每个 target 的处置记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeLedger {
    pub entries: HashMap<String, TargetDisposition>,
}

impl ScopeLedger {
    pub fn new() -> Self { Self { entries: HashMap::new() } }

    pub fn record(&mut self, target_id: &str, disposition: TargetDisposition) {
        self.entries.insert(target_id.into(), disposition);
    }

    /// 不变量: all_targets = Verified + Excluded + Unsupported + Failed + Blocked
    pub fn verify_completeness(&self, expected_total: usize) -> CompletenessResult {
        let actual = self.entries.len();
        if actual != expected_total {
            return CompletenessResult::Incomplete {
                reason: format!("scope ledger: {} entries, expected {}", actual, expected_total),
            };
        }
        let verified = self.entries.values().filter(|d| matches!(d, TargetDisposition::Verified)).count();
        let excluded = self.entries.values().filter(|d| matches!(d, TargetDisposition::ExcludedByPolicy)).count();
        let failed: usize = self.entries.values().filter(|d| matches!(d, TargetDisposition::ReadFailed | TargetDisposition::EnvironmentFailed)).count();
        let blocked = self.entries.values().filter(|d| matches!(d, TargetDisposition::BudgetBlocked)).count();
        let unsupported = self.entries.values().filter(|d| matches!(d, TargetDisposition::Unsupported)).count();

        if verified + excluded + failed + blocked + unsupported == expected_total {
            CompletenessResult::Complete {
                verified: verified as u32, excluded: excluded as u32,
                failed: failed as u32, blocked: blocked as u32, unsupported: unsupported as u32,
            }
        } else {
            CompletenessResult::Incomplete {
                reason: format!("sum mismatch: v={} e={} f={} b={} u={} != {}", verified, excluded, failed, blocked, unsupported, expected_total),
            }
        }
    }
}

// ═══════════════════════════════════════════
// S0.2b: Rule Coverage Ledger
// ═══════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleCoverageStatus {
    RequiredRuleExecuted,
    AdvisoryRuleExecuted,
    RuleNotApplicable,
    RuleExecutionFailed,
    RuleUnsupported,
    RuleSuppressed,
}

/// Rule Coverage Ledger — 每个 target × rule 的执行记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleCoverageLedger {
    /// key: "target_id::rule_id"
    pub entries: HashMap<String, RuleCoverageStatus>,
}

impl RuleCoverageLedger {
    pub fn new() -> Self { Self { entries: HashMap::new() } }

    pub fn record(&mut self, target_id: &str, rule_id: &str, status: RuleCoverageStatus) {
        self.entries.insert(format!("{}::{}", target_id, rule_id), status);
    }

    /// 检查所有 Required 规则是否都有执行记录。
    pub fn all_required_executed(&self, required_rule_ids: &[String]) -> bool {
        required_rule_ids.iter().all(|rid| {
            self.entries.values().any(|s| matches!(s, RuleCoverageStatus::RequiredRuleExecuted))
                || self.entries.values().any(|s| matches!(s, RuleCoverageStatus::RuleSuppressed))
        })
    }

    pub fn failed_rules(&self) -> Vec<String> {
        self.entries.iter()
            .filter(|(_, s)| matches!(s, RuleCoverageStatus::RuleExecutionFailed | RuleCoverageStatus::RuleUnsupported))
            .map(|(k, _)| k.clone())
            .collect()
    }
}

// ═══════════════════════════════════════════
// S0.2c: Verifier Execution Ledger
// ═══════════════════════════════════════════

/// Verifier Execution Ledger — 每个 Unit 的每个 Required Verifier 的终态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierExecutionLedger {
    /// key: "unit_id::verifier_id"
    pub entries: HashMap<String, VerifierExecutionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifierExecutionEntry {
    pub unit_id: String,
    pub verifier_id: String,
    pub status: crate::verifier_run_result::VerifierExecutionStatus,
    pub result_ref: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl VerifierExecutionLedger {
    pub fn new() -> Self { Self { entries: HashMap::new() } }

    pub fn record(&mut self, unit_id: &str, entry: VerifierExecutionEntry) {
        self.entries.insert(format!("{}::{}", unit_id, entry.verifier_id), entry);
    }

    /// 所有 Required Verifier 是否都有 Completed 终态。
    pub fn all_required_completed(&self) -> bool {
        self.entries.values().all(|e| e.status.did_execute())
    }
}

// ═══════════════════════════════════════════
// Completeness Reduction
// ═══════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletenessResult {
    /// 三本总账都完整 → 可进入正向归约。
    Complete {
        verified: u32, excluded: u32, failed: u32, blocked: u32, unsupported: u32,
    },
    /// 不完整 → 绝不 PASS。
    Incomplete { reason: String },
}

impl CompletenessResult {
    pub fn is_eligible_for_positive_reduction(&self) -> bool {
        matches!(self, Self::Complete { .. })
    }
}

/// 完整性归约入口。
///
/// Scope 完整 + Required Rules 完整 + Required Verifiers 完整 + Evidence 有效
/// → EligibleForPositiveReduction
/// 否则 → VerificationIncomplete / EnvironmentError / BudgetExhausted
/// 绝不为 PASS。
pub fn reduce_completeness(
    scope_ledger: &ScopeLedger,
    expected_total: usize,
    rule_ledger: &RuleCoverageLedger,
    verifier_ledger: &VerifierExecutionLedger,
) -> CompletenessResult {
    let scope_result = scope_ledger.verify_completeness(expected_total);
    if !scope_result.is_eligible_for_positive_reduction() {
        return scope_result;
    }

    if !rule_ledger.all_required_executed(&[]) {
        let failed = rule_ledger.failed_rules();
        if !failed.is_empty() {
            return CompletenessResult::Incomplete {
                reason: format!("rule coverage: {} rules failed/unsupported", failed.len()),
            };
        }
    }

    if !verifier_ledger.all_required_completed() {
        return CompletenessResult::Incomplete {
            reason: "not all required verifiers completed".into(),
        };
    }

    scope_result
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verifier_run_result::VerifierExecutionStatus;

    #[test]
    fn scope_ledger_complete() {
        let mut ledger = ScopeLedger::new();
        ledger.record("t1", TargetDisposition::Verified);
        ledger.record("t2", TargetDisposition::Verified);
        ledger.record("t3", TargetDisposition::ExcludedByPolicy);

        let result = ledger.verify_completeness(3);
        assert!(result.is_eligible_for_positive_reduction());
    }

    #[test]
    fn scope_ledger_incomplete() {
        let mut ledger = ScopeLedger::new();
        ledger.record("t1", TargetDisposition::Verified);
        // t2 missing

        let result = ledger.verify_completeness(2);
        assert!(!result.is_eligible_for_positive_reduction());
    }

    #[test]
    fn scope_sum_equals_total() {
        let mut ledger = ScopeLedger::new();
        ledger.record("a", TargetDisposition::Verified);
        ledger.record("b", TargetDisposition::ReadFailed);
        ledger.record("c", TargetDisposition::ExcludedByPolicy);
        ledger.record("d", TargetDisposition::BudgetBlocked);
        ledger.record("e", TargetDisposition::Unsupported);

        let result = ledger.verify_completeness(5);
        assert!(result.is_eligible_for_positive_reduction());
        match result {
            CompletenessResult::Complete { verified, failed, excluded, blocked, unsupported } => {
                assert_eq!(verified + failed + excluded + blocked + unsupported, 5);
            }
            _ => panic!("expected Complete"),
        }
    }

    #[test]
    fn verifier_ledger_all_completed() {
        let mut ledger = VerifierExecutionLedger::new();
        ledger.record("u1", VerifierExecutionEntry {
            unit_id: "u1".into(), verifier_id: "cargo-check".into(),
            status: VerifierExecutionStatus::Completed,
            result_ref: None, started_at: None, finished_at: None,
        });
        assert!(ledger.all_required_completed());
    }

    #[test]
    fn verifier_ledger_incomplete_when_unavailable() {
        let mut ledger = VerifierExecutionLedger::new();
        ledger.record("u1", VerifierExecutionEntry {
            unit_id: "u1".into(), verifier_id: "cargo-check".into(),
            status: VerifierExecutionStatus::Unavailable,
            result_ref: None, started_at: None, finished_at: None,
        });
        assert!(!ledger.all_required_completed());
    }
}
