//! IrreversibleDispatchPort adapter — M6-D mock email service.
//!
//! Simulates at-most-once dispatch with configurable response loss.

use std::collections::HashMap;
use std::sync::Mutex;

use onto_assurance_runtime::ports::{IrreversibleDispatchPort, IrreversibleEffectError};
use onto_assurance_types::ids::TransactionId;
use onto_assurance_types::transaction::{
    ContentHash, DispatchIntent, DispatchStatus, ExactInvocationLease,
    ExternalDispatchReceipt, PreExecutionDecision,
};

pub struct MockEmailService {
    sent: Mutex<HashMap<String, MockEmail>>,
    /// When true, next dispatch succeeds but response is lost.
    pub lose_next_response: Mutex<bool>,
}

struct MockEmail {
    #[allow(dead_code)]
    to: String,
    #[allow(dead_code)]
    subject: String,
    dispatched: bool,
}

impl MockEmailService {
    pub fn new() -> Self {
        Self { sent: Mutex::new(HashMap::new()), lose_next_response: Mutex::new(false) }
    }

    pub fn sent_count(&self) -> usize { self.sent.lock().unwrap().len() }

    /// Create a valid pre-execution decision with a fresh lease.
    pub fn authorize(
        &self,
        capability: &str,
        actor: &str,
    ) -> (PreExecutionDecision, ExactInvocationLease) {
        let lease = ExactInvocationLease {
            lease_id: format!("lease-{}", chrono::Utc::now().timestamp_millis()),
            capability: capability.into(), actor: actor.into(),
            run_id: onto_assurance_types::ids::RunId::new(),
            attempt_id: onto_assurance_types::ids::AttemptId::new(),
            params_hash: ContentHash::new("params-hash"),
            max_uses: 1, used_count: 0,
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
            decision_id: onto_assurance_types::ids::DecisionId::new(),
        };
        let pre = PreExecutionDecision {
            decision_id: lease.decision_id,
            authorized: true,
            required_approvals: vec!["admin".into()],
            granted_approvals: vec!["admin".into()],
            lease: Some(lease.clone()),
        };
        (pre, lease)
    }
}

#[async_trait::async_trait]
impl IrreversibleDispatchPort for MockEmailService {
    async fn record_intent(
        &self,
        _intent: DispatchIntent,
    ) -> Result<(), IrreversibleEffectError> {
        Ok(())
    }

    async fn dispatch(
        &self,
        intent: &DispatchIntent,
        _lease: &ExactInvocationLease,
        params: &serde_json::Value,
    ) -> Result<ExternalDispatchReceipt, IrreversibleEffectError> {
        let mut lose = self.lose_next_response.lock().unwrap();
        if *lose {
            *lose = false;
            let dispatch_id = format!("email-{}", chrono::Utc::now().timestamp_millis());
            // Email WAS sent, but response is lost
            self.sent.lock().unwrap().insert(dispatch_id.clone(), MockEmail {
                to: params["to"].as_str().unwrap_or("?").into(),
                subject: params.get("subject").and_then(|s| s.as_str()).unwrap_or("").into(),
                dispatched: true,
            });
            return Ok(ExternalDispatchReceipt {
                transaction_id: intent.transaction_id,
                dispatch_id,
                external_system: "email".into(),
                status: DispatchStatus::UnknownExternalOutcome,
                responded_at: chrono::Utc::now(),
            });
        }

        let dispatch_id = format!("email-{}", chrono::Utc::now().timestamp_millis());
        self.sent.lock().unwrap().insert(dispatch_id.clone(), MockEmail {
            to: params["to"].as_str().unwrap_or("?").into(),
            subject: params.get("subject").and_then(|s| s.as_str()).unwrap_or("").into(),
            dispatched: true,
        });

        Ok(ExternalDispatchReceipt {
            transaction_id: intent.transaction_id,
            dispatch_id,
            external_system: "email".into(),
            status: DispatchStatus::Confirmed,
            responded_at: chrono::Utc::now(),
        })
    }

