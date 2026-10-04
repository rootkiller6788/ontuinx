package ontoflow

import "fmt"

// ── Valid State Transitions ──

// Flow-level transitions
var validFlowTransitions = map[FlowPhase][]FlowPhase{
    FlowPhaseCreated:   {FlowPhaseRunning, FlowPhaseCancelled},
    FlowPhaseRunning:   {FlowPhaseCompleted, FlowPhaseFailed, FlowPhaseCancelled},
    FlowPhaseCompleted: {}, // terminal
    FlowPhaseFailed:    {}, // terminal
    FlowPhaseCancelled: {}, // terminal
}

// WorkItem-level transitions
var validWorkItemTransitions = map[WorkItemPhase][]WorkItemPhase{
    WIPhaseBlocked:            {WIPhaseReady, WIPhaseCancelled},
    WIPhaseReady:              {WIPhaseDispatched, WIPhaseOutcomeReported, WIPhaseCancelled, WIPhaseCommitted, WIPhaseEscalated},
    WIPhaseDispatched:         {WIPhaseRunning, WIPhaseOutcomeReported, WIPhaseCancelled, WIPhaseCommitted, WIPhaseEscalated, WIPhaseReady},
    WIPhaseRunning:            {WIPhaseOutcomeReported, WIPhaseCancelled, WIPhaseCommitted, WIPhaseEscalated, WIPhaseReady},
    WIPhaseOutcomeReported:    {WIPhaseAuthorityVerifying, WIPhaseCancelled, WIPhaseCommitted, WIPhaseEscalated, WIPhaseReady},
    WIPhaseAuthorityVerifying: {WIPhaseCommitted, WIPhaseEscalated, WIPhaseCancelled},
    WIPhaseCommitted:          {}, // terminal
    WIPhaseEscalated:          {}, // terminal
    WIPhaseBudgetExhausted:    {WIPhaseReady}, // retry with new budget grant
    WIPhaseCancelled:          {}, // terminal
}

// TransitionError represents an invalid state transition.
type TransitionError struct {
    ObjectID string
    From     string
    To       string
    Reason   string
}

func (e *TransitionError) Error() string {
    return fmt.Sprintf("invalid transition: %s from %s to %s: %s", e.ObjectID, e.From, e.To, e.Reason)
}

// ValidateFlowTransition checks if a Flow phase transition is valid.
func ValidateFlowTransition(flowID string, from, to FlowPhase) error {
    allowed, ok := validFlowTransitions[from]
    if !ok {
        return &TransitionError{flowID, string(from), string(to), "unknown source phase"}
    }
    for _, a := range allowed {
        if a == to {
            return nil
        }
    }
    return &TransitionError{flowID, string(from), string(to), "transition not allowed"}
}

// ValidateWorkItemTransition checks if a WorkItem phase transition is valid.
func ValidateWorkItemTransition(workItemID string, from, to WorkItemPhase) error {
    allowed, ok := validWorkItemTransitions[from]
    if !ok {
        return &TransitionError{workItemID, string(from), string(to), "unknown source phase"}
    }
    for _, a := range allowed {
        if a == to {
            return nil
        }
    }
    return &TransitionError{workItemID, string(from), string(to), "transition not allowed"}
}

// ── Transition Helpers ──

// TransitionFlow safely transitions a Flow phase.
func TransitionFlow(state *OntoFlowState, to FlowPhase) error {
    if err := ValidateFlowTransition(state.FlowID, state.Phase, to); err != nil {
        return err
    }
    state.Phase = to
    return nil
}

// TransitionWorkItem safely transitions a WorkItem phase.
func TransitionWorkItem(wi *WorkItemRuntimeState, to WorkItemPhase) error {
    if err := ValidateWorkItemTransition(wi.WorkItemID, wi.Phase, to); err != nil {
        return err
    }
    wi.Phase = to
    return nil
}

// CanSchedule returns true if the Flow can accept more running WorkItems.
func (s *OntoFlowState) CanSchedule() bool {
    return s.RunningCount < s.MaxConcurrency && s.Phase == FlowPhaseRunning
}

// AllTerminal returns true if all ActiveWorkItems are in terminal phases.
func (s *OntoFlowState) AllTerminal() bool {
    for _, wi := range s.ActiveWorkItems {
        if !wi.Phase.IsTerminal() {
            return false
        }
    }
    return true
}

// CommittedCount returns the number of committed WorkItems.
func (s *OntoFlowState) CommittedCount() uint32 {
    var count uint32
    for _, wi := range s.ActiveWorkItems {
        if wi.Phase == WIPhaseCommitted {
            count++
        }
    }
    return count
}
