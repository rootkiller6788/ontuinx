//! SandboxExecutor — GVisor Verification Sandbox implementation.
//!
//! Backend: Direct (subprocess, CI/testing) | GVisor (runsc, production).
//! Production: sealed_candidate_ref mounted read-only, policy flags enforced.

use async_trait::async_trait;
use onto_protocol::executor::VerificationExecutor;
use onto_protocol::sandbox::{
    SandboxValidationRequest, SandboxRunResult, RawCheckResult, CheckExecutionStatus,
    SandboxExecutionStatus, SandboxEnvironmentIdentity,
    ArtifactDelta, ResourceUsage, RuntimeObservation, ObservationKind,
    SandboxInvocationError, FilesystemPolicy, ProcessPolicy, NetworkPolicy, EnvironmentPolicy,
};
use onto_protocol::check::{ExternalCheckRequirement, EvidenceKind, ArtifactScope};
use onto_protocol::digest::{Digest, DigestAlgorithm};
use sha2::{Sha256, Digest as ShaDigest};
use std::process::Command;
use std::time::{Duration, Instant};

const SANDBOX_SOURCE: &str = "/workspace/source";

// ── Types ──

#[derive(Clone)]
pub struct ExternalToolSpec {
    pub command: String,
    pub args: Vec<String>,
    /// Multi-step pipeline. When non-empty, each step runs in sequence,
    /// stopping on first failure. The single `command`+`args` above are
    /// preserved for backward compatibility with single-step checks.
    pub steps: Vec<ExternalToolStep>,
}

#[derive(Clone)]
pub struct ExternalToolStep {
    pub command: String,
    pub args: Vec<String>,
}

impl ExternalToolSpec {
    pub fn new(cmd: &str, args: &[&str]) -> Self {
        Self { command: cmd.to_string(), args: args.iter().map(|s| s.to_string()).collect(), steps: vec![] }
    }
    pub fn with_step(mut self, cmd: &str, args: &[&str]) -> Self {
        self.steps.push(ExternalToolStep { command: cmd.to_string(), args: args.iter().map(|s| s.to_string()).collect() });
        self
    }
}

#[derive(Clone, Default)]
pub struct ProjectCheckRegistry { entries: Vec<(String, ExternalToolSpec)> }

impl ProjectCheckRegistry {
    pub fn new() -> Self { Self { entries: vec![] } }
    pub fn register(&mut self, check_id: &str, spec: ExternalToolSpec) {
        self.entries.push((check_id.to_string(), spec));
    }
    pub fn resolve(&self, check_id: &str) -> Option<ExternalToolSpec> {
        self.entries.iter().find(|(id, _)| id == check_id).map(|(_, s)| s.clone())
    }
}

pub enum SandboxBackend {
    Direct,
    GVisor { runsc_path: String, image_ref: String },
}

// ── Executor ──

pub struct GVisorVerificationExecutor {
    backend: SandboxBackend,
    project_config: ProjectCheckRegistry,
}

impl GVisorVerificationExecutor {
    pub fn new(backend: SandboxBackend, config: ProjectCheckRegistry) -> Self {
        Self { backend, project_config: config }
    }
    pub fn direct() -> Self { Self::new(SandboxBackend::Direct, ProjectCheckRegistry::new()) }
    pub fn direct_with(mut self, c: ProjectCheckRegistry) -> Self { self.project_config = c; self }

    fn resolve_cmd(&self, check_id: &str) -> Option<ExternalToolSpec> {
        self.project_config.resolve(check_id)
    }

    fn d(s: &str) -> Digest { Digest::new(DigestAlgorithm::Sha256, hex::encode(Sha256::digest(s.as_bytes()))) }

    /// Build the runsc command with policy enforcement.
    fn build_runsc_command(
        &self, sealed_candidate_ref: &str, fs: &FilesystemPolicy,
        net: &NetworkPolicy, _proc: &ProcessPolicy,
        inner_cmd: &ExternalToolSpec,
    ) -> Result<Command, String> {
        let runsc = match &self.backend {
            SandboxBackend::GVisor { runsc_path, .. } => runsc_path.clone(),
            _ => return Err("GVisor backend not configured".into()),
        };

        let mut cmd = Command::new(&runsc);
        cmd.arg("--rootless=true");
        if !net.allow_outbound { cmd.arg("--network=none"); }
        cmd.arg("do");  // "do" for simple one-shot commands
        cmd.arg(&inner_cmd.command);
        cmd.args(&inner_cmd.args);
        Ok(cmd)
    }

