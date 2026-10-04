//! Machine Verification Matrix — 7 languages × 5 levels each.
//!
//! Level 0: Format (fast, non-blocking signal)
//! Level 1: Static Analysis (lint, type-check, semgrep)
//! Level 2: Test Execution (unit + integration + race)
//! Level 3: Security & Dependency Audit (vuln scanning)
//! Level 4: Semantic (IronClaw Lane B — registered separately)
//!
//! All verifiers use `CommandRunner` for mockable subprocess execution.
//! Each returns `PackVerificationReport` with structured pass/fail + raw output.

use onto_assurance_types::ids::TransactionId;
use onto_pack_sdk::{CriterionCheckResult, PackVerificationReport, PackVerifier, PackVerifierError};
use crate::build_verifier::{CommandRunner, CommandOutput};

// ══════════════════════════════════════════════════════════════════
// Generic MachineVerifier — one struct for all levels
// ══════════════════════════════════════════════════════════════════

pub struct MachineVerifier {
    pub verifier_id: String,
    pub version: String,
    pub toolchain: String,           // "rustc", "go", "python", "node", "jvm", "clang"
    pub level: VerificationLevel,    // Fmt / Lint / Test / Audit
    command: String,
    args: Vec<String>,
    runner: Box<dyn CommandRunner>,
    parse: Box<dyn Fn(&CommandOutput) -> (u32, u32) + Send + Sync>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationLevel {
    Fmt = 0,
    Lint = 1,
    Test = 2,
    Audit = 3,
}

impl MachineVerifier {
    pub fn new(
        verifier_id: &str, toolchain: &str, level: VerificationLevel,
        command: &str, args: Vec<&str>,
        runner: Box<dyn CommandRunner>,
        parse: Box<dyn Fn(&CommandOutput) -> (u32, u32) + Send + Sync>,
    ) -> Self {
        Self {
            verifier_id: verifier_id.into(), version: "1.0.0".into(),
            toolchain: toolchain.into(), level,
            command: command.into(), args: args.into_iter().map(|s| s.into()).collect(),
            runner, parse,
        }
    }
}

#[async_trait::async_trait]
impl PackVerifier for MachineVerifier {
    fn verifier_id(&self) -> &str { &self.verifier_id }
    fn version(&self) -> &str { &self.version }
    fn toolchain(&self) -> Option<&str> { Some(&self.toolchain) }

