package ontoflow

import (
	"context"
	"testing"
)

// ── G5 Acceptance Tests ──
//
// G5-1: Sequence A→B→C — A Committed unlocks B, B Committed unlocks C
// G5-2: A NOT Committed → B stays Blocked
// G5-3: Fan-out: A Committed unlocks both B and C
// G5-4: Fan-in: D only unlocked when both B and C Committed
// G5-5: Escalated node blocks ALL downstream (not just direct)
// G5-6: Cycle detection rejects invalid DAG
// G5-7: Full sequence: 3 nodes all Committed → Flow Completed

// ── Helpers ──

func sequenceSpec() *FlowSpec {
	return &FlowSpec{
		FlowID: "g5-sequence",
		Nodes: []FlowNode{
			{NodeID: "A", TaskSpecRef: "spec/A", ContractRef: "c", PolicyRef: "p", ResourceClass: "s", RiskClass: "l"},
			{NodeID: "B", TaskSpecRef: "spec/B", ContractRef: "c", PolicyRef: "p", ResourceClass: "s", RiskClass: "l"},
			{NodeID: "C", TaskSpecRef: "spec/C", ContractRef: "c", PolicyRef: "p", ResourceClass: "s", RiskClass: "l"},
		},
		Dependencies: []DependencyEdge{
			{From: "A", To: "B"},
			{From: "B", To: "C"},
		},
		MaxConcurrency: 1,
		GlobalBudget:   BudgetSpec{MaxTokens: 10000, MaxCostCents: 500},
	}
}

func fanOutFanInSpec() *FlowSpec {
	return &FlowSpec{
		FlowID: "g5-fan",
		Nodes: []FlowNode{
			{NodeID: "A", TaskSpecRef: "spec/A", ContractRef: "c", PolicyRef: "p", ResourceClass: "s", RiskClass: "l"},
			{NodeID: "B", TaskSpecRef: "spec/B", ContractRef: "c", PolicyRef: "p", ResourceClass: "s", RiskClass: "l"},
			{NodeID: "C", TaskSpecRef: "spec/C", ContractRef: "c", PolicyRef: "p", ResourceClass: "s", RiskClass: "l"},
			{NodeID: "D", TaskSpecRef: "spec/D", ContractRef: "c", PolicyRef: "p", ResourceClass: "s", RiskClass: "l"},
		},
		Dependencies: []DependencyEdge{
			{From: "A", To: "B"},
			{From: "A", To: "C"},
			{From: "B", To: "D"},
			{From: "C", To: "D"},
		},
		MaxConcurrency: 3,
		GlobalBudget:   BudgetSpec{MaxTokens: 20000, MaxCostCents: 1000},
	}
}

// ── G5-1: Sequence A→B→C ──

func TestG5_1_SequenceABC(t *testing.T) {
	spec := sequenceSpec()
	g := NewDependencyGraph(spec)

	store := NewInMemoryFlowStateStore()
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(contextBackground())

	// Initial state: A Ready (no deps), B Blocked, C Blocked
	wiA, _ := comp.GetActiveWorkItem("A")
	if wiA.Phase != WIPhaseReady {
		t.Fatalf("A should be Ready (no deps), got %s", wiA.Phase)
	}

	wiB, _ := comp.GetActiveWorkItem("B")
	if wiB.Phase != WIPhaseBlocked || g.UnsatisfiedDeps("B", comp.State) != 1 {
		t.Fatalf("B should be Blocked with 1 unsatisfied dep, phase=%s deps=%d",
			wiB.Phase, g.UnsatisfiedDeps("B", comp.State))
	}

	// Step 1: Commit A → B becomes Ready
	_ = TransitionWorkItem(wiA, WIPhaseReady)
	_ = TransitionWorkItem(wiA, WIPhaseDispatched)
	_ = TransitionWorkItem(wiA, WIPhaseRunning)
	_ = TransitionWorkItem(wiA, WIPhaseOutcomeReported)
	_ = TransitionWorkItem(wiA, WIPhaseAuthorityVerifying)
	_ = TransitionWorkItem(wiA, WIPhaseCommitted)

	comp.unblockReady(contextBackground())

	wiB, _ = comp.GetActiveWorkItem("B")
	if wiB.Phase != WIPhaseReady {
		t.Errorf("B should be Ready after A Committed, got %s", wiB.Phase)
	}

	// Step 2: Commit B → C becomes Ready
	_ = TransitionWorkItem(wiB, WIPhaseReady)
	_ = TransitionWorkItem(wiB, WIPhaseDispatched)
	_ = TransitionWorkItem(wiB, WIPhaseRunning)
	_ = TransitionWorkItem(wiB, WIPhaseOutcomeReported)
	_ = TransitionWorkItem(wiB, WIPhaseAuthorityVerifying)
	_ = TransitionWorkItem(wiB, WIPhaseCommitted)

	comp.unblockReady(contextBackground())

	wiC, _ := comp.GetActiveWorkItem("C")
	if wiC.Phase != WIPhaseReady {
		t.Errorf("C should be Ready after B Committed, got %s", wiC.Phase)
	}
}

