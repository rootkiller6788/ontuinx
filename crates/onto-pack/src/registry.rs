use std::sync::Arc;
use crate::domain_pack::DomainPack;
use crate::locations::LocationResolver;
use crate::criteria::CriterionMapper;
use crate::rules::RuleSelector;
use crate::scope::ScopeProvider;

pub struct DomainRegistry {
    pub packs: Vec<Arc<dyn DomainPack>>,
    pub location_resolvers: Vec<Arc<dyn LocationResolver>>,
    pub criterion_mappers: Vec<Arc<dyn CriterionMapper>>,
    pub rule_selectors: Vec<Arc<dyn RuleSelector>>,
    pub scope_providers: Vec<Arc<dyn ScopeProvider>>,
}

impl DomainRegistry {
    pub fn new() -> Self { Self { packs: vec![], location_resolvers: vec![], criterion_mappers: vec![], rule_selectors: vec![], scope_providers: vec![] } }
    pub fn find_resolver(&self, scheme: &str) -> Option<&dyn LocationResolver> {
        let mut cs: Vec<_> = self.location_resolvers.iter().filter(|r| r.supported_schemes().iter().any(|s| s == scheme)).map(|r| r.as_ref()).collect();
        cs.sort_by_key(|r| -r.priority());
        cs.into_iter().next()
    }
}

impl Default for DomainRegistry { fn default() -> Self { Self::new() } }

pub trait DomainPlugin: Send + Sync {
    fn domain_id(&self) -> &str;
    fn domain_name(&self) -> &str;
    fn register(&self, registry: &mut DomainRegistry) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn registry_starts_empty() { let r = DomainRegistry::new(); assert!(r.packs.is_empty()); }
    #[test] fn unresolved_scheme_none() { assert!(DomainRegistry::new().find_resolver("x.v1").is_none()); }
}
