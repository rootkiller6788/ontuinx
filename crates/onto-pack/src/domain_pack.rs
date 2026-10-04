//! Domain SPI — the contract every domain plugin must fulfill.
//!
//! All traits defined here. All implementations in domain crates.
//! Harness never imports domain crates.

use async_trait::async_trait;
use onto_assurance_types::finding::FindingCandidate;
use onto_assurance_types::verification_plan::VerificationUnit;
use onto_assurance_types::verification_target::VerificationTarget;
use onto_assurance_types::contract::AcceptanceCriterion;
use std::sync::Arc;

// ══════════════════════════════════════════════════════════════════
// DomainPack — minimal contract (already defined, kept stable)
// ══════════════════════════════════════════════════════════════════

pub trait DomainPack: Send + Sync {
    fn domain_id(&self) -> &str;
    fn domain_name(&self) -> &str;
    fn supported_kinds(&self) -> Vec<String>;
    fn verify(&self, unit: &VerificationUnit) -> Result<Vec<FindingCandidate>, String>;
}

// ══════════════════════════════════════════════════════════════════
// LocationResolver — untrusted hint → authoritative anchor
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocationHint {
    pub scheme: String,           // "code.span.v1" | "document.section.v1" | ...
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LocationResolution {
    Resolved { scheme: String, anchor: serde_json::Value, confidence: f64 },
    Ambiguous { candidates: Vec<serde_json::Value>, reason: String },
    Unresolved { reason: String },
    Unsupported { scheme: String, reason: String },
}

#[async_trait]
pub trait LocationResolver: Send + Sync {
    fn resolver_id(&self) -> &str;
    fn supported_schemes(&self) -> Vec<String>;
    fn priority(&self) -> i32 { 0 }
    async fn resolve(&self, target: &VerificationTarget, hint: &LocationHint) -> Result<LocationResolution, String>;
}

// ══════════════════════════════════════════════════════════════════
// CriterionMapper — Finding → Contract Criteria
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct CriterionDraft {
    pub kind: String,
    pub description: String,
}

#[async_trait]
pub trait CriterionMapper: Send + Sync {
    fn mapper_id(&self) -> &str;
    fn supported_categories(&self) -> Vec<String>;
    fn map(&self, finding: &FindingCandidate, available: &[AcceptanceCriterion]) -> Result<Vec<CriterionDraft>, String>;
}

// ══════════════════════════════════════════════════════════════════
// RuleSelector — select rules for a target
// ══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct RuleBinding {
    pub rule_id: String,
    pub target_ref: String,
    pub rule_content: String,
    pub rule_hash: String,
}

#[async_trait]
pub trait RuleSelector: Send + Sync {
    fn selector_id(&self) -> &str;
    fn select(&self, target: &VerificationTarget) -> Result<Vec<RuleBinding>, String>;
}

// ══════════════════════════════════════════════════════════════════
// ScopeProvider — enumerate targets in a domain
// ══════════════════════════════════════════════════════════════════

#[async_trait]
pub trait ScopeProvider: Send + Sync {
    fn provider_id(&self) -> &str;
    fn supported_kinds(&self) -> Vec<String>;
    async fn enumerate(&self, root: &str, spec: &serde_json::Value) -> Result<Vec<VerificationTarget>, String>;
}

// ══════════════════════════════════════════════════════════════════
// DomainRegistry — collects all plugins. Owned by composition root.
// ══════════════════════════════════════════════════════════════════

pub struct DomainRegistry {
    pub packs: Vec<Arc<dyn DomainPack>>,
    pub location_resolvers: Vec<Arc<dyn LocationResolver>>,
    pub criterion_mappers: Vec<Arc<dyn CriterionMapper>>,
    pub rule_selectors: Vec<Arc<dyn RuleSelector>>,
    pub scope_providers: Vec<Arc<dyn ScopeProvider>>,
}

impl DomainRegistry {
    pub fn new() -> Self {
        Self { packs: vec![], location_resolvers: vec![], criterion_mappers: vec![], rule_selectors: vec![], scope_providers: vec![] }
    }

    pub fn find_resolver(&self, scheme: &str) -> Option<&dyn LocationResolver> {
        let mut cs: Vec<_> = self.location_resolvers.iter().filter(|r| r.supported_schemes().iter().any(|s| s == scheme)).map(|r| r.as_ref()).collect();
        cs.sort_by_key(|r| -r.priority());
        cs.into_iter().next()
    }
}

impl Default for DomainRegistry { fn default() -> Self { Self::new() } }

// ══════════════════════════════════════════════════════════════════
// DomainPlugin — single entry point. Called once at startup.
// ══════════════════════════════════════════════════════════════════

pub trait DomainPlugin: Send + Sync {
    fn domain_id(&self) -> &str;
    fn domain_name(&self) -> &str;
    fn register(&self, registry: &mut DomainRegistry) -> Result<(), String>;
}

// ══════════════════════════════════════════════════════════════════
// Tests — SPI compiles, registry works
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_starts_empty() {
        let r = DomainRegistry::new();
        assert!(r.packs.is_empty());
        assert!(r.location_resolvers.is_empty());
    }

    #[test]
    fn unresolved_scheme_returns_none() {
        assert!(DomainRegistry::new().find_resolver("unknown.scheme.v1").is_none());
    }
}