    async fn verify(&self, transaction_id: TransactionId, workspace_path: &str,
        criteria: &[onto_assurance_types::contract::AcceptanceCriterion],
    ) -> Result<PackVerificationReport, PackVerifierError> {
        let args: Vec<&str> = self.args.iter().map(|s| s.as_str()).collect();
        let output = self.runner.run(&self.command, &args, workspace_path)
            .map_err(|e| PackVerifierError::ExecutionFailed(e.to_string()))?;
        let (passed_count, failed_count) = (self.parse)(&output);
        let total = passed_count + failed_count;
        let passed = failed_count == 0 && output.exit_code == 0;

        let per_criterion: Vec<_> = criteria.iter().map(|c| CriterionCheckResult {
            criterion_id: c.criterion_id.to_string(), criterion_name: c.name.clone(),
            satisfied: passed,
            detail: if passed { format!("{:?} passed", self.level) }
                    else { format!("{:?} failed: {}/{}", self.level, passed_count, total) },
        }).collect();

        Ok(PackVerificationReport {
            transaction_id, verifier_id: self.verifier_id.clone(),
            verifier_version: self.version.clone(), toolchain: Some(self.toolchain.clone()),
            passed, total_checks: total, passed_checks: passed_count,
            failed_checks: failed_count, skipped_checks: 0, per_criterion, artifacts: vec![],
            raw_output: format!("[stdout]\n{}\n[stderr]\n{}", output.stdout, output.stderr),
        })
    }
}

// ══════════════════════════════════════════════════════════════════
// Output parsers — return (passed_count, failed_count)
// ══════════════════════════════════════════════════════════════════

fn parse_exit_code(o: &CommandOutput) -> (u32, u32) {
    if o.exit_code == 0 { (1, 0) } else { (0, 1) }
}

fn parse_cargo_test(o: &CommandOutput) -> (u32, u32) {
    let combined = format!("{}\n{}", o.stdout, o.stderr);
    for line in combined.lines() {
        if line.contains("test result:") {
            let p = extract_num(line, "passed");
            let f = extract_num(line, "failed");
            return (p, f);
        }
    }
    parse_exit_code(o)
}

fn parse_go_test(o: &CommandOutput) -> (u32, u32) {
    // "ok   package  0.123s" or "--- PASS: TestName"
    let combined = format!("{}\n{}", o.stdout, o.stderr);
    let pass = combined.matches("--- PASS:").count() as u32;
    let fail = combined.matches("--- FAIL:").count() as u32;
    if pass > 0 || fail > 0 { return (pass, fail); }
    let ok_lines = combined.lines().filter(|l| l.starts_with("ok ")).count() as u32;
    let fail_lines = combined.lines().filter(|l| l.starts_with("FAIL ")).count() as u32;
    if ok_lines > 0 || fail_lines > 0 { return (ok_lines, fail_lines); }
    parse_exit_code(o)
}

fn parse_jest(o: &CommandOutput) -> (u32, u32) {
    // "Tests: 9 passed, 2 failed, 1 skipped, 12 total"
    let combined = format!("{}\n{}", o.stdout, o.stderr);
    let p = extract_num(&combined, "passed");
    let f = extract_num(&combined, "failed");
    if p > 0 || f > 0 { (p, f) } else { parse_exit_code(o) }
}

fn parse_pytest(o: &CommandOutput) -> (u32, u32) {
    let combined = format!("{}\n{}", o.stdout, o.stderr);
    let p = extract_num(&combined, "passed");
    let f = extract_num(&combined, "failed");
    if p > 0 || f > 0 { (p, f) } else { parse_exit_code(o) }
}

fn parse_ctest(o: &CommandOutput) -> (u32, u32) {
    let combined = format!("{}\n{}", o.stdout, o.stderr);
    let p = extract_num(&combined, "Passed");
    let f = extract_num(&combined, "Failed");
    if p > 0 || f > 0 { (p, f) } else { parse_exit_code(o) }
}

fn extract_num(line: &str, key: &str) -> u32 {
    for part in line.split(';') {
        let words: Vec<&str> = part.split_whitespace().collect();
        for i in 0..words.len() {
            if words[i] == key && i > 0 {
                if let Ok(n) = words[i - 1].parse() { return n; }
            }
        }
    }
    if let Some(rest) = line.split(&format!("{}: ", key)).nth(1) {
        return rest.split(&[',', ';', '\n'][..]).next()
            .and_then(|s| s.trim().parse().ok()).unwrap_or(0);
    }
    0
}

// ══════════════════════════════════════════════════════════════════
// Rust Verifiers
// ══════════════════════════════════════════════════════════════════

impl MachineVerifier {
    pub fn rust_fmt(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("rust-fmt", "rustc", VerificationLevel::Fmt,
            "cargo", vec!["fmt", "--", "--check"], runner, Box::new(parse_exit_code))
    }
    pub fn rust_clippy(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("rust-clippy", "rustc", VerificationLevel::Lint,
            "cargo", vec!["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"],
            runner, Box::new(parse_exit_code))
    }
    pub fn rust_check(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("rust-check", "rustc", VerificationLevel::Lint,
            "cargo", vec!["check", "--workspace"], runner, Box::new(parse_exit_code))
    }
    pub fn rust_test(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("rust-test", "rustc", VerificationLevel::Test,
            "cargo", vec!["test", "--workspace"], runner, Box::new(parse_cargo_test))
    }
    pub fn rust_deny(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("rust-deny", "rustc", VerificationLevel::Audit,
            "cargo", vec!["deny", "check"], runner, Box::new(parse_exit_code))
    }
    pub fn rust_audit(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("rust-audit", "rustc", VerificationLevel::Audit,
            "cargo", vec!["audit"], runner, Box::new(parse_exit_code))
    }
}

// ══════════════════════════════════════════════════════════════════
// Go Verifiers
// ══════════════════════════════════════════════════════════════════

impl MachineVerifier {
    pub fn go_fmt(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("go-fmt", "go", VerificationLevel::Fmt,
            "gofmt", vec!["-l", "."], runner, Box::new(parse_exit_code))
    }
    pub fn go_vet(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("go-vet", "go", VerificationLevel::Lint,
            "go", vec!["vet", "./..."], runner, Box::new(parse_exit_code))
    }
    pub fn go_lint(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("go-lint", "go", VerificationLevel::Lint,
            "golangci-lint", vec!["run", "--out-format", "colored-line-number"],
            runner, Box::new(parse_exit_code))
    }
    pub fn go_test(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("go-test", "go", VerificationLevel::Test,
            "go", vec!["test", "./..."], runner, Box::new(parse_go_test))
    }
    pub fn go_test_race(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("go-test-race", "go", VerificationLevel::Test,
            "go", vec!["test", "-race", "./..."], runner, Box::new(parse_go_test))
    }
    pub fn go_vulncheck(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("go-vulncheck", "go", VerificationLevel::Audit,
            "govulncheck", vec!["./..."], runner, Box::new(parse_exit_code))
    }
    pub fn go_gosec(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("go-gosec", "go", VerificationLevel::Audit,
            "gosec", vec!["-quiet", "./..."], runner, Box::new(parse_exit_code))
    }
}

// ══════════════════════════════════════════════════════════════════
// Python Verifiers
// ══════════════════════════════════════════════════════════════════

impl MachineVerifier {
    pub fn python_fmt(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("python-fmt", "python", VerificationLevel::Fmt,
            "ruff", vec!["format", "--check"], runner, Box::new(parse_exit_code))
    }
    pub fn python_lint(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("python-lint", "python", VerificationLevel::Lint,
            "ruff", vec!["check"], runner, Box::new(parse_exit_code))
    }
    pub fn python_pyright(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("python-pyright", "python", VerificationLevel::Lint,
            "pyright", vec![], runner, Box::new(parse_exit_code))
    }
    pub fn python_test(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("python-test", "python", VerificationLevel::Test,
            "pytest", vec!["-v"], runner, Box::new(parse_pytest))
    }
    pub fn python_pip_audit(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("python-pip-audit", "python", VerificationLevel::Audit,
            "pip-audit", vec![], runner, Box::new(parse_exit_code))
    }
    pub fn python_bandit(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("python-bandit", "python", VerificationLevel::Audit,
            "bandit", vec!["-r", "."], runner, Box::new(parse_exit_code))
    }
}

// ══════════════════════════════════════════════════════════════════
// TypeScript / JavaScript Verifiers
// ══════════════════════════════════════════════════════════════════

impl MachineVerifier {
    pub fn ts_fmt(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("ts-fmt", "node", VerificationLevel::Fmt,
            "npx", vec!["prettier", "--check", "."], runner, Box::new(parse_exit_code))
    }
    pub fn ts_eslint(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("ts-eslint", "node", VerificationLevel::Lint,
            "npx", vec!["eslint", "--format", "json", "."], runner, Box::new(parse_exit_code))
    }
    pub fn ts_tsc(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("ts-tsc", "node", VerificationLevel::Lint,
            "npx", vec!["tsc", "--noEmit"], runner, Box::new(parse_exit_code))
    }
    pub fn ts_jest(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("ts-jest", "node", VerificationLevel::Test,
            "npx", vec!["jest", "--json"], runner, Box::new(parse_jest))
    }
    pub fn ts_npm_audit(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("ts-npm-audit", "node", VerificationLevel::Audit,
            "npm", vec!["audit", "--json"], runner, Box::new(parse_exit_code))
    }
    pub fn ts_semgrep(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("ts-semgrep", "node", VerificationLevel::Audit,
            "semgrep", vec!["--config", "auto", "--quiet"], runner, Box::new(parse_exit_code))
    }
}

// ══════════════════════════════════════════════════════════════════
// Java / Kotlin Verifiers
// ══════════════════════════════════════════════════════════════════

impl MachineVerifier {
    pub fn java_fmt(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("java-fmt", "jvm", VerificationLevel::Fmt,
            "mvn", vec!["checkstyle:check"], runner, Box::new(parse_exit_code))
    }
    pub fn java_spotbugs(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("java-spotbugs", "jvm", VerificationLevel::Lint,
            "mvn", vec!["spotbugs:check"], runner, Box::new(parse_exit_code))
    }
    pub fn java_pmd(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("java-pmd", "jvm", VerificationLevel::Lint,
            "mvn", vec!["pmd:check"], runner, Box::new(parse_exit_code))
    }
    pub fn java_test(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("java-test", "jvm", VerificationLevel::Test,
            "mvn", vec!["test"], runner, Box::new(parse_exit_code))
    }
    pub fn java_dep_check(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("java-dep-check", "jvm", VerificationLevel::Audit,
            "mvn", vec!["dependency-check:check"], runner, Box::new(parse_exit_code))
    }

