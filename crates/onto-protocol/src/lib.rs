//! OntoProtocol — single source of truth for cross-layer types.
//!
//! This crate defines ALL protocol types used across L1 OntoAssure,
//! L2 OntoRuntime, and L3 OntoLoop. Each layer depends on this crate
//! instead of defining its own incompatible version of the same concepts.

pub mod digest;
pub mod envelope;
pub mod persistence;
pub mod candidate;
pub mod context;
pub mod verifier;
pub mod finding;
pub mod check;
pub mod verdict;
pub mod sandbox;
pub mod loop_protocol;
pub mod progress;
pub mod graph_port;
pub mod executor;
pub mod events;