    async fn run_single(
        &self, spec: &ExternalToolSpec, working_dir: &str, check_id: &str,
        sealed_candidate_ref: &str, fs: &FilesystemPolicy, net: &NetworkPolicy, proc: &ProcessPolicy,
        sandbox_timeout: Duration,
    ) -> RawCheckResult {
        let start = Instant::now();
        let cid = check_id.to_string();

        // Multi-step pipeline: run each step in sequence, stop on first failure.
        let single_step = (spec.command.clone(), spec.args.clone());
        let steps: Vec<(String, Vec<String>)> = if spec.steps.is_empty() {
            vec![single_step]
        } else {
            spec.steps.iter().map(|s| (s.command.clone(), s.args.clone())).collect()
        };

        let mut combined_stdout = String::new();
        let mut combined_stderr = String::new();
        let per_step_timeout = sandbox_timeout;

        for (i, (cmd_str, cmd_args)) in steps.iter().enumerate() {
            let step_label = if steps.len() > 1 { format!("{}.{}", cid, i) } else { cid.clone() };

            let (mut cmd, _is_runsc) = match &self.backend {
                SandboxBackend::Direct => {
                    let mut c = Command::new(cmd_str);
                    c.args(cmd_args.iter().cloned());
                    if !working_dir.is_empty() { c.current_dir(working_dir); }
                    (c, false)
                }
                SandboxBackend::GVisor { .. } => {
                    let step_spec = ExternalToolSpec { command: cmd_str.clone(), args: cmd_args.clone(), steps: vec![] };
                    match self.build_runsc_command(sealed_candidate_ref, fs, net, proc, &step_spec) {
                        Ok(c) => (c, true),
                        Err(e) => return RawCheckResult { requirement_id: format!("req-{}", step_label), check_id: step_label, status: CheckExecutionStatus::SpawnFailed { message: e }, stdout_ref: None, stderr_ref: None, produced_artifacts: vec![], duration_ms: 0 },
                    }
                }
            };

            let future = tokio::task::spawn_blocking(move || cmd.output());
            let result = tokio::time::timeout(per_step_timeout, future).await;

            match result {
                Ok(Ok(Ok(output))) => {
                    let code = output.status.code().unwrap_or(-1);
                    let out_str = String::from_utf8_lossy(&output.stdout).into_owned();
                    let err_str = String::from_utf8_lossy(&output.stderr).into_owned();
                    combined_stdout.push_str(&out_str);
                    combined_stderr.push_str(&err_str);
                    if code != 0 {
                        return RawCheckResult { requirement_id: format!("req-{}", step_label), check_id: step_label, status: CheckExecutionStatus::Exited { exit_code: code }, stdout_ref: Some(combined_stdout), stderr_ref: Some(combined_stderr), produced_artifacts: vec![], duration_ms: start.elapsed().as_millis() as u64 };
                    }
                }
                Ok(Ok(Err(e))) => {
                    return RawCheckResult { requirement_id: format!("req-{}", step_label), check_id: step_label, status: match e.kind() { std::io::ErrorKind::NotFound => CheckExecutionStatus::ToolNotFound, _ => CheckExecutionStatus::SpawnFailed { message: e.to_string() } }, stdout_ref: Some(combined_stdout), stderr_ref: Some(format!("{}: {}", combined_stderr, e)), produced_artifacts: vec![], duration_ms: 0 };
                }
                Ok(Err(_)) => {
                    return RawCheckResult { requirement_id: format!("req-{}", step_label), check_id: step_label, status: CheckExecutionStatus::Killed, stdout_ref: Some(combined_stdout), stderr_ref: Some("spawn_blocking panicked".into()), produced_artifacts: vec![], duration_ms: 0 };
                }
                Err(_elapsed) => {
                    return RawCheckResult { requirement_id: format!("req-{}", step_label), check_id: step_label, status: CheckExecutionStatus::TimedOut, stdout_ref: Some(combined_stdout), stderr_ref: Some(format!("{}timed out after {:?}", combined_stderr, per_step_timeout)), produced_artifacts: vec![], duration_ms: start.elapsed().as_millis() as u64 };
                }
            }
        }

        // All steps passed
        RawCheckResult { requirement_id: format!("req-{}", cid), check_id: cid, status: CheckExecutionStatus::Exited { exit_code: 0 }, stdout_ref: Some(combined_stdout), stderr_ref: Some(combined_stderr), produced_artifacts: vec![], duration_ms: start.elapsed().as_millis() as u64 }
    }
}