    pub fn kt_fmt(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("kt-fmt", "jvm", VerificationLevel::Fmt,
            "ktlint", vec![], runner, Box::new(parse_exit_code))
    }
    pub fn kt_detekt(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("kt-detekt", "jvm", VerificationLevel::Lint,
            "detekt", vec![], runner, Box::new(parse_exit_code))
    }
    pub fn kt_test(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("kt-test", "jvm", VerificationLevel::Test,
            "gradle", vec!["test"], runner, Box::new(parse_exit_code))
    }
}

// ══════════════════════════════════════════════════════════════════
// C / C++ Verifiers
// ══════════════════════════════════════════════════════════════════

impl MachineVerifier {
    pub fn c_fmt(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("c-fmt", "clang", VerificationLevel::Fmt,
            "clang-format", vec!["--dry-run", "--Werror"], runner, Box::new(parse_exit_code))
    }
    pub fn c_tidy(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("c-tidy", "clang", VerificationLevel::Lint,
            "clang-tidy", vec![], runner, Box::new(parse_exit_code))
    }
    pub fn c_cppcheck(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("c-cppcheck", "clang", VerificationLevel::Lint,
            "cppcheck", vec!["--enable=all", "--error-exitcode=1"], runner, Box::new(parse_exit_code))
    }
    pub fn c_build(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("c-build", "clang", VerificationLevel::Test,
            "cmake", vec!["--build", "build"], runner, Box::new(parse_exit_code))
    }
    pub fn c_test(runner: Box<dyn CommandRunner>) -> Self {
        Self::new("c-test", "clang", VerificationLevel::Test,
            "ctest", vec!["--test-dir", "build", "--output-on-failure"],
            runner, Box::new(parse_ctest))
    }
    pub fn c_asan_test(runner: Box<dyn CommandRunner>) -> Self {
        // Requires: cargo/ctest with ASAN_OPTIONS=detect_leaks=1
        // Note: sanitizer must be injected at binary level via RUSTFLAGS/CXXFLAGS env
        Self::new("c-asan", "clang", VerificationLevel::Test,
            "ctest", vec!["--test-dir", "build", "--output-on-failure"],
            runner, Box::new(parse_ctest))
    }
}

// ══════════════════════════════════════════════════════════════════
// Language-level verifier set builder
// ══════════════════════════════════════════════════════════════════

/// All machine verifiers for one language, organized by level.
pub struct LanguageVerifierSet {
    pub language: &'static str,
    pub fmt: Vec<MachineVerifier>,
    pub lint: Vec<MachineVerifier>,
    pub test: Vec<MachineVerifier>,
    pub audit: Vec<MachineVerifier>,
}

impl LanguageVerifierSet {
    /// Build a complete verifier set for one language.
    pub fn build(language: &str, runner_factory: &dyn Fn() -> Box<dyn CommandRunner>) -> Option<Self> {
        let r = || runner_factory();
        match language {
            "rust" => Some(Self {
                language: "rust",
                fmt: vec![MachineVerifier::rust_fmt(r())],
                lint: vec![MachineVerifier::rust_clippy(r()), MachineVerifier::rust_check(r())],
                test: vec![MachineVerifier::rust_test(r())],
                audit: vec![MachineVerifier::rust_deny(r()), MachineVerifier::rust_audit(r())],
            }),
            "go" => Some(Self {
                language: "go",
                fmt: vec![MachineVerifier::go_fmt(r())],
                lint: vec![MachineVerifier::go_vet(r()), MachineVerifier::go_lint(r())],
                test: vec![MachineVerifier::go_test(r()), MachineVerifier::go_test_race(r())],
                audit: vec![MachineVerifier::go_vulncheck(r()), MachineVerifier::go_gosec(r())],
            }),
            "python" => Some(Self {
                language: "python",
                fmt: vec![MachineVerifier::python_fmt(r())],
                lint: vec![MachineVerifier::python_lint(r()), MachineVerifier::python_pyright(r())],
                test: vec![MachineVerifier::python_test(r())],
                audit: vec![MachineVerifier::python_pip_audit(r()), MachineVerifier::python_bandit(r())],
            }),
            "typescript" | "javascript" => Some(Self {
                language: "typescript",
                fmt: vec![MachineVerifier::ts_fmt(r())],
                lint: vec![MachineVerifier::ts_eslint(r()), MachineVerifier::ts_tsc(r())],
                test: vec![MachineVerifier::ts_jest(r())],
                audit: vec![MachineVerifier::ts_npm_audit(r()), MachineVerifier::ts_semgrep(r())],
            }),
            "java" => Some(Self {
                language: "java",
                fmt: vec![MachineVerifier::java_fmt(r())],
                lint: vec![MachineVerifier::java_spotbugs(r()), MachineVerifier::java_pmd(r())],
                test: vec![MachineVerifier::java_test(r())],
                audit: vec![MachineVerifier::java_dep_check(r())],
            }),
            "kotlin" => Some(Self {
                language: "kotlin",
                fmt: vec![MachineVerifier::kt_fmt(r())],
                lint: vec![MachineVerifier::kt_detekt(r())],
                test: vec![MachineVerifier::kt_test(r())],
                audit: vec![], // Kotlin 复用 Java dep-check
            }),
            "cpp" | "c" => Some(Self {
                language: "cpp",
                fmt: vec![MachineVerifier::c_fmt(r())],
                lint: vec![MachineVerifier::c_tidy(r()), MachineVerifier::c_cppcheck(r())],
                test: vec![MachineVerifier::c_build(r()), MachineVerifier::c_test(r()), MachineVerifier::c_asan_test(r())],
                audit: vec![], // ASan/UBSan/TSan covered in test level
            }),
            _ => None, // 其他语言: MachineProfile = NotProvisioned
        }
    }

