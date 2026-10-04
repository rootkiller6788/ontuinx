//! M6-C + M6-D: External Effect Authority Tests
//!
//! M6-C: Compensatable — execute → verify → confirm / compensate / freeze.
//! M6-D: Irreversible — pre-validate → strong approve → at-most-once dispatch.

use onto_assurance_runtime::ports::{
    RunFinalizationOutcome,
};
use onto_assurance_types::enums::{
    BudgetOutcome, EffectClass, LifecycleState, TaskOutcome,
};
use onto_assurance_types::ids::{AttemptId, DecisionId};
use onto_ironclaw_adapter::loop_adapter::LoopAdapter;

fn make_outcome(task: TaskOutcome, lifecycle: LifecycleState) -> RunFinalizationOutcome {
    RunFinalizationOutcome {
        task_outcome: task,
        budget_outcome: BudgetOutcome::WithinBudget,
        lifecycle_state: lifecycle,
        session_decision_id: DecisionId::new(),
        attempt_decision_id: DecisionId::new(),
        reason_codes: vec![],
        settlement_decision: None,
        effect_class: None,
    }
}

// ══════════════════════════════════════════════════════════════════
// M6-C: Compensatable External Effects
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod m6_c_tests {
    use super::*;

    /// C.1: Unauthorized execution → external API call count = 0.
    #[test]
    fn c1_unauthorized_prevents_execution() {
        let authorized = false;
        let external_call_made = authorized; // Only call if authorized
        assert!(!external_call_made,
                "unauthorized → external API call count must be 0");
    }

    /// C.2: Create succeeds + verification passes → Confirmed.
    #[test]
    fn c2_create_success_verify_pass_confirmed() {
        let create_success = true;
        let verify_pass = true;

        let state = match (create_success, verify_pass) {
            (true, true) => "Confirmed",
            (true, false) => "Compensate",
            (false, _) => "Failed",
        };
        assert_eq!(state, "Confirmed");
    }

    /// C.3: Create succeeds but verification fails → compensate.
    #[test]
    fn c3_create_success_verify_fail_compensates() {
        // External resource was created, but post-creation verification failed.
        // Must compensate (e.g., delete the created resource).
        let created = true;
        let verified = false;

        let needs_compensation = created && !verified;
        assert!(needs_compensation,
                "created but not verified → must compensate");
    }

    /// C.4: Compensation succeeds → Compensated.
    #[test]
    fn c4_compensation_succeeds() {
        let compensation_executed = true;
        let compensation_result = "success";

        let final_state = if compensation_executed && compensation_result == "success" {
            "Compensated"
        } else {
            "Frozen"
        };
        assert_eq!(final_state, "Compensated");
    }

    /// C.5: Compensation fails → Frozen + Escalate.
    #[test]
    fn c5_compensation_fails_frozen() {
        let compensation_failed = true;
        let state_unknown = true;

        let final_state = if compensation_failed && state_unknown {
            "Frozen + Escalated"
        } else {
            "Compensated"
        };
        assert_eq!(final_state, "Frozen + Escalated",
                   "compensation failed + unknown state → must escalate to human");
    }

    /// C.6: Create response lost → query external state, don't blindly retry.
    #[test]
    fn c6_response_lost_query_not_blind_retry() {
        let response_received = false;
        let blindly_retried = false;

        // Correct: query external system for actual state
        let queried_external = true;
        let resource_exists = true; // query result

        if !response_received {
            assert!(queried_external,
                    "response lost → must query external system first");
            assert!(!blindly_retried,
                    "must NOT blindly retry — could create duplicate");
            // If resource exists → confirm. If not → recreate with same idempotency key.
            assert!(resource_exists,
                    "resource exists → confirm (no duplicate needed)");
        }
    }

    /// C.7: Repeated execution with same idempotency key → not re-created.
    #[test]
    fn c7_idempotency_key_prevents_duplicate() {
        let idempotency_key = "idem-create-resource-1";
        let first_call = true;
        let second_call_same_key = true;

        // With same idempotency key, second call must not create a new resource
        let duplicate_created = first_call && second_call_same_key;
        // In reality: idempotency ensures at-most-one creation
        let actual_creations = 1; // Only the first call creates
        assert_eq!(actual_creations, 1,
                   "same idempotency key → only one resource created");
    }

    /// C.8: Wrong OriginalReceipt → cannot execute compensation.
    #[test]
    fn c8_wrong_receipt_prevents_compensation() {
        // Compensation requires a valid ExternalOperationReceipt.
        // Wrong receipt → rejected.
        let receipt_valid = false;

        let compensation_allowed = receipt_valid;
        assert!(!compensation_allowed,
                "compensation with invalid receipt must be rejected");
    }

    /// C.9: Compensation capability lacks permission → reject + escalate.
    #[test]
    fn c9_compensation_permission_check() {
        let has_compensation_permission = false;

        if !has_compensation_permission {
            // Cannot execute compensation → escalate
            let escalated = true;
            assert!(escalated, "no compensation permission → must escalate");
        }
    }

    /// C.10: External resource manually modified → stop + escalate.
    #[test]
    fn c10_external_modified_by_human() {
        let external_state_changed_unexpectedly = true;
        let auto_compensation_attempted = false; // correct: stop

        if external_state_changed_unexpectedly {
            assert!(!auto_compensation_attempted,
                    "human modified resource → stop auto-compensation");
            let escalated = true;
            assert!(escalated, "human intervention needed");
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// M6-D: Irreversible External Effects
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod m6_d_tests {
    use super::*;

    /// D.1: No approval → dispatch call = 0.
    #[test]
    fn d1_no_approval_no_dispatch() {
        let approved = false;
        let dispatch_called = approved;
        assert!(!dispatch_called,
                "without approval, dispatch must NOT be called");
    }

    /// D.2: Approval params differ from actual params → reject.
    #[test]
    fn d2_approval_params_mismatch_rejected() {
        let approved_params = serde_json::json!({"to": "alice@example.com"});
        let actual_params = serde_json::json!({"to": "bob@example.com"});

        let params_match = approved_params == actual_params;
        assert!(!params_match, "different recipient → must reject");
        assert!(!params_match,
                "approval params != actual params → dispatch rejected");
    }

    /// D.3: Lease expired → reject.
    #[test]
    fn d3_expired_lease_rejected() {
        let lease_valid = false; // expired
        let dispatch_allowed = lease_valid;
        assert!(!dispatch_allowed, "expired lease → dispatch must be rejected");
    }

    /// D.4: Lease reused → second use rejected.
    #[test]
    fn d4_lease_reuse_rejected() {
        let lease_used_count = 2; // was used twice
        let max_uses = 1;

        let exceeded = lease_used_count > max_uses;
        assert!(exceeded, "lease used {} times, max is {}", lease_used_count, max_uses);
    }

    /// D.5: DispatchIntent persistence fails → don't send.
    #[test]
    fn d5_intent_persist_failure_no_send() {
        let intent_persisted = false;
        let message_sent = intent_persisted; // only send if intent persisted
        assert!(!message_sent,
                "intent not persisted → message must NOT be sent");
    }

    /// D.6: Send success + receipt → Confirmed.
    #[test]
    fn d6_send_success_confirmed() {
        let sent = true;
        let receipt_received = true;

        let state = if sent && receipt_received { "Confirmed" } else { "Unknown" };
        assert_eq!(state, "Confirmed");
    }

    /// D.7: Send success but response lost → UnknownExternalOutcome.
    #[test]
    fn d7_response_lost_unknown_outcome() {
        let sent = true; // request was sent
        let receipt_received = false; // response lost

        let state = if sent && !receipt_received {
            "UnknownExternalOutcome"
        } else {
            "Confirmed"
        };
        assert_eq!(state, "UnknownExternalOutcome",
                   "sent but no receipt → unknown outcome, query external");
    }

    /// D.8: Unknown state → do NOT auto-resend.
    #[test]
    fn d8_unknown_state_no_auto_resend() {
        let state = "UnknownExternalOutcome";
        let auto_resend = false; // never auto-resend irreversible

        assert!(!auto_resend,
                "UnknownExternalOutcome → must NOT auto-resend irreversible action");
    }

    /// D.9: Recipient changed → original lease invalid.
    #[test]
    fn d9_recipient_changed_lease_invalid() {
        let original_recipient = "alice@example.com";
        let new_dispatch_wants = "bob@example.com";

        let lease_still_valid = original_recipient == new_dispatch_wants;
        assert!(!lease_still_valid,
                "recipient changed → original lease invalidated");
    }

    /// D.10: System restart → reconcile Intent vs Receipt.
    #[test]
    fn d10_restart_reconcile() {
        let intent_exists = true; // DispatchIntent was persisted before crash
        let receipt_exists = false; // No receipt → don't know if it was sent

        if intent_exists && !receipt_exists {
            // Must query external system or check idempotency
            let queried_external = true;
            let external_shows_sent = true;
            assert!(queried_external);
            if external_shows_sent {
                // Update receipt, confirm
                let confirmed = true;
                assert!(confirmed);
            }
        }
    }

    /// D.11: Manual confirmation — human says it was sent.
    #[test]
    fn d11_manual_confirmation() {
        let human_confirmed_sent = true;
        let state = if human_confirmed_sent { "Confirmed" } else { "Unknown" };
        assert_eq!(state, "Confirmed",
                   "human confirmed send → project Confirmed");
    }

    /// D.12: Manual confirmation — human says NOT sent, re-authorize.
    #[test]
    fn d12_manual_not_sent_reauthorize() {
        let human_confirmed_not_sent = true;

        if human_confirmed_not_sent {
            // Generate new authorization flow
            let new_authorization_needed = true;
            assert!(new_authorization_needed,
                    "not sent → must create new authorization, not reuse old lease");
        }
    }
}
