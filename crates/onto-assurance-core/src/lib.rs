//! Onto Assurance Kernel — Deterministic Pure Functions
//!
//! Zero dependencies on OntoRuntime, Python, Docker, PostgreSQL, or any
//! network/DB types.  Every function here is deterministic: same input
//! always produces the same output, regardless of language runtime.
//!

pub mod canonical;
pub mod evidence_chain;
pub mod reduction;
pub mod session_decision;
pub mod attempt_decision;
pub mod checkpoint;
pub mod replay;
pub mod invalidation;
pub mod effect_classifier;
pub mod settlement;
pub mod verification;
pub mod location_resolution;
