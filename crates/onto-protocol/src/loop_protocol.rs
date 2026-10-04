//! Loop protocol — L3 decision types.
//!
//! The Loop consumes ConformanceVerdict from L1 and RunCompletion from L2,
//! then produces a single LoopDecision per Attempt.

use crate::candidate::{RunCompletion, SealedCandidateRef};
use crate::digest::Digest;
use crate::verdict::ConformanceVerdict;

// ── Assurance Observation ──

/// What L3 receives about L1's diagnosis.
/// Fail-Closed: Unavailable → no Finalize → Retry / Freeze / Escalate.
#[derive(Debug, Clone)]
pub enum AssuranceObservation {
    Verdict(ConformanceVerdict),
    Unavailable {
        reason: String,
        diagnostic_ref: String,
    },
}

// ── Attempt Observation ──

/// Assembled by the neutral AttemptRunner from L1+L2 results.
#[derive(Debug, Clone)]
pub enum AttemptObservation {
    CandidateAvailable {
        attempt_id: String,
        run_completion: RunCompletion,
        candidate: SealedCandidateRef,
        assurance: AssuranceObservation,
        evidence_bundle_ref: Option<String>,
    },
    NoCandidate {
        attempt_id: String,
        outcome: NoCandidateOutcome,
        diagnostic_ref: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum NoCandidateOutcome {
    AgentFailed,
    Cancelled,
    RuntimeLost,
}

// ── Loop Decisions ──

#[derive(Debug, Clone)]
pub enum CandidateLoopDecision {
    Continue { feedback: Vec<String> },
    DiscardCandidate { reason: String },
    RestoreCheckpoint { checkpoint_id: String },
    FinalizeCandidate,
    Freeze { reason: String },
    Escalate { reason: String },
}

#[derive(Debug, Clone)]
pub enum NoCandidateLoopDecision {
    Freeze { reason: String },
    Escalate { reason: String },
    CloseFailed,
}

// ── Loop Directive (issued to Runtime) ──

/// The Loop's instruction to Runtime. No transition_request field —
/// Runtime executes the one action implied by the decision.
#[derive(Debug, Clone)]
pub enum LoopDirective {
    CandidateBound {
        decision_id: String,
        attempt_id: String,
        candidate_id: String,
        candidate_digest: Digest,
        verdict_id: String,
        verdict_digest: Digest,
        decision: CandidateLoopDecision,
    },
    AttemptBound {
        decision_id: String,
        attempt_id: String,
        decision: NoCandidateLoopDecision,
    },
}

// ── Transition ──

#[derive(Debug, Clone)]
pub enum TransitionOutcome {
    Continued,
    CandidateDiscarded,
    CheckpointRestored { checkpoint_id: String },
    Finalized { settlement: SettlementState, receipt_ref: String },
    Frozen,
    Escalated,
    AttemptClosed,
}

#[derive(Debug, Clone)]
pub struct TransitionReceipt {
    pub receipt_id: String,
    pub receipt_digest: Digest,
    pub decision_id: String,
    pub directive_digest: Digest,
    pub attempt_id: String,
    pub candidate_id: Option<String>,
    pub candidate_digest: Option<Digest>,
    pub verdict_id: Option<String>,
    pub verdict_digest: Option<Digest>,
    pub outcome: TransitionOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettlementState {
    Committed,
    RolledBack,
    Frozen,
    Unknown,
}

// ── Attempt Outcome ──

#[derive(Debug, Clone)]
pub enum AttemptDecision {
    Candidate(CandidateLoopDecision),
    NoCandidate(NoCandidateLoopDecision),
}

#[derive(Debug, Clone)]
pub struct AttemptOutcome {
    pub attempt_id: String,
    pub decision: AttemptDecision,
    pub transition: TransitionOutcome,
    pub task_outcome: TaskOutcome,
    pub lifecycle_state: LifecycleState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskOutcome {
    Success,
    Failed,
    Incomplete,
    EnvironmentError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    Running,
    Continuing,
    Committed,
    RolledBack,
    Escalated,
    Frozen,
}
// Sequence: LoopDirective → apply_transition → TransitionReceipt → AttemptOutcome
// NOT: LoopDecision → AttemptOutcome → Runtime execute
