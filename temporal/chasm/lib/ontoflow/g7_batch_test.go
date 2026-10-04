package ontoflow

import (
	"context"
	"encoding/json"
	"testing"
)

// ── G7 Acceptance Tests ──
//
// G7-1: 100 WorkItems with MaxConcurrency=10 → only 10 dispatched at once
// G7-2: One completes → next one fills the slot (conveyor belt)
// G7-3: Failed WorkItems don't block the batch (others continue)
// G7-4: Batch summary correct after all complete
// G7-5: PeakConcurrency never exceeds MaxConcurrency
// G7-6: Escalated count tracked separately in summary

// ── Helpers ──

func batchSpec(count int, maxConcurrency uint32) *FlowSpec {
	nodes := make([]FlowNode, count)
	for i := 0; i < count; i++ {
		nodes[i] = FlowNode{
			NodeID:        string(rune('A' + (i % 26))) + string(rune('0'+i/26)),
			TaskSpecRef:   "spec/task",
			ContractRef:   "c",
			PolicyRef:     "p",
			ResourceClass: "s",
			RiskClass:     "l",
		}
	}
	return &FlowSpec{
		FlowID:         "g7-batch",
		Nodes:          nodes,
		Dependencies:   []DependencyEdge{},
		MaxConcurrency: maxConcurrency,
		GlobalBudget:   BudgetSpec{MaxTokens: 100000, MaxCostCents: 10000},
	}
}

func makeEnvelopeJSON(flowID, wiID, loopID string) []byte {
	env, _ := json.Marshal(map[string]interface{}{
		"schema_version":          float64(1),
		"flow_id":                 flowID,
		"work_item_id":            wiID,
		"loop_id":                 loopID,
		"execution_generation":    float64(1),
		"request_binding_hash":    "req-hash",
		"reported_terminal_state": "committed",
		"decision_id":             "dec-" + wiID,
		"decision_hash":           "hash-" + wiID,
		"outcome_binding_hash":    "out-hash-" + wiID,
		"total_attempts":          float64(1),
		"terminal_reason":         "done",
	})
	return env
}

// ── G7-1: MaxConcurrency=3, 10 nodes → max 3 dispatched at once ──

func TestG7_1_MaxConcurrencyEnforced(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	bs := NewBatchScheduler(lib, "onto-workers", 3)

	spec := batchSpec(10, 3)
	store := NewInMemoryFlowStateStore()
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(context.Background())

	_ = bs.BatchOrchestrate(context.Background(), comp)

	// Only 3 should be Dispatched
	dispatched := countByPhase(comp.State, WIPhaseDispatched)
	if dispatched != 3 {
		t.Errorf("expected 3 dispatched (max=3), got %d", dispatched)
	}
	if comp.State.RunningCount != 3 {
		t.Errorf("expected RunningCount=3, got %d", comp.State.RunningCount)
	}
	if bs.batchMetrics.PeakConcurrency != 3 {
		t.Errorf("expected peak=3, got %d", bs.batchMetrics.PeakConcurrency)
	}

	// Remaining 7 should be Ready
	ready := countByPhase(comp.State, WIPhaseReady)
	if ready != 7 {
		t.Errorf("expected 7 Ready, got %d", ready)
	}
}

// ── G7-2: Complete one → slot filled by next Ready ──

func TestG7_2_ConveyorBelt(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	bs := NewBatchScheduler(lib, "onto-workers", 2)

	spec := batchSpec(5, 2)
	store := NewInMemoryFlowStateStore()
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(context.Background())

	// First orchestrate: 2 dispatched
	_ = bs.BatchOrchestrate(context.Background(), comp)
	if comp.State.RunningCount != 2 {
		t.Fatalf("expected 2 running, got %d", comp.State.RunningCount)
	}

	// Complete first WorkItem
	var firstWI *WorkItemRuntimeState
	for _, wi := range comp.State.ActiveWorkItems {
		if wi.Phase == WIPhaseDispatched {
			firstWI = wi
			break
		}
	}
	// Schedule then complete the activity (so it exists in the library)
	actID, _ := lib.Schedule(context.Background(), "execute_onto_loop", nil)
	firstWI.CurrentActivityID = actID
	lib.CompleteActivity(actID, makeEnvelopeJSON(spec.FlowID, firstWI.WorkItemID, firstWI.LoopID))

	// Second orchestrate: 1 completed → 1 new dispatched
	_ = bs.BatchOrchestrate(context.Background(), comp)

	if comp.State.RunningCount != 2 {
		t.Errorf("slot should be refilled: expected 2 running, got %d", comp.State.RunningCount)
	}

	outcomeReported := countByPhase(comp.State, WIPhaseOutcomeReported)
	if outcomeReported != 1 {
		t.Errorf("expected 1 OutcomeReported (completed), got %d", outcomeReported)
	}
}

