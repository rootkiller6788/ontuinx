use serde::{Deserialize,Serialize}; use crate::verification_plan::VerificationPlan;
#[derive(Debug,Clone,Serialize,Deserialize)] pub struct VerificationSession{pub session_id:String,pub plan:VerificationPlan,pub state:SessionState,pub completed_units:Vec<String>,pub failed_units:Vec<String>,pub findings:Vec<String>,pub tokens_used:u64,pub started_at:String,pub updated_at:String}
#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize)] #[serde(rename_all="snake_case")] pub enum SessionState{Created,Planning,Running,Paused,Completed,Failed,Cancelled}
impl SessionState{pub fn is_terminal(&self)->bool{matches!(self,Self::Completed|Self::Failed|Self::Cancelled)}}
