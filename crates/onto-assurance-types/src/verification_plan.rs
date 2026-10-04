use serde::{Deserialize,Serialize}; use crate::scope_manifest::ScopeManifest; use crate::rule_binding::RuleBinding;
#[derive(Debug,Clone,Serialize,Deserialize)] pub struct VerificationPlan{pub plan_id:String,pub units:Vec<VerificationUnit>,pub scope:ScopeManifest,pub rules:Vec<RuleBinding>,pub estimated_tokens:u64,pub max_parallel_units:u32}
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)] pub struct VerificationUnit{pub unit_id:String,pub target_id:String,pub rule_set_id:String,pub verifier_kind:VerifierKind,pub priority:u32,pub depends_on:Vec<String>,#[serde(default)] pub metadata:serde_json::Value}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)] #[serde(rename_all="snake_case")] pub enum VerifierKind{Deterministic,Semantic,Hybrid}
