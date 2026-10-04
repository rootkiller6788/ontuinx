use serde::{Deserialize,Serialize}; use crate::verification_target::VerificationTarget;
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)] pub struct ScopeManifest{pub targets:Vec<VerificationTarget>,pub excluded_patterns:Vec<String>,pub max_targets:usize,pub generated_at:String}