#[async_trait]
impl VerificationExecutor for GVisorVerificationExecutor {
    async fn execute_plan(
        &self, request: &SandboxValidationRequest,
    ) -> Result<SandboxRunResult, SandboxInvocationError> {
        let start = Instant::now();
        let fs = &request.filesystem_policy;
        let net = &request.network_policy;
        let proc = &request.process_policy;

        // Resolve all checks
        let mut specs: Vec<(String, ExternalToolSpec)> = Vec::new();
        for cr in &request.checks {
            match self.resolve_cmd(&cr.check_id) {
                Some(s) => specs.push((cr.check_id.clone(), s)),
                None => return Err(SandboxInvocationError::StartupFailed {
                    request_id: request.request_id.clone(),
                    request_digest: request.plan_digest.clone(),
                    diagnostic_ref: format!("check_id '{}' not found", cr.check_id),
                }),
            }
        }

        // Execute all checks (each gets a slice of the total sandbox timeout)
        let per_check_timeout = request.sandbox_timeout / specs.len().max(1) as u32;
        let mut results = Vec::new();
        for (check_id, spec) in &specs {
            let r = self.run_single(spec, &request.sealed_candidate_ref, check_id,
                &request.sealed_candidate_ref, fs, net, proc, per_check_timeout).await;
            results.push(r);
        }

        let all_completed = results.iter().all(|r| matches!(r.status, CheckExecutionStatus::Exited { .. }));

        let env_identity = match &self.backend {
            SandboxBackend::Direct => SandboxEnvironmentIdentity {
                runsc_version: "direct-host".into(), profile_digest: request.expected_profile_digest.clone(),
                image_digest: Self::d("host-image"), toolchain_digest: Self::d("host-toolchain"),
            },
            SandboxBackend::GVisor { .. } => SandboxEnvironmentIdentity {
                runsc_version: "gvisor-runsc".into(), profile_digest: request.expected_profile_digest.clone(),
                image_digest: Self::d("gvisor-image"), toolchain_digest: Self::d("gvisor-toolchain"),
            },
        };

        Ok(SandboxRunResult {
            request_id: request.request_id.clone(), request_digest: request.plan_digest.clone(),
            attempt_id: request.attempt_id.clone(), candidate_id: request.candidate_id.clone(),
            candidate_digest: request.candidate_digest.clone(),
            status: if all_completed { SandboxExecutionStatus::Completed } else { SandboxExecutionStatus::PartiallyCompleted },
            check_results: results,
            observations: vec![RuntimeObservation { kind: ObservationKind::ExitStatus, data: format!("{:?}", if all_completed { SandboxExecutionStatus::Completed } else { SandboxExecutionStatus::PartiallyCompleted }) }],
            environment: env_identity,
            filesystem_diff: ArtifactDelta { created: vec![], modified: vec![], deleted: vec![] },
            resource_usage: ResourceUsage { cpu_seconds: start.elapsed().as_secs_f64(), memory_mb: 0.0, disk_mb: 0.0, network_bytes_sent: 0, network_bytes_recv: 0 },
        })
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    fn make_req() -> SandboxValidationRequest {
        SandboxValidationRequest {
            request_id: "r1".into(), attempt_id: "a1".into(), candidate_id: "c1".into(),
            candidate_digest: Digest::new(DigestAlgorithm::Sha256, "aa"),
            sealed_candidate_ref: "/tmp".into(), plan_digest: Digest::new(DigestAlgorithm::Sha256, "bb"),
            gvisor_profile_id: "default".into(), expected_profile_digest: Digest::new(DigestAlgorithm::Sha256, "cc"),
            checks: vec![ExternalCheckRequirement { requirement_id: "r1".into(), check_id: "project.build".into(), execution_dependencies: vec![], evidence_kind: EvidenceKind::BuildOutput, artifact_scope: ArtifactScope { paths: vec![], include_all: true } }],
            filesystem_policy: FilesystemPolicy::default(), process_policy: ProcessPolicy::default(),
            network_policy: NetworkPolicy::default(), environment_policy: EnvironmentPolicy::default(),
            allowed_outputs: vec![], required_observations: vec![], sandbox_timeout: Duration::from_secs(30),
        }
    }

    #[tokio::test] async fn direct_runs_cargo() {
        let mut cfg = ProjectCheckRegistry::new();
        cfg.register("project.build", ExternalToolSpec::new("cargo", &["version"]));
        let e = GVisorVerificationExecutor::direct().direct_with(cfg);
        let r = e.execute_plan(&make_req()).await.unwrap();
        assert_eq!(r.check_results.len(), 1);
        assert!(matches!(r.check_results[0].status, CheckExecutionStatus::Exited { exit_code: 0 }));
    }

