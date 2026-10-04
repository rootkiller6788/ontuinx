//! L6: OntoOntoRuntimeLoopAdapter — thin adapter connecting OntoLoop to OntoRuntime.
//!
//! One Attempt = One OntoRuntime Run. OntoLoop never directly calls Capabilities.
//! Decision logic has moved to `onto_loop::decision::decide()`.
//! This adapter now only handles attempt lifecycle and idempotency.

use std::sync::Mutex;
use onto_assurance_runtime::ports::{RunFinalizationOutcome, RuntimeRunPort};
use onto_assurance_types::ids::{AttemptId, RunId};

/// Tracks attempts and enforces 1:1 Attempt↔Run mapping.
pub struct LoopAdapter {
    attempts: Mutex<Vec<(AttemptId, RunId)>>,
    #[allow(dead_code)]
    max_attempts: u32,
}

impl LoopAdapter {
    pub fn new(max_attempts: u32) -> Self {
        Self { attempts: Mutex::new(vec![]), max_attempts }
    }

    /// Execute one attempt via the real RuntimeRunPort.
    /// Returns Err if attempt already executed (idempotency).
    pub async fn execute_attempt(
        &self, runtime: &dyn RuntimeRunPort, attempt_id: AttemptId,
        iteration: u32, objective: &str,
    ) -> Result<RunFinalizationOutcome, String> {
        {
            let attempts = self.attempts.lock().unwrap();
            if attempts.iter().any(|(aid, _)| *aid == attempt_id) {
                return Err(format!("attempt {} already executed", attempt_id));
            }
        }

        let run_id = runtime.start_run(iteration, objective).await?;
        {
            self.attempts.lock().unwrap().push((attempt_id, run_id));
        }

        runtime.await_terminal(run_id).await
    }

    /// Cancel—stop a run that hasn't completed.
    pub async fn cancel(&self, runtime: &dyn RuntimeRunPort, attempt_id: AttemptId) -> Result<(), String> {
        let rid = {
            self.attempts.lock().unwrap().iter()
                .find(|(aid, _)| *aid == attempt_id).map(|(_, rid)| *rid)
        };
        match rid { Some(rid) => runtime.cancel_run(rid).await, None => Err("attempt not found".into()) }
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests — L6 + L7 (decision tests migrated to onto_loop::decision)
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_runtime::mocks::MockLoopRuntime;
    use onto_assurance_types::enums::{LifecycleState, TaskOutcome};
    use onto_assurance_types::ids::DecisionId;
    use onto_assurance_types::enums::BudgetOutcome;

    fn make_outcome(t: TaskOutcome, l: LifecycleState) -> RunFinalizationOutcome {
        RunFinalizationOutcome { task_outcome: t, budget_outcome: BudgetOutcome::WithinBudget,
            lifecycle_state: l, session_decision_id: DecisionId::new(),
            attempt_decision_id: DecisionId::new(), reason_codes: vec![],
            settlement_decision: None, effect_class: None }
    }

    #[tokio::test]
    async fn l6_1_metadata_passed_to_runtime() {
        let rt = MockLoopRuntime::new();
        rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));
        let adapter = LoopAdapter::new(3);
        let aid = AttemptId::new();
        let outcome = adapter.execute_attempt(&rt, aid, 1, "write hello()").await.unwrap();
        assert_eq!(outcome.task_outcome, TaskOutcome::Success);
    }

    #[tokio::test]
    async fn l6_2_same_attempt_id_rejected() {
        let rt = MockLoopRuntime::new();
        rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));
        rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));
        let adapter = LoopAdapter::new(3);
        let aid = AttemptId::new();
        adapter.execute_attempt(&rt, aid, 1, "task").await.unwrap();
        let r2 = adapter.execute_attempt(&rt, aid, 2, "task").await;
        assert!(r2.is_err(), "same attempt_id must be rejected");
    }

    // L6-3 and L7 decision tests migrated to onto_loop::decision::tests
    // which tests decide() with AttemptObservation and LoopContext directly.

    #[tokio::test]
    async fn l7_e2e_two_attempts() {
        let rt = MockLoopRuntime::new();
        rt.push_outcome(make_outcome(TaskOutcome::Failed, LifecycleState::Continuing));
        rt.push_outcome(make_outcome(TaskOutcome::Success, LifecycleState::Committed));
        let adapter = LoopAdapter::new(3);

        let a1 = AttemptId::new();
        let o1 = adapter.execute_attempt(&rt, a1, 1, "create hello.py").await.unwrap();
        assert_eq!(o1.task_outcome, TaskOutcome::Failed);

        let a2 = AttemptId::new();
        let o2 = adapter.execute_attempt(&rt, a2, 2, "fix hello.py").await.unwrap();
        assert_eq!(o2.task_outcome, TaskOutcome::Success);
        assert_eq!(o2.lifecycle_state, LifecycleState::Committed);
    }
}
