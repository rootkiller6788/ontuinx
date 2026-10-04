//! OntoFlow Adapter — bridges OntoFlow ActivityTask to Rust OntoLoop Worker.
//!
//! F2: Authority Resolution — AuthorityProjectionPort for Go-side verification.
//!
//! Go OntoFlow dispatches WorkItems as OntoFlow ActivityTasks.
//! Rust OntoLoop Workers execute them using existing OntoLoop + OntoRuntime infrastructure.
//! Go verifies Worker reports via AuthorityProjectionPort before accepting Committed.
//!
//! ## Crate map
//!
//! - `protocol`     — LoopInvocationRequest / LoopTerminalEnvelope / OntoLoopHeartbeat
//! - `worker`       — OntoLoopWorker trait + WorkerError
//! - `loop_runner`  — RuntimeLoopRunner: orchestrate request → attempts → envelope
//! - `heartbeat`    — HeartbeatSender trait + MockHeartbeatSender
//! - `idempotency`  — IdempotencyStore trait + InMemoryIdempotencyStore
//! - `authority_projection` — AuthorityProjectionPort + InMemoryAuthorityProjection
//! - `lease`      — LoopExecutionLease + InMemoryLoopLease
//! - `dag_simulation` — DAG node execution tests (F4)
//! - `planning`   — Dynamic task splitting + plan validation (F7)
//! - `discussion` — Multi-round decentralized discussion (F8)
//! - `agent_task` — Native AgentWorkItemTask types (F9, v1.0 optional)

pub mod protocol;
pub mod worker;
pub mod loop_runner;
pub mod heartbeat;
pub mod idempotency;
pub mod authority_projection;
pub mod lease;
pub mod dag_simulation;
pub mod assured_coordinator;
pub mod progress_store;
pub mod production;
pub mod planning;
pub mod discussion;
pub mod agent_task;

pub use authority_projection::{
    AuthorityProjectionPort, InMemoryAuthorityProjection,
    ResolveLoopOutcomeRequest, VerifiedLoopOutcome, VerifiedOutcome,
};
pub use heartbeat::{HeartbeatSender, MockHeartbeatSender};
pub use idempotency::{IdempotencyStore, InMemoryIdempotencyStore};
pub use loop_runner::RuntimeLoopRunner;
pub use protocol::{
    LoopInvocationRequest, LoopTerminalEnvelope, LoopTerminalState, LoopLifecycle,
    OntoLoopHeartbeat,
};
pub use worker::{OntoLoopWorker, WorkerError};
