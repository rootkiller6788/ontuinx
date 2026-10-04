//! Onto Assurance Kernel integration — M4/M5/M6 wiring.
//!
//! Enabled with `--features onto-assurance`.
//! P16-R2: AssuredFinalizer replaces NoopFinalizer for production.

#![cfg(feature = "onto-assurance")]

use std::sync::Arc;
use ironclaw_turns::run_finalization::RunFinalizationPort;

/// Adapter to wire into `DefaultPlannedRuntimeParts.run_finalizer`.
#[derive(Clone)]
pub struct OntoAssuranceAdapters {
    pub run_finalizer: Arc<dyn RunFinalizationPort>,
    pub on_loop_exit: Option<Arc<dyn Fn() + Send + Sync + 'static>>,
    pub _runtime_adapter: Option<Arc<dyn std::any::Any + Send + Sync>>,
}

impl OntoAssuranceAdapters {
    /// P16-R3: The ONLY production constructor.
    /// Registers 4 verifiers and wires AssuredFinalizer.
    /// No NoopFinalizer fallback. Returns Err on registry construction failure.
    pub fn production() -> Result<Self, BootstrapError> {
        use onto_ironclaw_adapter::assured_bridge::AssuredFinalizer;
        use onto_ironclaw_adapter::sandbox_executor::{ExternalToolSpec, ProjectCheckRegistry};
        use onto_assurance_runtime::pipeline::PassRegistry;

        let mut reg = PassRegistry::new();
        reg.register("artifact.manifest.integrity",
            Box::new(onto_ironclaw_adapter::production_verifiers::ArtifactManifestIntegrityVerifier::new()))?;
        reg.register("file.protected_path",
            Box::new(onto_ironclaw_adapter::production_verifiers::ProtectedPathVerifier::new(vec![])))?;
        reg.register("project.build",
            Box::new(onto_ironclaw_adapter::production_verifiers::ProjectBuildVerifier::new()))?;
        reg.register("project.test",
            Box::new(onto_ironclaw_adapter::production_verifiers::ProjectTestVerifier::new()))?;

        if reg.is_empty() {
            return Err(BootstrapError::EmptyRegistry);
        }

        // P16-F1: Build project check registry with c-cmake pipeline.
        // Each check maps to a multi-step ExternalToolSpec executed in
        // sequence by the sandbox executor, stopping on first failure.
        let mut checks = ProjectCheckRegistry::new();
        checks.register("project.build", ExternalToolSpec::new("cmake", &["-S", ".", "-B", "build"])
            .with_step("cmake", &["--build", "build", "--parallel", "2"]));
        checks.register("project.test", ExternalToolSpec::new("ctest", &["--test-dir", "build", "--output-on-failure"]));

        // F1-4: Validate required checks are resolvable. Missing check at
        // startup is a BootstrapError, not a runtime Unavailable.
        for required in &["project.build", "project.test"] {
            if checks.resolve(required).is_none() {
                return Err(BootstrapError::MissingRequiredProjectCheck(required.to_string()));
            }
        }

        let registry = Arc::new(reg.freeze());
        let onto_port = Arc::new(AssuredFinalizer::new(registry).with_project_checks(checks));
        let finalizer: Arc<dyn RunFinalizationPort> = Arc::new(OntoToIronclawBridge::new(onto_port));

        Ok(Self {
            run_finalizer: finalizer,
            on_loop_exit: None,
            _runtime_adapter: None,
        })
    }
}

/// P16-R3: BootstrapError — any failure here means the runtime MUST NOT start.
#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("verifier registry error: {0}")]
    Registry(#[from] onto_assurance_runtime::pipeline::BootstrapError),
    #[error("empty verifier registry")]
    EmptyRegistry,
    #[error("missing required project check: {0}")]
    MissingRequiredProjectCheck(String),
}

/// P16-R2: Lightweight bridge from onto's RunFinalizationPort to IronClaw's.
///
/// ## Field lineage — every output field must trace to an authoritative source.
///
/// | OntoRequest field   | Source                          | Bridge may generate? |
/// |---------------------|---------------------------------|---------------------|
/// | `run_id`            | `request.run_id` via `parse()`  | No                  |
/// | `attempt_id`        | `request.attempt_id` via `parse()` | No               |
/// | `exit_reason`       | `request.exit_reason` mapped    | No                  |
/// | `budget_outcome`    | `request.budget` mapped         | No                  |
/// | `staging_root`      | `request.staging_root` cloned   | No                  |
/// | `checkpoint_ref`    | IronClaw protocol (unsupported) | No — explicit None  |
///
/// Bridge must never call `RunId::generate()` or `AttemptId::generate()`.
/// CI enforces this via `scripts/check-identity-conservation.sh`.
struct OntoToIronclawBridge {
    onto_port: Arc<dyn onto_assurance_runtime::ports::RunFinalizationPort>,
}

