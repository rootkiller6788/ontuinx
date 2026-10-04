//! LoopDecisionEngine — L3 decision logic.
//!
//! Consumes AttemptObservation (containing ConformanceVerdict from L1),
//! examines budget and progress history, produces a single LoopDecision.
//! Fail-Closed: no Verdict → no Finalize.

use onto_protocol::loop_protocol::{
    AttemptObservation, AssuranceObservation, CandidateLoopDecision, AttemptDecision,
    NoCandidateLoopDecision, NoCandidateOutcome,
};
use onto_protocol::verdict::{ConformanceOutcome, ConformanceVerdict, CoverageState, FreshnessState, SandboxValidationSummary};
use onto_protocol::progress::{ProgressSnapshot, ProgressComparison};
use onto_protocol::finding::RemediationClass;
use crate::budget::LoopBudget;

/// Context available to the DecisionEngine on each Evaluation.
pub struct LoopContext {
    pub budget: LoopBudget,
    pub prev_progress: Option<ProgressSnapshot>,
    pub max_attempts: u32,
}

/// The authoritative decision for one Attempt observation.
pub fn decide(
    observation: &AttemptObservation,
    ctx: &LoopContext,
) -> AttemptDecision {
    match observation {
        AttemptObservation::CandidateAvailable { assurance, .. } => {
            match assurance {
                AssuranceObservation::Verdict(verdict) => {
                    let base = decide_with_verdict(verdict, ctx);

                    // P16-4: Progress gate — check for cross-attempt stagnation
                    match &base {
                        CandidateLoopDecision::Continue { .. } => {
                            if let Some(ref prev) = ctx.prev_progress {
                                // If all blocking fingerprints from previous attempt persist,
                                // and no new ones appeared, we're stalled
                                let same_blocking = verdict.blocking_findings.len() as u32 == prev.total_blocking;
                                let no_regression = prev.regressions.is_empty();
                                if same_blocking && no_regression && ctx.budget.no_progress_count() > 1 {
                                    return AttemptDecision::Candidate(
                                        CandidateLoopDecision::Freeze {
                                            reason: format!(
                                                "No progress after {} attempts: {} blocking findings persist",
                                                ctx.budget.no_progress_count(),
                                                verdict.blocking_findings.len(),
                                            ),
                                        }
                                    );
                                }
                            }
                        }
                        _ => {}
                    }

                    AttemptDecision::Candidate(base)
                }
                AssuranceObservation::Unavailable { reason, .. } => {
                    AttemptDecision::Candidate(CandidateLoopDecision::Escalate {
                        reason: format!("Assurance unavailable: {}", reason),
                    })
                }
            }
        }
        AttemptObservation::NoCandidate { outcome, .. } => {
            AttemptDecision::NoCandidate(match outcome {
                NoCandidateOutcome::AgentFailed | NoCandidateOutcome::RuntimeLost => {
                    if ctx.budget.attempt_exhausted() {
                        NoCandidateLoopDecision::CloseFailed
                    } else {
                        NoCandidateLoopDecision::Freeze {
                            reason: format!("{:?} — freeze for inspection", outcome),
                        }
                    }
                }
                NoCandidateOutcome::Cancelled => NoCandidateLoopDecision::CloseFailed,
            })
        }
    }
}

fn decide_with_verdict(
    verdict: &ConformanceVerdict,
    ctx: &LoopContext,
) -> CandidateLoopDecision {
    // 1. First check conformance outcome
    match verdict.conformance {
        ConformanceOutcome::Conformant => {
            // P16-4: Completeness checks — Conformant alone is not enough
            if !matches!(verdict.freshness, FreshnessState::Current) {
                return CandidateLoopDecision::Freeze {
                    reason: "Verdict not current — cannot finalize".into(),
                };
            }
            if !matches!(verdict.coverage, CoverageState::Complete) {
                return CandidateLoopDecision::Freeze {
                    reason: format!("Coverage {:?} — cannot finalize", verdict.coverage),
                };
            }
            if !verdict.sandbox.is_executed_and_completed()
                && !matches!(verdict.sandbox, SandboxValidationSummary::NotRunDueToBlockingPrerequisite { .. })
            {
                return CandidateLoopDecision::Freeze {
                    reason: "Sandbox not executed/completed — cannot finalize".into(),
                };
            }
            if !verdict.all_required_units_have_determinate_result() {
                return CandidateLoopDecision::Freeze {
                    reason: "Not all required units have determinate results".into(),
                };
            }

            CandidateLoopDecision::FinalizeCandidate
        }

        ConformanceOutcome::NonConformant => {
            let fixable = verdict.blocking_findings.iter().all(|f| {
                matches!(f.remediation, RemediationClass::AutoRepairable | RemediationClass::RetryWithFeedback)
            });

            if fixable && !ctx.budget.attempt_exhausted() {
                let feedback: Vec<String> = verdict.blocking_findings.iter()
                    .map(|f| f.message.clone())
                    .collect();
                CandidateLoopDecision::Continue { feedback }
            } else if !fixable {
                CandidateLoopDecision::Escalate {
                    reason: format!(
                        "{} unfixable blocking findings: {}",
                        verdict.blocking_findings.len(),
                        verdict.blocking_findings.first()
                            .map(|f| f.message.as_str()).unwrap_or("unknown")
                    ),
                }
            } else {
                CandidateLoopDecision::Escalate {
                    reason: "budget exhausted with blocking findings".to_string(),
                }
            }
        }

        ConformanceOutcome::Inconclusive => {
            // Check Sandbox status for more info
            match &verdict.sandbox {
                SandboxValidationSummary::StartupFailed { .. }
                | SandboxValidationSummary::RuntimeLost { .. } => {
                    if ctx.budget.env_errors_exhausted() {
                        CandidateLoopDecision::Escalate {
                            reason: "sandbox repeatedly unavailable".to_string(),
                        }
                    } else {
                        CandidateLoopDecision::Freeze {
                            reason: "sandbox unavailable — retry after recovery".to_string(),
                        }
                    }
                }
                SandboxValidationSummary::NotRunDueToBlockingPrerequisite { .. } => {
                    // Should not reach here — Pre-Sandbox should have produced NonConformant
                    CandidateLoopDecision::Escalate {
                        reason: "inconclusive verdict with pre-sandbox blocking prerequisite".to_string(),
                    }
                }
                SandboxValidationSummary::Executed { status, .. } => {
                    CandidateLoopDecision::Escalate {
                        reason: format!("inconclusive verdict, sandbox status: {:?}", status),
                    }
                }
            }
        }
    }
}

