//! Onto → OntoRuntime Adapter
//!
//! Concrete implementations of `onto_assurance_runtime::ports` traits
//! that bridge to OntoRuntime's real services.  Each adapter wraps the
//! corresponding OntoRuntime trait object and translates between Onto
//! domain types and OntoRuntime host types.
//!
//! ## OntoRuntime Trait Mapping
//!
//! | Onto Port | OntoRuntime Trait | Crate |
//! |-----------|---------------|-------|
//! | `AuthorizationPort` | `CapabilityDispatchAuthorizer` | `ironclaw_authorization` |
//! | `ApprovalPort` | `ApprovalRequestStorePort` | `ironclaw_approvals` |
//! | `RuntimePort` | `HostRuntime` | `ironclaw_host_runtime` |
//! | `VerifierPort` | (custom verifier worker) | — |
//! | `EvidenceStorePort` | `DurableEventLog` | `ironclaw_events` |
//! | `EventSinkPort` | `EventSink` | `ironclaw_events` |
//! | `CheckpointPort` | `RunStateStorePort` | `ironclaw_run_state` |
//! | `ClockPort` | `Utc::now()` | `chrono` |
//!
//! ## Builtin Hook Registration
//!
//! Onto registers as a `Builtin`-tier hook in OntoRuntime's hook system
//! and can only TIGHTEN OntoRuntime decisions, never relax them.

pub mod auth_adapter;
pub mod approval_adapter;
pub mod runtime_adapter;
pub mod event_adapter;
pub mod sandbox_executor;
pub mod attempt_executor_impl;
pub mod checkpoint_adapter;
pub mod finalization_adapter;
pub mod builtin_bridge;
#[cfg(any(test, feature = "test-support"))]
pub mod run_finalization_adapter;
pub mod staged_filesystem;
pub mod staged_reconciler;
pub mod run_ingress;
pub mod run_ingress_cli;
pub mod capability_descriptor;
pub mod envelope_capability_port;
pub mod model_capabilities;
pub mod runtime_observation;
pub mod database_adapter;
pub mod postgres_adapter;
pub mod pg_effect_store;
pub mod external_resource_service;
pub mod irreversible_message_service;
pub mod loop_adapter;
pub mod compensation_adapter;
pub mod irreversible_adapter;
pub mod direct_run;
pub mod redis_adapter;
pub mod outbox_publisher;
pub mod graph_read_capability;
pub mod graph_context_injector;
pub mod semantic_verifier_adapter;
pub mod attempt_run_port;
pub mod lane_isolation;
pub mod rollout_stage;
pub mod capability_gateway;
pub mod ironclaw_run_port;
pub mod production_verifiers;
pub mod assured_bridge;
#[cfg(feature = "pg-store")]
pub mod pg_assurance_store;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