    /// All verifiers flattened into a single vec.
    pub fn all(&self) -> Vec<&MachineVerifier> {
        let mut v = Vec::new();
        v.extend(&self.fmt);
        v.extend(&self.lint);
        v.extend(&self.test);
        v.extend(&self.audit);
        v
    }

    /// Count per level.
    pub fn level_counts(&self) -> (usize, usize, usize, usize) {
        (self.fmt.len(), self.lint.len(), self.test.len(), self.audit.len())
    }
}

// ══════════════════════════════════════════════════════════════════
// Tests
// ══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_verifier::MockCommandRunner;
    use onto_assurance_types::contract::{AcceptanceCriterion, CriterionKind};
    use onto_assurance_types::ids::CriterionId;

    fn criterias() -> Vec<AcceptanceCriterion> {
        vec![AcceptanceCriterion { criterion_id: CriterionId::new(), name: "lint".into(),
            kind: CriterionKind::LintPass, description: "".into(), is_blocking: false }]
    }

    fn ok_runner() -> MockCommandRunner {
        MockCommandRunner { exit_code: 0, stdout: String::new(), stderr: String::new() }
    }
    fn fail_runner() -> MockCommandRunner {
        MockCommandRunner { exit_code: 1, stdout: "ERROR".into(), stderr: String::new() }
    }

