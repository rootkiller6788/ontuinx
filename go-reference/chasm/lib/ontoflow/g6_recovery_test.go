package ontoflow

import (
	"context"
	"encoding/json"
	"testing"
)

// ── G6 Acceptance Tests ──
//
// G6-1: Worker crash → re-dispatch with same loop_id, not create second loop
// G6-2: Activity retry reuses same generation (no new OntoLoop)
// G6-3: Commit response lost → re-Respond returns same envelope
// G6-4: Duplicate Activity completion rejected
// G6-5: Old envelope rejected for new generation
// G6-6: Terminal WorkItem cannot be re-executed
// G6-7: Side effects not duplicated on committed WorkItem

// ── G6-1: Worker crash → re-dispatch with same loop_id ──

func TestG6_1_ReDispatchSameLoopID(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	rm := NewRecoveryManager(NewInMemoryFlowStateStore(), lib)

	// A WorkItem was dispatched but Worker crashed before completing
	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)
	originalLoopID := wi.LoopID

	_ = TransitionWorkItem(wi, WIPhaseReady)
	_ = TransitionWorkItem(wi, WIPhaseDispatched)

	// Simulate crash: WorkItem found in Dispatched phase with no activity ID
	wi.CurrentActivityID = ""

	recovered, err := rm.RecoverWorkItem(context.Background(), wi, spec.FlowID)
	if err != nil {
		t.Fatalf("RecoverWorkItem failed: %v", err)
	}

	// Must be Ready for re-dispatch, NOT a new loop_id
	if recovered.Phase != WIPhaseReady {
		t.Errorf("expected Ready for re-dispatch, got %s", recovered.Phase)
	}
	if recovered.LoopID != originalLoopID {
		t.Errorf("loop_id must not change: was %s, got %s", originalLoopID, recovered.LoopID)
	}
}

// ── G6-2: Activity retry does not create new OntoLoop (same generation) ──

func TestG6_2_RetrySameGeneration(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	scheduler := NewActivityScheduler(lib, "onto-workers")
	rm := NewRecoveryManager(NewInMemoryFlowStateStore(), lib)

	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)
	originalGen := wi.ExecutionGeneration

	_ = TransitionWorkItem(wi, WIPhaseReady)
	_ = scheduler.DispatchWorkItem(context.Background(), wi, spec.FlowID)

	// Simulate Activity timed out
	_ = TransitionWorkItem(wi, WIPhaseRunning)

	// Recovery: should retry with same generation
	recovered, err := rm.RecoverWorkItem(context.Background(), wi, spec.FlowID)
	if err != nil {
		t.Fatalf("RecoverWorkItem failed: %v", err)
	}

	if recovered.ExecutionGeneration != originalGen {
		t.Errorf("generation must stay %d on retry, got %d", originalGen, recovered.ExecutionGeneration)
	}
}

// ── G6-3: Commit response lost → cached envelope returned ──

func TestG6_3_CommitResponseLost(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	rm := NewRecoveryManager(NewInMemoryFlowStateStore(), lib)

	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)

	_ = TransitionWorkItem(wi, WIPhaseOutcomeReported)
	wi.TerminalEnvelopeHash = "cached-envelope-hash"
	wi.DecisionID = "dec-cached"

	// Recovery: OutcomeReported → re-trigger Authority (nothing to replay)
	recovered, err := rm.RecoverWorkItem(context.Background(), wi, spec.FlowID)
	if err != nil {
		t.Fatalf("RecoverWorkItem failed: %v", err)
	}

	if recovered.Phase != WIPhaseOutcomeReported {
		t.Errorf("OutcomeReported should stay, got %s", recovered.Phase)
	}
	if recovered.DecisionID != "dec-cached" {
		t.Error("DecisionID must be preserved across recovery")
	}
}

// ── G6-4: Duplicate Activity completion rejected ──

