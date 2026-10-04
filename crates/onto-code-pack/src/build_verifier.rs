//! Build verifier — runs `gcc`, `cargo build`, `make`, etc.
//!
//! Captures exit code, stdout/stderr, and any build artifacts produced.
//! Pure deterministic driver — the actual subprocess execution is injected
//! via the `CommandRunner` trait so it can be mocked for testing.

use onto_assurance_types::ids::TransactionId;
use onto_pack_sdk::{PackVerificationReport, PackVerifier, PackVerifierError};

// ══════════════════════════════════════════════════════════════════
// CommandRunner — abstraction over subprocess execution
// ══════════════════════════════════════════════════════════════════

/// Trait for running external commands.  Allows mocking for tests.
pub trait CommandRunner: Send + Sync {
    fn run(&self, command: &str, args: &[&str], working_dir: &str)
        -> Result<CommandOutput, CommandError>;
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("command not found: {0}")]
    NotFound(String),
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
}

// ══════════════════════════════════════════════════════════════════
// BuildVerifier
// ══════════════════════════════════════════════════════════════════

pub struct BuildVerifier {
    verifier_id: String,
    version: String,
    toolchain: String,
    build_command: String,
    runner: Box<dyn CommandRunner>,
}

impl BuildVerifier {
    pub fn new(
        verifier_id: impl Into<String>,
        toolchain: impl Into<String>,
        build_command: impl Into<String>,
        runner: Box<dyn CommandRunner>,
    ) -> Self {
        Self {
            verifier_id: verifier_id.into(),
            version: "1.0.0".into(),
            toolchain: toolchain.into(),
            build_command: build_command.into(),
            runner,
        }
    }

    pub fn gcc(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("code-build-gcc", "gcc", "gcc", runner)
    }

    pub fn cargo(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("code-build-cargo", "rustc", "cargo build", runner)
    }

    pub fn make(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("code-build-make", "make", "make", runner)
    }
}

#[async_trait::async_trait]
impl PackVerifier for BuildVerifier {
    fn verifier_id(&self) -> &str { &self.verifier_id }
    fn version(&self) -> &str { &self.version }
    fn toolchain(&self) -> Option<&str> { Some(&self.toolchain) }

    async fn verify(
        &self,
        transaction_id: TransactionId,
        workspace_path: &str,
        criteria: &[onto_assurance_types::contract::AcceptanceCriterion],
    ) -> Result<PackVerificationReport, PackVerifierError> {
        let output = self.runner.run(&self.build_command, &[], workspace_path)
            .map_err(|e| PackVerifierError::ExecutionFailed(e.to_string()))?;

        let passed = output.exit_code == 0;
        let per_criterion: Vec<_> = criteria.iter().map(|c| {
            onto_pack_sdk::CriterionCheckResult {
                criterion_id: c.criterion_id.to_string(),
                criterion_name: c.name.clone(),
                satisfied: passed,
                detail: if passed {
                    "build succeeded".into()
                } else {
                    format!("build failed with exit code {}", output.exit_code)
                },
            }
        }).collect();

        let total = criteria.len() as u32;
        let passed_count = if passed { total } else { 0 };
        let failed_count = if passed { 0 } else { total };
        Ok(PackVerificationReport {
            transaction_id,
            verifier_id: self.verifier_id.clone(),
            verifier_version: self.version.clone(),
            toolchain: Some(self.toolchain.clone()),
            passed,
            total_checks: total,
            passed_checks: passed_count,
            failed_checks: failed_count,
            skipped_checks: 0,
            per_criterion,
            artifacts: vec![],
            raw_output: format!("[stdout]\n{}\n[stderr]\n{}", output.stdout, output.stderr),
        })
    }
}

// ══════════════════════════════════════════════════════════════════
// Mock runner for testing
// ══════════════════════════════════════════════════════════════════

#[cfg(any(test, feature = "test-support"))]
pub struct MockCommandRunner {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[cfg(any(test, feature = "test-support"))]
impl CommandRunner for MockCommandRunner {
    fn run(&self, _command: &str, _args: &[&str], _working_dir: &str) -> Result<CommandOutput, CommandError> {
        Ok(CommandOutput {
            exit_code: self.exit_code,
            stdout: self.stdout.clone(),
            stderr: self.stderr.clone(),
            duration_ms: 100,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onto_assurance_types::contract::{AcceptanceCriterion, CriterionKind};
    use onto_assurance_types::ids::CriterionId;

    fn make_criteria() -> Vec<AcceptanceCriterion> {
        vec![AcceptanceCriterion {
            criterion_id: CriterionId::new(),
            name: "build".into(),
            kind: CriterionKind::TestPass,
            description: "project must compile".into(),
            is_blocking: true,
        }]
    }

    #[tokio::test]
    async fn build_success_reports_pass() {
        let runner = MockCommandRunner { exit_code: 0, stdout: "OK".into(), stderr: String::new() };
        let verifier = BuildVerifier::gcc(Box::new(runner));

        let report = verifier.verify(
            TransactionId::new(), "/tmp/project", &make_criteria(),
        ).await.unwrap();

        assert!(report.passed);
        assert_eq!(report.passed_checks, 1);
    }

    #[tokio::test]
    async fn build_failure_reports_fail() {
        let runner = MockCommandRunner {
            exit_code: 1,
            stdout: String::new(),
            stderr: "error: undefined reference".into(),
        };
        let verifier = BuildVerifier::gcc(Box::new(runner));

        let report = verifier.verify(
            TransactionId::new(), "/tmp/project", &make_criteria(),
        ).await.unwrap();

        assert!(!report.passed);
        assert_eq!(report.failed_checks, 1);
    }
}
