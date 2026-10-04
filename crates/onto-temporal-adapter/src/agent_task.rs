//! F9: Native AgentWorkItemTask types (v1.0 optional).
//!
//! In v0.1, OntoLoop reuses OntoFlow's standard ActivityTask protocol.
//! In v1.0, Agent tasks can become first-class server types with dedicated
//! history events, state machine transitions, and worker API.
//!
//! This module defines the Rust-side types that map to the proposed
//! OntoFlow Proto additions. These types are for reference and future
//! implementation — they are NOT wired into the v0.1 runtime.

use serde::{Deserialize, Serialize};

// ══════════════════════════════════════════════════════════════════
// Task Lifecycle Events (History)
// ══════════════════════════════════════════════════════════════════

/// A native AgentWorkItem task has been scheduled by the OntoFlow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkItemTaskScheduled {
    pub workflow_id: String,
    pub work_item_id: String,
    pub loop_id: String,
    pub task_queue: String,
    pub payload: String, // LoopInvocationRequest JSON
    pub scheduled_event_id: i64,
}

/// A Rust Worker has started executing this AgentWorkItem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkItemTaskStarted {
    pub work_item_id: String,
    pub loop_id: String,
    pub worker_id: String,
    pub started_at: String,
}

/// The Worker has reported an outcome (NOT yet accepted by Go).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkItemOutcomeReported {
    pub work_item_id: String,
    pub loop_id: String,
    pub reported_state: String,
    pub envelope_hash: String,
}

/// Go has verified the outcome via AuthorityProjectionPort and accepted it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkItemOutcomeAccepted {
    pub work_item_id: String,
    pub loop_id: String,
    pub accepted_state: String,
    pub decision_id: String,
    pub accepted_at: String,
}

// ══════════════════════════════════════════════════════════════════
// Worker API (replaces PollActivityTaskQueue / RespondActivityTaskCompleted)
// ══════════════════════════════════════════════════════════════════

/// Request to poll for available AgentWorkItem tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollAgentWorkItemTaskQueueRequest {
    pub task_queue: String,
    pub worker_id: String,
    pub worker_version: String,
    pub max_tasks: u32,
}

/// Response containing assigned AgentWorkItem tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollAgentWorkItemTaskQueueResponse {
    pub tasks: Vec<AgentWorkItemTask>,
}

/// A single AgentWorkItem task delivered to a Worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWorkItemTask {
    pub task_token: String,
    pub work_item_id: String,
    pub loop_id: String,
    pub input: String, // LoopInvocationRequest JSON
}

/// Worker reports an outcome for an AgentWorkItem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespondAgentWorkItemOutcomeRequest {
    pub task_token: String,
    pub work_item_id: String,
    pub loop_id: String,
    pub outcome: String, // LoopTerminalEnvelope JSON
}

/// Worker sends a heartbeat for an AgentWorkItem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordAgentWorkItemHeartbeatRequest {
    pub task_token: String,
    pub work_item_id: String,
    pub loop_id: String,
    pub details: String, // OntoLoopHeartbeat JSON
}

// ══════════════════════════════════════════════════════════════════
// Comparison: v0.1 vs v1.0
// ══════════════════════════════════════════════════════════════════

/// Maps v0.1 (standard ActivityTask) concepts to v1.0 (native AgentTask).
#[derive(Debug, Clone)]
pub struct AgentTaskMapping;

