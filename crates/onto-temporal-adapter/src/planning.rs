//! F7: Dynamic Task Splitting — structured plan generation and validation.
//!
//! An OntoLoop (typically a "Planning" loop) can produce a structured WorkPlan
//! that Go uses to create sub-WorkItems. The plan MUST pass deterministic schema
//! validation before Go acts on it.
//!
//! ## Invariants
//!
//! - Plan comes from OntoLoop output (ArtifactRef), not from LLM raw text
//! - Plan MUST pass deterministic validation before Go creates sub-WorkItems
//! - Invalid plan → Escalate (never silently create wrong sub-tasks)

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A work plan produced by a Planning OntoLoop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkPlan {
    pub plan_id: String,
    pub nodes: Vec<WorkPlanNode>,
    pub max_concurrency: u32,
}

/// A single node in the work plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkPlanNode {
    pub node_id: String,
    pub task_spec_ref: String,
    pub contract_ref: String,
    pub depends_on: Vec<String>,
    pub input_artifact_refs: Vec<String>,
}

/// Validation result for a work plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanValidation {
    /// Plan is valid — Go can create sub-WorkItems.
    Valid,
    /// Plan is invalid — must Escalate, never silently fix.
    Invalid { reasons: Vec<String> },
}

impl PlanValidation {
    pub fn is_valid(&self) -> bool { matches!(self, Self::Valid) }
}

/// Deterministic plan validator.
///
/// Checks: node count limit, unique IDs, acyclic DAG, dependency integrity,
/// artifact ref format. Does NOT check LLM quality — only structural correctness.
pub struct WorkPlanValidator {
    max_nodes: usize,
}

impl WorkPlanValidator {
    pub fn new(max_nodes: usize) -> Self {
        Self { max_nodes }
    }

    /// Validate a work plan structurally.
    pub fn validate(&self, plan: &WorkPlan) -> PlanValidation {
        let mut reasons: Vec<String> = Vec::new();

        // 1. Node count limit
        if plan.nodes.len() > self.max_nodes {
            reasons.push(format!(
                "too many nodes: {} > max {}",
                plan.nodes.len(),
                self.max_nodes
            ));
        }

        // 2. Empty plan
        if plan.nodes.is_empty() {
            reasons.push("plan has no nodes".into());
            return PlanValidation::Invalid { reasons };
        }

        // 3. Unique node IDs
        let mut seen_ids: HashSet<&str> = HashSet::new();
        let id_map: HashMap<&str, &WorkPlanNode> = plan
            .nodes
            .iter()
            .map(|n| (n.node_id.as_str(), n))
            .collect();

        for node in &plan.nodes {
            if !seen_ids.insert(&node.node_id) {
                reasons.push(format!("duplicate node_id: {}", node.node_id));
            }
        }

        // 4. Dependency integrity — all deps must reference existing nodes
        for node in &plan.nodes {
            for dep in &node.depends_on {
                if !id_map.contains_key(dep.as_str()) {
                    reasons.push(format!(
                        "node '{}' depends on unknown node '{}'",
                        node.node_id, dep
                    ));
                }
            }
        }

        // 5. Acyclic check (topological sort)
        if reasons.is_empty() {
            if let Err(cycle) = Self::check_acyclic(&plan.nodes) {
                reasons.push(format!("cycle detected: {}", cycle));
            }
        }

        // 6. Ref format validation (must not be empty strings)
        for node in &plan.nodes {
            if node.task_spec_ref.is_empty() {
                reasons.push(format!("node '{}' has empty task_spec_ref", node.node_id));
            }
            if node.contract_ref.is_empty() {
                reasons.push(format!("node '{}' has empty contract_ref", node.node_id));
            }
        }

        if reasons.is_empty() {
            PlanValidation::Valid
        } else {
            PlanValidation::Invalid { reasons }
        }
    }

