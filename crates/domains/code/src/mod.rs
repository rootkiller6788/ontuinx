//! Code DomainPack — reference implementation proving the harness works.
//!
//! Scope: Git diff + full repository scan.
//! Rules: 26 language-specific semantic rule files (OCR system ported).
//! Verifiers: 7 languages × 4 levels of machine verification.
//! Location: Dual-channel (hunk + file scan) resolution.
//!
//! This is NOT the product. It's a verified example of DomainPack + MachineVerifier.
//! Production users write their own packs using the harness traits.

pub mod scope;
pub mod location;
pub mod rules;
pub mod verifiers;
pub mod build_verifier;
pub mod test_verifier;
pub mod lint_verifier;
pub mod machine_verifiers;
pub mod file_verifiers;
pub mod artifact_collector;
pub mod project_profiler;
pub mod pg_stores;
pub mod artifact_store;
pub mod observability;
