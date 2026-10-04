package ontoflow

import (
    "fmt"
    "crypto/sha256"
    "encoding/hex"
    "strings"
)

// ── WorkItem Factory ──

// NewWorkItem creates a new WorkItem in Blocked phase from a FlowNode spec.
func NewWorkItem(flowID string, node FlowNode, generation uint64) *WorkItemRuntimeState {
    loopID := fmt.Sprintf("loop-%s-%s-gen%d", flowID, node.NodeID, generation)

    return &WorkItemRuntimeState{
        WorkItemID: node.NodeID,
        LoopID:     loopID,
        Phase:      WIPhaseBlocked,

        NodeSpec: node,
        InputRefs:  []string{},
        OutputRefs: []string{},

        ExecutionGeneration: generation,
    }
}

// ── LoopInvocationRequest (Go → Rust) ──

// BuildLoopInvocationRequest constructs the JSON payload for ExecuteOntoLoop.
// Go sends references, NOT content. Rust owns Attempt management.
func BuildLoopInvocationRequest(wi *WorkItemRuntimeState, flowID string) LoopInvocationRequest {
    req := LoopInvocationRequest{
        SchemaVersion: 1,
        FlowID:        flowID,
        WorkItemID:    wi.WorkItemID,
        LoopID:        wi.LoopID,
        TaskSpecRef:   wi.NodeSpec.TaskSpecRef,
        ContractRef:   wi.NodeSpec.ContractRef,
        PolicyRef:     wi.NodeSpec.PolicyRef,
        InputArtifactRefs: wi.InputRefs,
        ResourceClass: wi.NodeSpec.ResourceClass,
        RiskClass:     wi.NodeSpec.RiskClass,
        TrustRequirement: "basic",
        BudgetGrantRef:   wi.BudgetGrant.GrantID,
        BudgetGrantHash:  "",
        ExecutionGeneration: wi.ExecutionGeneration,
        IdempotencyKey:  fmt.Sprintf("idem-%s-%s-gen%d", flowID, wi.WorkItemID, wi.ExecutionGeneration),
    }
    req.BudgetGrantHash = hashString(req.BudgetGrantRef)
    req.RequestBindingHash = ComputeRequestBindingHash(req)
    return req
}

// LoopInvocationRequest is the Go→Rust protocol message.
type LoopInvocationRequest struct {
    SchemaVersion        uint32   `json:"schema_version"`
    FlowID               string   `json:"flow_id"`
    WorkItemID           string   `json:"work_item_id"`
    LoopID               string   `json:"loop_id"`
    TaskSpecRef          string   `json:"task_spec_ref"`
    ContractRef          string   `json:"contract_ref"`
    PolicyRef            string   `json:"policy_ref"`
    InputArtifactRefs    []string `json:"input_artifact_refs"`
    ResourceClass        string   `json:"resource_class"`
    RiskClass            string   `json:"risk_class"`
    TrustRequirement     string   `json:"trust_requirement"`
    BudgetGrantRef       string   `json:"budget_grant_ref"`
    BudgetGrantHash      string   `json:"budget_grant_hash"`
    ExecutionGeneration  uint64   `json:"execution_generation"`
    IdempotencyKey       string   `json:"idempotency_key"`
    Deadline             string   `json:"deadline,omitempty"`
    RequestBindingHash   string   `json:"request_binding_hash"`
}

// ComputeRequestBindingHash MUST match Rust's implementation exactly.
// Protocol v1: DefaultHasher(SipHash-1-3) over colon-joined fields.
func ComputeRequestBindingHash(req LoopInvocationRequest) string {
    payload := fmt.Sprintf("%s:%s:%s:%s:%s:%s:%s:%s:%s:%d",
        req.FlowID, req.WorkItemID, req.LoopID,
        req.TaskSpecRef, req.ContractRef, req.PolicyRef,
        strings.Join(req.InputArtifactRefs, ","),
        req.BudgetGrantRef, req.BudgetGrantHash,
        req.ExecutionGeneration,
    )
    h := sha256.Sum256([]byte(payload))
    return hex.EncodeToString(h[:8]) // first 8 bytes = 16 hex chars
}

func hashString(s string) string {
    h := sha256.Sum256([]byte(s))
    return hex.EncodeToString(h[:8])
}
