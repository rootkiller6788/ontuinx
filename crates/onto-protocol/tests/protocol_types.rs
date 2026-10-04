//! Unit tests for onto-protocol core types.
use onto_protocol::digest::{Digest, DigestAlgorithm};
use onto_protocol::envelope::{ProtocolEnvelope, Versioned};
use onto_protocol::candidate::SealedCandidateRef;
use onto_protocol::context::{
    CandidateVerificationContext, GraphVerificationContext, VerificationContext,
};
use onto_protocol::finding::{
    Finding, FindingFingerprint, FindingSeverity, CategoryId, FindingDisposition,
    RemediationClass, FindingPolicy,
};
use onto_protocol::verifier::{Pass, VerifierStatus, VerifierResult};
use onto_protocol::verdict::{
    ConformanceOutcome, FreshnessState, CoverageState, UnitExecutionStatus,
};
use onto_protocol::check::EvidenceKind;
use onto_protocol::sandbox::SandboxExecutionStatus;
use onto_protocol::loop_protocol::{
    AttemptDecision, CandidateLoopDecision, NoCandidateLoopDecision,
    AssuranceObservation, AttemptObservation, NoCandidateOutcome,
    TransitionOutcome, SettlementState,
};
use onto_protocol::progress::{ProgressSnapshot, ProgressComparison, compare_progress};
use onto_protocol::candidate::RunCompletion;

fn ph() -> Digest { Digest::new(DigestAlgorithm::Sha256, "aa") }

// ══════════════════════════ Digest ══════════════════════════

#[test]
fn digest_display_format() {
    let d = Digest::new(DigestAlgorithm::Sha256, "abcdef");
    assert_eq!(d.to_string(), "sha256:abcdef");
}