impl AgentTaskMapping {
    /// v0.1 → v1.0 mapping table.
    pub fn mapping() -> Vec<(&'static str, &'static str)> {
        vec![
            ("ActivityTaskScheduled",       "AgentWorkItemTaskScheduled"),
            ("ActivityTaskStarted",         "AgentWorkItemTaskStarted"),
            ("ActivityTaskCompleted",       "AgentWorkItemOutcomeReported"),
            ("(Go validates via Authority)", "AgentWorkItemOutcomeAccepted"),
            ("RecordActivityTaskHeartbeat", "RecordAgentWorkItemHeartbeat"),
            ("PollActivityTaskQueue",       "PollAgentWorkItemTaskQueue"),
            ("RespondActivityTaskCompleted","RespondAgentWorkItemOutcome"),
            ("RespondActivityTaskFailed",   "RespondAgentWorkItemOutcome (with failure)"),
        ]
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests — F9
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// F9.1: All lifecycle events serialize/deserialize correctly.
    #[test]
    fn f9_1_event_serialization() {
        let scheduled = AgentWorkItemTaskScheduled {
            workflow_id: "wf-1".into(),
            work_item_id: "wi-A".into(),
            loop_id: "loop-A".into(),
            task_queue: "onto-workers".into(),
            payload: "{}".into(),
            scheduled_event_id: 42,
        };
        let json = serde_json::to_string(&scheduled).unwrap();
        let back: AgentWorkItemTaskScheduled = serde_json::from_str(&json).unwrap();
        assert_eq!(back.work_item_id, "wi-A");
        assert_eq!(back.scheduled_event_id, 42);
    }

    /// F9.2: OutcomeReported → OutcomeAccepted lifecycle distinction.
    #[test]
    fn f9_2_reported_vs_accepted() {
        // Reported: Worker's claim
        let reported = AgentWorkItemOutcomeReported {
            work_item_id: "wi-X".into(),
            loop_id: "loop-X".into(),
            reported_state: "committed".into(),
            envelope_hash: "hash-xyz".into(),
        };

        // Accepted: Go verified via AuthorityProjectionPort
        let accepted = AgentWorkItemOutcomeAccepted {
            work_item_id: reported.work_item_id.clone(),
            loop_id: reported.loop_id.clone(),
            accepted_state: "committed".into(),
            decision_id: "dec-123".into(),
            accepted_at: "2026-07-26T12:00:00Z".into(),
        };

        assert_eq!(reported.work_item_id, accepted.work_item_id);
        assert_eq!(reported.loop_id, accepted.loop_id);
        // Reported != Accepted — Go verification step exists between them
        assert!(accepted.decision_id.len() > 0, "accepted must have decision_id");
    }

    /// F9.3: AgentTaskMapping covers all v0.1 concepts.
    #[test]
    fn f9_3_mapping_completeness() {
        let mapping = AgentTaskMapping::mapping();
        assert_eq!(mapping.len(), 8, "8 mappings for complete v0.1→v1.0 coverage");

        // Verify key mappings exist
        let has_schedule = mapping.iter().any(|(v01, _)| *v01 == "ActivityTaskScheduled");
        let has_complete = mapping.iter().any(|(v01, _)| *v01 == "ActivityTaskCompleted");
        let has_heartbeat = mapping.iter().any(|(v01, _)| *v01 == "RecordActivityTaskHeartbeat");
        assert!(has_schedule && has_complete && has_heartbeat);
    }

    /// F9.4: Poll request/response round-trip.
    #[test]
    fn f9_4_poll_roundtrip() {
        let req = PollAgentWorkItemTaskQueueRequest {
            task_queue: "onto-workers".into(),
            worker_id: "w-1".into(),
            worker_version: "0.1.0".into(),
            max_tasks: 5,
        };
        let json = serde_json::to_string(&req).unwrap();

        let task = AgentWorkItemTask {
            task_token: "tok-1".into(),
            work_item_id: "wi-1".into(),
            loop_id: "loop-1".into(),
            input: json.clone(),
        };
        let resp = PollAgentWorkItemTaskQueueResponse {
            tasks: vec![task],
        };

        let resp_json = serde_json::to_string(&resp).unwrap();
        let back: PollAgentWorkItemTaskQueueResponse = serde_json::from_str(&resp_json).unwrap();
        assert_eq!(back.tasks.len(), 1);
        assert_eq!(back.tasks[0].work_item_id, "wi-1");
    }
}
