package ontoflow

import (
	"context"
	"fmt"
)

// ── Outcome Handler ──
//
// G3: Processes LoopTerminalEnvelope from Rust Worker.
// Validates the envelope against the original request before accepting.
// Marks OutcomeReported — NOT Committed (that's G4 Authority Resolution).

// OutcomeHandler validates and processes Worker reports.
type OutcomeHandler struct {
	// In production: holds a reference to the original request for validation.
}

// NewOutcomeHandler creates a new outcome handler.
func NewOutcomeHandler() *OutcomeHandler {
	return &OutcomeHandler{}
}

// ValidateEnvelope checks the Worker's LoopTerminalEnvelope against the
// original LoopInvocationRequest that was sent.
//
// Returns nil if the envelope is structurally valid. This does NOT mean
// the outcome is Committed — that requires G4 Authority verification.
func (h *OutcomeHandler) ValidateEnvelope(
	req *LoopInvocationRequest,
	env *LoopTerminalEnvelope,
) error {
	// 1. Identity match
	if env.FlowID != req.FlowID {
		return fmt.Errorf("flow_id mismatch: sent %s, got %s", req.FlowID, env.FlowID)
	}
	if env.WorkItemID != req.WorkItemID {
		return fmt.Errorf("work_item_id mismatch: sent %s, got %s", req.WorkItemID, env.WorkItemID)
	}
	if env.LoopID != req.LoopID {
		return fmt.Errorf("loop_id mismatch: sent %s, got %s", req.LoopID, env.LoopID)
	}

	// 2. Generation match — prevents old envelopes from being applied to new generations
	if env.ExecutionGeneration != req.ExecutionGeneration {
		return fmt.Errorf("generation mismatch: sent %d, got %d",
			req.ExecutionGeneration, env.ExecutionGeneration)
	}

	// 3. Request binding hash — Rust must return the hash unchanged
	if env.RequestBindingHash != req.RequestBindingHash {
		return fmt.Errorf("request_binding_hash mismatch: sent %s, got %s",
			req.RequestBindingHash, env.RequestBindingHash)
	}

	// 4. Schema version check
	if env.SchemaVersion != req.SchemaVersion {
		return fmt.Errorf("schema_version mismatch: sent %d, got %d",
			req.SchemaVersion, env.SchemaVersion)
	}

	// 5. Terminal state validity
	if !isValidTerminalState(env.ReportedTerminalState) {
		return fmt.Errorf("unknown terminal state: %s", env.ReportedTerminalState)
	}

	// 6. If Committed, must have decision_id
	if env.ReportedTerminalState == TerminalStateCommitted && env.DecisionID == nil {
		return fmt.Errorf("committed but no decision_id")
	}

	return nil
}

// ApplyOutcome validates the envelope and transitions the WorkItem.
//
// IMPORTANT: This transitions to OutcomeReported, NOT Committed.
// G4 Authority Resolution is required before marking Committed.
func (h *OutcomeHandler) ApplyOutcome(
	ctx context.Context,
	req *LoopInvocationRequest,
	env *LoopTerminalEnvelope,
	wi *WorkItemRuntimeState,
) error {
	// Validate first
	if err := h.ValidateEnvelope(req, env); err != nil {
		return fmt.Errorf("validate envelope for %s: %w", wi.WorkItemID, err)
	}

	// Store envelope metadata
	wi.TerminalEnvelopeHash = env.OutcomeBindingHash
	if env.DecisionID != nil {
		wi.DecisionID = *env.DecisionID
	}

	// Transition to OutcomeReported (NOT Committed!)
	if wi.Phase != WIPhaseDispatched && wi.Phase != WIPhaseRunning {
		return &TransitionError{
			wi.WorkItemID, string(wi.Phase), string(WIPhaseOutcomeReported),
			"can only apply outcome to dispatched/running WorkItem",
		}
	}

	return TransitionWorkItem(wi, WIPhaseOutcomeReported)
}

// ── Terminal State Validation ──

var validTerminalStates = map[string]bool{
	TerminalStateCommitted:           true,
	TerminalStateEscalated:           true,
	TerminalStateEnvironmentBlocked:  true,
	TerminalStateLoopBudgetExhausted: true,
	TerminalStateCancelled:           true,
	TerminalStateProtocolFailed:      true,
}

func isValidTerminalState(state string) bool {
	return validTerminalStates[state]
}

// ── Multi-Attempt Summary ──

// AttemptSummary provides a human-readable summary of the loop execution.
type AttemptSummary struct {
	LoopID         string
	TotalAttempts  uint32
	TerminalState  string
	DecisionID     string
	IsCommitted    bool
}

// SummarizeEnvelope extracts key information from the envelope for logging/UI.
func SummarizeEnvelope(env *LoopTerminalEnvelope) AttemptSummary {
	decID := ""
	if env.DecisionID != nil {
		decID = *env.DecisionID
	}
	return AttemptSummary{
		LoopID:        env.LoopID,
		TotalAttempts: env.TotalAttempts,
		TerminalState: env.ReportedTerminalState,
		DecisionID:    decID,
		IsCommitted:   env.ReportedTerminalState == TerminalStateCommitted,
	}
}

// ── Rust-Side Golden Test Helpers ──
//
// These functions mirror the Rust golden vector tests to ensure Go and Rust
// produce identical results for the same inputs.

// GoldenEnvelopeCommitted returns the canonical Committed envelope for testing.
func GoldenEnvelopeCommitted(flowID, workItemID, loopID string, generation uint64, requestBindingHash string, totalAttempts uint32) *LoopTerminalEnvelope {
	decID := fmt.Sprintf("dec-%s", loopID)
	decHash := fmt.Sprintf("hash-%s", loopID)
	ckptHash := fmt.Sprintf("ckpt-%s", loopID)
	receiptRef := fmt.Sprintf("receipt-%s", loopID)

	return &LoopTerminalEnvelope{
		SchemaVersion:         1,
		FlowID:                flowID,
		WorkItemID:            workItemID,
		LoopID:                loopID,
		ExecutionGeneration:   generation,
		RequestBindingHash:    requestBindingHash,
		ReportedTerminalState: TerminalStateCommitted,
		DecisionID:            &decID,
		DecisionHash:          &decHash,
		EvidenceBundleRef:     nil,
		SettlementReceiptRef:  &receiptRef,
		OutputArtifactRefs:    []string{fmt.Sprintf("artifact/output-%s", loopID)},
		OutputCheckpointHash:  &ckptHash,
		OutcomeBindingHash:    "golden-outcome-hash",
		TotalAttempts:         totalAttempts,
		TerminalReason:        fmt.Sprintf("task committed after %d attempt(s)", totalAttempts),
	}
}
