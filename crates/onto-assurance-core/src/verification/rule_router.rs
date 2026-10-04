//! 确定性规则路由
use onto_assurance_types::language_profile::LanguageProfile;
use onto_assurance_types::rule_binding::{RuleBinding, RuleSeverity};
use onto_assurance_types::verification_target::VerificationTarget;

pub fn route_rules(targets: &[VerificationTarget], profiles: &[LanguageProfile], base: &[RuleBinding]) -> Vec<RuleBinding> {
    let mut bindings = vec![];
    for t in targets {
        let ext = t.target_ref.rsplit('.').next().unwrap_or("");
        let prof = profiles.iter().find(|p| p.matches_extension(ext));
        for r in base {
            if r.target_ids.is_empty() || r.target_ids.contains(&t.target_id) {
                bindings.push(RuleBinding { rule_set_id: r.rule_set_id.clone(), rule_set_name: r.rule_set_name.clone(), target_ids: vec![t.target_id.clone()], severity: r.severity, parameters: r.parameters.clone() });
            }
        }
        if let Some(p) = prof {
            for rs in &p.default_rule_sets {
                bindings.push(RuleBinding { rule_set_id: rs.clone(), rule_set_name: rs.clone(), target_ids: vec![t.target_id.clone()], severity: RuleSeverity::Blocking, parameters: serde_json::json!({}) });
            }
        }
    }
    bindings
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::verification_target::TargetKind;

    #[test]
    fn rust_file_gets_rust_rules() {
        let t = VerificationTarget::new("1", TargetKind::SourceFile, "src/main.rs", "h");
        let p = vec![LanguageProfile::rust()];
        let b = route_rules(&[t], &p, &[]);
        assert!(b.iter().any(|r| r.rule_set_id == "rust-clippy"));
    }
}