/// Build progress snapshot from a ConformanceVerdict for cross-Attempt comparison.
pub fn verdict_to_progress(verdict: &ConformanceVerdict, attempt_number: u32) -> ProgressSnapshot {
    let resolved: Vec<_> = vec![]; // tracked across attempts
    let new_fingerprints: Vec<_> = verdict.blocking_findings.iter()
        .chain(verdict.advisory_findings.iter())
        .map(|f| f.fingerprint.clone())
        .collect();

    ProgressSnapshot {
        attempt_number,
        total_blocking: verdict.blocking_findings.len() as u32,
        total_advisory: verdict.advisory_findings.len() as u32,
        resolved_fingerprints: resolved,
        new_fingerprints,
        persistent_fingerprints: vec![],
        regressions: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_protocol::digest::{Digest, DigestAlgorithm};
    use onto_protocol::verdict::{
        ConformanceOutcome, CoverageState, FreshnessState, GraphValidationBinding,
        ConformanceUnitResult, UnitExecutionStatus,
    };
    use onto_protocol::sandbox::SandboxExecutionStatus;
    use onto_protocol::check::{ConformancePlanSummary, Applicability};
    use onto_protocol::finding::{Finding, FindingFingerprint, FindingSeverity, CategoryId, FindingDisposition};

    fn placeholder_digest() -> Digest { Digest::new(DigestAlgorithm::Sha256, "aa") }

    fn minimal_verdict(conformance: ConformanceOutcome, blocking_count: usize) -> ConformanceVerdict {
        let mut blocking = Vec::new();
        for i in 0..blocking_count {
            blocking.push(Finding {
                finding_id: format!("f{}", i),
                fingerprint: FindingFingerprint {
                    rule_id: format!("r{}", i), entity_key: None,
                    artifact_path: format!("f{}.rs", i), semantic_key: format!("k{}", i), line_hint: None,
                },
                pass: onto_protocol::verifier::Pass::Build,
                rule_id: format!("r{}", i), rule_version: "1.0".into(),
                severity: FindingSeverity::Critical,
                category: CategoryId::new("code.build"),
                disposition: FindingDisposition::Blocking,
                remediation: RemediationClass::RetryWithFeedback,
                location: None, message: format!("error {}", i), fix_hint: Some("fix".into()),
                confidence: 1.0, evidence_refs: vec![],
            });
        }

        ConformanceVerdict {
            verdict_id: "v1".into(), verdict_digest: placeholder_digest(),
            attempt_id: "a1".into(), candidate_id: "c1".into(), candidate_digest: placeholder_digest(),
            graph: GraphValidationBinding::Unavailable {
                reason: onto_protocol::verdict::GraphUnavailableReason::ServiceDown,
                diagnostic_ref: "test".into(),
            },
            plan_id: "p1".into(), plan_digest: placeholder_digest(),
            evidence_bundle_ref: "e1".into(), evidence_bundle_digest: placeholder_digest(),
            conformance,
            freshness: FreshnessState::Current,
            coverage: if blocking_count == 0 { CoverageState::Complete } else { CoverageState::Partial },
            sandbox: onto_protocol::verdict::SandboxValidationSummary::Executed {
                request_digest: placeholder_digest(),
                environment_digest: placeholder_digest(),
                run_ref: "r1".into(),
                status: SandboxExecutionStatus::Completed,
                observation_refs: vec![],
            },
            blocking_findings: blocking,
            advisory_findings: vec![],
            unit_results: vec![],
            plan: ConformancePlanSummary { plan_id: "p1".into(), profile_id: "pf1".into(), total_units: 1, required_units: 1, applied_units: 1 },
        }
    }

    fn make_ctx() -> LoopContext {
        LoopContext {
            budget: LoopBudget::new(5),
            prev_progress: None,
            max_attempts: 5,
        }
    }

    fn make_observation(verdict: ConformanceVerdict) -> AttemptObservation {
        AttemptObservation::CandidateAvailable {
            attempt_id: "a1".into(),
            run_completion: onto_protocol::candidate::RunCompletion {
                staging_root: "/tmp".into(),
                artifact_manifest: None,
                observations: vec![],
                duration_ms: 0,
            },
            candidate: onto_protocol::candidate::SealedCandidateRef::new("c1", placeholder_digest(), placeholder_digest(), "/tmp"),
            assurance: AssuranceObservation::Verdict(verdict),
            evidence_bundle_ref: Some("e1".into()),
        }
    }

    #[test]
    fn conformant_verdict_finalizes() {
        let verdict = minimal_verdict(ConformanceOutcome::Conformant, 0);
        let obs = make_observation(verdict);
        let ctx = make_ctx();
        let decision = decide(&obs, &ctx);
        assert!(matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::FinalizeCandidate)));
    }

    #[test]
    fn nonconformant_fixable_continues() {
        let verdict = minimal_verdict(ConformanceOutcome::NonConformant, 2);
        let obs = make_observation(verdict);
        let ctx = make_ctx();
        let decision = decide(&obs, &ctx);
        assert!(matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::Continue { .. })));
    }

    #[test]
    fn assurance_unavailable_escalates() {
        let obs = AttemptObservation::CandidateAvailable {
            attempt_id: "a1".into(),
            run_completion: onto_protocol::candidate::RunCompletion {
                staging_root: "/tmp".into(), artifact_manifest: None, observations: vec![], duration_ms: 0,
            },
            candidate: onto_protocol::candidate::SealedCandidateRef::new("c1", placeholder_digest(), placeholder_digest(), "/tmp"),
            assurance: AssuranceObservation::Unavailable { reason: "L1 down".into(), diagnostic_ref: "d1".into() },
            evidence_bundle_ref: None,
        };
        let ctx = make_ctx();
        let decision = decide(&obs, &ctx);
        assert!(matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::Escalate { .. })));
    }

    #[test]
    fn no_candidate_agent_failed_freezes() {
        let obs = AttemptObservation::NoCandidate {
            attempt_id: "a1".into(),
            outcome: NoCandidateOutcome::AgentFailed,
            diagnostic_ref: None,
        };
        let ctx = make_ctx();
        let decision = decide(&obs, &ctx);
        assert!(matches!(decision, AttemptDecision::NoCandidate(NoCandidateLoopDecision::Freeze { .. })));
    }

    #[test]
    fn verdict_to_progress_tracks_findings() {
        let verdict = minimal_verdict(ConformanceOutcome::NonConformant, 3);
        let progress = verdict_to_progress(&verdict, 1);
        assert_eq!(progress.attempt_number, 1);
        assert_eq!(progress.total_blocking, 3);
        assert_eq!(progress.new_fingerprints.len(), 3);
    }

    #[test]
    fn nonconformant_unfixable_escalates() {
        let mut verdict = minimal_verdict(ConformanceOutcome::NonConformant, 1);
        verdict.blocking_findings[0].remediation = RemediationClass::NonRemediable;
        let obs = make_observation(verdict);
        let ctx = make_ctx();
        let decision = decide(&obs, &ctx);
        assert!(matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::Escalate { .. })));
    }

    #[test]
    fn inconclusive_startup_failed_freezes_when_budget_ok() {
        let mut verdict = minimal_verdict(ConformanceOutcome::Inconclusive, 0);
        verdict.sandbox = SandboxValidationSummary::StartupFailed {
            request_digest: placeholder_digest(),
            diagnostic_ref: "sandbox-down".into(),
        };
        let obs = make_observation(verdict);
        let ctx = make_ctx();
        let decision = decide(&obs, &ctx);
        assert!(matches!(decision, AttemptDecision::Candidate(CandidateLoopDecision::Freeze { .. })));
    }

    #[test]
    fn no_candidate_budget_exhausted_closes_failed() {
        let mut ctx = make_ctx();
        for _ in 0..5 { ctx.budget.record_attempt(); }
        let obs = AttemptObservation::NoCandidate {
            attempt_id: "a1".into(),
            outcome: NoCandidateOutcome::AgentFailed,
            diagnostic_ref: None,
        };
        let decision = decide(&obs, &ctx);
        assert!(matches!(decision, AttemptDecision::NoCandidate(NoCandidateLoopDecision::CloseFailed)));
    }
}
