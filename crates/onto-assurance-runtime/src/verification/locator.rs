use onto_assurance_core::verification::location_binding;
use onto_assurance_types::finding::FindingCandidate;
pub struct LocationResolverService;
impl LocationResolverService { pub fn new() -> Self { Self } pub fn resolve(&self, f: &FindingCandidate, offset: i64) -> FindingCandidate { location_binding::resolve_location(f, offset) } pub fn validate(&self, f: &FindingCandidate) -> bool { f.location.as_ref().map(|l| location_binding::is_valid_location(l)).unwrap_or(false) } }
#[cfg(test)] mod tests { use super::*; use onto_assurance_types::finding::{FindingCategory,FindingSeverity,SourceLocation};
    fn mf() -> FindingCandidate { FindingCandidate{finding_id:"f".into(),target_id:"t".into(),rule_id:"r".into(),verifier_id:"v".into(),severity:FindingSeverity::High,category:FindingCategory::Other,location:Some(SourceLocation{file_path:"f.rs".into(),start_line:10,end_line:10,start_column:None,end_column:None,existing_code:None,suggestion_code:None}),message:"m".into(),suggestion_code:None,confidence:0.9} }
    #[test] fn resolves() { let r = LocationResolverService::new().resolve(&mf(), 5); assert_eq!(r.location.unwrap().start_line, 15); }
}
