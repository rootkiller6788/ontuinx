//! Finding → Criterion 映射
use onto_assurance_types::contract::AcceptanceCriterion;
use onto_assurance_types::finding::{FindingCandidate, FindingCategory};

pub fn category_to_kind(c: FindingCategory) -> &'static str {
    match c { FindingCategory::Bug=>"test_pass", FindingCategory::Security=>"no_regression", FindingCategory::Performance=>"benchmark_ok", FindingCategory::Maintainability=>"lint_pass", FindingCategory::Test=>"test_pass", FindingCategory::Style=>"lint_pass", FindingCategory::Documentation=>"lint_pass", FindingCategory::Other=>"lint_pass" }
}

pub fn criterion_kind_to_str(k: &onto_assurance_types::contract::CriterionKind) -> &'static str {
    match k { onto_assurance_types::contract::CriterionKind::TestPass=>"test_pass", onto_assurance_types::contract::CriterionKind::LintPass=>"lint_pass", onto_assurance_types::contract::CriterionKind::TypeCheckPass=>"type_check_pass", onto_assurance_types::contract::CriterionKind::BenchmarkOk=>"benchmark_ok", onto_assurance_types::contract::CriterionKind::NoRegression=>"no_regression", onto_assurance_types::contract::CriterionKind::Custom(_)=>"lint_pass" }
}
pub fn matches_criterion(f: &FindingCandidate, c: &AcceptanceCriterion) -> bool { category_to_kind(f.category) == criterion_kind_to_str(&c.kind) }

pub fn group_by_criterion(fs: &[FindingCandidate], cs: &[AcceptanceCriterion]) -> Vec<(AcceptanceCriterion, Vec<FindingCandidate>)> {
    cs.iter().map(|c| { let m: Vec<_> = fs.iter().filter(|f| matches_criterion(f, c)).cloned().collect(); (c.clone(), m) }).collect()
}

#[cfg(test)] mod tests {
    use super::*;
    use onto_assurance_types::contract::CriterionKind;
    #[test] fn bug_maps_test_pass() { assert_eq!(category_to_kind(FindingCategory::Bug), "test_pass"); }
    #[test] fn groups_correctly() {
        let f = FindingCandidate { finding_id:"f".into(), target_id:"t".into(), rule_id:"r".into(), verifier_id:"v".into(), severity: onto_assurance_types::finding::FindingSeverity::Critical, category: FindingCategory::Bug, location: None, message: "b".into(), suggestion_code: None, confidence: 0.9 };
        let c = AcceptanceCriterion { criterion_id: onto_assurance_types::ids::CriterionId::new(), name: "T".into(), kind: CriterionKind::TestPass, description: "".into(), is_blocking: true };
        assert_eq!(group_by_criterion(&[f], &[c])[0].1.len(), 1);
    }
}
