//! IrreversibleMessageService — M6-D at-most-once dispatch.
//!
//! Simulates sending messages/emails. Once sent, cannot be taken back.
//! All verification and authorization must happen BEFORE dispatch.

use std::sync::{Arc, Mutex};
use onto_assurance_types::external_effects::{
    DispatchIntent, DispatchStatus, ExactInvocationLease, ExternalDispatchReceipt,
    PreExecutionDecision,
};

pub struct IrreversibleMessageService {
    sent: Arc<Mutex<Vec<SentMessage>>>,
    pub lose_next_response: Mutex<bool>,
}

#[derive(Clone)]
struct SentMessage {
    dispatch_id: String,
    to: String,
    body_hash: String,
    lease_id: String,
}

impl IrreversibleMessageService {
    pub fn new() -> Self {
        Self { sent: Arc::new(Mutex::new(vec![])), lose_next_response: Mutex::new(false) }
    }
    pub fn sent_count(&self) -> usize { self.sent.lock().unwrap().len() }

    /// D1: Pre-execution validation — all checks must pass before dispatch.
    pub fn validate(&self, pre: &PreExecutionDecision, lease: &ExactInvocationLease) -> Result<(), String> {
        if !pre.authorized { return Err("not authorized".into()); }
        if !lease.is_valid() { return Err("lease invalid or expired".into()); }
        Ok(())
    }

    /// D2+D4: At-most-once dispatch with lease consumption.
    pub fn dispatch(
        &self, intent: &DispatchIntent, lease: &mut ExactInvocationLease,
        params: &str,
    ) -> Result<ExternalDispatchReceipt, String> {
        if !lease.consume() { return Err("lease already consumed".into()); }
        if intent.lease_id != lease.lease_id { return Err("lease mismatch".into()); }
        if intent.params_hash != hash_str(params) { return Err("params hash mismatch".into()); }

        let dispatch_id = format!("msg-{}", chrono::Utc::now().timestamp_millis());
        let lose = self.lose_next_response.lock().unwrap();
        let status = if *lose {
            DispatchStatus::UnknownExternalOutcome
        } else {
            DispatchStatus::Confirmed
        };

        // Always record the message as sent (even if response is lost)
        self.sent.lock().unwrap().push(SentMessage {
            dispatch_id: dispatch_id.clone(), to: params.to_string(),
            body_hash: intent.params_hash.clone(), lease_id: lease.lease_id.clone(),
        });

        Ok(ExternalDispatchReceipt {
            transaction_id: intent.transaction_id.clone(),
            dispatch_id,
            external_system: "message".into(),
            status,
            responded_at: chrono::Utc::now(),
        })
    }

    /// D5: Query external system for actual outcome.
    pub fn query_external(&self, dispatch_id: &str) -> Result<Option<ExternalDispatchReceipt>, String> {
        let found = self.sent.lock().unwrap().iter()
            .find(|m| m.dispatch_id == dispatch_id)
            .cloned();
        Ok(found.map(|m| ExternalDispatchReceipt {
            transaction_id: String::new(),
            dispatch_id: m.dispatch_id,
            external_system: "message".into(),
            status: DispatchStatus::Confirmed,
            responded_at: chrono::Utc::now(),
        }))
    }
}

fn hash_str(s: &str) -> String {
    use sha2::{Sha256, Digest};
    hex::encode(Sha256::digest(s.as_bytes()))
}

