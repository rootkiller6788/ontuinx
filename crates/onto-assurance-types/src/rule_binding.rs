use serde::{Deserialize,Serialize};
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)] pub struct RuleBinding{pub rule_set_id:String,pub rule_set_name:String,pub target_ids:Vec<String>,pub severity:RuleSeverity,pub parameters:serde_json::Value}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)] #[serde(rename_all="snake_case")] pub enum RuleSeverity{Blocking,Advisory,Low}
impl RuleSeverity{pub fn is_blocking(&self)->bool{matches!(self,Self::Blocking)}}
