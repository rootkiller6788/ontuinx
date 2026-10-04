package ontoflow

import (
    "context"
    "testing"
)

// ── G1 Acceptance Tests ──
//
// G1-1: Library registers, Flow starts, Component initializes
// G1-2: WorkItems created in Blocked → transition to Ready
// G1-3: State persists and recovers after restart
// G1-4: Invalid Flow transitions rejected
// G1-5: Invalid WorkItem transitions rejected
// G1-6: No-dependency nodes become Ready immediately

func simpleFlowSpec() *FlowSpec {
    return &FlowSpec{
        FlowID: "g1-test-flow",
        Nodes: []FlowNode{
            {NodeID: "A", TaskSpecRef: "spec/A", ContractRef: "contract/default", PolicyRef: "policy/default", ResourceClass: "standard", RiskClass: "low"},
            {NodeID: "B", TaskSpecRef: "spec/B", ContractRef: "contract/default", PolicyRef: "policy/default", ResourceClass: "standard", RiskClass: "low"},
        },
        Dependencies: []DependencyEdge{
            {From: "A", To: "B"},
        },
        MaxConcurrency: 2,
        GlobalBudget:   BudgetSpec{MaxTokens: 10000, MaxCostCents: 500},
    }
}

// G1-1: Library registers and Flow starts successfully.
func TestG1_1_LibraryStartFlow(t *testing.T) {
    store := NewInMemoryFlowStateStore()
    lib := NewLibrary(store)

    comp, err := lib.StartFlow(context.Background(), simpleFlowSpec())
    if err != nil {
        t.Fatalf("StartFlow failed: %v", err)
    }
    if comp == nil {
        t.Fatal("component is nil")
    }
    if comp.State.Phase != FlowPhaseRunning {
        t.Errorf("expected Running phase, got %s", comp.State.Phase)
    }
    if comp.WorkItemCount() != 2 {
        t.Errorf("expected 2 WorkItems, got %d", comp.WorkItemCount())
    }
}

// G1-2: WorkItems transition Blocked → Ready.
func TestG1_2_WorkItemBlockedToReady(t *testing.T) {
    store := NewInMemoryFlowStateStore()
    lib := NewLibrary(store)

    comp, _ := lib.StartFlow(context.Background(), simpleFlowSpec())

    // Node A has no dependencies → should be Ready
    wiA, ok := comp.GetActiveWorkItem("A")
    if !ok {
        t.Fatal("WorkItem A not found")
    }
    if wiA.Phase != WIPhaseReady {
        t.Errorf("A should be Ready (no deps), got %s", wiA.Phase)
    }

    // Node B depends on A → should still be Blocked
    wiB, ok := comp.GetActiveWorkItem("B")
    if !ok {
        t.Fatal("WorkItem B not found")
    }
    if wiB.Phase != WIPhaseBlocked {
        t.Errorf("B should be Blocked (depends on A), got %s", wiB.Phase)
    }
}

// G1-3: State persists and recovers after simulated restart.
func TestG1_3_StatePersistenceAndRecovery(t *testing.T) {
    store := NewInMemoryFlowStateStore()
    lib := NewLibrary(store)

    comp, _ := lib.StartFlow(context.Background(), simpleFlowSpec())

    // Simulate restart: create a new library with the same store
    lib2 := NewLibrary(store)
    recovered, err := lib2.RecoverFlow(context.Background(), "g1-test-flow")
    if err != nil {
        t.Fatalf("RecoverFlow failed: %v", err)
    }

    if recovered.State.Phase != FlowPhaseRunning {
        t.Errorf("recovered phase: expected Running, got %s", recovered.State.Phase)
    }
    if recovered.WorkItemCount() != 2 {
        t.Errorf("recovered WorkItem count: expected 2, got %d", recovered.WorkItemCount())
    }

    // Verify WorkItem states are preserved
    wiA, _ := recovered.GetActiveWorkItem("A")
    if wiA.Phase != WIPhaseReady {
        t.Errorf("recovered A: expected Ready, got %s", wiA.Phase)
    }
}

// G1-4: Invalid Flow transitions rejected.
func TestG1_4_InvalidFlowTransitionRejected(t *testing.T) {
    // Completed → Running (not allowed)
    state := &OntoFlowState{FlowID: "test", Phase: FlowPhaseCompleted}
    err := TransitionFlow(state, FlowPhaseRunning)
    if err == nil {
        t.Error("expected error for Completed→Running transition")
    }

    // Created → Completed (skip Running)
    state2 := &OntoFlowState{FlowID: "test2", Phase: FlowPhaseCreated}
    err = TransitionFlow(state2, FlowPhaseCompleted)
    if err == nil {
        t.Error("expected error for Created→Completed transition")
    }
}

// G1-5: Invalid WorkItem transitions rejected.
func TestG1_5_InvalidWorkItemTransitionRejected(t *testing.T) {
    // Committed → Running (terminal can't go back)
    wi := &WorkItemRuntimeState{WorkItemID: "wi-1", Phase: WIPhaseCommitted}
    err := TransitionWorkItem(wi, WIPhaseRunning)
    if err == nil {
        t.Error("expected error for Committed→Running transition")
    }

    // Blocked → Committed (skip Ready, Dispatched, etc.)
    wi2 := &WorkItemRuntimeState{WorkItemID: "wi-2", Phase: WIPhaseBlocked}
    err = TransitionWorkItem(wi2, WIPhaseCommitted)
    if err == nil {
        t.Error("expected error for Blocked→Committed transition")
    }
}

// G1-6: LoopInvocationRequest has correct binding hash.
func TestG1_6_LoopInvocationRequestBindingHash(t *testing.T) {
    spec := simpleFlowSpec()
    node := spec.Nodes[0]
    wi := NewWorkItem(spec.FlowID, node, 1)

    req := BuildLoopInvocationRequest(wi, spec.FlowID)
    if req.SchemaVersion != 1 {
        t.Errorf("expected schema_version 1, got %d", req.SchemaVersion)
    }
    if req.FlowID != spec.FlowID {
        t.Errorf("flow_id mismatch: %s", req.FlowID)
    }
    if req.LoopID != wi.LoopID {
        t.Errorf("loop_id mismatch: %s vs %s", req.LoopID, wi.LoopID)
    }
    if len(req.RequestBindingHash) != 16 {
        t.Errorf("request_binding_hash must be 16 hex chars, got %d", len(req.RequestBindingHash))
    }
    if req.ExecutionGeneration != 1 {
        t.Errorf("expected generation 1, got %d", req.ExecutionGeneration)
    }
}

// G1-7: Duplicate FlowID is rejected.
func TestG1_7_DuplicateFlowIDRejected(t *testing.T) {
    store := NewInMemoryFlowStateStore()
    lib := NewLibrary(store)

    _, err := lib.StartFlow(context.Background(), simpleFlowSpec())
    if err != nil {
        t.Fatalf("first StartFlow failed: %v", err)
    }

    _, err = lib.StartFlow(context.Background(), simpleFlowSpec())
    if err == nil {
        t.Error("expected error for duplicate FlowID")
    }
}

// G1-8: Recovery with nonexistent FlowID fails.
func TestG1_8_RecoverNonexistentFlowFails(t *testing.T) {
    store := NewInMemoryFlowStateStore()
    lib := NewLibrary(store)

    _, err := lib.RecoverFlow(context.Background(), "nonexistent-flow")
    if err == nil {
        t.Error("expected error for nonexistent flow recovery")
    }
}