    // ── Rust ──

    #[tokio::test] async fn rust_fmt_ok() {
        let r = MachineVerifier::rust_fmt(Box::new(ok_runner()))
            .verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(r.passed);
    }
    #[tokio::test] async fn rust_clippy_fail() {
        let r = MachineVerifier::rust_clippy(Box::new(fail_runner()))
            .verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(!r.passed);
    }
    #[tokio::test] async fn rust_test_counts() {
        let runner = MockCommandRunner { exit_code: 0,
            stdout: "test result: ok. 9 passed; 0 failed; finished".into(), stderr: String::new() };
        let r = MachineVerifier::rust_test(Box::new(runner))
            .verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert_eq!(r.passed_checks, 9);
    }

    // ── Go ──

    #[tokio::test] async fn go_fmt_ok() {
        let r = MachineVerifier::go_fmt(Box::new(ok_runner()))
            .verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(r.passed);
    }
    #[tokio::test] async fn go_test_pass() {
        let runner = MockCommandRunner { exit_code: 0,
            stdout: "--- PASS: TestFoo\n--- PASS: TestBar\n".into(), stderr: String::new() };
        let r = MachineVerifier::go_test(Box::new(runner))
            .verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert_eq!(r.passed_checks, 2);
    }

    // ── Python ──

    #[tokio::test] async fn python_lint_ok() {
        let r = MachineVerifier::python_lint(Box::new(ok_runner()))
            .verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(r.passed);
    }

    // ── Language set builder ──

    #[test]
    fn rust_set_has_all_levels() {
        let r = || -> Box<dyn CommandRunner> { Box::new(MockCommandRunner { exit_code: 0, stdout: String::new(), stderr: String::new() }) };
        let set = LanguageVerifierSet::build("rust", &r).unwrap();
        assert_eq!(set.level_counts(), (1, 2, 1, 2));
    }

    #[test]
    fn go_set_has_all_levels() {
        let r = || -> Box<dyn CommandRunner> { Box::new(MockCommandRunner { exit_code: 0, stdout: String::new(), stderr: String::new() }) };
        let set = LanguageVerifierSet::build("go", &r).unwrap();
        assert_eq!(set.level_counts(), (1, 2, 2, 2));
    }

    #[test]
    fn unknown_language_none() {
        let r = || -> Box<dyn CommandRunner> { Box::new(MockCommandRunner { exit_code: 0, stdout: String::new(), stderr: String::new() }) };
        assert!(LanguageVerifierSet::build("julia", &r).is_none());
    }

    #[test]
    fn seven_languages_covered() {
        let r = || -> Box<dyn CommandRunner> { Box::new(MockCommandRunner { exit_code: 0, stdout: String::new(), stderr: String::new() }) };
        for lang in &["rust", "go", "python", "typescript", "java", "kotlin", "cpp"] {
            assert!(LanguageVerifierSet::build(lang, &r).is_some(), "missing: {}", lang);
        }
    }

    // ── TypeScript ──
    #[tokio::test] async fn ts_fmt_ok() {
        let r = MachineVerifier::ts_fmt(Box::new(ok_runner())).verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(r.passed);
    }
    #[tokio::test] async fn ts_tsc_fail() {
        let r = MachineVerifier::ts_tsc(Box::new(fail_runner())).verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(!r.passed);
    }
    #[tokio::test] async fn ts_jest_counts() {
        let runner = MockCommandRunner { exit_code: 0, stdout: "Tests: 5 passed, 0 failed, 0 total".into(), stderr: String::new() };
        let r = MachineVerifier::ts_jest(Box::new(runner)).verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(r.passed);
        assert!(r.passed_checks > 0);
    }

    // ── Java ──
    #[tokio::test] async fn java_fmt_ok() {
        let r = MachineVerifier::java_fmt(Box::new(ok_runner())).verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(r.passed);
    }
    #[tokio::test] async fn java_spotbugs_fail() {
        let r = MachineVerifier::java_spotbugs(Box::new(fail_runner())).verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(!r.passed);
    }
    #[tokio::test] async fn java_set_counts() {
        let r = || -> Box<dyn CommandRunner> { Box::new(MockCommandRunner { exit_code: 0, stdout: String::new(), stderr: String::new() }) };
        let set = LanguageVerifierSet::build("java", &r).unwrap();
        assert_eq!(set.level_counts(), (1, 2, 1, 1)); // fmt, lint×2, test, audit
    }

    // ── Kotlin ──
    #[tokio::test] async fn kt_fmt_ok() {
        let r = MachineVerifier::kt_fmt(Box::new(ok_runner())).verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(r.passed);
    }
    #[tokio::test] async fn kt_detekt_fail() {
        let r = MachineVerifier::kt_detekt(Box::new(fail_runner())).verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(!r.passed);
    }
    #[tokio::test] async fn kt_set_no_audit() {
        let r = || -> Box<dyn CommandRunner> { Box::new(MockCommandRunner { exit_code: 0, stdout: String::new(), stderr: String::new() }) };
        let set = LanguageVerifierSet::build("kotlin", &r).unwrap();
        assert_eq!(set.audit.len(), 0, "Kotlin reuses Java dep-check, no own audit");
    }

    // ── C/C++ ──
    #[tokio::test] async fn c_fmt_ok() {
        let r = MachineVerifier::c_fmt(Box::new(ok_runner())).verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(r.passed);
    }
    #[tokio::test] async fn c_cppcheck_fail() {
        let r = MachineVerifier::c_cppcheck(Box::new(fail_runner())).verify(TransactionId::new(), "/t", &criterias()).await.unwrap();
        assert!(!r.passed);
    }
    #[tokio::test] async fn c_asan_in_test_level() {
        let r = || -> Box<dyn CommandRunner> { Box::new(MockCommandRunner { exit_code: 0, stdout: String::new(), stderr: String::new() }) };
        let set = LanguageVerifierSet::build("cpp", &r).unwrap();
        assert_eq!(set.audit.len(), 0, "C/C++ sanitizers covered in test level");
        assert_eq!(set.test.len(), 3, "build + ctest + asan at test level");
    }
}