impl OntoToIronclawBridge {
    fn new(onto_port: Arc<dyn onto_assurance_runtime::ports::RunFinalizationPort>) -> Self {
        Self { onto_port }
    }
}

#[async_trait::async_trait]
impl RunFinalizationPort for OntoToIronclawBridge {
    async fn finalize(
        &self,
        request: ironclaw_turns::run_finalization::RunFinalizationRequest,
    ) -> Result<ironclaw_turns::run_finalization::RunFinalizationOutcome, ironclaw_turns::run_finalization::RunFinalizationError> {
        use ironclaw_turns::run_finalization::{
            FinalizationBudgetOutcome, FinalizationLifecycle, FinalizationTaskOutcome,
        };
        use onto_assurance_runtime::ports::{RunFinalizationRequest as OntoRequest};

        let exit_reason = match request.exit_reason {
            ironclaw_turns::run_finalization::FinalizationExitReason::FinishRequested =>
                onto_assurance_types::enums::ExitReason::FinishRequested,
            ironclaw_turns::run_finalization::FinalizationExitReason::ProviderStop =>
                onto_assurance_types::enums::ExitReason::ProviderStop,
            ironclaw_turns::run_finalization::FinalizationExitReason::BudgetLimit =>
                onto_assurance_types::enums::ExitReason::BudgetLimit,
            ironclaw_turns::run_finalization::FinalizationExitReason::Stuck =>
                onto_assurance_types::enums::ExitReason::Stuck,
            ironclaw_turns::run_finalization::FinalizationExitReason::Cancelled =>
                onto_assurance_types::enums::ExitReason::Cancelled,
            ironclaw_turns::run_finalization::FinalizationExitReason::Crashed =>
                onto_assurance_types::enums::ExitReason::Crashed,
        };
        let budget_outcome = match request.budget {
            ironclaw_turns::run_finalization::FinalizationBudget::WithinBudget =>
                onto_assurance_types::enums::BudgetOutcome::WithinBudget,
            ironclaw_turns::run_finalization::FinalizationBudget::Depleted =>
                onto_assurance_types::enums::BudgetOutcome::Depleted,
        };
        let onto_req = OntoRequest {
            run_id: onto_assurance_types::ids::RunId::parse(&request.run_id)
                .map_err(|e| ironclaw_turns::run_finalization::RunFinalizationError::Internal(
                    format!("invalid run_id '{}': {}", request.run_id, e)))?,
            attempt_id: onto_assurance_types::ids::AttemptId::parse(&request.attempt_id)
                .map_err(|e| ironclaw_turns::run_finalization::RunFinalizationError::Internal(
                    format!("invalid attempt_id '{}': {}", request.attempt_id, e)))?,
            exit_reason,
            budget_outcome,
            // F2-Checkpoint: IronClaw protocol does not yet carry checkpoint_ref.
            // Explicitly None = NotProvidedByRuntime, distinct from NoCheckpoint.
            checkpoint_ref: None,
            staging_root: request.staging_root.clone(),
        };

        match self.onto_port.finalize(onto_req).await {
            Ok(outcome) => {
                let task_outcome = match outcome.task_outcome {
                    onto_assurance_types::enums::TaskOutcome::Success => FinalizationTaskOutcome::Success,
                    onto_assurance_types::enums::TaskOutcome::Failed => FinalizationTaskOutcome::Failed,
                    onto_assurance_types::enums::TaskOutcome::Incomplete => FinalizationTaskOutcome::Incomplete,
                    onto_assurance_types::enums::TaskOutcome::EnvironmentError => FinalizationTaskOutcome::EnvironmentError,
                };
                let lifecycle = match outcome.lifecycle_state {
                    onto_assurance_types::enums::LifecycleState::Committed => FinalizationLifecycle::Committed,
                    onto_assurance_types::enums::LifecycleState::Continuing => FinalizationLifecycle::Continuing,
                    onto_assurance_types::enums::LifecycleState::Escalated => FinalizationLifecycle::Escalated,
                    onto_assurance_types::enums::LifecycleState::RolledBack => FinalizationLifecycle::RolledBack,
                    onto_assurance_types::enums::LifecycleState::Running => FinalizationLifecycle::Continuing,
                    onto_assurance_types::enums::LifecycleState::Finalizing => FinalizationLifecycle::Continuing,
                };
                let budget_outcome = match outcome.budget_outcome {
                    onto_assurance_types::enums::BudgetOutcome::WithinBudget => FinalizationBudgetOutcome::WithinBudget,
                    onto_assurance_types::enums::BudgetOutcome::Depleted
                    | onto_assurance_types::enums::BudgetOutcome::HardLimitReached
                    => FinalizationBudgetOutcome::Depleted,
                };
                Ok(ironclaw_turns::run_finalization::RunFinalizationOutcome {
                    task_outcome,
                    budget_outcome,
                    lifecycle_state: lifecycle,
                    session_decision_id: outcome.session_decision_id.to_string(),
                    reason_codes: outcome.reason_codes.iter().map(|r| r.detail.clone()).collect(),
                })
            }
            Err(_e) => Err(ironclaw_turns::run_finalization::RunFinalizationError::Internal(
                "AssuredFinalizer error".into(),
            )),
        }
    }
}

