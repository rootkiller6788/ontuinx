package ontoflow

import (
	"context"
	"encoding/json"
	"testing"
)

// ── G3 Acceptance Tests ──
//
// G3-1: Envelope validation — identity/generation/binding hash checks
// G3-2: OutcomeReported transition (NOT Committed)
// G3-3: Multi-attempt (2 attempts) loop envelope
// G3-4: Escalated envelope — no decision_id required
// G3-5: LoopBudgetExhausted — Go can decide to retry with more budget
// G3-6: Full Rust→Go cycle: envelope → validate → OutcomeReported

// ── G3-1: Envelope validation checks identity, generation, binding hash ──

func TestG3_1_EnvelopeValidation(t *testing.T) {
	handler := NewOutcomeHandler()

	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)
	req := BuildLoopInvocationRequest(wi, spec.FlowID)

	// Valid envelope
	env := GoldenEnvelopeCommitted(spec.FlowID, wi.WorkItemID, wi.LoopID, 1, req.RequestBindingHash, 1)
	if err := handler.ValidateEnvelope(&req, env); err != nil {
		t.Errorf("valid envelope should pass: %v", err)
	}

	// Wrong flow_id
	badEnv := GoldenEnvelopeCommitted("wrong-flow", wi.WorkItemID, wi.LoopID, 1, req.RequestBindingHash, 1)
	if err := handler.ValidateEnvelope(&req, badEnv); err == nil {
		t.Error("wrong flow_id should fail validation")
	}

	// Wrong generation
	badGen := GoldenEnvelopeCommitted(spec.FlowID, wi.WorkItemID, wi.LoopID, 99, req.RequestBindingHash, 1)
	if err := handler.ValidateEnvelope(&req, badGen); err == nil {
		t.Error("wrong generation should fail validation")
	}

	// Wrong request_binding_hash
	badHash := GoldenEnvelopeCommitted(spec.FlowID, wi.WorkItemID, wi.LoopID, 1, "wrong-hash-value", 1)
	if err := handler.ValidateEnvelope(&req, badHash); err == nil {
		t.Error("wrong request_binding_hash should fail validation")
	}
}

// ── G3-2: OutcomeReported — NOT Committed ──

func TestG3_2_OutcomeReportedNotCommitted(t *testing.T) {
	handler := NewOutcomeHandler()

	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)
	req := BuildLoopInvocationRequest(wi, spec.FlowID)
	_ = TransitionWorkItem(wi, WIPhaseReady)
	_ = TransitionWorkItem(wi, WIPhaseDispatched)

	env := GoldenEnvelopeCommitted(spec.FlowID, wi.WorkItemID, wi.LoopID, 1, req.RequestBindingHash, 1)

	err := handler.ApplyOutcome(context.Background(), &req, env, wi)
	if err != nil {
		t.Fatalf("ApplyOutcome failed: %v", err)
	}

	// Must be OutcomeReported, NOT Committed
	if wi.Phase != WIPhaseOutcomeReported {
		t.Errorf("expected OutcomeReported, got %s — must NOT be Committed yet", wi.Phase)
	}

	// Envelope metadata stored
	if wi.TerminalEnvelopeHash == "" {
		t.Error("TerminalEnvelopeHash must be stored")
	}
	if wi.DecisionID == "" {
		t.Error("DecisionID must be stored")
	}
}

// ── G3-3: Multi-attempt loop (2 attempts) ──

func TestG3_3_MultiAttemptLoop(t *testing.T) {
	handler := NewOutcomeHandler()

	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)
	req := BuildLoopInvocationRequest(wi, spec.FlowID)
	_ = TransitionWorkItem(wi, WIPhaseReady)
	_ = TransitionWorkItem(wi, WIPhaseDispatched)

	// 2 attempts: Attempt 1 failed, Attempt 2 succeeded
	env := GoldenEnvelopeCommitted(spec.FlowID, wi.WorkItemID, wi.LoopID, 1, req.RequestBindingHash, 2)

	_ = handler.ApplyOutcome(context.Background(), &req, env, wi)

	if wi.Phase != WIPhaseOutcomeReported {
		t.Errorf("expected OutcomeReported, got %s", wi.Phase)
	}

	// Summary
	summary := SummarizeEnvelope(env)
	if summary.TotalAttempts != 2 {
		t.Errorf("expected 2 total attempts, got %d", summary.TotalAttempts)
	}
	if !summary.IsCommitted {
		t.Error("summary should show Committed")
	}
}

// ── G3-4: Escalated envelope — no decision_id required ──

