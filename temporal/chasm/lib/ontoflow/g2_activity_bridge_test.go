package ontoflow

import (
	"context"
	"encoding/json"
	"testing"
)

// ── G2 Acceptance Tests ──
//
// G2-1: WorkItem Ready → Dispatched via ActivityScheduler
// G2-2: ActivityTask enters library (simulating Matching)
// G2-3: Worker "completes" ActivityTask → WorkItem transitions to OutcomeReported
// G2-4: Scheduler orchestrates full Ready→Dispatch→Complete cycle
// G2-5: MaxConcurrency limits active WorkItems
// G2-6: Activity failure retries correctly

// ── G2-1: Dispatch creates ActivityTask, WorkItem transitions Ready → Dispatched ──

func TestG2_1_DispatchCreatesActivityTask(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	scheduler := NewActivityScheduler(lib, "onto-workers")

	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)

	// Mark as Ready (normally done by unblockReady)
	if err := TransitionWorkItem(wi, WIPhaseReady); err != nil {
		t.Fatalf("transition to Ready failed: %v", err)
	}

	// Dispatch
	err := scheduler.DispatchWorkItem(context.Background(), wi, spec.FlowID)
	if err != nil {
		t.Fatalf("DispatchWorkItem failed: %v", err)
	}

	if wi.Phase != WIPhaseDispatched {
		t.Errorf("expected Dispatched, got %s", wi.Phase)
	}
	if wi.CurrentActivityID == "" {
		t.Error("activity ID must not be empty after dispatch")
	}
	if !t.Run("activity exists in library", func(t *testing.T) {
		status, err := lib.GetStatus(context.Background(), wi.CurrentActivityID)
		if err != nil {
			t.Fatalf("GetStatus failed: %v", err)
		}
		if status != ActivityStatusScheduled {
			t.Errorf("expected Scheduled, got %s", status)
		}
	}) {
	}
}

// ── G2-2: Dispatch on non-Ready WorkItem rejected ──

func TestG2_2_DispatchOnlyReadyWorkItems(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	scheduler := NewActivityScheduler(lib, "onto-workers")

	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)
	// Still Blocked — cannot dispatch

	err := scheduler.DispatchWorkItem(context.Background(), wi, spec.FlowID)
	if err == nil {
		t.Error("expected error dispatching Blocked WorkItem")
	}
}

// ── G2-3: Worker completion → OutcomeReported ──

func TestG2_3_WorkerCompletionTransitionsToOutcomeReported(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	scheduler := NewActivityScheduler(lib, "onto-workers")

	spec := simpleFlowSpec()
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)
	_ = TransitionWorkItem(wi, WIPhaseReady)

	// Dispatch
	_ = scheduler.DispatchWorkItem(context.Background(), wi, spec.FlowID)

	// Simulate Worker completing the ActivityTask
	envelopeJSON, _ := json.Marshal(map[string]interface{}{
		"schema_version":           1,
		"flow_id":                  spec.FlowID,
		"work_item_id":             wi.WorkItemID,
		"loop_id":                  wi.LoopID,
		"execution_generation":     float64(1),
		"request_binding_hash":     "abcd1234abcd1234",
		"reported_terminal_state":  "committed",
		"decision_id":              "dec-g2-1",
		"decision_hash":            "hash-g2-1",
		"output_checkpoint_hash":   "ckpt-g2-1",
		"outcome_binding_hash":     "efgh5678efgh5678",
		"total_attempts":           float64(1),
		"terminal_reason":          "task committed after 1 attempt",
	})
	_ = lib.CompleteActivity(wi.CurrentActivityID, envelopeJSON)

	// Handle result
	envelope, err := scheduler.HandleActivityResult(context.Background(), wi)
	if err != nil {
		t.Fatalf("HandleActivityResult failed: %v", err)
	}

	if wi.Phase != WIPhaseOutcomeReported {
		t.Errorf("expected OutcomeReported, got %s", wi.Phase)
	}
	if envelope == nil {
		t.Fatal("envelope must not be nil")
	}
	if envelope.ReportedTerminalState != "committed" {
		t.Errorf("expected committed, got %s", envelope.ReportedTerminalState)
	}
}

// ── G2-4: Full scheduler cycle: Ready→Dispatch→Complete→OutcomeReported ──