// ══════════════════════════════════════════════════════════════════
// Tests — D1-D5: 12 scenarios
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::ids::{AttemptId, DecisionId, RunId};

    fn make_lease() -> ExactInvocationLease {
        ExactInvocationLease {
            lease_id: uuid::Uuid::new_v4().to_string(),
            capability: "send_message".into(), actor: "agent-1".into(),
            run_id: RunId::new(), attempt_id: AttemptId::new(),
            params_hash: hash_str(r#"{"to":"a@b.com"}"#),
            max_uses: 1, used_count: 0,
            expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
            decision_id: DecisionId::new(),
        }
    }

    fn make_intent(lease: &ExactInvocationLease) -> DispatchIntent {
        DispatchIntent {
            transaction_id: uuid::Uuid::new_v4().to_string(),
            capability: "send_message".into(),
            params_hash: lease.params_hash.clone(),
            lease_id: lease.lease_id.clone(),
            idempotency_key: uuid::Uuid::new_v4().to_string(),
            recorded_at: chrono::Utc::now(),
        }
    }

    // D1-1: No approval → rejected
    #[test]
    fn d1_no_approval_rejected() {
        let svc = IrreversibleMessageService::new();
        let pre = PreExecutionDecision { decision_id: DecisionId::new(), authorized: false,
            required_approvals: vec![], granted_approvals: vec![], lease: None };
        let lease = make_lease();
        assert!(svc.validate(&pre, &lease).is_err());
    }

    // D1-2: Expired lease → rejected
    #[test]
    fn d2_expired_lease_rejected() {
        let svc = IrreversibleMessageService::new();
        let pre = PreExecutionDecision { decision_id: DecisionId::new(), authorized: true,
            required_approvals: vec![], granted_approvals: vec![], lease: None };
        let mut lease = make_lease();
        lease.expires_at = chrono::Utc::now() - chrono::Duration::minutes(1);
        assert!(svc.validate(&pre, &lease).is_err());
    }

    // D3: Intent matches lease
    #[test]
    fn d3_intent_params_mismatch_rejected() {
        let svc = IrreversibleMessageService::new();
        let mut lease = make_lease();
        let intent = make_intent(&lease);
        assert!(svc.dispatch(&intent, &mut lease, r#"{"to":"DIFFERENT"}"#).is_err());
    }

    // D4-1: Valid dispatch succeeds
    #[test]
    fn d4_valid_dispatch_succeeds() {
        let svc = IrreversibleMessageService::new();
        let mut lease = make_lease();
        let intent = make_intent(&lease);
        let receipt = svc.dispatch(&intent, &mut lease, r#"{"to":"a@b.com"}"#).unwrap();
        assert_eq!(receipt.status, DispatchStatus::Confirmed);
        assert_eq!(svc.sent_count(), 1);
        assert_eq!(lease.used_count, 1);
    }

    // D4-2: Lease consumed twice → rejected
    #[test]
    fn d4_lease_consumed_twice_rejected() {
        let svc = IrreversibleMessageService::new();
        let mut lease = make_lease();
        let intent = make_intent(&lease);
        svc.dispatch(&intent, &mut lease, r#"{"to":"a@b.com"}"#).unwrap();
        let r2 = svc.dispatch(&intent, &mut lease, r#"{"to":"a@b.com"}"#);
        assert!(r2.is_err());
        assert_eq!(svc.sent_count(), 1, "only one message sent");
    }

    // D4-3: Lease ID mismatch → rejected
    #[test]
    fn d4_lease_id_mismatch_rejected() {
        let svc = IrreversibleMessageService::new();
        let mut lease = make_lease();
        let mut intent = make_intent(&lease);
        intent.lease_id = "wrong-lease".into();
        assert!(svc.dispatch(&intent, &mut lease, r#"{"to":"a@b.com"}"#).is_err());
    }

    // D5-1: Unknown outcome — message WAS sent, response lost
    #[test]
    fn d5_unknown_outcome_message_sent() {
        let svc = IrreversibleMessageService::new();
        *svc.lose_next_response.lock().unwrap() = true;
        let mut lease = make_lease();
        let intent = make_intent(&lease);

        let receipt = svc.dispatch(&intent, &mut lease, r#"{"to":"a@b.com"}"#).unwrap();
        assert_eq!(receipt.status, DispatchStatus::UnknownExternalOutcome);
        assert_eq!(svc.sent_count(), 1, "message WAS sent despite lost response");
        assert_eq!(lease.used_count, 1, "lease consumed");
    }

    // D5-2: Unknown outcome → query confirms it was sent
    #[test]
    fn d5_query_confirms_sent() {
        let svc = IrreversibleMessageService::new();
        let mut lease = make_lease();
        let intent = make_intent(&lease);
        let receipt = svc.dispatch(&intent, &mut lease, r#"{"to":"a@b.com"}"#).unwrap();

        let found = svc.query_external(&receipt.dispatch_id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().status, DispatchStatus::Confirmed);
    }

    // D5-3: After unknown, no auto-retry
    #[test]
    fn d5_unknown_never_auto_retry() {
        let svc = IrreversibleMessageService::new();
        *svc.lose_next_response.lock().unwrap() = true;
        let mut lease = make_lease();
        let intent = make_intent(&lease);
        let receipt = svc.dispatch(&intent, &mut lease, r#"{"to":"a@b.com"}"#).unwrap();
        assert_eq!(receipt.status, DispatchStatus::UnknownExternalOutcome);

        // Lease already consumed — cannot retry with same lease
        assert!(!lease.is_valid());
    }

    // D5-4: Not found → safe to create new authorization
    #[test]
    fn d5_not_found_safe_for_new_auth() {
        let svc = IrreversibleMessageService::new();
        let found = svc.query_external("non-existent-id").unwrap();
        assert!(found.is_none(), "not found → safe to create new authorization flow");
    }

    // Regression: Unauthorized → dispatch never called
    #[test]
    fn d1_unauthorized_dispatch_count_zero() {
        let svc = IrreversibleMessageService::new();
        let pre = PreExecutionDecision { decision_id: DecisionId::new(), authorized: false,
            required_approvals: vec![], granted_approvals: vec![], lease: None };
        let lease = make_lease();
        assert!(svc.validate(&pre, &lease).is_err());
        assert_eq!(svc.sent_count(), 0, "dispatch count must be zero");
    }
}