#[test]
fn digest_equality() {
    let a = Digest::new(DigestAlgorithm::Blake3, "x");
    let b = Digest::new(DigestAlgorithm::Blake3, "x");
    let c = Digest::new(DigestAlgorithm::Sha256, "x");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// ══════════════════════════ Versioned ══════════════════════════

#[test]
fn versioned_default_schema() {
    let v: Versioned<String> = Versioned::new("hello".into());
    assert_eq!(v.protocol.schema_version, 1);
    assert_eq!(v.protocol.canonicalization_version, 1);
}

#[test]
fn versioned_custom_schema() {
    let v: Versioned<i32> = Versioned::with_version(42, 2, 3);
    assert_eq!(v.protocol.schema_version, 2);
    assert_eq!(v.payload, 42);
}

// ══════════════════════════ SealedCandidate ══════════════════════════

#[test]
fn sealed_candidate_ref_constructs() {
    let c = SealedCandidateRef::new("c1", ph(), ph(), "/tmp/ref");
    assert_eq!(c.candidate_id, "c1");
}

// ══════════════════════════ VerificationContext ══════════════════════════

#[test]
fn pregraph_context_candidate_access() {
    let c = SealedCandidateRef::new("c1", ph(), ph(), "/tmp");
    let ctx = VerificationContext::PreGraph(CandidateVerificationContext {
        attempt_id: "a1".into(), candidate: c,
        repository_name: "r".into(), base_commit_sha: "s".into(),
        execution_generation: 1, changed_files: vec![], language: "rust".into(),
    });
    assert_eq!(ctx.candidate().attempt_id, "a1");
    assert!(ctx.graph().is_none());
}

#[test]
fn withgraph_context_graph_access() {
    let c = SealedCandidateRef::new("c1", ph(), ph(), "/tmp");
    let candidate = CandidateVerificationContext {
        attempt_id: "a2".into(), candidate: c,
        repository_name: "r".into(), base_commit_sha: "s".into(),
        execution_generation: 1, changed_files: vec![], language: "go".into(),
    };
    let ctx = VerificationContext::WithGraph(GraphVerificationContext {
        candidate, snapshot_id: "snap-1".into(), snapshot_digest: ph(),
        changed_entity_keys: vec!["main".into()],
    });
    assert!(ctx.graph().is_some());
    assert_eq!(ctx.graph().unwrap().changed_entity_keys.len(), 1);
    assert_eq!(ctx.candidate().language, "go");
}

// ══════════════════════════ Finding ══════════════════════════

#[test]
fn finding_fingerprint_match_strength() {
    let a = FindingFingerprint {
        rule_id: "r1".into(), entity_key: Some("main".into()),
        artifact_path: "src/main.rs".into(), semantic_key: "k1".into(), line_hint: Some(10),
    };
    let b = FindingFingerprint {
        rule_id: "r1".into(), entity_key: Some("main".into()),
        artifact_path: "src/main.rs".into(), semantic_key: "k1".into(), line_hint: Some(10),
    };
    assert_eq!(a.match_strength(&b), 161); // 100+50+10+1
}

#[test]
fn finding_severity_ordering() {
    assert!(FindingSeverity::Critical > FindingSeverity::High);
    assert!(FindingSeverity::Info < FindingSeverity::Low);
}

#[test]
fn category_id_constructs() {
    let cat = CategoryId::new("code.bug");
    assert_eq!(cat.to_string(), "code.bug");
}

#[test]
fn finding_policy_holds_severity_and_disposition() {
    let policy = FindingPolicy { severity: FindingSeverity::Medium, disposition: FindingDisposition::Blocking };
    assert_eq!(policy.severity, FindingSeverity::Medium);
}

// ══════════════════════════ Verifier ══════════════════════════

#[test]
fn pass_all_returns_9() {
    assert_eq!(Pass::all().len(), 9);
}

#[test]
fn verifier_statuses_are_distinct() {
    let statuses = [VerifierStatus::Completed, VerifierStatus::PartiallyCompleted,
        VerifierStatus::PrerequisiteFailed, VerifierStatus::Unavailable,
        VerifierStatus::TimedOut, VerifierStatus::NotApplicable];
    assert_eq!(statuses.len(), 6);
}

// ══════════════════════════ Verdict ══════════════════════════

#[test]
fn unit_execution_status_variants() {
    assert!(matches!(UnitExecutionStatus::Passed, UnitExecutionStatus::Passed));
    assert!(matches!(UnitExecutionStatus::EvidenceIncomplete, UnitExecutionStatus::EvidenceIncomplete));
}

// ══════════════════════════ Sandbox ══════════════════════════

#[test]
fn sandbox_execution_status() {
    let s = SandboxExecutionStatus::Completed;
    assert!(matches!(s, SandboxExecutionStatus::Completed));
}

// ══════════════════════════ EvidenceKind ══════════════════════════

#[test]
fn evidence_kind_covers_all_pass_types() {
    let kinds = [EvidenceKind::BuildOutput, EvidenceKind::TestReport,
        EvidenceKind::LintOutput, EvidenceKind::FormatOutput,
        EvidenceKind::AuditReport, EvidenceKind::SimulationOutput, EvidenceKind::RawLog];
    assert_eq!(kinds.len(), 7);
}

// ══════════════════════════ Loop Protocol ══════════════════════════

#[test]
fn attempt_decision_candidate_variants() {
    let d = AttemptDecision::Candidate(CandidateLoopDecision::FinalizeCandidate);
    assert!(matches!(d, AttemptDecision::Candidate(CandidateLoopDecision::FinalizeCandidate)));
}

#[test]
fn attempt_decision_nocandidate_variants() {
    let d = AttemptDecision::NoCandidate(NoCandidateLoopDecision::CloseFailed);
    assert!(matches!(d, AttemptDecision::NoCandidate(NoCandidateLoopDecision::CloseFailed)));
}

#[test]
fn assurance_observation_unavailable() {
    let obs = AssuranceObservation::Unavailable { reason: "down".into(), diagnostic_ref: "d1".into() };
    assert!(matches!(obs, AssuranceObservation::Unavailable { .. }));
}

#[test]
fn attempt_observation_candidate_available() {
    let c = SealedCandidateRef::new("c1", ph(), ph(), "/tmp");
    let obs = AttemptObservation::CandidateAvailable {
        attempt_id: "a1".into(),
        run_completion: RunCompletion { staging_root: "/t".into(), artifact_manifest: None, observations: vec![], duration_ms: 0 },
        candidate: c,
        assurance: AssuranceObservation::Unavailable { reason: "x".into(), diagnostic_ref: "d".into() },
        evidence_bundle_ref: None,
    };
    assert!(matches!(obs, AttemptObservation::CandidateAvailable { .. }));
}

#[test]
fn attempt_observation_nocandidate() {
    let obs = AttemptObservation::NoCandidate {
        attempt_id: "a1".into(), outcome: NoCandidateOutcome::AgentFailed, diagnostic_ref: None,
    };
    assert!(matches!(obs, AttemptObservation::NoCandidate { .. }));
}

// ══════════════════════════ Transition ══════════════════════════

#[test]
fn transition_outcome_variants() {
    assert!(matches!(TransitionOutcome::Continued, TransitionOutcome::Continued));
    assert!(matches!(TransitionOutcome::Finalized { settlement: SettlementState::Committed, receipt_ref: "r".into() }, TransitionOutcome::Finalized { .. }));
    assert!(matches!(TransitionOutcome::AttemptClosed, TransitionOutcome::AttemptClosed));
}

// ══════════════════════════ Progress ══════════════════════════

#[test]
fn progress_improved_when_resolved_and_no_regressions() {
    let prev = ProgressSnapshot { attempt_number: 1, total_blocking: 3, total_advisory: 2,
        resolved_fingerprints: vec![], new_fingerprints: vec![], persistent_fingerprints: vec![], regressions: vec![] };
    let curr = ProgressSnapshot { attempt_number: 2, total_blocking: 1, total_advisory: 1,
        resolved_fingerprints: vec![make_fp("r1")], new_fingerprints: vec![],
        persistent_fingerprints: vec![], regressions: vec![] };
    assert_eq!(compare_progress(&prev, &curr), ProgressComparison::Improved);
}

#[test]
fn progress_unchanged_when_nothing_changed() {
    let snap = ProgressSnapshot { attempt_number: 1, total_blocking: 0, total_advisory: 0,
        resolved_fingerprints: vec![], new_fingerprints: vec![], persistent_fingerprints: vec![], regressions: vec![] };
    assert_eq!(compare_progress(&snap, &snap), ProgressComparison::Unchanged);
}

#[test]
fn progress_regressed_when_regressions_appear() {
    let prev = ProgressSnapshot { attempt_number: 1, total_blocking: 0, total_advisory: 0,
        resolved_fingerprints: vec![], new_fingerprints: vec![], persistent_fingerprints: vec![], regressions: vec![] };
    let curr = ProgressSnapshot { attempt_number: 2, total_blocking: 1, total_advisory: 0,
        resolved_fingerprints: vec![], new_fingerprints: vec![],
        persistent_fingerprints: vec![], regressions: vec![make_fp("r1")] };
    assert_eq!(compare_progress(&prev, &curr), ProgressComparison::Regressed);
}

fn make_fp(rule: &str) -> FindingFingerprint {
    FindingFingerprint { rule_id: rule.into(), entity_key: None, artifact_path: "f.rs".into(), semantic_key: rule.into(), line_hint: None }
}