    async fn query_external(
        &self,
        dispatch_id: &str,
        _external_system: &str,
    ) -> Result<ExternalDispatchReceipt, IrreversibleEffectError> {
        self.sent.lock().unwrap().get(dispatch_id)
            .map(|email| ExternalDispatchReceipt {
                transaction_id: TransactionId::new(),
                dispatch_id: dispatch_id.into(),
                external_system: "email".into(),
                status: if email.dispatched { DispatchStatus::Confirmed } else { DispatchStatus::Rejected },
                responded_at: chrono::Utc::now(),
            })
            .ok_or_else(|| IrreversibleEffectError::UnknownOutcome(dispatch_id.into()))
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_runtime::irreversible_dispatcher::AtMostOnceDispatcher;
    use std::sync::Arc;

    #[tokio::test]
    async fn authorized_dispatch_succeeds() {
        let svc = Arc::new(MockEmailService::new());
        let (pre, mut lease) = svc.authorize("send_email", "agent-1");
        let dispatcher = AtMostOnceDispatcher::new(svc.clone());

        dispatcher.authorize(&pre, &lease).unwrap();
        let receipt = dispatcher.safe_dispatch(
            TransactionId::new(), "send_email",
            ContentHash::new("h"), &mut lease,
            &serde_json::json!({"to": "a@b.com", "subject": "hi"}),
        ).await.unwrap();

        assert_eq!(receipt.status, DispatchStatus::Confirmed);
        assert_eq!(svc.sent_count(), 1);
    }

    #[tokio::test]
    async fn unauthorized_dispatch_rejected() {
        let svc = Arc::new(MockEmailService::new());
        let (mut pre, lease) = svc.authorize("send_email", "agent-1");
        pre.authorized = false;
        let dispatcher = AtMostOnceDispatcher::new(svc);

        let result = dispatcher.authorize(&pre, &lease);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn lease_consumed_twice_rejected() {
        let svc = Arc::new(MockEmailService::new());
        let (pre, mut lease) = svc.authorize("send_email", "a");
        let dispatcher = AtMostOnceDispatcher::new(svc.clone());
        dispatcher.authorize(&pre, &lease).unwrap();

        // First use consumes lease
        let _ = dispatcher.safe_dispatch(
            TransactionId::new(), "send_email", ContentHash::new("h"),
            &mut lease, &serde_json::json!({"to": "x"})).await.unwrap();

        // Second use with same lease → rejected
        let result = dispatcher.safe_dispatch(
            TransactionId::new(), "send_email", ContentHash::new("h"),
            &mut lease, &serde_json::json!({"to": "y"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn unknown_outcome_is_not_retried() {
        let svc = Arc::new(MockEmailService::new());
        *svc.lose_next_response.lock().unwrap() = true;
        let (pre, mut lease) = svc.authorize("send_email", "a");
        let dispatcher = AtMostOnceDispatcher::new(svc.clone());

        dispatcher.authorize(&pre, &lease).unwrap();
        let result = dispatcher.safe_dispatch(
            TransactionId::new(), "send_email", ContentHash::new("h"),
            &mut lease, &serde_json::json!({"to": "x"})).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            IrreversibleEffectError::UnknownOutcome(_) => {}
            e => panic!("expected UnknownOutcome, got {:?}", e),
        }
        // Email was sent despite response loss
        assert_eq!(svc.sent_count(), 1);
    }

    #[tokio::test]
    async fn expired_lease_rejected() {
        let svc = Arc::new(MockEmailService::new());
        let (pre, mut lease) = svc.authorize("send_email", "a");
        lease.expires_at = chrono::Utc::now() - chrono::Duration::minutes(1);
        let dispatcher = AtMostOnceDispatcher::new(svc);

        let result = dispatcher.authorize(&pre, &lease);
        assert!(result.is_err());
    }
}