// ── G5-2: A NOT Committed → B stays Blocked ──

func TestG5_2_ANotCommittedBBlocked(t *testing.T) {
	spec := sequenceSpec()
	g := NewDependencyGraph(spec)

	store := NewInMemoryFlowStateStore()
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(contextBackground())

	// A Escalated (not Committed)
	wiA, _ := comp.GetActiveWorkItem("A")
	_ = TransitionWorkItem(wiA, WIPhaseReady)
	_ = TransitionWorkItem(wiA, WIPhaseDispatched)
	_ = TransitionWorkItem(wiA, WIPhaseRunning)
	_ = TransitionWorkItem(wiA, WIPhaseOutcomeReported)
	_ = TransitionWorkItem(wiA, WIPhaseAuthorityVerifying)
	_ = TransitionWorkItem(wiA, WIPhaseEscalated)

	comp.unblockReady(contextBackground())

	// B must stay Blocked
	wiB, _ := comp.GetActiveWorkItem("B")
	if wiB.Phase != WIPhaseBlocked {
		t.Errorf("B must stay Blocked when A is Escalated, got %s", wiB.Phase)
	}

	if !g.EscalationBlocksDownstream("A", comp.State) {
		t.Error("escalated A should block downstream B")
	}
}

// ── G5-3: Fan-out: A Committed unlocks both B and C ──

func TestG5_3_FanOut(t *testing.T) {
	spec := fanOutFanInSpec()
	g := NewDependencyGraph(spec)

	if !g.IsFanOut("A") {
		t.Error("A should be a fan-out node (>1 downstream)")
	}

	store := NewInMemoryFlowStateStore()
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(contextBackground())

	// A Ready, B Blocked, C Blocked
	wiA, _ := comp.GetActiveWorkItem("A")
	if wiA.Phase != WIPhaseReady {
		t.Fatalf("A should be Ready, got %s", wiA.Phase)
	}

	// Commit A
	_ = TransitionWorkItem(wiA, WIPhaseCommitted)
	comp.unblockReady(contextBackground())

	// Both B and C should now be Ready
	wiB, _ := comp.GetActiveWorkItem("B")
	wiC, _ := comp.GetActiveWorkItem("C")
	if wiB.Phase != WIPhaseReady {
		t.Errorf("B should be Ready after A Committed, got %s", wiB.Phase)
	}
	if wiC.Phase != WIPhaseReady {
		t.Errorf("C should be Ready after A Committed, got %s", wiC.Phase)
	}
}

// ── G5-4: Fan-in: D only when BOTH B and C Committed ──

