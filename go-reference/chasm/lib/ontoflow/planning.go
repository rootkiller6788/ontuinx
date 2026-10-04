package ontoflow

import (
	"context"
	"fmt"
)

// ── Dynamic Planning ──
//
// G8: A Planning OntoLoop produces a structured WorkPlan. Go validates
// the plan deterministically before creating sub-WorkItems.
//
// Invariants:
// - Plan comes from OntoLoop output (ArtifactRef), NOT raw LLM text
// - Plan MUST pass deterministic validation before Go creates sub-WorkItems
// - Invalid plan → Escalate (never silently fix)

// ── WorkPlan Types ──

// WorkPlan is a structured plan produced by a Planning OntoLoop.
type WorkPlan struct {
	PlanID          string          `json:"plan_id"`
	Nodes           []WorkPlanNode  `json:"nodes"`
	MaxConcurrency  uint32          `json:"max_concurrency"`
}

// WorkPlanNode is a single node in the work plan.
type WorkPlanNode struct {
	NodeID             string   `json:"node_id"`
	TaskSpecRef        string   `json:"task_spec_ref"`
	ContractRef        string   `json:"contract_ref"`
	DependsOn          []string `json:"depends_on"`
	InputArtifactRefs  []string `json:"input_artifact_refs"`
}

// ── Plan Validation ──

// PlanValidation is the result of validating a WorkPlan.
type PlanValidation struct {
	Valid   bool     `json:"valid"`
	Reasons []string `json:"reasons,omitempty"`
}

// WorkPlanValidator validates WorkPlans deterministically.
type WorkPlanValidator struct {
	MaxNodes int
}

// NewWorkPlanValidator creates a validator with the given node limit.
func NewWorkPlanValidator(maxNodes int) *WorkPlanValidator {
	return &WorkPlanValidator{MaxNodes: maxNodes}
}

// Validate checks a WorkPlan structurally.
func (v *WorkPlanValidator) Validate(plan *WorkPlan) *PlanValidation {
	var reasons []string

	// 1. Node count limit
	if len(plan.Nodes) > v.MaxNodes {
		reasons = append(reasons, fmt.Sprintf(
			"too many nodes: %d > max %d", len(plan.Nodes), v.MaxNodes,
		))
	}

	// 2. Empty plan
	if len(plan.Nodes) == 0 {
		reasons = append(reasons, "plan has no nodes")
		return &PlanValidation{Valid: false, Reasons: reasons}
	}

	// 3. Unique node IDs
	seenIDs := make(map[string]bool)
	idMap := make(map[string]*WorkPlanNode)
	for i := range plan.Nodes {
		node := &plan.Nodes[i]
		if seenIDs[node.NodeID] {
			reasons = append(reasons, "duplicate node_id: "+node.NodeID)
		}
		seenIDs[node.NodeID] = true
		idMap[node.NodeID] = node
	}

	// 4. Dependency integrity
	for i := range plan.Nodes {
		node := &plan.Nodes[i]
		for _, dep := range node.DependsOn {
			if _, exists := idMap[dep]; !exists {
				reasons = append(reasons, fmt.Sprintf(
					"node '%s' depends on unknown node '%s'", node.NodeID, dep,
				))
			}
		}
	}

	// 5. Acyclic check
	if len(reasons) == 0 {
		if cycle := detectCycle(plan.Nodes); cycle != "" {
			reasons = append(reasons, "cycle detected: "+cycle)
		}
	}

	// 6. Required field validation
	for i := range plan.Nodes {
		node := &plan.Nodes[i]
		if node.TaskSpecRef == "" {
			reasons = append(reasons, "node '"+node.NodeID+"' has empty task_spec_ref")
		}
		if node.ContractRef == "" {
			reasons = append(reasons, "node '"+node.NodeID+"' has empty contract_ref")
		}
	}

	if len(reasons) == 0 {
		return &PlanValidation{Valid: true}
	}
	return &PlanValidation{Valid: false, Reasons: reasons}
}

