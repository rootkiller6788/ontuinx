package ontoflow

import (
	"context"
	"fmt"
)

// ── Durable Recovery ──
//
// G6: Handles Worker crashes, Activity timeouts, duplicate completions,
// and Temporal Server restarts. Recovery must be completed before G7 batch.

// ── Recovery Manager ──

// RecoveryManager handles crash recovery for OntoFlow executions.
type RecoveryManager struct {
	store       FlowStateStore
	activityLib ActivityLibrary
}

// NewRecoveryManager creates a recovery manager.
func NewRecoveryManager(store FlowStateStore, lib ActivityLibrary) *RecoveryManager {
	return &RecoveryManager{store: store, activityLib: lib}
}

// RecoverFlow restores a Flow after Temporal Server restart.
func (rm *RecoveryManager) RecoverFlow(ctx context.Context, flowID string) (*OntoFlowState, error) {
	state, err := rm.store.Load(ctx, flowID)
	if err != nil {
		return nil, fmt.Errorf("recover load state: %w", err)
	}
	return state, nil
}

// RecoverWorkItem handles recovery for a single WorkItem after crash/restart.
//
// Scenarios handled:
// - Dispatched/Running with active Activity → query status, resume or retry
// - OutcomeReported → re-trigger Authority verification
// - Already Committed/Escalated → no-op (idempotent)
func (rm *RecoveryManager) RecoverWorkItem(
	ctx context.Context,
	wi *WorkItemRuntimeState,
	flowID string,
) (*WorkItemRuntimeState, error) {
	switch wi.Phase {
	case WIPhaseDispatched, WIPhaseRunning:
		return rm.recoverInFlight(ctx, wi, flowID)

	case WIPhaseOutcomeReported:
		// Envelope was received but Authority hasn't verified yet.
		// Re-trigger verification in G4 cycle.
		return wi, nil

	case WIPhaseAuthorityVerifying:
		// Was in the middle of verification. Try again.
		return wi, nil

	case WIPhaseCommitted, WIPhaseEscalated, WIPhaseCancelled, WIPhaseBudgetExhausted:
		// Already terminal — nothing to recover
		return wi, nil

	default:
		return wi, nil
	}
}

// recoverInFlight handles WorkItems that were Dispatched or Running at crash time.
func (rm *RecoveryManager) recoverInFlight(
	ctx context.Context,
	wi *WorkItemRuntimeState,
	flowID string,
) (*WorkItemRuntimeState, error) {
	if wi.CurrentActivityID == "" {
		// No activity was scheduled yet — transition back to Ready for re-dispatch
		_ = TransitionWorkItem(wi, WIPhaseReady)
		return wi, nil
	}

	// Query the Activity status
	status, err := rm.activityLib.GetStatus(ctx, wi.CurrentActivityID)
	if err != nil {
		// Activity not found — may have been lost. Re-dispatch with same loop_id.
		wi.CurrentActivityID = ""
		_ = TransitionWorkItem(wi, WIPhaseReady)
		wi.LastError = fmt.Sprintf("activity lost, re-dispatching: %v", err)
		return wi, nil
	}

	switch status {
	case ActivityStatusCompleted:
		// Activity completed but we missed the response.
		// Retrieve result and transition to OutcomeReported.
		result, err := rm.activityLib.GetResult(ctx, wi.CurrentActivityID)
		if err != nil {
			return nil, fmt.Errorf("recover get result for %s: %w", wi.WorkItemID, err)
		}
		envelope, err := ParseLoopTerminalEnvelope(result)
		if err != nil {
			return nil, fmt.Errorf("recover parse envelope for %s: %w", wi.WorkItemID, err)
		}
		wi.TerminalEnvelopeHash = envelope.OutcomeBindingHash
		if envelope.DecisionID != nil {
			wi.DecisionID = *envelope.DecisionID
		}
		_ = TransitionWorkItem(wi, WIPhaseOutcomeReported)
		return wi, nil

	case ActivityStatusRunning:
		// Still running — wait for completion (heartbeat-based timeout handled by Temporal)
		return wi, nil

	case ActivityStatusFailed, ActivityStatusTimedOut:
		// Activity failed or timed out. Retry with same loop_id + generation.
		wi.CurrentActivityID = ""
		wi.RetryCount++
		if wi.RetryCount > 3 {
			_ = TransitionWorkItem(wi, WIPhaseEscalated)
			wi.LastError = fmt.Sprintf("activity %s after %d retries", status.String(), wi.RetryCount)
		} else {
			_ = TransitionWorkItem(wi, WIPhaseReady)
			wi.LastError = fmt.Sprintf("activity %s, retry %d", status.String(), wi.RetryCount)
		}
		return wi, nil

	default:
		return wi, nil
	}
}

// ── Idempotency Guards ──

// ValidateIdempotency ensures that a re-delivered ActivityTask does not
// create a duplicate OntoLoop execution.
//
// Invariants:
// - Same flow_id + work_item_id + generation → same loop_id
// - Same loop_id → same OntoLoop instance
// - Duplicate scheduling with same loop_id → return cached envelope if terminal
func ValidateIdempotency(existingWI *WorkItemRuntimeState, newReq *LoopInvocationRequest) error {
	// 1. Same WorkItem + generation → must have same loop_id
	if existingWI.WorkItemID == newReq.WorkItemID &&
		existingWI.ExecutionGeneration == newReq.ExecutionGeneration {
		if existingWI.LoopID != newReq.LoopID {
			return fmt.Errorf(
				"idempotency violation: work_item=%s generation=%d has loop_id=%s, requested=%s",
				existingWI.WorkItemID, existingWI.ExecutionGeneration,
				existingWI.LoopID, newReq.LoopID,
			)
		}
	}

	// 2. Already terminal → do not re-execute
	if existingWI.Phase.IsTerminal() {
		return fmt.Errorf(
			"work_item %s already terminal (%s), cannot re-execute",
			existingWI.WorkItemID, existingWI.Phase,
		)
	}

	return nil
}

// ── Duplicate Completion Guard ──

// ValidateNoDuplicateCompletion ensures a completed Activity is not applied twice.
func ValidateNoDuplicateCompletion(wi *WorkItemRuntimeState) error {
	if wi.Phase == WIPhaseOutcomeReported ||
		wi.Phase == WIPhaseAuthorityVerifying ||
		wi.Phase.IsTerminal() {
		return fmt.Errorf(
			"work_item %s already at phase %s — duplicate completion rejected",
			wi.WorkItemID, wi.Phase,
		)
	}
	return nil
}

// ── Side-Effect Idempotency ──

// ValidateNoDuplicateSideEffects ensures committed side effects are not duplicated.
// If OntoAssure already committed (decision exists) but Go hasn't received the
// completion, the recovery path returns the cached outcome.
func ValidateNoDuplicateSideEffects(wi *WorkItemRuntimeState) error {
	if wi.Phase == WIPhaseCommitted {
		return fmt.Errorf(
			"work_item %s already committed — side effects must not be duplicated",
			wi.WorkItemID,
		)
	}
	return nil
}

// ── Generation Validation ──

// ValidateGeneration ensures old envelopes are not applied to new generations.
func ValidateGeneration(envelope *LoopTerminalEnvelope, expectedGen uint64) error {
	if envelope.ExecutionGeneration != expectedGen {
		return fmt.Errorf(
			"generation mismatch: envelope has gen=%d, expected gen=%d — old envelope rejected",
			envelope.ExecutionGeneration, expectedGen,
		)
	}
	return nil
}
