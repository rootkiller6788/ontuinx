use onto_assurance_types::scope_manifest::ScopeManifest; use onto_assurance_types::rule_binding::RuleBinding; use onto_assurance_types::verification_plan::{VerificationPlan,VerificationUnit,VerifierKind};
pub trait PlanGenerator: Send+Sync { fn generate(&self, scope: &ScopeManifest, rules: &[RuleBinding], max_parallel: u32) -> VerificationPlan; }
pub struct SimplePlanGenerator;
impl PlanGenerator for SimplePlanGenerator {
    fn generate(&self, scope: &ScopeManifest, rules: &[RuleBinding], max_parallel: u32) -> VerificationPlan {
        let mut units = vec![]; let mut n = 0;
        for t in &scope.targets { for r in rules { if r.target_ids.is_empty() || r.target_ids.contains(&t.target_id) { n+=1; units.push(VerificationUnit{unit_id:format!("unit-{}",n),target_id:t.target_id.clone(),rule_set_id:r.rule_set_id.clone(),verifier_kind:VerifierKind::Deterministic,priority:if r.severity.is_blocking(){1}else{10},depends_on:vec![],metadata:serde_json::Value::default()}); } } }
        VerificationPlan{plan_id:format!("plan-{}",chrono::Utc::now().timestamp_millis()),units,scope:scope.clone(),rules:rules.to_vec(),estimated_tokens:n as u64*2000,max_parallel_units:max_parallel}
    }
}
#[cfg(test)] mod tests { use super::*; use onto_assurance_types::verification_target::VerificationTarget; use onto_assurance_types::rule_binding::RuleSeverity;
    #[test] fn generates_units() { let s = ScopeManifest{targets:vec![VerificationTarget::new("t1", onto_assurance_types::verification_target::TargetKind::SourceFile, "a.rs", "h")],excluded_patterns:vec![],max_targets:10,generated_at:"n".into()}; let r = vec![RuleBinding{rule_set_id:"r1".into(),rule_set_name:"R1".into(),target_ids:vec!["t1".into()],severity:RuleSeverity::Blocking,parameters:serde_json::json!({})}]; let p = SimplePlanGenerator.generate(&s,&r,2); assert_eq!(p.units.len(),1); }
}