    /// Kahn's algorithm for cycle detection.
    fn check_acyclic(nodes: &[WorkPlanNode]) -> Result<(), String> {
        let mut in_degree: HashMap<&str, usize> = HashMap::new();
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();

        for node in nodes {
            in_degree.entry(&node.node_id).or_insert(0);
            adjacency.entry(&node.node_id).or_default();
            for dep in &node.depends_on {
                *in_degree.entry(dep.as_str()).or_insert(0) += 0; // ensure dep exists
                adjacency.entry(dep.as_str()).or_default().push(&node.node_id);
                *in_degree.entry(&node.node_id).or_insert(0) += 1;
            }
        }

        let mut queue: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut sorted = 0;
        while let Some(node) = queue.pop() {
            sorted += 1;
            if let Some(children) = adjacency.get(node) {
                for &child in children {
                    if let Some(deg) = in_degree.get_mut(child) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(child);
                        }
                    }
                }
            }
        }

        if sorted != nodes.len() {
            // Find nodes still in cycle
            let cycle_nodes: Vec<&str> = in_degree
                .iter()
                .filter(|(_, &deg)| deg > 0)
                .map(|(&id, _)| id)
                .collect();
            Err(format!("{:?}", cycle_nodes))
        } else {
            Ok(())
        }
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests — F7
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, deps: &[&str]) -> WorkPlanNode {
        WorkPlanNode {
            node_id: id.to_string(),
            task_spec_ref: format!("spec/{}", id),
            contract_ref: format!("contract/{}", id),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            input_artifact_refs: vec![],
        }
    }

    /// F7.1: Valid DAG plan passes validation.
    #[test]
    fn f7_1_valid_dag_accepted() {
        let plan = WorkPlan {
            plan_id: "p1".into(),
            max_concurrency: 2,
            nodes: vec![
                make_node("A", &[]),
                make_node("B", &["A"]),
                make_node("C", &["A"]),
                make_node("D", &["B", "C"]),
            ],
        };

        let validator = WorkPlanValidator::new(10);
        assert_eq!(validator.validate(&plan), PlanValidation::Valid);
    }

    /// F7.2: Cyclic plan rejected.
    #[test]
    fn f7_2_cycle_rejected() {
        let plan = WorkPlan {
            plan_id: "p2".into(),
            max_concurrency: 1,
            nodes: vec![
                make_node("A", &["C"]),  // A → C → B → A
                make_node("B", &["A"]),
                make_node("C", &["B"]),
            ],
        };

        let validator = WorkPlanValidator::new(10);
        let result = validator.validate(&plan);
        assert!(!result.is_valid());
        match result {
            PlanValidation::Invalid { reasons } => {
                assert!(reasons.iter().any(|r| r.contains("cycle")));
            }
            _ => panic!("expected Invalid"),
        }
    }

    /// F7.3: Too many nodes rejected.
    #[test]
    fn f7_3_too_many_nodes_rejected() {
        let nodes: Vec<_> = (0..15).map(|i| make_node(&format!("N{}", i), &[])).collect();
        let plan = WorkPlan {
            plan_id: "p3".into(),
            max_concurrency: 4,
            nodes,
        };

        let validator = WorkPlanValidator::new(10);
        let result = validator.validate(&plan);
        assert!(!result.is_valid());
    }

    /// F7.4: Unknown dependency rejected.
    #[test]
    fn f7_4_unknown_dependency_rejected() {
        let plan = WorkPlan {
            plan_id: "p4".into(),
            max_concurrency: 1,
            nodes: vec![
                make_node("A", &["NONEXISTENT"]),
                make_node("B", &["A"]),
            ],
        };

        let validator = WorkPlanValidator::new(10);
        let result = validator.validate(&plan);
        assert!(!result.is_valid());
    }

    /// F7.5: Empty plan rejected.
    #[test]
    fn f7_5_empty_plan_rejected() {
        let plan = WorkPlan {
            plan_id: "p5".into(),
            max_concurrency: 1,
            nodes: vec![],
        };

        let validator = WorkPlanValidator::new(10);
        assert!(!validator.validate(&plan).is_valid());
    }

    /// F7.6: Plan JSON round-trip.
    #[test]
    fn f7_6_plan_json_roundtrip() {
        let plan = WorkPlan {
            plan_id: "p6".into(),
            max_concurrency: 2,
            nodes: vec![
                make_node("X", &[]),
                make_node("Y", &["X"]),
            ],
        };
        let json = serde_json::to_string_pretty(&plan).unwrap();
        let back: WorkPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(plan, back);
    }
}
