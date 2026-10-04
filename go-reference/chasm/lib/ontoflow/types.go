// Package ontoflow — durable multi-loop orchestration for OntoOS.
//
// G1: Core types for FlowSpec, OntoFlowState, WorkItemRuntimeState.
package ontoflow

// ── Flow Specification (immutable, stored as Artifact) ──

type FlowSpec struct {
    FlowID         string        `json:"flow_id"`
    Nodes          []FlowNode    `json:"nodes"`
    Dependencies   []DependencyEdge `json:"dependencies"`
    MaxConcurrency uint32        `json:"max_concurrency"`
    GlobalBudget   BudgetSpec    `json:"global_budget"`
}

type FlowNode struct {
    NodeID        string `json:"node_id"`
    TaskSpecRef   string `json:"task_spec_ref"`   // immutable artifact ref
    ContractRef   string `json:"contract_ref"`
    PolicyRef     string `json:"policy_ref"`
    ResourceClass string `json:"resource_class"`
    RiskClass     string `json:"risk_class"`
}

type DependencyEdge struct {
    From string `json:"from"`
    To   string `json:"to"`
}

type BudgetSpec struct {
    MaxTokens    uint64 `json:"max_tokens"`
    MaxCostCents uint64 `json:"max_cost_cents"`
    MaxDurationS uint64 `json:"max_duration_s"`
}

// ── Runtime State (persisted in Temporal History) ──

type FlowPhase string

const (
    FlowPhaseCreated   FlowPhase = "created"
    FlowPhaseRunning   FlowPhase = "running"
    FlowPhaseCompleted FlowPhase = "completed"
    FlowPhaseFailed    FlowPhase = "failed"
    FlowPhaseCancelled FlowPhase = "cancelled"
)

func (p FlowPhase) IsTerminal() bool {
    return p == FlowPhaseCompleted || p == FlowPhaseFailed || p == FlowPhaseCancelled
}

type OntoFlowState struct {
    FlowID      string    `json:"flow_id"`
    Phase       FlowPhase `json:"phase"`
    FlowSpecRef string    `json:"flow_spec_ref"`
    GraphHash   string    `json:"graph_hash"`

    // Runtime projections
    ActiveWorkItems  map[string]*WorkItemRuntimeState `json:"active_work_items"`
    CompletedSummary FlowCompletionSummary            `json:"completed_summary"`

    MaxConcurrency uint32 `json:"max_concurrency"`
    RunningCount   uint32 `json:"running_count"`

    // Budget tracking
    TotalBudget    BudgetAmount `json:"total_budget"`
    ConsumedBudget BudgetAmount `json:"consumed_budget"`
}

type FlowCompletionSummary struct {
    TotalWorkItems     uint32 `json:"total_work_items"`
    CommittedCount     uint32 `json:"committed_count"`
    EscalatedCount     uint32 `json:"escalated_count"`
    BudgetExhaustedCount uint32 `json:"budget_exhausted_count"`
    CancelledCount     uint32 `json:"cancelled_count"`
}

type BudgetAmount struct {
    Tokens      uint64 `json:"tokens"`
    CostCents   uint64 `json:"cost_cents"`
}

// ── WorkItem Runtime State ──

type WorkItemPhase string

const (
    WIPhaseBlocked            WorkItemPhase = "blocked"
    WIPhaseReady              WorkItemPhase = "ready"
    WIPhaseDispatched         WorkItemPhase = "dispatched"
    WIPhaseRunning            WorkItemPhase = "running"
    WIPhaseOutcomeReported    WorkItemPhase = "outcome_reported"
    WIPhaseAuthorityVerifying WorkItemPhase = "authority_verifying"
    WIPhaseCommitted          WorkItemPhase = "committed"
    WIPhaseEscalated          WorkItemPhase = "escalated"
    WIPhaseBudgetExhausted    WorkItemPhase = "budget_exhausted"
    WIPhaseCancelled          WorkItemPhase = "cancelled"
)

func (p WorkItemPhase) IsTerminal() bool {
    switch p {
    case WIPhaseCommitted, WIPhaseEscalated, WIPhaseBudgetExhausted, WIPhaseCancelled:
        return true
    }
    return false
}

func (p WorkItemPhase) AllowsDownstream() bool {
    return p == WIPhaseCommitted
}

type WorkItemRuntimeState struct {
    WorkItemID string         `json:"work_item_id"`
    LoopID     string         `json:"loop_id"`
    Phase      WorkItemPhase  `json:"phase"`

    NodeSpec    FlowNode  `json:"node_spec"`
    InputRefs   []string  `json:"input_refs"`
    OutputRefs  []string  `json:"output_refs"`

    ExecutionGeneration uint64 `json:"execution_generation"`

    CurrentActivityID             string `json:"current_activity_id"`
    CurrentActivityScheduleEventID int64 `json:"current_activity_schedule_event_id"`

    TerminalEnvelopeHash string `json:"terminal_envelope_hash"`
    DecisionID           string `json:"decision_id"`
    VerifiedOutcome      string `json:"verified_outcome"`

    BudgetGrant BudgetGrant `json:"budget_grant"`

    LastError  string `json:"last_error"`
    RetryCount int    `json:"retry_count"`
}

type BudgetGrant struct {
    GrantID          string `json:"grant_id"`
    MaxAttempts      uint32 `json:"max_attempts"`
    MaxCostCents     uint64 `json:"max_cost_cents"`
    MaxDurationSeconds uint64 `json:"max_duration_seconds"`
}
