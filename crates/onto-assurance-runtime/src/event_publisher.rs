//! EventPublisher — emits cross-layer events for OntoGraph consumption.
//!
//! Each event carries an EventEnvelope with attempt_id and sequence_number.
//! OntoGraph deduplicates by event_id and detects gaps by sequence_number.

use onto_protocol::events::{
    EventEnvelope, CandidateProduced, CandidateSealed, PlanResolved,
    FindingProduced, VerdictIssued, DecisionMade, SandboxExecutionCompleted,
};
use onto_protocol::verdict::ConformanceVerdict;
use onto_protocol::check::ConformancePlan;
use onto_protocol::digest::Digest;
use chrono::Utc;

pub struct EventPublisher {
    attempt_id: String,
    sequence: u64,
}

impl EventPublisher {
    pub fn new(attempt_id: &str) -> Self {
        Self { attempt_id: attempt_id.to_string(), sequence: 0 }
    }

    fn next_seq(&mut self) -> u64 { let s = self.sequence; self.sequence += 1; s }

    fn envelope<T>(&mut self, payload: T) -> EventEnvelope<T> {
        EventEnvelope {
            event_id: uuid::Uuid::new_v4().to_string(),
            schema_version: 1,
            attempt_id: self.attempt_id.clone(),
            sequence_number: self.next_seq(),
            correlation_id: self.attempt_id.clone(),
            causation_id: None,
            occurred_at: Utc::now().to_rfc3339(),
            payload,
        }
    }

    pub fn candidate_produced(&mut self, candidate_id: &str, count: u32) -> EventEnvelope<CandidateProduced> {
        self.envelope(CandidateProduced { candidate_id: candidate_id.to_string(), artifact_count: count })
    }

    pub fn candidate_sealed(&mut self, candidate_id: &str, digest: &Digest, manifest_digest: &Digest) -> EventEnvelope<CandidateSealed> {
        self.envelope(CandidateSealed { candidate_id: candidate_id.to_string(), digest: digest.value.clone(), manifest_digest: manifest_digest.value.clone() })
    }

    pub fn plan_resolved(&mut self, plan: &ConformancePlan) -> EventEnvelope<PlanResolved> {
        self.envelope(PlanResolved { plan_id: plan.plan_id.clone(), unit_count: plan.units.len() as u32 })
    }

    pub fn finding_produced(&mut self, finding_id: &str, verifier_id: &str, pass: &str, severity: &str) -> EventEnvelope<FindingProduced> {
        self.envelope(FindingProduced { finding_id: finding_id.to_string(), verifier_id: verifier_id.to_string(), pass: pass.to_string(), severity: severity.to_string() })
    }

    pub fn verdict_issued(&mut self, verdict: &ConformanceVerdict) -> EventEnvelope<VerdictIssued> {
        self.envelope(VerdictIssued { verdict_id: verdict.verdict_id.clone(), outcome: format!("{:?}", verdict.conformance), blocking_count: verdict.blocking_findings.len() as u32, advisory_count: verdict.advisory_findings.len() as u32 })
    }

    pub fn sandbox_completed(&mut self, request_id: &str, status: &str, check_count: u32) -> EventEnvelope<SandboxExecutionCompleted> {
        self.envelope(SandboxExecutionCompleted { request_id: request_id.to_string(), status: status.to_string(), check_count })
    }

    pub fn decision_made(&mut self, decision_id: &str, decision: &str) -> EventEnvelope<DecisionMade> {
        self.envelope(DecisionMade { decision_id: decision_id.to_string(), decision: decision.to_string() })
    }

    pub fn sequence(&self) -> u64 { self.sequence }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_protocol::digest::DigestAlgorithm;

    #[test]
    fn events_have_monotonic_sequence() {
        let mut p = EventPublisher::new("att-1");
        assert_eq!(p.sequence(), 0);
        let e1 = p.candidate_produced("c1", 3);
        let e2 = p.candidate_produced("c1", 3);
        assert!(e2.sequence_number > e1.sequence_number);
    }

    #[test]
    fn events_have_correct_attempt_id() {
        let mut p = EventPublisher::new("att-42");
        let e = p.candidate_produced("c1", 1);
        assert_eq!(e.attempt_id, "att-42");
    }
}
