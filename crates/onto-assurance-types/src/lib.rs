//! Onto Assurance Kernel — Foundational Types
//!
//! Zero dependencies beyond serde, sha2, uuid, chrono, thiserror.
//! No OntoRuntime, Python, Docker, PostgreSQL, or any network/DB types.
//!
//! These types form the language-agnostic trust-transaction protocol.
//! Schema version: v1.

// ── Modules ──
pub mod ids;
pub mod enums;
pub mod contract;
pub mod decision;
pub mod evidence;
pub mod transaction;
pub mod hash;
pub mod ingress;
pub mod capability;
pub mod observation;
pub mod ontoloop;
pub mod external_effects;
pub mod profile;
pub mod verification_target;
pub mod scope_manifest;
pub mod verification_plan;
pub mod verification_session;
pub mod finding;
pub mod verification_budget;
pub mod rule_binding;
pub mod language_profile;
pub mod evidence_location;
pub mod verifier_run_result;
pub mod verification_ledger;
