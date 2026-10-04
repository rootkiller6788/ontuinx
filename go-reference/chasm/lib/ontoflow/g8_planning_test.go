package ontoflow

import (
	"context"
	"testing"
)

// ── G8 Acceptance Tests ──
//
// G8-1: Valid plan → passes validation → converted to FlowSpec
// G8-2: Cyclic plan → rejected
// G8-3: Too many nodes → rejected
// G8-4: Unknown dependency → rejected
// G8-5: Empty plan → rejected
// G8-6: ValidatePlanFromLoop with non-committed envelope → rejected
// G8-7: Generated FlowSpec preserves DAG structure

// ── G8-1: Valid plan passes validation ──

func TestG8_1_ValidPlan(t *testing.T) {
	plan := &WorkPlan{
		PlanID: "p1",
		Nodes: []WorkPlanNode{
			{NodeID: "A", TaskSpecRef: "spec/A", ContractRef: "contract/A", DependsOn: []string{}},
			{NodeID: "B", TaskSpecRef: "spec/B", ContractRef: "contract/B", DependsOn: []string{"A"}},
			{NodeID: "C", TaskSpecRef: "spec/C", ContractRef: "contract/C", DependsOn: []string{"A"}},
			{NodeID: "D", TaskSpecRef: "spec/D", ContractRef: "contract/D", DependsOn: []string{"B", "C"}},
		},
		MaxConcurrency: 2,
	}

	v := NewWorkPlanValidator(10)
	result := v.Validate(plan)
	if !result.Valid {
		t.Errorf("valid plan should pass: %v", result.Reasons)
	}

	// Convert to FlowSpec
	spec, err := ApplyPlan(plan, "flow-g8-1", BudgetSpec{MaxTokens: 1000, MaxCostCents: 100})
	if err != nil {
		t.Fatalf("ApplyPlan failed: %v", err)
	}
	if len(spec.Nodes) != 4 {
		t.Errorf("expected 4 nodes, got %d", len(spec.Nodes))
	}
	if spec.MaxConcurrency != 2 {
		t.Errorf("expected MaxConcurrency=2, got %d", spec.MaxConcurrency)
	}
}

// ── G8-2: Cyclic plan rejected ──

func TestG8_2_CyclicRejected(t *testing.T) {
	plan := &WorkPlan{
		PlanID: "p2",
		Nodes: []WorkPlanNode{
			{NodeID: "X", TaskSpecRef: "s", ContractRef: "c", DependsOn: []string{"Z"}},
			{NodeID: "Y", TaskSpecRef: "s", ContractRef: "c", DependsOn: []string{"X"}},
			{NodeID: "Z", TaskSpecRef: "s", ContractRef: "c", DependsOn: []string{"Y"}},
		},
	}

	v := NewWorkPlanValidator(10)
	result := v.Validate(plan)
	if result.Valid {
		t.Error("cyclic plan must be rejected")
	}
}

// ── G8-3: Too many nodes rejected ──

func TestG8_3_TooManyNodesRejected(t *testing.T) {
	nodes := make([]WorkPlanNode, 15)
	for i := 0; i < 15; i++ {
		nodes[i] = WorkPlanNode{
			NodeID:      string(rune('A' + i)),
			TaskSpecRef: "s",
			ContractRef: "c",
		}
	}
	plan := &WorkPlan{PlanID: "p3", Nodes: nodes}

	v := NewWorkPlanValidator(10)
	result := v.Validate(plan)
	if result.Valid {
		t.Error("plan with 15 nodes should be rejected (max=10)")
	}
}

// ── G8-4: Unknown dependency rejected ──

func TestG8_4_UnknownDependencyRejected(t *testing.T) {
	plan := &WorkPlan{
		PlanID: "p4",
		Nodes: []WorkPlanNode{
			{NodeID: "A", TaskSpecRef: "s", ContractRef: "c", DependsOn: []string{"NONEXISTENT"}},
		},
	}

	v := NewWorkPlanValidator(10)
	result := v.Validate(plan)
	if result.Valid {
		t.Error("unknown dependency must be rejected")
	}
}

// ── G8-5: Empty plan rejected ──

func TestG8_5_EmptyPlanRejected(t *testing.T) {
	plan := &WorkPlan{PlanID: "p5", Nodes: []WorkPlanNode{}}

	v := NewWorkPlanValidator(10)
	result := v.Validate(plan)
	if result.Valid {
		t.Error("empty plan must be rejected")
	}
}

// ── G8-6: Non-committed planning loop rejected ──

func TestG8_6_NonCommittedLoopRejected(t *testing.T) {
	result := &PlanningLoopResult{
		Envelope: &LoopTerminalEnvelope{
			ReportedTerminalState: TerminalStateEscalated,
		},
		Plan: &WorkPlan{PlanID: "p6", Nodes: []WorkPlanNode{
			{NodeID: "A", TaskSpecRef: "s", ContractRef: "c"},
		}},
	}

	err := ValidatePlanFromLoop(result, 10)
	if err == nil {
		t.Error("escalated planning loop must be rejected")
	}
}

// ── G8-7: ExecutePlan creates a real Flow ──

func TestG8_7_ExecutePlanCreatesFlow(t *testing.T) {
	plan := &WorkPlan{
		PlanID: "p7",
		Nodes: []WorkPlanNode{
			{NodeID: "A", TaskSpecRef: "s/A", ContractRef: "c/A", DependsOn: []string{}},
			{NodeID: "B", TaskSpecRef: "s/B", ContractRef: "c/B", DependsOn: []string{"A"}},
		},
		MaxConcurrency: 2,
	}

	store := NewInMemoryFlowStateStore()
	lib := NewLibrary(store)

	comp, err := ExecutePlan(context.Background(), lib, plan, "parent-flow", BudgetSpec{MaxTokens: 5000, MaxCostCents: 500})
	if err != nil {
		t.Fatalf("ExecutePlan failed: %v", err)
	}
	if comp == nil {
		t.Fatal("component is nil")
	}
	if comp.WorkItemCount() != 2 {
		t.Errorf("expected 2 WorkItems, got %d", comp.WorkItemCount())
	}
	if comp.State.Phase != FlowPhaseRunning {
		t.Errorf("child flow should be Running, got %s", comp.State.Phase)
	}

	// Verify DAG dependency: B depends on A
	wiA, _ := comp.GetActiveWorkItem("A")
	wiB, _ := comp.GetActiveWorkItem("B")
	if wiA.Phase != WIPhaseReady {
		t.Errorf("A (no deps) should be Ready, got %s", wiA.Phase)
	}
	if wiB.Phase != WIPhaseBlocked {
		t.Errorf("B (depends on A) should be Blocked, got %s", wiB.Phase)
	}
}
