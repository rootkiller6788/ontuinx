package ontoflow

import (
    "context"
    "fmt"
    "sync"
)

// ── OntoFlowLibrary ──
//
// Registered in CHASM Engine at startup. Provides factory methods for
// creating and recovering OntoFlowExecutionComponents.

// OntoFlowLibrary is the CHASM library entry point for OntoFlow.
type OntoFlowLibrary struct {
    mu     sync.RWMutex
    store  FlowStateStore
    flows  map[string]*OntoFlowExecutionComponent
}

// NewLibrary creates a new OntoFlowLibrary.
func NewLibrary(store FlowStateStore) *OntoFlowLibrary {
    return &OntoFlowLibrary{
        store: store,
        flows: make(map[string]*OntoFlowExecutionComponent),
    }
}

// StartFlow creates a new OntoFlow from a spec and initializes it.
func (lib *OntoFlowLibrary) StartFlow(ctx context.Context, spec *FlowSpec) (*OntoFlowExecutionComponent, error) {
    lib.mu.Lock()
    defer lib.mu.Unlock()

    if _, exists := lib.flows[spec.FlowID]; exists {
        return nil, fmt.Errorf("flow %s already exists", spec.FlowID)
    }

    comp := NewOntoFlowComponent(spec, lib.store)
    if err := comp.Initialize(ctx); err != nil {
        return nil, fmt.Errorf("initialize flow %s: %w", spec.FlowID, err)
    }

    lib.flows[spec.FlowID] = comp
    return comp, nil
}

// GetFlow retrieves an active Flow component by ID.
func (lib *OntoFlowLibrary) GetFlow(flowID string) (*OntoFlowExecutionComponent, error) {
    lib.mu.RLock()
    defer lib.mu.RUnlock()

    comp, exists := lib.flows[flowID]
    if !exists {
        return nil, fmt.Errorf("flow %s not found", flowID)
    }
    return comp, nil
}

// RecoverFlow restores a Flow from persistent state.
func (lib *OntoFlowLibrary) RecoverFlow(ctx context.Context, flowID string) (*OntoFlowExecutionComponent, error) {
    lib.mu.Lock()
    defer lib.mu.Unlock()

    // Load state from store
    state, err := lib.store.Load(ctx, flowID)
    if err != nil {
        return nil, fmt.Errorf("recover load state: %w", err)
    }

    // Rebuild the spec from the state reference
    // In production, FlowSpec is loaded from immutable artifact store
    spec := &FlowSpec{
        FlowID:         state.FlowID,
        Nodes:          make([]FlowNode, 0),
        MaxConcurrency: state.MaxConcurrency,
    }

    comp := &OntoFlowExecutionComponent{
        Spec:  spec,
        State: state,
        Store: lib.store,
    }

    lib.flows[flowID] = comp
    return comp, nil
}

// ── In-Memory FlowStateStore for G1 testing ──

// InMemoryFlowStateStore is an in-memory implementation for G1 testing.
// Production: backed by Temporal History persistence.
type InMemoryFlowStateStore struct {
    mu     sync.RWMutex
    states map[string]*OntoFlowState
}

func NewInMemoryFlowStateStore() *InMemoryFlowStateStore {
    return &InMemoryFlowStateStore{
        states: make(map[string]*OntoFlowState),
    }
}

func (s *InMemoryFlowStateStore) Save(ctx context.Context, state *OntoFlowState) error {
    s.mu.Lock()
    defer s.mu.Unlock()
    // Deep copy to prevent external mutation
    copy := *state
    copy.ActiveWorkItems = make(map[string]*WorkItemRuntimeState)
    for k, v := range state.ActiveWorkItems {
        wiCopy := *v
        copy.ActiveWorkItems[k] = &wiCopy
    }
    s.states[state.FlowID] = &copy
    return nil
}

func (s *InMemoryFlowStateStore) Load(ctx context.Context, flowID string) (*OntoFlowState, error) {
    s.mu.RLock()
    defer s.mu.RUnlock()
    state, exists := s.states[flowID]
    if !exists {
        return nil, fmt.Errorf("flow %s not found in store", flowID)
    }
    return state, nil
}