// detectCycle uses Kahn's algorithm to find cycles.
func detectCycle(nodes []WorkPlanNode) string {
	inDegree := make(map[string]int)
	children := make(map[string][]string)

	for i := range nodes {
		node := &nodes[i]
		if _, exists := inDegree[node.NodeID]; !exists {
			inDegree[node.NodeID] = 0
		}
		if _, exists := children[node.NodeID]; !exists {
			children[node.NodeID] = []string{}
		}
		for _, dep := range node.DependsOn {
			inDegree[node.NodeID]++
			children[dep] = append(children[dep], node.NodeID)
		}
	}

	var queue []string
	for nodeID, deg := range inDegree {
		if deg == 0 {
			queue = append(queue, nodeID)
		}
	}

	sorted := 0
	for len(queue) > 0 {
		node := queue[0]
		queue = queue[1:]
		sorted++

		for _, child := range children[node] {
			inDegree[child]--
			if inDegree[child] == 0 {
				queue = append(queue, child)
			}
		}
	}

	if sorted != len(nodes) {
		var cycleNodes []string
		for id, deg := range inDegree {
			if deg > 0 {
				cycleNodes = append(cycleNodes, id)
			}
		}
		return fmt.Sprintf("%v", cycleNodes)
	}
	return ""
}

// ── Plan Application ──

// ApplyPlan converts a validated WorkPlan into a FlowSpec that Go can execute.
func ApplyPlan(plan *WorkPlan, flowID string, budget BudgetSpec) (*FlowSpec, error) {
	if plan.MaxConcurrency == 0 {
		plan.MaxConcurrency = 1
	}

	nodes := make([]FlowNode, len(plan.Nodes))
	for i, pn := range plan.Nodes {
		nodes[i] = FlowNode{
			NodeID:        pn.NodeID,
			TaskSpecRef:   pn.TaskSpecRef,
			ContractRef:   pn.ContractRef,
			PolicyRef:     "policy/default",
			ResourceClass: "standard",
			RiskClass:     "low",
		}
	}

	var deps []DependencyEdge
	for _, pn := range plan.Nodes {
		for _, dep := range pn.DependsOn {
			deps = append(deps, DependencyEdge{From: dep, To: pn.NodeID})
		}
	}

	return &FlowSpec{
		FlowID:         flowID,
		Nodes:          nodes,
		Dependencies:   deps,
		MaxConcurrency: plan.MaxConcurrency,
		GlobalBudget:   budget,
	}, nil
}

// ── Planning Loop Integration ──

// PlanningLoopResult wraps the output of a Planning OntoLoop.
type PlanningLoopResult struct {
	Envelope *LoopTerminalEnvelope
	Plan     *WorkPlan
}

// ValidatePlanFromLoop validates a plan produced by a Planning OntoLoop.
// Returns an error if the plan is invalid (Go must Escalate, not fix).
func ValidatePlanFromLoop(result *PlanningLoopResult, maxNodes int) error {
	if result.Envelope == nil {
		return fmt.Errorf("no envelope from planning loop")
	}
	if !result.Envelope.IsCommitted() {
		return fmt.Errorf("planning loop not committed: %s", result.Envelope.ReportedTerminalState)
	}
	if result.Plan == nil {
		return fmt.Errorf("planning loop produced no plan")
	}

	validator := NewWorkPlanValidator(maxNodes)
	validation := validator.Validate(result.Plan)
	if !validation.Valid {
		return fmt.Errorf("plan validation failed: %v", validation.Reasons)
	}

	return nil
}

// ExecutePlan takes a validated plan and starts it as a new OntoFlow.
func ExecutePlan(
	ctx context.Context,
	lib *OntoFlowLibrary,
	plan *WorkPlan,
	parentFlowID string,
	budget BudgetSpec,
) (*OntoFlowExecutionComponent, error) {
	childFlowID := fmt.Sprintf("%s-plan-%s", parentFlowID, plan.PlanID)

	spec, err := ApplyPlan(plan, childFlowID, budget)
	if err != nil {
		return nil, fmt.Errorf("apply plan: %w", err)
	}

	return lib.StartFlow(ctx, spec)
}
