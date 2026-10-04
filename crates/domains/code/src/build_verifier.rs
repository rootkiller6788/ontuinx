//! Build verifier — runs `gcc`, `cargo build`, `make`, etc.
//!
//! Captures exit code, stdout/stderr, and any build artifacts produced.
//! Pure deterministic driver — the actual subprocess execution is injected
//! via the `CommandRunner` trait so it can be mocked for testing.

use async_trait::async_trait;
use onto_assurance_types::ids::TransactionId;
use onto_pack_sdk::{PackVerificationReport, PackVerifier, PackVerifierError};
use onto_protocol::verifier::{
    Verifier, VerifierDescriptor, Pass, VerificationStage, VerificationMode, VerifierResult, VerifierStatus, VerifierServices,
};
use onto_protocol::context::VerificationContext;
use onto_protocol::check::ExternalCheckRequirement;
use onto_protocol::sandbox::RawCheckResult;

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
    proto_desc: VerifierDescriptor,
}

impl BuildVerifier {
    pub fn new(
        verifier_id: impl Into<String>,
        toolchain: impl Into<String>,
        build_command: impl Into<String>,
        runner: Box<dyn CommandRunner>,
    ) -> Self {
        let id: String = verifier_id.into();
        Self {
            verifier_id: id.clone(),
            version: "1.0.0".into(),
            toolchain: toolchain.into(),
            build_command: build_command.into(),
            runner,
            proto_desc: VerifierDescriptor {
                verifier_id: id,
                pass: Pass::Build,
                stage: VerificationStage::SandboxEvidence,
                mode: VerificationMode::ExternalEvidence,
                supported_rules: vec!["build.success".to_string()],
            },
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

// ── New unified Verifier trait impl ──

#[async_trait]
impl Verifier for BuildVerifier {
    fn descriptor(&self) -> &VerifierDescriptor { &self.proto_desc }
    fn external_requirements(&self, _: &VerificationContext) -> Vec<ExternalCheckRequirement> {
        vec![ExternalCheckRequirement {
            requirement_id: format!("{}-build", self.verifier_id),
            check_id: "project.build".to_string(),
            execution_dependencies: vec![],
            evidence_kind: onto_protocol::check::EvidenceKind::BuildOutput,
            artifact_scope: onto_protocol::check::ArtifactScope { paths: vec![], include_all: true },
        }]
    }
    async fn evaluate(
        &self, _: &VerificationContext, evidence: &[(&String, &RawCheckResult)], _: &VerifierServices<'_>,
    ) -> VerifierResult {
        let passed = evidence.iter().all(|(_, r)| matches!(r.status, onto_protocol::sandbox::CheckExecutionStatus::Exited { exit_code: 0 }));
        VerifierResult {
            verifier_id: self.verifier_id.clone(), pass: Pass::Build,
            status: VerifierStatus::Completed,
            findings: if !passed {
                vec![onto_protocol::finding::Finding {
                    finding_id: uuid::Uuid::new_v4().to_string(),
                    fingerprint: onto_protocol::finding::FindingFingerprint {
                        rule_id: "build.success".to_string(), entity_key: None,
                        artifact_path: String::new(), semantic_key: "build-failed".to_string(), line_hint: None,
                    },
                    pass: Pass::Build, rule_id: "build.success".to_string(), rule_version: "1.0".to_string(),
                    severity: onto_protocol::finding::FindingSeverity::Critical,
                    category: onto_protocol::finding::CategoryId::new("code.build"),
                    disposition: onto_protocol::finding::FindingDisposition::Blocking,
                    remediation: onto_protocol::finding::RemediationClass::RetryWithFeedback,
                    location: None, message: "build failed".to_string(), fix_hint: None,
                    confidence: 1.0, evidence_refs: vec![],
                }]
            } else { vec![] },
            raw_evidence: vec![], diagnostic: None,
        }
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
