//! approval_adapter — bridges onto ApprovalPort to OntoRuntime approvals.
//!
//! Conditional compilation:
//!   - `#[cfg(feature = "ironclaw-integration")]`: real OntoRuntime adapter
//!   - Default: stub implementation for standalone testing

#[cfg(feature = "ironclaw-integration")]
pub mod ironclaw {
    // Will be implemented during M4 integration inside OntoRuntime workspace
}

#[cfg(not(feature = "ironclaw-integration"))]
pub mod stub {
    use onto_assurance_runtime::ports::{
        ApprovalDecision, ApprovalError, ApprovalPort, ApprovalStatus,
    };
    use onto_assurance_types::ids::RunId;

    pub struct StubApprovalPort {
        pub decision: ApprovalDecision,
    }

    impl Default for StubApprovalPort {
        fn default() -> Self { Self { decision: ApprovalDecision::Granted } }
    }

    #[async_trait::async_trait]
    impl ApprovalPort for StubApprovalPort {
        async fn request_approval(&self, _run_id: RunId, _reason: &str) -> Result<String, ApprovalError> {
            Ok("stub-approval-id".into())
        }
        async fn check_status(&self, _id: &str) -> Result<ApprovalStatus, ApprovalError> {
            Ok(match self.decision {
                ApprovalDecision::Granted => ApprovalStatus::Granted,
                ApprovalDecision::Rejected { .. } => ApprovalStatus::Rejected,
                ApprovalDecision::TimedOut => ApprovalStatus::Expired,
            })
        }
        async fn wait_for_decision(&self, _id: &str, _t: u64) -> Result<ApprovalDecision, ApprovalError> {
            Ok(self.decision.clone())
        }
    }
}
