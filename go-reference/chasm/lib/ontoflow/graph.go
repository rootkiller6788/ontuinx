package ontoflow

import (
	"fmt"
	"sort"
)

// ── Dependency Graph ──
//
// G5: Manages DAG dependency tracking and unlock logic.
// Supports sequence (A→B→C), fan-out (A→[B,C]), and fan-in ([B,C]→D).

// DependencyGraph tracks node dependencies and determines which nodes are
// ready to execute based on committed upstream nodes.
type DependencyGraph struct {
	nodes    map[string]*FlowNode
	edges    []DependencyEdge
	inDegree map[string]int         // how many unsatisfied deps each node has
	children map[string][]string    // node → downstream nodes
	hasDeps  map[string]bool        // nodes that have at least one dependency
}

// NewDependencyGraph builds a graph from a FlowSpec.
func NewDependencyGraph(spec *FlowSpec) *DependencyGraph {
	g := &DependencyGraph{
		nodes:    make(map[string]*FlowNode),
		inDegree: make(map[string]int),
		children: make(map[string][]string),
		hasDeps:  make(map[string]bool),
	}

	for i := range spec.Nodes {
		node := &spec.Nodes[i]
		g.nodes[node.NodeID] = node
		g.inDegree[node.NodeID] = 0
		g.children[node.NodeID] = []string{}
	}

	for _, edge := range spec.Dependencies {
		g.edges = append(g.edges, edge)
		g.inDegree[edge.To]++
		g.children[edge.From] = append(g.children[edge.From], edge.To)
		g.hasDeps[edge.To] = true
	}

	return g
}

// HasDependencies returns true if the node has at least one upstream dependency.
func (g *DependencyGraph) HasDependencies(nodeID string) bool {
	return g.hasDeps[nodeID]
}

// UnsatisfiedDeps returns the number of upstream nodes not yet committed.
func (g *DependencyGraph) UnsatisfiedDeps(nodeID string, state *OntoFlowState) int {
	count := 0
	for _, edge := range g.edges {
		if edge.To == nodeID {
			upstream, exists := state.ActiveWorkItems[edge.From]
			if !exists || upstream.Phase != WIPhaseCommitted {
				count++
			}
		}
	}
	return count
}

// AllDepsCommitted returns true if all upstream dependencies are Committed.
func (g *DependencyGraph) AllDepsCommitted(nodeID string, state *OntoFlowState) bool {
	return g.UnsatisfiedDeps(nodeID, state) == 0
}

// UnlockedNodes returns node IDs that are currently Blocked but have all
// dependencies satisfied.
func (g *DependencyGraph) UnlockedNodes(state *OntoFlowState) []string {
	var ready []string
	for nodeID := range g.nodes {
		wi, exists := state.ActiveWorkItems[nodeID]
		if !exists || wi.Phase != WIPhaseBlocked {
			continue
		}
		if g.AllDepsCommitted(nodeID, state) {
			ready = append(ready, nodeID)
		}
	}
	sort.Strings(ready)
	return ready
}

// DownstreamNodes returns node IDs that depend on the given node.
func (g *DependencyGraph) DownstreamNodes(nodeID string) []string {
	return g.children[nodeID]
}

// IsFanIn returns true if the node has more than one upstream dependency.
func (g *DependencyGraph) IsFanIn(nodeID string) bool {
	return g.inDegree[nodeID] > 1
}

// IsFanOut returns true if the node has more than one downstream dependent.
func (g *DependencyGraph) IsFanOut(nodeID string) bool {
	return len(g.children[nodeID]) > 1
}

// ValidateAcyclic checks the graph has no cycles using topological sort.
func (g *DependencyGraph) ValidateAcyclic() error {
	inDeg := make(map[string]int)
	for k, v := range g.inDegree {
		inDeg[k] = v
	}

	var queue []string
	for nodeID, deg := range inDeg {
		if deg == 0 {
			queue = append(queue, nodeID)
		}
	}

	sorted := 0
	for len(queue) > 0 {
		node := queue[0]
		queue = queue[1:]
		sorted++

		for _, child := range g.children[node] {
			inDeg[child]--
			if inDeg[child] == 0 {
				queue = append(queue, child)
			}
		}
	}

	if sorted != len(g.nodes) {
		return fmt.Errorf("cycle detected: %d nodes sorted out of %d", sorted, len(g.nodes))
	}
	return nil
}

// NodeCount returns the number of nodes in the graph.
func (g *DependencyGraph) NodeCount() int {
	return len(g.nodes)
}

// ── Sequence Validator ──

// ValidateSequence checks that a planned sequence A→B→C is a valid DAG path.
func (g *DependencyGraph) ValidateSequence(nodeIDs ...string) error {
	for i := 0; i < len(nodeIDs)-1; i++ {
		current := nodeIDs[i]
		next := nodeIDs[i+1]

		// Check next depends on current
		found := false
		for _, child := range g.children[current] {
			if child == next {
				found = true
				break
			}
		}
		if !found {
			return fmt.Errorf("sequence break: %s does not depend on %s", next, current)
		}
	}
	return nil
}

// ── Escalation Check ──

// EscalationBlocksDownstream checks that an escalated node does NOT unlock
// downstream nodes. Returns true if ANY downstream node is blocked by
// this escalation.
func (g *DependencyGraph) EscalationBlocksDownstream(escalatedNodeID string, state *OntoFlowState) bool {
	wi, exists := state.ActiveWorkItems[escalatedNodeID]
	if !exists || wi.Phase != WIPhaseEscalated {
		return false
	}

	// Check all downstream nodes — they must NOT be unlocked
	for _, downstream := range g.children[escalatedNodeID] {
		dwi, exists := state.ActiveWorkItems[downstream]
		if !exists {
			continue
		}
		// If downstream is still Blocked, it's correctly blocked
		if dwi.Phase == WIPhaseBlocked {
			return true
		}
	}
	return false
}
