//! L5: Loop budget with no-progress detection and orthogonal state.

use onto_assurance_types::enums::{TaskOutcome, BudgetOutcome};
use onto_assurance_types::ontoloop::LoopStatus;

/// Loop budget — limits on attempts, no-progress, regressions, env errors.
#[derive(Debug, Clone)]
pub struct LoopBudget {
    pub max_attempts: u32,
    pub attempts_used: u32,
    pub max_consecutive_no_progress: u32,
    pub consecutive_no_progress: u32,
    pub max_regressions: u32,
    pub regressions: u32,
    pub max_environment_errors: u32,
    pub environment_errors: u32,
}

impl LoopBudget {
    pub fn new(max_attempts: u32) -> Self {
        Self { max_attempts, attempts_used: 0, max_consecutive_no_progress: 2,
               consecutive_no_progress: 0, max_regressions: 2, regressions: 0,
               max_environment_errors: 3, environment_errors: 0 }
    }

    pub fn attempt_exhausted(&self) -> bool { self.attempts_used >= self.max_attempts }
    pub fn no_progress_exhausted(&self) -> bool { self.consecutive_no_progress >= self.max_consecutive_no_progress }
    pub fn no_progress_count(&self) -> u32 { self.consecutive_no_progress }
    pub fn regressions_exhausted(&self) -> bool { self.regressions >= self.max_regressions }
    pub fn env_errors_exhausted(&self) -> bool { self.environment_errors >= self.max_environment_errors }

    pub fn any_exhausted(&self) -> bool {
        self.attempt_exhausted() || self.no_progress_exhausted()
            || self.regressions_exhausted() || self.env_errors_exhausted()
    }

    pub fn record_attempt(&mut self) { self.attempts_used += 1; }
    pub fn record_no_progress(&mut self) { self.consecutive_no_progress += 1; }
    pub fn reset_no_progress(&mut self) { self.consecutive_no_progress = 0; }
    pub fn record_regression(&mut self) { self.regressions += 1; }
    pub fn record_env_error(&mut self) { self.environment_errors += 1; }
}

/// L5: Orthogonal outcome — budget and task are independent dimensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopOutcome {
    pub task_outcome: TaskOutcome,
    pub budget_outcome: BudgetOutcome,
    pub loop_status: LoopStatus,
}

impl LoopOutcome {
    /// Budget exhausted + task succeeded = still Completed.
    pub fn finalize(task: TaskOutcome, budget: &LoopBudget) -> Self {
        let budget_outcome = if budget.attempt_exhausted() { BudgetOutcome::Depleted } else { BudgetOutcome::WithinBudget };
        let loop_status = match task {
            TaskOutcome::Success => LoopStatus::Completed,
            TaskOutcome::EnvironmentError => LoopStatus::Escalated,
            _ if budget.any_exhausted() => LoopStatus::BudgetExhausted,
            _ => LoopStatus::Running,
        };
        LoopOutcome { task_outcome: task, budget_outcome, loop_status }
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests — L5.1 to L5.3
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l5_1_success_with_depleted_budget_orthogonal() {
        let mut budget = LoopBudget::new(1);
        budget.record_attempt();
        assert!(budget.attempt_exhausted());

        let outcome = LoopOutcome::finalize(TaskOutcome::Success, &budget);
        assert_eq!(outcome.task_outcome, TaskOutcome::Success);
        assert_eq!(outcome.budget_outcome, BudgetOutcome::Depleted);
        assert_eq!(outcome.loop_status, LoopStatus::Completed);
        // Budget depleted does NOT override Success
    }

    #[test]
    fn l5_2_consecutive_no_progress_stops() {
        let mut budget = LoopBudget::new(10);
        budget.record_no_progress();
        budget.record_no_progress();
        assert!(budget.no_progress_exhausted());
    }

    #[test]
    fn l5_3_regression_count_exhausted() {
        let mut budget = LoopBudget::new(10);
        budget.record_regression();
        budget.record_regression();
        assert!(budget.regressions_exhausted());
    }

    #[test]
    fn l5_4_reset_no_progress_on_improvement() {
        let mut budget = LoopBudget::new(10);
        budget.record_no_progress();
        budget.reset_no_progress();
        assert_eq!(budget.consecutive_no_progress, 0);
    }
}