// ── G7-3: Failed WorkItems don't block the batch ──

func TestG7_3_FailedDoesNotBlockBatch(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	bs := NewBatchScheduler(lib, "onto-workers", 5)

	spec := batchSpec(10, 5)
	store := NewInMemoryFlowStateStore()
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(context.Background())

	// Dispatch all 5 slots
	_ = bs.BatchOrchestrate(context.Background(), comp)

	// 2 of 5 escalate (fail >3 times)
	escalatedCount := 0
	for _, wi := range comp.State.ActiveWorkItems {
		if wi.Phase == WIPhaseDispatched && escalatedCount < 2 {
			wi.RetryCount = 4
			_ = TransitionWorkItem(wi, WIPhaseEscalated)
			comp.State.RunningCount--
			escalatedCount++
		}
	}

	if escalatedCount != 2 {
		t.Fatalf("expected 2 escalated, got %d", escalatedCount)
	}

	// Next orchestrate: 2 slots freed → 2 new dispatched
	_ = bs.BatchOrchestrate(context.Background(), comp)

	if comp.State.RunningCount != 5 {
		t.Errorf("2 escalated + 2 new = 5 running, got %d", comp.State.RunningCount)
	}
}

// ── G7-4: Batch summary correct after all complete ──

func TestG7_4_BatchSummaryCorrect(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	bs := NewBatchScheduler(lib, "onto-workers", 10)

	spec := batchSpec(5, 10)
	store := NewInMemoryFlowStateStore()
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(context.Background())

	// Commit all 5 directly
	for _, wi := range comp.State.ActiveWorkItems {
		_ = TransitionWorkItem(wi, WIPhaseReady)
		_ = TransitionWorkItem(wi, WIPhaseDispatched)
		_ = TransitionWorkItem(wi, WIPhaseRunning)
		_ = TransitionWorkItem(wi, WIPhaseOutcomeReported)
		_ = TransitionWorkItem(wi, WIPhaseAuthorityVerifying)
		_ = TransitionWorkItem(wi, WIPhaseCommitted)
	}
	comp.State.RunningCount = 0

	bs.updateSummary(comp.State)

	if bs.batchMetrics.TotalCommitted != 5 {
		t.Errorf("expected 5 committed, got %d", bs.batchMetrics.TotalCommitted)
	}
	if bs.IsComplete(comp.State) != true {
		t.Error("batch should be complete")
	}
}

// ── G7-5: PeakConcurrency never exceeds limit ──

func TestG7_5_PeakConcurrencyNeverExceeds(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	bs := NewBatchScheduler(lib, "onto-workers", 4)

	spec := batchSpec(20, 4)
	store := NewInMemoryFlowStateStore()
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(context.Background())

	// Multiple cycles
	for i := 0; i < 5; i++ {
		_ = bs.BatchOrchestrate(context.Background(), comp)
		if comp.State.RunningCount > 4 {
			t.Errorf("cycle %d: running=%d exceeds max=4", i, comp.State.RunningCount)
		}
		// Simulate completions to free slots
		for _, wi := range comp.State.ActiveWorkItems {
			if wi.Phase == WIPhaseDispatched {
				wi.CurrentActivityID = "act-" + wi.WorkItemID
				lib.CompleteActivity("act-"+wi.WorkItemID, makeEnvelopeJSON(spec.FlowID, wi.WorkItemID, wi.LoopID))
			}
		}
	}

	if bs.batchMetrics.PeakConcurrency > 4 {
		t.Errorf("peak=%d exceeds max=4", bs.batchMetrics.PeakConcurrency)
	}
}

// ── G7-6: Escalated count tracked ──

func TestG7_6_EscalatedCountTracked(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	bs := NewBatchScheduler(lib, "onto-workers", 5)

	spec := batchSpec(6, 5)
	store := NewInMemoryFlowStateStore()
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(context.Background())

	// Escalate 2, commit 4
	escalated := 0
	for _, wi := range comp.State.ActiveWorkItems {
		if escalated < 2 {
			_ = TransitionWorkItem(wi, WIPhaseEscalated)
			escalated++
		} else {
			_ = TransitionWorkItem(wi, WIPhaseCommitted)
		}
	}
	comp.State.RunningCount = 0

	bs.updateSummary(comp.State)

	if bs.batchMetrics.TotalEscalated != 2 {
		t.Errorf("expected 2 escalated, got %d", bs.batchMetrics.TotalEscalated)
	}
	if bs.batchMetrics.TotalCommitted != 4 {
		t.Errorf("expected 4 committed, got %d", bs.batchMetrics.TotalCommitted)
	}
}

// ── Helper ──

func countByPhase(state *OntoFlowState, phase WorkItemPhase) int {
	count := 0
	for _, wi := range state.ActiveWorkItems {
		if wi.Phase == phase {
			count++
		}
	}
	return count
}
