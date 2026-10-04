use serde::{Deserialize,Serialize};
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq)]
pub struct FindingCandidate{pub finding_id:String,pub target_id:String,pub rule_id:String,pub verifier_id:String,pub severity:FindingSeverity,pub category:FindingCategory,pub location:Option<SourceLocation>,pub message:String,pub suggestion_code:Option<String>,pub confidence:f64}
#[derive(Debug,Clone,Copy,PartialEq,Eq,PartialOrd,Ord,Serialize,Deserialize)] #[serde(rename_all="snake_case")]
pub enum FindingSeverity{Critical=4,High=3,Medium=2,Low=1}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)] #[serde(rename_all="snake_case")]
pub enum FindingCategory{Bug,Security,Performance,Maintainability,Test,Style,Documentation,Other}
impl FindingCategory{pub fn is_blocking(&self)->bool{matches!(self,Self::Bug|Self::Security)}}
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct SourceLocation{pub file_path:String,pub start_line:u32,pub end_line:u32,pub start_column:Option<u32>,pub end_column:Option<u32>,pub existing_code:Option<String>,pub suggestion_code:Option<String>}
impl FindingCandidate{pub fn is_blocking(&self)->bool{self.severity>=FindingSeverity::High||self.category.is_blocking()}}
#[derive(Debug,Clone,Serialize,Deserialize)] pub struct CodeReviewResult{pub relevant_file:String,pub suggestion_content:String,pub existing_code:String,pub suggestion_code:String}
