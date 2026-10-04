//! Authorization adapter — bridges onto AuthorizationPort to OntoRuntime.
//!
//! Default: StubAuthorizationAdapter (standalone, no OntoRuntime deps).
//! With `--features ironclaw-integration`: wraps ironclaw_authorization.

use async_trait::async_trait;
use onto_assurance_runtime::ports::{
    AuthorizationError, AuthorizationPort, AuthorizationReceipt,
};
use onto_assurance_types::contract::ExecutionIntent;

// ══════════════════════════════════════════════════════════════════
// Stub (default — no OntoRuntime deps)
// ══════════════════════════════════════════════════════════════════

pub struct StubAuthorizationAdapter {
    pub always_allow: bool,
}

impl Default for StubAuthorizationAdapter {
    fn default() -> Self { Self { always_allow: true } }
}

#[async_trait]
impl AuthorizationPort for StubAuthorizationAdapter {
    async fn authorize(&self, _intent: &ExecutionIntent) -> Result<AuthorizationReceipt, AuthorizationError> {
        Ok(AuthorizationReceipt {
            run_id: onto_assurance_types::ids::RunId::new(),
            authorized: self.always_allow,
            grant_id: "stub-grant".into(),
            lease_id: Some("stub-lease".into()),
            restrictions: vec![],
        })
    }

    async fn check_valid(&self, receipt: &AuthorizationReceipt) -> Result<bool, AuthorizationError> {
        Ok(receipt.authorized)
    }
}

// ══════════════════════════════════════════════════════════════════
// OntoRuntime adapter (requires `--features ironclaw-integration`)
// ══════════════════════════════════════════════════════════════════

#[cfg(feature = "ironclaw-integration")]
pub mod ironclaw {
    use super::*;
    use ironclaw_authorization::CapabilityDispatchAuthorizer;
    use ironclaw_host_api::{
        CapabilityDescriptor, CapabilityId, CapabilitySet, Decision, ExecutionContext,
        ExtensionId, MountView, PermissionMode, ResourceEstimate, RuntimeKind, TrustClass, UserId,
    };

    pub struct AuthorizationAdapter<A: CapabilityDispatchAuthorizer> {
        pub inner: A,
        pub user_id: UserId,
        pub extension_id: ExtensionId,
        pub runtime: RuntimeKind,
        pub trust: TrustClass,
        pub grants: CapabilitySet,
        pub mounts: MountView,
    }

    impl<A: CapabilityDispatchAuthorizer> AuthorizationAdapter<A> {
        pub fn new(
            inner: A,
            user_id: UserId,
            extension_id: ExtensionId,
            runtime: RuntimeKind,
            trust: TrustClass,
            grants: CapabilitySet,
            mounts: MountView,
        ) -> Self {
            Self { inner, user_id, extension_id, runtime, trust, grants, mounts }
        }

        fn to_context(&self) -> Result<ExecutionContext, ironclaw_host_api::HostApiError> {
            ExecutionContext::local_default(
                self.user_id.clone(),
                self.extension_id.clone(),
                self.runtime,
                self.trust,
                self.grants.clone(),
                self.mounts.clone(),
            )
        }

        fn to_descriptor() -> CapabilityDescriptor {
            CapabilityDescriptor {
                id: CapabilityId::new("onto.assurance.authorize").expect("valid capability id"),
                provider: ExtensionId::new("onto").expect("valid extension id"),
                runtime: RuntimeKind::System,
                trust_ceiling: TrustClass::System,
                description: "Onto Assurance Kernel — authorization gate".into(),
                parameters_schema: serde_json::Value::Object(Default::default()),
                effects: vec![],
                default_permission: PermissionMode::Allow,
                runtime_credentials: vec![],
                network_targets: vec![],
                max_egress_bytes: None,
                resource_profile: None,
                origin_gate_matrix: None,
            }
        }

        fn map_decision(decision: &Decision) -> AuthorizationReceipt {
            match decision {
                Decision::Allow { .. } => AuthorizationReceipt {
                    run_id: onto_assurance_types::ids::RunId::new(),
                    authorized: true,
                    grant_id: "ironclaw-grant".into(),
                    lease_id: None,
                    restrictions: vec![],
                },
                Decision::Deny { reason } => AuthorizationReceipt {
                    run_id: onto_assurance_types::ids::RunId::new(),
                    authorized: false,
                    grant_id: "denied".into(),
                    lease_id: None,
                    restrictions: vec![format!("denied: {:?}", reason)],
                },
                Decision::RequireApproval { .. } => AuthorizationReceipt {
                    run_id: onto_assurance_types::ids::RunId::new(),
                    authorized: false,
                    grant_id: "requires-approval".into(),
                    lease_id: None,
                    restrictions: vec!["requires_approval".into()],
                },
            }
        }
    }

    #[async_trait]
    impl<A: CapabilityDispatchAuthorizer + Send + Sync> AuthorizationPort for AuthorizationAdapter<A> {
        async fn authorize(&self, _intent: &ExecutionIntent) -> Result<AuthorizationReceipt, AuthorizationError> {
            let context = self.to_context().map_err(|e| AuthorizationError::Host(e.to_string()))?;
            let descriptor = Self::to_descriptor();
            let estimate = ResourceEstimate::default();
            let decision = self.inner.authorize_dispatch(&context, &descriptor, &estimate).await;
            Ok(Self::map_decision(&decision))
        }

        async fn check_valid(&self, receipt: &AuthorizationReceipt) -> Result<bool, AuthorizationError> {
            Ok(receipt.authorized)
        }
    }
}