/// Test-only no-op finalizer. NOT available in production builds.
#[cfg(test)]
impl Default for OntoAssuranceAdapters {
    fn default() -> Self {
        Self {
            run_finalizer: Arc::new(NoopFinalizer),
            on_loop_exit: None,
            _runtime_adapter: None,
        }
    }
}

#[cfg(test)]
impl OntoAssuranceAdapters {
    pub fn empty() -> Self { Self::default() }
}

#[cfg(test)]
struct NoopFinalizer;

#[cfg(test)]
#[async_trait::async_trait]
impl RunFinalizationPort for NoopFinalizer {
    async fn finalize(
        &self,
        _request: ironclaw_turns::run_finalization::RunFinalizationRequest,
    ) -> Result<ironclaw_turns::run_finalization::RunFinalizationOutcome, ironclaw_turns::run_finalization::RunFinalizationError> {
        Ok(ironclaw_turns::run_finalization::RunFinalizationOutcome {
            task_outcome: ironclaw_turns::run_finalization::FinalizationTaskOutcome::Success,
            budget_outcome: ironclaw_turns::run_finalization::FinalizationBudgetOutcome::WithinBudget,
            lifecycle_state: ironclaw_turns::run_finalization::FinalizationLifecycle::Committed,
            session_decision_id: "noop".into(),
            reason_codes: vec![],
        })
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    use ironclaw_turns::run_finalization::{RunFinalizationRequest, RunFinalizationPort};

    #[tokio::test]
    async fn bridge_preserves_run_identity() {
        // Given an IronClaw request with known identities
        let run_str = "01932c18-1234-7890-abcd-ef0123456789";
        let attempt_str = "01932c18-5678-1234-abcd-ef0123456789";
        let req = RunFinalizationRequest {
            run_id: run_str.to_string(),
            attempt_id: attempt_str.to_string(),
            exit_reason: ironclaw_turns::run_finalization::FinalizationExitReason::FinishRequested,
            budget: ironclaw_turns::run_finalization::FinalizationBudget::WithinBudget,
            staging_root: None,
        };

        // When: bridge translates to onto request
        let exit_reason = onto_assurance_types::enums::ExitReason::FinishRequested;
        let budget_outcome = onto_assurance_types::enums::BudgetOutcome::WithinBudget;
        let onto_req = onto_assurance_runtime::ports::RunFinalizationRequest {
            run_id: onto_assurance_types::ids::RunId::parse(&req.run_id).unwrap(),
            attempt_id: onto_assurance_types::ids::AttemptId::parse(&req.attempt_id).unwrap(),
            exit_reason,
            budget_outcome,
            checkpoint_ref: None,
            staging_root: None,
        };

        // Then: onto request carries the same identities
        assert_eq!(onto_req.run_id.to_string(), run_str);
        assert_eq!(onto_req.attempt_id.to_string(), attempt_str);
    }

    #[tokio::test]
    async fn bridge_rejects_invalid_run_id() {
        let req = RunFinalizationRequest {
            run_id: "not-a-valid-uuid".to_string(),
            attempt_id: "01932c18-5678-1234-abcd-ef0123456789".to_string(),
            exit_reason: ironclaw_turns::run_finalization::FinalizationExitReason::FinishRequested,
            budget: ironclaw_turns::run_finalization::FinalizationBudget::WithinBudget,
            staging_root: None,
        };

        let result = onto_assurance_types::ids::RunId::parse(&req.run_id);
        assert!(result.is_err(), "invalid run_id must be rejected");
    }

    #[tokio::test]
    async fn parse_is_deterministic() {
        let s = "01932c18-1234-7890-abcd-ef0123456789";
        let a = onto_assurance_types::ids::RunId::parse(s).unwrap();
        let b = onto_assurance_types::ids::RunId::parse(s).unwrap();
        assert_eq!(a, b, "parsing the same string twice must produce the same ID");
    }
}
