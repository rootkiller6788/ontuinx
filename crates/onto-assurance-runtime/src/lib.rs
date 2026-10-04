//! Onto Assurance Runtime — Port traits and transaction coordinator.
//!
//! The runtime crate defines the trait interfaces that the core kernel
//! uses to interact with the outside world.  Concrete implementations
//! (OntoRuntime adapters, test doubles) live in separate crates.
//!
//! Dependency direction: core ← runtime ← ironclaw-adapter
//! The runtime NEVER depends on OntoRuntime concrete types.

pub mod ports;
pub mod pipeline;
pub mod verdict;
pub mod plan;
pub mod artifact_reader;
pub mod graph_port_impl;
pub mod evidence_store;
pub mod event_publisher;
pub mod assurance_run;
pub mod staged_settlement;
pub mod database_settlement;
pub mod compensation_coordinator;
pub mod irreversible_dispatcher;
pub mod verification;

#[cfg(any(test, feature = "test-support"))]
pub mod mocks;
