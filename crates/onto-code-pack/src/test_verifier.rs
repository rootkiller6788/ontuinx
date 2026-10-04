//! Test verifier — runs test frameworks and parses results.
//! Supports: CargoTest, Pytest, CTest. Uses `CommandRunner` for mockable execution.

use onto_assurance_types::ids::TransactionId;
use onto_pack_sdk::{CriterionCheckResult, PackVerificationReport, PackVerifier, PackVerifierError};
use crate::build_verifier::{CommandRunner, CommandOutput};

pub struct TestVerifier {
    verifier_id: String,
    version: String,
    toolchain: String,
    test_command: String,
    test_args: Vec<String>,
    runner: Box<dyn CommandRunner>,
}

impl TestVerifier {
    pub fn new(
        verifier_id: impl Into<String>, toolchain: impl Into<String>,
        test_command: impl Into<String>, test_args: Vec<String>,
        runner: Box<dyn CommandRunner>,
    ) -> Self {
        Self { verifier_id: verifier_id.into(), version: "1.0.0".into(),
            toolchain: toolchain.into(), test_command: test_command.into(), test_args, runner }
    }

    pub fn cargo_test(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("code-test-cargo", "rustc", "cargo", vec!["test".into()], runner)
    }
    pub fn pytest(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("code-test-pytest", "python", "pytest", vec!["-v".into()], runner)
    }
    pub fn ctest(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("code-test-ctest", "ctest", "ctest", vec!["--output-on-failure".into()], runner)
    }

    fn parse_counts(output: &CommandOutput) -> (u32, u32) {
        let combined = format!("{}\n{}", output.stdout, output.stderr);
        // Cargo: "test result: ok. 9 passed; 0 failed"
        for line in combined.lines() {
            if line.contains("test result:") {
                let passed = extract_num(line, "passed");
                let failed = extract_num(line, "failed");
                return (passed, failed);
            }
        }
        // Pytest/CTest fallback: find "N passed" / "N failed" patterns
        let passed = extract_num(&combined, "passed");
        let failed = extract_num(&combined, "failed");
        if passed > 0 || failed > 0 { return (passed, failed); }
        // Final fallback: exit code based
        if output.exit_code == 0 { (1, 0) } else { (0, 1) }
    }
}

#[async_trait::async_trait]
impl PackVerifier for TestVerifier {
    fn verifier_id(&self) -> &str { &self.verifier_id }
    fn version(&self) -> &str { &self.version }
    fn toolchain(&self) -> Option<&str> { Some(&self.toolchain) }

    async fn verify(&self, transaction_id: TransactionId, workspace_path: &str,
        criteria: &[onto_assurance_types::contract::AcceptanceCriterion],
    ) -> Result<PackVerificationReport, PackVerifierError> {
        let args: Vec<&str> = self.test_args.iter().map(|s| s.as_str()).collect();
        let output = self.runner.run(&self.test_command, &args, workspace_path)
            .map_err(|e| PackVerifierError::ExecutionFailed(e.to_string()))?;
        let (passed_count, failed_count) = Self::parse_counts(&output);
        let total = passed_count + failed_count;
        let passed = failed_count == 0 && total > 0;

        let per_criterion: Vec<_> = criteria.iter().map(|c| CriterionCheckResult {
            criterion_id: c.criterion_id.to_string(), criterion_name: c.name.clone(),
            satisfied: passed,
            detail: if passed { format!("{} tests passed", passed_count) }
                    else { format!("{}/{} passed, {} failed", passed_count, total, failed_count) },
        }).collect();

        Ok(PackVerificationReport { transaction_id, verifier_id: self.verifier_id.clone(),
            verifier_version: self.version.clone(), toolchain: Some(self.toolchain.clone()),
            passed, total_checks: total, passed_checks: passed_count, failed_checks: failed_count,
            skipped_checks: 0, per_criterion, artifacts: vec![],
            raw_output: format!("[stdout]\n{}\n[stderr]\n{}", output.stdout, output.stderr),
        })
    }
}

fn extract_num(line: &str, key: &str) -> u32 {
    // Format: "N passed" or "N failed" — number precedes key in the same segment
    for part in line.split(';') {
        let words: Vec<&str> = part.split_whitespace().collect();
        for i in 0..words.len() {
            if words[i] == key && i > 0 {
                if let Ok(n) = words[i - 1].parse() {
                    return n;
                }
            }
        }
    }
    // Fallback: "key: N" format
    if let Some(rest) = line.split(&format!("{}: ", key)).nth(1) {
        return rest.split(';').next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_verifier::MockCommandRunner;
    use onto_assurance_types::contract::{AcceptanceCriterion, CriterionKind};
    use onto_assurance_types::ids::CriterionId;

    fn c() -> Vec<AcceptanceCriterion> { vec![AcceptanceCriterion {
        criterion_id: CriterionId::new(), name: "tests".into(), kind: CriterionKind::TestPass,
        description: "".into(), is_blocking: true }] }

    #[tokio::test]
    async fn cargo_all_pass() {
        let r = MockCommandRunner { exit_code: 0,
            stdout: "test result: ok. 9 passed; 0 failed; finished".into(), stderr: String::new() };
        let report = TestVerifier::cargo_test(Box::new(r)).verify(TransactionId::new(), "/t", &c()).await.unwrap();
        assert!(report.passed); assert_eq!(report.passed_checks, 9);
    }

    #[tokio::test]
    async fn cargo_some_fail() {
        let r = MockCommandRunner { exit_code: 101,
            stdout: "test result: FAILED. 5 passed; 4 failed; finished".into(), stderr: String::new() };
        let report = TestVerifier::cargo_test(Box::new(r)).verify(TransactionId::new(), "/t", &c()).await.unwrap();
        assert!(!report.passed); assert_eq!(report.failed_checks, 4);
    }

    #[tokio::test]
    async fn pytest_format() {
        let r = MockCommandRunner { exit_code: 0,
            stdout: "test_a PASSED\ntest_b PASSED\n===== 2 passed in 0.05s =====".into(), stderr: String::new() };
        let report = TestVerifier::pytest(Box::new(r)).verify(TransactionId::new(), "/t", &c()).await.unwrap();
        assert!(report.passed); assert_eq!(report.passed_checks, 2);
    }
}