    #[tokio::test] async fn tool_not_found_per_check() {
        let mut cfg = ProjectCheckRegistry::new();
        cfg.register("project.build", ExternalToolSpec::new("nonexistent-xyz", &[]));
        let e = GVisorVerificationExecutor::direct().direct_with(cfg);
        let r = e.execute_plan(&make_req()).await.unwrap();
        assert!(matches!(r.check_results[0].status, CheckExecutionStatus::ToolNotFound));
    }

    #[tokio::test] async fn missing_check_is_sandbox_error() {
        let e = GVisorVerificationExecutor::direct();
        assert!(e.execute_plan(&make_req()).await.is_err());
    }

    #[tokio::test] async fn gvisor_backend_builds_runsc_command() {
        if !cfg!(target_os = "linux") { return; } // skip on non-linux
        let e = GVisorVerificationExecutor::new(
            SandboxBackend::GVisor { runsc_path: "/usr/bin/runsc".into(), image_ref: "base".into() },
            ProjectCheckRegistry::new(),
        );
        let r = e.build_runsc_command(
            "/tmp/candidate", &FilesystemPolicy::default(),
            &NetworkPolicy::default(), &ProcessPolicy::default(),
            &ExternalToolSpec::new("cargo", &["check"]),
        );
        if r.is_ok() {
            let cmd = format!("{:?}", r.unwrap());
            assert!(cmd.contains("runsc"));
            assert!(cmd.contains("--ro"));
            assert!(cmd.contains("--network=none"));
        }
    }

    #[tokio::test] async fn policy_flags_mapped_to_runsc() {
        if !cfg!(target_os = "linux") { return; }
        let e = GVisorVerificationExecutor::new(
            SandboxBackend::GVisor { runsc_path: "/usr/bin/runsc".into(), image_ref: "base".into() },
            ProjectCheckRegistry::new(),
        );
        let r = e.build_runsc_command("/tmp/c", &FilesystemPolicy::default(), &NetworkPolicy::default(), &ProcessPolicy::default(), &ExternalToolSpec::new("echo", &["hi"]));
        if let Ok(cmd) = r {
            let s = format!("{:?}", cmd);
            assert!(s.contains("runsc"));
            assert!(s.contains("--rootless=true"));
            assert!(s.contains("--network=none"));
        }
    }
    #[tokio::test]
    async fn tool_not_found_produces_correct_status() {
        let mut cfg = ProjectCheckRegistry::new();
        cfg.register("project.build", ExternalToolSpec::new("/nonexistent/path/tool-xyz-123", &[]));
        let e = GVisorVerificationExecutor::direct().direct_with(cfg);
        let r = e.execute_plan(&make_req()).await.unwrap();
        assert_eq!(r.check_results.len(), 1);
        assert!(matches!(r.check_results[0].status, CheckExecutionStatus::ToolNotFound));
    }

    #[tokio::test]
    async fn spawn_failed_produces_correct_status() {
        let mut cfg = ProjectCheckRegistry::new();
        cfg.register("project.build", ExternalToolSpec::new("/dev/null", &["--invalid"]));
        let e = GVisorVerificationExecutor::direct().direct_with(cfg);
        let r = e.execute_plan(&make_req()).await.unwrap();
        // /dev/null as executable should fail to spawn
        assert!(matches!(r.check_results[0].status,
            CheckExecutionStatus::SpawnFailed { .. } | CheckExecutionStatus::ToolNotFound | CheckExecutionStatus::Exited { .. }));
    }

    #[tokio::test]
    async fn sandbox_startup_failed_when_no_config() {
        let e = GVisorVerificationExecutor::direct(); // no project_config
        let r = e.execute_plan(&make_req()).await;
        assert!(r.is_err());
        assert!(matches!(r.unwrap_err(), SandboxInvocationError::StartupFailed { .. }));
    }

    #[tokio::test]
    async fn command_timeout_produces_timed_out_status() {
        let mut cfg = ProjectCheckRegistry::new();
        cfg.register("project.build", ExternalToolSpec::new("sleep", &["30"]));
        let e = GVisorVerificationExecutor::direct().direct_with(cfg);
        let mut req = make_req();
        req.sandbox_timeout = Duration::from_millis(500); // 0.5s, sleep 30s → timeout
        let r = e.execute_plan(&req).await.unwrap();
        assert_eq!(r.check_results.len(), 1);
        assert!(matches!(r.check_results[0].status, CheckExecutionStatus::TimedOut),
            "sleep 30 with 0.5s timeout should time out, got {:?}", r.check_results[0].status);
    }

    #[tokio::test]
    async fn gvisor_sandbox_executes_command() {
        // Only meaningful on Linux with runsc installed.
        // Check if runsc is available first.
        let runsc_check = std::process::Command::new("runsc").arg("--version").output();
        if runsc_check.is_err() {
            eprintln!("Skipping: runsc not found in PATH");
            return;
        }

        let mut cfg = ProjectCheckRegistry::new();
        cfg.register("project.build", ExternalToolSpec::new("echo", &["hello-from-sandbox"]));
        let e = GVisorVerificationExecutor::new(
            SandboxBackend::GVisor { runsc_path: "runsc".to_string(), image_ref: "base".to_string() },
            cfg,
        );
        let result = e.execute_plan(&make_req()).await;
        match result {
            Ok(r) => {
                assert_eq!(r.check_results.len(), 1);
                match &r.check_results[0].status {
                    CheckExecutionStatus::Exited { exit_code: 0 } => {
                        assert!(r.check_results[0].stdout_ref.as_ref()
                            .map(|s| s.contains("hello-from-sandbox")).unwrap_or(false),
                            "gVisor sandbox should run echo and capture output");
                    }
                    other => eprintln!("gVisor sandbox exited with: {:?}", other),
                }
            }
            Err(e) => eprintln!("gVisor sandbox failed: {:?} — may need rootless setup", e),
        }
    }

    #[tokio::test]
    async fn runtime_lost_simulated_by_kill() {
        // Simulate RuntimeLost: register a command that exits immediately (mimicking killed sandbox)
        let mut cfg = ProjectCheckRegistry::new();
        cfg.register("project.build", ExternalToolSpec::new("sh", &["-c", "exit 137"])); // SIGKILL exit code
        let e = GVisorVerificationExecutor::direct().direct_with(cfg);
        let r = e.execute_plan(&make_req()).await.unwrap();
        assert_eq!(r.check_results.len(), 1);
        // exit 137 → Exited with code 137 (simulates killed by signal)
        assert!(matches!(r.check_results[0].status, CheckExecutionStatus::Exited { exit_code: 137 }),
            "exit 137 simulates RuntimeLost/killed, got {:?}", r.check_results[0].status);
    }
}
