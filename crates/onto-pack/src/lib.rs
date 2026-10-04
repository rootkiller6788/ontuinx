//! Onto Pack SDK — Assurance SPI (Service Provider Interface).
//!
//! Defines the contract every domain plugin must fulfill.
//! No domain implementations — those are in `crates/domains/`.
//!
//! ## Modules
//!
//!   domain_pack.rs  — DomainPack trait (minimal contract)
//!   locations.rs    — LocationResolver trait + LocationHint/Resolution types
//!   criteria.rs     — CriterionMapper trait + CriterionDraft
//!   rules.rs        — RuleSelector trait + RuleBinding
//!   scope.rs        — ScopeProvider trait
//!   registry.rs     — DomainRegistry + DomainPlugin
//!   profile/        — VerificationProfile types (dual-plane routing)
//!
//! ## Adding a new domain
//!
//!   1. Create `crates/domains/<name>/` with Cargo.toml
//!   2. Implement `DomainPlugin` (register verifiers, resolvers, mappers)
//!   3. Add to workspace Cargo.toml
//!   4. Register in composition root
//!
//! No changes needed to `onto-pack` itself.

pub mod domain_pack;
pub mod locations;
pub mod criteria;
pub mod rules;
pub mod scope;
pub mod registry;
pub mod profile;

// Re-exports for convenience
pub use domain_pack::DomainPack;
pub use registry::{DomainRegistry, DomainPlugin};
pub use locations::{LocationResolver, LocationHint, LocationResolution};
pub use criteria::{CriterionMapper, CriterionDraft};
pub use rules::{RuleSelector, RuleBinding};
pub use scope::ScopeProvider;
