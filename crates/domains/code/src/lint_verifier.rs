//! Lint verifier — runs static analysis tools (clippy, ruff, eslint, shellcheck).
//! Uses `CommandRunner` for mockable subprocess execution.

use onto_assurance_types::ids::TransactionId;
use onto_pack_sdk::{CriterionCheckResult, PackVerificationReport, PackVerifier, PackVerifierError};
use crate::build_verifier::CommandRunner;

pub struct LintVerifier {
    verifier_id: String, version: String, toolchain: String,
    lint_command: String, lint_args: Vec<String>, runner: Box<dyn CommandRunner>,
}

impl LintVerifier {
    pub fn new(id: impl Into<String>, toolchain: impl Into<String>,
        cmd: impl Into<String>, args: Vec<String>, runner: Box<dyn CommandRunner>) -> Self {
        Self { verifier_id: id.into(), version: "1.0.0".into(), toolchain: toolchain.into(),
            lint_command: cmd.into(), lint_args: args, runner }
    }
    pub fn clippy(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("code-lint-clippy", "rustc", "cargo", vec!["clippy".into()], runner)
    }
    pub fn ruff(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("code-lint-ruff", "python", "ruff", vec!["check".into()], runner)
    }
}

#[async_trait::async_trait]
impl PackVerifier for LintVerifier {
    fn verifier_id(&self) -> &str { &self.verifier_id }
    fn version(&self) -> &str { &self.version }
    fn toolchain(&self) -> Option<&str> { Some(&self.toolchain) }

    async fn verify(&self, transaction_id: TransactionId, workspace_path: &str,
        criteria: &[onto_assurance_types::contract::AcceptanceCriterion],
    ) -> Result<PackVerificationReport, PackVerifierError> {
        let args: Vec<&str> = self.lint_args.iter().map(|s| s.as_str()).collect();
        let output = self.runner.run(&self.lint_command, &args, workspace_path)
            .map_err(|e| PackVerifierError::ExecutionFailed(e.to_string()))?;
        let passed = output.exit_code == 0;
        let per_criterion: Vec<_> = criteria.iter().map(|c| CriterionCheckResult {
            criterion_id: c.criterion_id.to_string(), criterion_name: c.name.clone(),
            satisfied: passed,
            detail: if passed { "no lints".into() } else { format!("lint exit code {}", output.exit_code) },
        }).collect();
        let total = criteria.len() as u32;
        Ok(PackVerificationReport { transaction_id, verifier_id: self.verifier_id.clone(),
            verifier_version: self.version.clone(), toolchain: Some(self.toolchain.clone()),
            passed, total_checks: total, passed_checks: if passed { total } else { 0 },
            failed_checks: if passed { 0 } else { total }, skipped_checks: 0,
            per_criterion, artifacts: vec![],
            raw_output: format!("[stdout]\n{}\n[stderr]\n{}", output.stdout, output.stderr),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_verifier::MockCommandRunner;
    use onto_assurance_types::contract::{AcceptanceCriterion, CriterionKind};
    use onto_assurance_types::ids::CriterionId;

    fn c() -> Vec<AcceptanceCriterion> { vec![AcceptanceCriterion {
        criterion_id: CriterionId::new(), name: "lint".into(), kind: CriterionKind::LintPass,
        description: "".into(), is_blocking: false }] }

    #[tokio::test]
    async fn lint_pass() {
        let r = MockCommandRunner { exit_code: 0, stdout: String::new(), stderr: String::new() };
        let report = LintVerifier::clippy(Box::new(r)).verify(TransactionId::new(), "/t", &c()).await.unwrap();
        assert!(report.passed);
    }

    #[tokio::test]
    async fn lint_fail() {
        let r = MockCommandRunner { exit_code: 1, stdout: String::new(), stderr: "warning: ...".into() };
        let report = LintVerifier::ruff(Box::new(r)).verify(TransactionId::new(), "/t", &c()).await.unwrap();
        assert!(!report.passed);
    }
}
