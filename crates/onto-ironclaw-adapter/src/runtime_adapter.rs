//! runtime_adapter — bridges Onto `RuntimePort` to OntoRuntime `HostRuntime`.
//!
//! Default: stub. With `--features ironclaw-integration`: wraps HostRuntime.

use async_trait::async_trait;
use onto_assurance_runtime::ports::{ExecutionResult, RuntimeError, RuntimePort, StageHandle};
use onto_assurance_types::decision::SettlementDecision;
use onto_assurance_types::ids::TransactionId;
use onto_assurance_types::transaction::{ExecutionTransaction, SideEffectManifest};

// ══════════════════════════════════════════════════════════════════
// Stub
// ══════════════════════════════════════════════════════════════════

pub struct StubRuntimePort;

#[async_trait]
impl RuntimePort for StubRuntimePort {
    async fn stage(&self, txn: &ExecutionTransaction) -> Result<StageHandle, RuntimeError> {
        Ok(StageHandle { transaction_id: txn.transaction_id, handle_id: "stub".into(), environment_info: "stub".into() })
    }
    async fn execute(&self, _h: &StageHandle, _c: &str) -> Result<ExecutionResult, RuntimeError> {
        Ok(ExecutionResult { exit_code: 0, stdout: "stub ok".into(), stderr: String::new(), duration_ms: 0 })
    }
    async fn capture_effects(&self, _h: &StageHandle) -> Result<SideEffectManifest, RuntimeError> {
        Ok(SideEffectManifest { transaction_id: TransactionId::new(), filesystem_changes: vec![], network_requests: vec![], subprocess_invocations: vec![], external_receipts: vec![] })
    }
    async fn publish(&self, _h: &StageHandle, _d: &SettlementDecision) -> Result<(), RuntimeError> { Ok(()) }
    async fn discard(&self, _h: &StageHandle) -> Result<(), RuntimeError> { Ok(()) }
    async fn freeze(&self, _h: &StageHandle) -> Result<(), RuntimeError> { Ok(()) }
}

// ══════════════════════════════════════════════════════════════════
// OntoRuntime adapter
// ══════════════════════════════════════════════════════════════════

#[cfg(feature = "ironclaw-integration")]
pub mod ironclaw {
    use super::*;
    use ironclaw_host_api::{
        CapabilityId, CapabilitySet, ExtensionId,
        ResourceEstimate, RuntimeKind, TrustClass, UserId, MountView,
    };
    use ironclaw_host_runtime::{
        HostRuntime, RuntimeCapabilityOutcome, RuntimeInvocation,
    };
    use std::sync::Arc;

    pub struct Adapter {
        host: Arc<dyn HostRuntime>,
        user_id: UserId,
        extension_id: ExtensionId,
    }

    impl Adapter {
        pub fn new(host: Arc<dyn HostRuntime>, user_id: UserId, extension_id: ExtensionId) -> Self {
            Self { host, user_id, extension_id }
        }

        fn ctx(&self) -> ironclaw_host_api::ExecutionContext {
            ironclaw_host_api::ExecutionContext::local_default(
                self.user_id.clone(),
                self.extension_id.clone(),
                RuntimeKind::System,
                TrustClass::System,
                CapabilitySet { grants: vec![] },
                MountView::default(),
            )
            .expect("valid local execution context")
        }

        fn map(outcome: RuntimeCapabilityOutcome) -> ExecutionResult {
            match outcome {
                RuntimeCapabilityOutcome::Completed(..) =>
                    ExecutionResult { exit_code: 0, stdout: String::new(), stderr: String::new(), duration_ms: 0 },
                RuntimeCapabilityOutcome::Failed(f) =>
                    ExecutionResult { exit_code: 1, stdout: String::new(), stderr: f.message.unwrap_or_default(), duration_ms: 0 },
                _ => ExecutionResult { exit_code: 2, stdout: String::new(), stderr: "suspended".into(), duration_ms: 0 },
            }
        }
    }

    #[async_trait]
    impl RuntimePort for Adapter {
        async fn stage(&self, txn: &ExecutionTransaction) -> Result<StageHandle, RuntimeError> {
            Ok(StageHandle { transaction_id: txn.transaction_id, handle_id: format!("run-{}", txn.run_id), environment_info: "ironclaw".into() })
        }

        async fn execute(&self, _h: &StageHandle, command: &str) -> Result<ExecutionResult, RuntimeError> {
            let cap = CapabilityId::new("onto.assurance.run").map_err(|e| RuntimeError::Host(e.to_string()))?;
            let inv: RuntimeInvocation = (self.ctx(), cap, ResourceEstimate::default(), serde_json::json!({"cmd": command}));
            let outcome = self.host.invoke_capability(inv).await.map_err(|e| RuntimeError::ExecutionFailed(e.to_string()))?;
            Ok(Self::map(outcome))
        }

        async fn capture_effects(&self, _h: &StageHandle) -> Result<SideEffectManifest, RuntimeError> {
            Ok(SideEffectManifest { transaction_id: TransactionId::new(), filesystem_changes: vec![], network_requests: vec![], subprocess_invocations: vec![], external_receipts: vec![] })
        }

        async fn publish(&self, _h: &StageHandle, _d: &SettlementDecision) -> Result<(), RuntimeError> { Ok(()) }
        async fn discard(&self, _h: &StageHandle) -> Result<(), RuntimeError> { Ok(()) }
        async fn freeze(&self, _h: &StageHandle) -> Result<(), RuntimeError> { Ok(()) }
    }
}
