package ontoflow

import (
	"context"
	"fmt"
)

// ── Flow Scheduler ──
//
// G2: PureTask orchestration. Called by CHASM Engine on each WorkflowTask.
// Picks Ready WorkItems, dispatches them via ActivityScheduler, and handles
// completions.

// FlowScheduler orchestrates the full OntoFlow execution loop.
type FlowScheduler struct {
	activityScheduler *ActivityScheduler
	maxConcurrency    uint32
}

// NewFlowScheduler creates a scheduler with the given activity library.
func NewFlowScheduler(lib ActivityLibrary, taskQueue string, maxConcurrency uint32) *FlowScheduler {
	return &FlowScheduler{
		activityScheduler: NewActivityScheduler(lib, taskQueue),
		maxConcurrency:    maxConcurrency,
	}
}

// Orchestrate is the main PureTask entry point. Called by CHASM Engine.
//
// Steps:
// 1. Check all Dispatched/Running WorkItems for completed activities
// 2. Unblock dependencies
// 3. Dispatch Ready WorkItems (up to MaxConcurrency)
// 4. Check Flow completion
func (fs *FlowScheduler) Orchestrate(ctx context.Context, comp *OntoFlowExecutionComponent) error {
	comp.mu.Lock()
	defer comp.mu.Unlock()

	state := comp.State

	if state.Phase.IsTerminal() {
		return nil
	}

	// ── Step 1: Process completed activities ──
	if err := fs.processCompletions(ctx, comp); err != nil {
		return fmt.Errorf("process completions: %w", err)
	}

	// ── Step 2: Unblock dependencies ──
	comp.unblockReady(ctx)

	// ── Step 3: Dispatch Ready WorkItems ──
	if err := fs.dispatchReady(ctx, state); err != nil {
		return fmt.Errorf("dispatch ready: %w", err)
	}

	// ── Step 4: Check completion ──
	if state.AllTerminal() {
		if state.CommittedCount() == state.CompletedSummary.TotalWorkItems {
			_ = TransitionFlow(state, FlowPhaseCompleted)
		} else {
			_ = TransitionFlow(state, FlowPhaseFailed)
		}
	}

	return comp.Store.Save(ctx, state)
}

// processCompletions checks Dispatched/Running WorkItems for completed activities.
func (fs *FlowScheduler) processCompletions(ctx context.Context, comp *OntoFlowExecutionComponent) error {
	for _, wi := range comp.State.ActiveWorkItems {
		if wi.Phase != WIPhaseDispatched && wi.Phase != WIPhaseRunning {
			continue
		}
		if wi.CurrentActivityID == "" {
			continue
		}

		status, err := fs.activityScheduler.CheckActivityStatus(ctx, wi)
		if err != nil {
			continue // activity not found or not ready yet
		}

		switch status {
		case ActivityStatusCompleted:
			// Transition: Dispatched/Running → OutcomeReported
			if _, err := fs.activityScheduler.HandleActivityResult(ctx, wi); err != nil {
				wi.LastError = err.Error()
			}
			comp.State.RunningCount--

		case ActivityStatusFailed, ActivityStatusTimedOut:
			// Activity failed — mark for retry (G6 recovery handles this)
			wi.LastError = fmt.Sprintf("activity %s: %s", wi.CurrentActivityID, status.String())
			wi.RetryCount++
			comp.State.RunningCount--
			// Transition back to Ready for retry (if under retry limit)
			// G2: single retry; G6 handles full recovery logic
			if wi.RetryCount <= 3 {
				_ = TransitionWorkItem(wi, WIPhaseReady)
			} else {
				_ = TransitionWorkItem(wi, WIPhaseEscalated)
			}

		case ActivityStatusRunning:
			// Update phase tracking
			if wi.Phase == WIPhaseDispatched {
				_ = TransitionWorkItem(wi, WIPhaseRunning)
			}
		}
	}
	return nil
}

// dispatchReady dispatches Ready WorkItems via ActivityScheduler.
func (fs *FlowScheduler) dispatchReady(ctx context.Context, state *OntoFlowState) error {
	for _, wi := range state.ActiveWorkItems {
		if wi.Phase != WIPhaseReady {
			continue
		}
		if !state.CanSchedule() {
			break // max concurrency reached
		}

		if err := fs.activityScheduler.DispatchWorkItem(ctx, wi, state.FlowID); err != nil {
			wi.LastError = err.Error()
			continue
		}

		state.RunningCount++
	}
	return nil
}