func TestG5_4_FanIn(t *testing.T) {
	spec := fanOutFanInSpec()
	g := NewDependencyGraph(spec)

	if !g.IsFanIn("D") {
		t.Error("D should be a fan-in node (>1 upstream dep)")
	}

	store := NewInMemoryFlowStateStore()
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(contextBackground())

	// Commit A
	wiA, _ := comp.GetActiveWorkItem("A")
	_ = TransitionWorkItem(wiA, WIPhaseCommitted)
	comp.unblockReady(contextBackground())

	// Commit B but NOT C
	wiB, _ := comp.GetActiveWorkItem("B")
	_ = TransitionWorkItem(wiB, WIPhaseCommitted)
	comp.unblockReady(contextBackground())

	// D should still be Blocked
	wiD, _ := comp.GetActiveWorkItem("D")
	if wiD.Phase != WIPhaseBlocked {
		t.Errorf("D must be Blocked when only B is Committed (C not), got %s", wiD.Phase)
	}
	if g.UnsatisfiedDeps("D", comp.State) != 1 {
		t.Errorf("D should have 1 unsatisfied dep (C), got %d", g.UnsatisfiedDeps("D", comp.State))
	}

	// Commit C
	wiC, _ := comp.GetActiveWorkItem("C")
	_ = TransitionWorkItem(wiC, WIPhaseCommitted)
	comp.unblockReady(contextBackground())

	// Now D should be Ready
	wiD, _ = comp.GetActiveWorkItem("D")
	if wiD.Phase != WIPhaseReady {
		t.Errorf("D should be Ready after B and C Committed, got %s", wiD.Phase)
	}
}

// ── G5-5: Cycle detection ──

func TestG5_5_CycleDetection(t *testing.T) {
	spec := &FlowSpec{
		FlowID: "g5-cycle",
		Nodes: []FlowNode{
			{NodeID: "A", TaskSpecRef: "s", ContractRef: "c", PolicyRef: "p"},
			{NodeID: "B", TaskSpecRef: "s", ContractRef: "c", PolicyRef: "p"},
			{NodeID: "C", TaskSpecRef: "s", ContractRef: "c", PolicyRef: "p"},
		},
		Dependencies: []DependencyEdge{
			{From: "A", To: "B"},
			{From: "B", To: "C"},
			{From: "C", To: "A"}, // cycle!
		},
		MaxConcurrency: 1,
	}

	g := NewDependencyGraph(spec)
	err := g.ValidateAcyclic()
	if err == nil {
		t.Error("cyclic graph must be rejected")
	}
}

// ── G5-6: Sequence validation ──

func TestG5_6_ValidateSequence(t *testing.T) {
	spec := sequenceSpec()
	g := NewDependencyGraph(spec)

	// Valid sequence
	if err := g.ValidateSequence("A", "B", "C"); err != nil {
		t.Errorf("valid sequence should pass: %v", err)
	}

	// Invalid: B does not depend on C
	if err := g.ValidateSequence("A", "C", "B"); err == nil {
		t.Error("A→C→B should fail because C does not depend on A")
	}
}

// ── G5-7: Full sequence completion → Flow Completed ──

func TestG5_7_FullSequenceFlowCompleted(t *testing.T) {
	spec := sequenceSpec()
	store := NewInMemoryFlowStateStore()
	comp := NewOntoFlowComponent(spec, store)
	_ = comp.Initialize(contextBackground())

	// Run all three nodes to Committed
	for _, nodeID := range []string{"A", "B", "C"} {
		wi, _ := comp.GetActiveWorkItem(nodeID)
		_ = TransitionWorkItem(wi, WIPhaseReady)
		_ = TransitionWorkItem(wi, WIPhaseDispatched)
		_ = TransitionWorkItem(wi, WIPhaseRunning)
		_ = TransitionWorkItem(wi, WIPhaseOutcomeReported)
		_ = TransitionWorkItem(wi, WIPhaseAuthorityVerifying)
		_ = TransitionWorkItem(wi, WIPhaseCommitted)
		comp.unblockReady(contextBackground())
	}

	if !comp.State.AllTerminal() {
		t.Error("all WorkItems should be terminal")
	}
	if comp.State.CommittedCount() != 3 {
		t.Errorf("expected 3 committed, got %d", comp.State.CommittedCount())
	}

	// Flow should be completable
	_ = TransitionFlow(comp.State, FlowPhaseCompleted)
	if comp.State.Phase != FlowPhaseCompleted {
		t.Errorf("expected Flow Completed, got %s", comp.State.Phase)
	}
}

// contextBackground helper for tests
func contextBackground() context.Context {
	return context.Background()
}
