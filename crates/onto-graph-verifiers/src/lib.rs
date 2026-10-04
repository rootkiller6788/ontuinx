//! onto-graph-verifiers — P7 Graph Verifiers for OntoAssure
//!
//! Two DeterministicVerifierPort implementations:
//! - GraphIntegrityVerifier: enforced for all code changes
//! - GraphRiskVerifier: scheduled by policy
//!
//! Call authority belongs exclusively to VerifierScheduler, never Agent.

pub mod client;
pub mod graph_integrity;
pub mod graph_risk;
pub mod schedule_rules;

pub use client::{GraphClientError, GraphRiskClient, SnapshotQueryClient};
pub use graph_integrity::GraphIntegrityVerifier;
pub use graph_risk::{GraphRiskVerifier, RiskDimension, RiskDimensions};
pub use schedule_rules::{
    classify_change, integrity_policy, risk_policy, ChangeType, EnforcementLevel,
};
// pub use scheduler::GraphVerifierScheduler; — removed, use PipelineManager
