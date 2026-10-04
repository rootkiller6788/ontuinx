package ontoflow

import (
	"context"
	"fmt"
)

// ── Batch Scheduler ──
//
// G7: Manages large-scale WorkItem dispatch with concurrency limits.
// Fills available slots as WorkItems complete.
// Tracks batch completion statistics.

// BatchScheduler extends FlowScheduler with batch-specific behavior.
type BatchScheduler struct {
	*FlowScheduler
	batchMetrics *BatchMetrics
}

// BatchMetrics tracks batch execution statistics.
type BatchMetrics struct {
	TotalDispatched  uint32
	TotalCompleted   uint32
	TotalCommitted   uint32
	TotalEscalated   uint32
	TotalFailed      uint32
	PeakConcurrency  uint32
	CurrentConcurrency uint32
}

func NewBatchScheduler(lib ActivityLibrary, taskQueue string, maxConcurrency uint32) *BatchScheduler {
	return &BatchScheduler{
		FlowScheduler: NewFlowScheduler(lib, taskQueue, maxConcurrency),
		batchMetrics:  &BatchMetrics{},
	}
}

// BatchOrchestrate performs one orchestration cycle, filling all available
// concurrency slots with Ready WorkItems.
func (bs *BatchScheduler) BatchOrchestrate(ctx context.Context, comp *OntoFlowExecutionComponent) error {
	comp.mu.Lock()
	defer comp.mu.Unlock()

	state := comp.State

	if state.Phase.IsTerminal() {
		return nil
	}

	// Step 1: Process completed activities, free up slots
	completed := bs.processBatchCompletions(ctx, comp)
	bs.batchMetrics.CurrentConcurrency = state.RunningCount

	// Step 2: Unblock dependencies
	comp.unblockReady(ctx)

	// Step 3: Fill all available slots
	dispatched := bs.fillSlots(ctx, state)

	// Update metrics
	bs.batchMetrics.TotalCompleted += completed
	bs.batchMetrics.TotalDispatched += dispatched
	if state.RunningCount > bs.batchMetrics.PeakConcurrency {
		bs.batchMetrics.PeakConcurrency = state.RunningCount
	}

	// Step 4: Update summary
	bs.updateSummary(state)

	// Step 5: Check completion
	if state.AllTerminal() {
		if state.CompletedSummary.EscalatedCount > 0 {
			_ = TransitionFlow(state, FlowPhaseFailed)
		} else {
			_ = TransitionFlow(state, FlowPhaseCompleted)
		}
	}

	return comp.Store.Save(ctx, state)
}

// processBatchCompletions handles all completed activities and frees slots.
func (bs *BatchScheduler) processBatchCompletions(ctx context.Context, comp *OntoFlowExecutionComponent) uint32 {
	var completed uint32
	for _, wi := range comp.State.ActiveWorkItems {
		if wi.Phase != WIPhaseDispatched && wi.Phase != WIPhaseRunning {
			continue
		}
		if wi.CurrentActivityID == "" {
			continue
		}

		status, err := bs.activityScheduler.CheckActivityStatus(ctx, wi)
		if err != nil {
			continue
		}

		switch status {
		case ActivityStatusCompleted:
			if _, err := bs.activityScheduler.HandleActivityResult(ctx, wi); err != nil {
				wi.LastError = err.Error()
			}
			comp.State.RunningCount--
			completed++

		case ActivityStatusFailed, ActivityStatusTimedOut:
			wi.RetryCount++
			if wi.RetryCount > 3 {
				_ = TransitionWorkItem(wi, WIPhaseEscalated)
				comp.State.CompletedSummary.EscalatedCount++
			} else {
				_ = TransitionWorkItem(wi, WIPhaseReady)
			}
			comp.State.RunningCount--

		case ActivityStatusRunning:
			if wi.Phase == WIPhaseDispatched {
				_ = TransitionWorkItem(wi, WIPhaseRunning)
			}
		}
	}
	return completed
}

// fillSlots dispatches Ready WorkItems to fill available concurrency slots.
func (bs *BatchScheduler) fillSlots(ctx context.Context, state *OntoFlowState) uint32 {
	var dispatched uint32
	for _, wi := range state.ActiveWorkItems {
		if wi.Phase != WIPhaseReady {
			continue
		}
		if !state.CanSchedule() {
			break
		}

		if err := bs.activityScheduler.DispatchWorkItem(ctx, wi, state.FlowID); err != nil {
			wi.LastError = err.Error()
			continue
		}

		state.RunningCount++
		dispatched++
	}
	return dispatched
}

// updateSummary refreshes the Flow completion summary.
func (bs *BatchScheduler) updateSummary(state *OntoFlowState) {
	summary := FlowCompletionSummary{TotalWorkItems: uint32(len(state.ActiveWorkItems))}
	for _, wi := range state.ActiveWorkItems {
		switch wi.Phase {
		case WIPhaseCommitted:
			summary.CommittedCount++
		case WIPhaseEscalated:
			summary.EscalatedCount++
		case WIPhaseBudgetExhausted:
			summary.BudgetExhaustedCount++
		case WIPhaseCancelled:
			summary.CancelledCount++
		}
	}
	bs.batchMetrics.TotalCommitted = summary.CommittedCount
	bs.batchMetrics.TotalEscalated = summary.EscalatedCount
	state.CompletedSummary = summary
}

// IsComplete returns true if all WorkItems have reached a terminal state.
func (bs *BatchScheduler) IsComplete(state *OntoFlowState) bool {
	return state.AllTerminal()
}

// BatchProgress returns a progress summary string.
func (bs *BatchScheduler) BatchProgress(state *OntoFlowState) string {
	return fmt.Sprintf(
		"%d/%d committed, %d running, %d escalated, %d remaining",
		bs.batchMetrics.TotalCommitted,
		bs.batchMetrics.TotalDispatched,
		state.RunningCount,
		bs.batchMetrics.TotalEscalated,
		bs.remainingCount(state),
	)
}

func (bs *BatchScheduler) remainingCount(state *OntoFlowState) uint32 {
	remaining := uint32(0)
	for _, wi := range state.ActiveWorkItems {
		if !wi.Phase.IsTerminal() {
			remaining++
		}
	}
	return remaining
}