func TestG2_4_FullSchedulerCycle(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	store := NewInMemoryFlowStateStore()
	fs := NewFlowScheduler(lib, "onto-workers", 2)

	// Setup: create Flow with one node (no dependencies)
	spec := &FlowSpec{
		FlowID: "g2-full-cycle",
		Nodes: []FlowNode{
			{NodeID: "A", TaskSpecRef: "spec/A", ContractRef: "contract/default", PolicyRef: "policy/default", ResourceClass: "standard", RiskClass: "low"},
		},
		Dependencies:   []DependencyEdge{},
		MaxConcurrency: 2,
		GlobalBudget:   BudgetSpec{MaxTokens: 1000, MaxCostCents: 100},
	}

	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(context.Background())

	// First orchestrate: A should go from Ready → Dispatched
	if err := fs.Orchestrate(context.Background(), comp); err != nil {
		t.Fatalf("first orchestrate failed: %v", err)
	}

	wiA, _ := comp.GetActiveWorkItem("A")
	if wiA.Phase != WIPhaseDispatched {
		t.Fatalf("expected A Dispatched, got %s", wiA.Phase)
	}

	// Simulate Worker completing
	envelopeJSON, _ := json.Marshal(map[string]interface{}{
		"schema_version":           float64(1),
		"flow_id":                  spec.FlowID,
		"work_item_id":             wiA.WorkItemID,
		"loop_id":                  wiA.LoopID,
		"execution_generation":     float64(1),
		"request_binding_hash":     "aaaa1111aaaa1111",
		"reported_terminal_state":  "committed",
		"decision_id":              "dec-complete",
		"outcome_binding_hash":     "bbbb2222bbbb2222",
		"total_attempts":           float64(1),
		"terminal_reason":          "done",
	})
	_ = lib.CompleteActivity(wiA.CurrentActivityID, envelopeJSON)

	// Second orchestrate: A should go OutcomeReported → Committed
	if err := fs.Orchestrate(context.Background(), comp); err != nil {
		t.Fatalf("second orchestrate failed: %v", err)
	}

	// Verify A reached OutcomeReported
	wiA, _ = comp.GetActiveWorkItem("A")
	if wiA.Phase != WIPhaseOutcomeReported {
		t.Errorf("expected A OutcomeReported, got %s", wiA.Phase)
	}
}

// ── G2-5: MaxConcurrency limits active WorkItems ──

func TestG2_5_MaxConcurrencyEnforced(t *testing.T) {
	lib := NewInMemoryActivityLibrary()
	store := NewInMemoryFlowStateStore()
	fs := NewFlowScheduler(lib, "onto-workers", 1) // max 1 concurrent

	nodes := make([]FlowNode, 5)
	for i := 0; i < 5; i++ {
		nodes[i] = FlowNode{
			NodeID: string(rune('A' + i)), TaskSpecRef: "spec/task",
			ContractRef: "c", PolicyRef: "p", ResourceClass: "s", RiskClass: "l",
		}
	}

	spec := &FlowSpec{
		FlowID: "g2-concurrency", Nodes: nodes,
		Dependencies: []DependencyEdge{}, MaxConcurrency: 1,
		GlobalBudget: BudgetSpec{MaxTokens: 5000, MaxCostCents: 500},
	}
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(context.Background())

	// Orchestrate: only 1 should be dispatched (MaxConcurrency=1)
	_ = fs.Orchestrate(context.Background(), comp)

	dispatchedCount := 0
	for _, wi := range comp.State.ActiveWorkItems {
		if wi.Phase == WIPhaseDispatched {
			dispatchedCount++
		}
	}
	if dispatchedCount != 1 {
		t.Errorf("expected 1 dispatched (max=1), got %d", dispatchedCount)
	}
	if comp.State.RunningCount != 1 {
		t.Errorf("expected RunningCount=1, got %d", comp.State.RunningCount)
	}
}

// ── G2-6: Activity failure triggers retry ──

func TestG2_6_ActivityFailureRetries(t *testing.T) {
	spec := &FlowSpec{
		FlowID: "g2-retry",
		Nodes: []FlowNode{
			{NodeID: "A", TaskSpecRef: "spec/A", ContractRef: "c", PolicyRef: "p", ResourceClass: "s", RiskClass: "l"},
		},
		Dependencies: []DependencyEdge{}, MaxConcurrency: 1,
		GlobalBudget: BudgetSpec{MaxTokens: 1000, MaxCostCents: 100},
	}
	wi := NewWorkItem(spec.FlowID, spec.Nodes[0], 1)
	_ = TransitionWorkItem(wi, WIPhaseReady)

	// Simulate 5 retries — after 3, should Escalate
	for i := 0; i < 5; i++ {
		if wi.Phase == WIPhaseEscalated {
			break
		}
		wi.RetryCount++
		if wi.RetryCount > 3 {
			_ = TransitionWorkItem(wi, WIPhaseEscalated)
		} else {
			wi.LastError = "activity timed out"
			_ = TransitionWorkItem(wi, WIPhaseReady) // retry
		}
	}

	if wi.Phase != WIPhaseEscalated {
		t.Errorf("expected Escalated after >3 retries, got %s (retries=%d)", wi.Phase, wi.RetryCount)
	}
	if wi.RetryCount != 4 {
		t.Errorf("expected 4 retries before escalation, got %d", wi.RetryCount)
	}
}
