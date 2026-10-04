package ontoflow

import (
    "context"
    "crypto/sha256"
    "encoding/hex"
    "fmt"
    "sync"
)

// ── OntoFlowExecutionComponent ──
//
// CHASM root component for an OntoFlow execution. Manages the lifecycle
// of a single FlowSpec → multiple WorkItems → completion.
//
// G1 scope: Create Flow from spec, transition one WorkItem Blocked→Ready.
// Later phases add: Activity dispatch, outcome resolution, DAG unlock, barriers.

type FlowStateStore interface {
    Save(ctx context.Context, state *OntoFlowState) error
    Load(ctx context.Context, flowID string) (*OntoFlowState, error)
}

type OntoFlowExecutionComponent struct {
    mu    sync.Mutex
    Spec  *FlowSpec
    State *OntoFlowState
    Store FlowStateStore
}

// NewOntoFlowComponent creates a new Flow execution component.
func NewOntoFlowComponent(spec *FlowSpec, store FlowStateStore) *OntoFlowExecutionComponent {
    state := &OntoFlowState{
        FlowID:           spec.FlowID,
        Phase:            FlowPhaseCreated,
        FlowSpecRef:      fmt.Sprintf("spec/%s", spec.FlowID),
        GraphHash:        hashFlowSpec(spec),
        ActiveWorkItems:  make(map[string]*WorkItemRuntimeState),
        CompletedSummary: FlowCompletionSummary{},
        MaxConcurrency:   spec.MaxConcurrency,
        RunningCount:     0,
        TotalBudget:      BudgetAmount{Tokens: spec.GlobalBudget.MaxTokens, CostCents: spec.GlobalBudget.MaxCostCents},
    }
    return &OntoFlowExecutionComponent{
        Spec:  spec,
        State: state,
        Store: store,
    }
}

// ── Lifecycle ──

// Initialize transitions FlowPhaseCreated → FlowPhaseRunning and creates
// all WorkItems in Blocked phase.
func (c *OntoFlowExecutionComponent) Initialize(ctx context.Context) error {
    c.mu.Lock()
    defer c.mu.Unlock()

    if c.State.Phase != FlowPhaseCreated {
        return &TransitionError{c.State.FlowID, string(c.State.Phase), string(FlowPhaseRunning), "not in created phase"}
    }

    // Create WorkItems from spec nodes
    for _, node := range c.Spec.Nodes {
        wi := NewWorkItem(c.State.FlowID, node, 1)
        c.State.ActiveWorkItems[node.NodeID] = wi
    }

    c.State.CompletedSummary.TotalWorkItems = uint32(len(c.Spec.Nodes))

    // Mark dependency-free nodes as Ready
    c.unblockReady(ctx)

    if err := TransitionFlow(c.State, FlowPhaseRunning); err != nil {
        return err
    }

    return c.Store.Save(ctx, c.State)
}

// Orchestrate is the main PureTask handler. Called by CHASM Engine on each
// WorkflowTask for this component.
//
// G1 scope: Mark WorkItems with no dependencies as Ready.
func (c *OntoFlowExecutionComponent) Orchestrate(ctx context.Context) error {
    c.mu.Lock()
    defer c.mu.Unlock()

    if c.State.Phase.IsTerminal() {
        return nil // nothing to do
    }

    // G1: Unblock WorkItems that have no dependencies
    c.unblockReady(ctx)

    return c.Store.Save(ctx, c.State)
}

// unblockReady marks all Blocked WorkItems with satisfied dependencies as Ready.
// G1: Any WorkItem with no incoming edges in the DAG is immediately Ready.
func (c *OntoFlowExecutionComponent) unblockReady(ctx context.Context) {
    for _, wi := range c.State.ActiveWorkItems {
        if wi.Phase != WIPhaseBlocked {
            continue
        }
        if c.allDependenciesCommitted(wi.WorkItemID) {
            _ = TransitionWorkItem(wi, WIPhaseReady)
        }
    }
}

// allDependenciesCommitted checks if all upstream nodes of this WorkItem are Committed.
// G1: Nodes with no dependencies always return true (immediately Ready).
func (c *OntoFlowExecutionComponent) allDependenciesCommitted(nodeID string) bool {
    for _, edge := range c.Spec.Dependencies {
        if edge.To == nodeID {
            upstream, exists := c.State.ActiveWorkItems[edge.From]
            if !exists || upstream.Phase != WIPhaseCommitted {
                return false
            }
        }
    }
    return true // no unsatisfied dependencies
}

// ── Recovery ──

// Recover restores Flow state from the store after server restart.
func (c *OntoFlowExecutionComponent) Recover(ctx context.Context, flowID string) error {
    if c.Store == nil {
        return fmt.Errorf("no store configured for recovery")
    }
    state, err := c.Store.Load(ctx, flowID)
    if err != nil {
        return fmt.Errorf("recover load: %w", err)
    }
    c.mu.Lock()
    defer c.mu.Unlock()
    c.State = state
    return nil
}

// ── Helpers ──

func hashFlowSpec(spec *FlowSpec) string {
    h := sha256.New()
    h.Write([]byte(spec.FlowID))
    for _, n := range spec.Nodes {
        h.Write([]byte(n.NodeID))
    }
    return hex.EncodeToString(h.Sum(nil)[:8])
}

// GetActiveWorkItem returns a WorkItem by ID (read-only).
func (c *OntoFlowExecutionComponent) GetActiveWorkItem(nodeID string) (*WorkItemRuntimeState, bool) {
    c.mu.Lock()
    defer c.mu.Unlock()
    wi, ok := c.State.ActiveWorkItems[nodeID]
    return wi, ok
}

// WorkItemCount returns the number of active WorkItems.
func (c *OntoFlowExecutionComponent) WorkItemCount() int {
    c.mu.Lock()
    defer c.mu.Unlock()
    return len(c.State.ActiveWorkItems)
}

// ReadyCount returns the number of Ready-phase WorkItems.
func (c *OntoFlowExecutionComponent) ReadyCount() int {
    c.mu.Lock()
    defer c.mu.Unlock()
    count := 0
    for _, wi := range c.State.ActiveWorkItems {
        if wi.Phase == WIPhaseReady {
            count++
        }
    }
    return count
}