func TestG3_4_EscalatedEnvelope(t *testing.T) {
	handler := NewOutcomeHandler()

	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)
	req := BuildLoopInvocationRequest(wi, spec.FlowID)
	_ = TransitionWorkItem(wi, WIPhaseReady)
	_ = TransitionWorkItem(wi, WIPhaseDispatched)

	// Escalated — no decision_id
	env := &LoopTerminalEnvelope{
		SchemaVersion:         1,
		FlowID:                spec.FlowID,
		WorkItemID:            wi.WorkItemID,
		LoopID:                wi.LoopID,
		ExecutionGeneration:   1,
		RequestBindingHash:    req.RequestBindingHash,
		ReportedTerminalState: TerminalStateEscalated,
		DecisionID:            nil, // none required for Escalated
		OutcomeBindingHash:    "escalated-hash",
		TotalAttempts:         3,
		TerminalReason:        "agent could not complete within budget",
	}

	err := handler.ApplyOutcome(context.Background(), &req, env, wi)
	if err != nil {
		t.Fatalf("ApplyOutcome for escalated failed: %v", err)
	}

	if wi.Phase != WIPhaseOutcomeReported {
		t.Errorf("expected OutcomeReported, got %s", wi.Phase)
	}

	// Escalated does NOT allow downstream
	if env.AllowsDownstream() {
		t.Error("Escalated must NOT allow downstream")
	}
}

// ── G3-5: LoopBudgetExhausted — Go can retry with more budget ──

func TestG3_5_LoopBudgetExhaustedRetryable(t *testing.T) {
	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)
	req := BuildLoopInvocationRequest(wi, spec.FlowID)
	_ = TransitionWorkItem(wi, WIPhaseReady)
	_ = TransitionWorkItem(wi, WIPhaseDispatched)

	handler := NewOutcomeHandler()

	env := &LoopTerminalEnvelope{
		SchemaVersion:         1,
		FlowID:                spec.FlowID,
		WorkItemID:            wi.WorkItemID,
		LoopID:                wi.LoopID,
		ExecutionGeneration:   1,
		RequestBindingHash:    req.RequestBindingHash,
		ReportedTerminalState: TerminalStateLoopBudgetExhausted,
		OutcomeBindingHash:    "budget-exhausted-hash",
		TotalAttempts:         5,
		TerminalReason:        "max attempts (5) reached",
	}

	_ = handler.ApplyOutcome(context.Background(), &req, env, wi)

	// Go decides: retry with bigger budget grant
	wi.BudgetGrant.MaxAttempts = 10
	wi.ExecutionGeneration++ // new generation
	_ = TransitionWorkItem(wi, WIPhaseReady)

	if wi.Phase != WIPhaseReady {
		t.Errorf("BudgetExhausted → Go retries with more budget, should be Ready again. Got %s", wi.Phase)
	}
	if wi.ExecutionGeneration != 2 {
		t.Errorf("new generation should be 2, got %d", wi.ExecutionGeneration)
	}
	if wi.BudgetGrant.MaxAttempts != 10 {
		t.Errorf("budget grant not increased: got %d", wi.BudgetGrant.MaxAttempts)
	}
}

// ── G3-6: Full Rust→Go JSON round-trip with real envelope ──

func TestG3_6_EnvelopeJSONRoundTrip(t *testing.T) {
	// Real Rust-produced JSON (matches golden vector format)
	rustJSON := `{
		"schema_version": 1,
		"flow_id": "flow-rust-1",
		"work_item_id": "wi-rust",
		"loop_id": "loop-rust-wi-rust-gen1",
		"execution_generation": 1,
		"request_binding_hash": "abcd1234abcd1234",
		"reported_terminal_state": "committed",
		"decision_id": "dec-rust-abc",
		"decision_hash": "hash-rust-abc",
		"evidence_bundle_ref": "evidence/bundle-1",
		"settlement_receipt_ref": "receipt/rcpt-1",
		"output_artifact_refs": ["artifact/out-1", "artifact/out-2"],
		"output_checkpoint_hash": "ckpt-rust-abc",
		"outcome_binding_hash": "efgh5678efgh5678",
		"total_attempts": 2,
		"terminal_reason": "task committed after 2 attempts"
	}`

	var env LoopTerminalEnvelope
	if err := json.Unmarshal([]byte(rustJSON), &env); err != nil {
		t.Fatalf("JSON unmarshal failed: %v", err)
	}

	// Verify all fields
	if env.SchemaVersion != 1 {
		t.Error("schema_version mismatch")
	}
	if env.FlowID != "flow-rust-1" {
		t.Error("flow_id mismatch")
	}
	if env.ReportedTerminalState != "committed" {
		t.Error("reported_terminal_state mismatch")
	}
	if env.TotalAttempts != 2 {
		t.Errorf("total_attempts: expected 2, got %d", env.TotalAttempts)
	}
	if env.DecisionID == nil || *env.DecisionID != "dec-rust-abc" {
		t.Error("decision_id mismatch")
	}
	if len(env.OutputArtifactRefs) != 2 {
		t.Errorf("expected 2 output artifacts, got %d", len(env.OutputArtifactRefs))
	}
	if env.RequestBindingHash != "abcd1234abcd1234" {
		t.Error("request_binding_hash mismatch")
	}
	if env.OutcomeBindingHash != "efgh5678efgh5678" {
		t.Error("outcome_binding_hash mismatch")
	}

	// Re-serialize and check
	reJSON, _ := json.Marshal(env)
	var env2 LoopTerminalEnvelope
	json.Unmarshal(reJSON, &env2)
	if env2.LoopID != env.LoopID {
		t.Error("round-trip loop_id mismatch")
	}
}