func TestG6_4_DuplicateCompletionRejected(t *testing.T) {
	// Already Committed → cannot complete again
	wi := &WorkItemRuntimeState{
		WorkItemID: "wi-dup",
		Phase:      WIPhaseCommitted,
	}
	err := ValidateNoDuplicateCompletion(wi)
	if err == nil {
		t.Error("duplicate completion on Committed must be rejected")
	}

	// Already OutcomeReported → cannot complete again
	wi.Phase = WIPhaseOutcomeReported
	err = ValidateNoDuplicateCompletion(wi)
	if err == nil {
		t.Error("duplicate completion on OutcomeReported must be rejected")
	}
}

// ── G6-5: Old envelope on new generation rejected ──

func TestG6_5_OldEnvelopeNewGeneration(t *testing.T) {
	envelope := &LoopTerminalEnvelope{
		ExecutionGeneration: 1,
	}

	err := ValidateGeneration(envelope, 2)
	if err == nil {
		t.Error("old generation envelope must be rejected for new generation")
	}

	err = ValidateGeneration(envelope, 1)
	if err != nil {
		t.Errorf("matching generation should pass: %v", err)
	}
}

// ── G6-6: Terminal WorkItem cannot be re-executed ──

func TestG6_6_TerminalCannotReExecute(t *testing.T) {
	req := &LoopInvocationRequest{
		WorkItemID: "wi-term", LoopID: "loop-term",
		ExecutionGeneration: 1,
	}

	existing := &WorkItemRuntimeState{
		WorkItemID: "wi-term", LoopID: "loop-term",
		Phase: WIPhaseCommitted, ExecutionGeneration: 1,
	}

	err := ValidateIdempotency(existing, req)
	if err == nil {
		t.Error("terminal WorkItem must not be re-executed")
	}
}

// ── G6-7: Side effects not duplicated ──

func TestG6_7_NoDuplicateSideEffects(t *testing.T) {
	wi := &WorkItemRuntimeState{
		WorkItemID: "wi-se", Phase: WIPhaseCommitted,
	}
	err := ValidateNoDuplicateSideEffects(wi)
	if err == nil {
		t.Error("committed WorkItem must reject duplicate side effects")
	}

	// Non-committed should pass
	wi.Phase = WIPhaseOutcomeReported
	err = ValidateNoDuplicateSideEffects(wi)
	if err != nil {
		t.Errorf("OutcomeReported should allow (not yet committed): %v", err)
	}
}

// ── G6-8: Envelope metadata preserved through recovery ──

func TestG6_8_EnvelopeMetadataPreserved(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	rm := NewRecoveryManager(NewInMemoryFlowStateStore(), lib)

	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)

	// Simulate: Activity completed, result stored in library
	_ = TransitionWorkItem(wi, WIPhaseReady)
	_ = TransitionWorkItem(wi, WIPhaseDispatched)
	wi.CurrentActivityID = "activity-recovery-1"

	envJSON, _ := json.Marshal(map[string]interface{}{
		"schema_version":          float64(1),
		"flow_id":                 spec.FlowID,
		"work_item_id":            wi.WorkItemID,
		"loop_id":                 wi.LoopID,
		"execution_generation":    float64(1),
		"request_binding_hash":    "req-hash-1",
		"reported_terminal_state": "committed",
		"decision_id":             "dec-recovery",
		"decision_hash":           "hash-recovery",
		"outcome_binding_hash":    "outcome-hash-1",
		"total_attempts":          float64(1),
		"terminal_reason":         "done",
	})
	lib.CompleteActivity("activity-recovery-1", envJSON)

	// Recovery discovers completed activity
	recovered, err := rm.RecoverWorkItem(context.Background(), wi, spec.FlowID)
	if err != nil {
		t.Fatalf("RecoverWorkItem failed: %v", err)
	}

	if recovered.Phase != WIPhaseOutcomeReported {
		t.Errorf("expected OutcomeReported after recovery, got %s", recovered.Phase)
	}
	if recovered.TerminalEnvelopeHash != "outcome-hash-1" {
		t.Errorf("envelope hash not preserved: got %s", recovered.TerminalEnvelopeHash)
	}
	if recovered.DecisionID != "dec-recovery" {
		t.Errorf("decision_id not preserved: got %s", recovered.DecisionID)
	}
}
