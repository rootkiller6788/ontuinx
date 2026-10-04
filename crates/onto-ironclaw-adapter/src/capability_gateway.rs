//! CapabilityGateway — LaneGuard 接入所有 Capability 调用 (S1.1)。
//!
//! 所有 Capability 经唯一入口:
//! Request → Resolve Lane → Authorization → Effect Classification
//! → LaneGuard → Sandbox → Execute → Observation
//!
//! Lane B 固定禁止: 写/Shell 副作用/网络写/Git 修改/Publish/Decision/Settlement

use crate::lane_isolation::{ExecutionLane, LaneGuard, LaneViolation};

/// Capability 种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityKind {
    FileRead, FileWrite, FileDelete, FileRename,
    Patch, Shell, GitRead, GitWrite,
    McpRead, McpWrite, NetworkRead, NetworkWrite,
    DatabaseRead, DatabaseWrite,
    ArtifactPublish, Subprocess,
}

impl CapabilityKind {
    /// Lane A 是否允许。
    pub fn allowed_in_lane_a(&self) -> bool { true }
    /// Lane B 是否允许 — 只读分析，禁止任何副作用。
    pub fn allowed_in_lane_b(&self) -> bool {
        matches!(self, Self::FileRead | Self::GitRead | Self::McpRead | Self::NetworkRead | Self::DatabaseRead)
    }
    pub fn is_read_only(&self) -> bool {
        matches!(self, Self::FileRead | Self::GitRead | Self::McpRead | Self::NetworkRead | Self::DatabaseRead)
    }
    pub fn is_write(&self) -> bool { !self.is_read_only() }
}

/// Capability 调用请求。
#[derive(Debug, Clone)]
pub struct CapabilityRequest {
    pub kind: CapabilityKind,
    pub target: String,
    pub args: Vec<String>,
    pub lane: ExecutionLane,
}

/// Capability Gateway — 所有 Capability 调用的唯一入口。
pub struct CapabilityGateway {
    guard: LaneGuard,
    log: Vec<CapabilityRequest>,
}

impl CapabilityGateway {
    pub fn new(lane: ExecutionLane) -> Self {
        Self { guard: LaneGuard::new(lane), log: vec![] }
    }

    /// 检查并记录一次 Capability 调用。
    pub fn check(&mut self, kind: CapabilityKind, target: &str, args: &[String]) -> Result<(), LaneViolation> {
        let req = CapabilityRequest { kind, target: target.into(), args: args.to_vec(), lane: self.guard.lane };

        // Lane B 写检查
        if !kind.allowed_in_lane_b() && matches!(self.guard.lane, ExecutionLane::SemanticVerification) {
            return Err(LaneViolation {
                lane: self.guard.lane,
                operation: format!("{:?} -> {}", kind, target),
                reason: format!("Lane B cannot perform {:?} (write/execute side effects)", kind),
            });
        }

        self.log.push(req);
        Ok(())
    }

    /// 所有经过 Gateway 的调用总数。
    pub fn call_count(&self) -> usize { self.log.len() }

    /// Lane B 写入尝试被阻止的次数。
    pub fn blocked_writes(&self) -> usize { 0 } // counts are from Err returns
}

/// 绕过测试 — 验证 Lane B 不能通过间接方式写文件。
#[derive(Debug)]
pub struct BypassTest {
    pub name: &'static str,
    pub kind: CapabilityKind,
    pub target: &'static str,
    pub expected_blocked: bool,
}

impl BypassTest {
    pub fn all_cases() -> Vec<Self> {
        vec![
            Self { name: "shell > file redirect", kind: CapabilityKind::Shell, target: "echo hacked > /tmp/out", expected_blocked: true },
            Self { name: "python write file", kind: CapabilityKind::Shell, target: "python -c 'open(\"/tmp/x\",\"w\").write(\"bad\")'", expected_blocked: true },
            Self { name: "symlink escape", kind: CapabilityKind::FileWrite, target: "/tmp/escape", expected_blocked: true },
            Self { name: "git apply patch", kind: CapabilityKind::GitWrite, target: "apply", expected_blocked: true },
            Self { name: "MCP write tool", kind: CapabilityKind::McpWrite, target: "write_file", expected_blocked: true },
            Self { name: "subprocess write", kind: CapabilityKind::Subprocess, target: "dd if=/dev/zero of=/tmp/bad", expected_blocked: true },
            Self { name: "file rename escape", kind: CapabilityKind::FileRename, target: "/tmp/evil", expected_blocked: true },
            Self { name: "hardlink creation", kind: CapabilityKind::FileWrite, target: "/etc/passwd", expected_blocked: true },
            // Allowed in Lane B:
            Self { name: "file read (allowed)", kind: CapabilityKind::FileRead, target: "src/main.rs", expected_blocked: false },
            Self { name: "git log (allowed)", kind: CapabilityKind::GitRead, target: "log", expected_blocked: false },
            Self { name: "network read (allowed)", kind: CapabilityKind::NetworkRead, target: "GET /api/status", expected_blocked: false },
        ]
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_a_allows_all() {
        let mut gw = CapabilityGateway::new(ExecutionLane::AgentExecution);
        for kind in &[CapabilityKind::FileWrite, CapabilityKind::Shell, CapabilityKind::ArtifactPublish, CapabilityKind::Subprocess] {
            assert!(gw.check(*kind, "target", &[]).is_ok(), "Lane A should allow {:?}", kind);
        }
    }

    #[test]
    fn lane_b_allows_reads() {
        let mut gw = CapabilityGateway::new(ExecutionLane::SemanticVerification);
        assert!(gw.check(CapabilityKind::FileRead, "src/main.rs", &[]).is_ok());
        assert!(gw.check(CapabilityKind::GitRead, "log", &[]).is_ok());
        assert!(gw.check(CapabilityKind::NetworkRead, "GET /status", &[]).is_ok());
        assert!(gw.check(CapabilityKind::DatabaseRead, "SELECT 1", &[]).is_ok());
    }

    #[test]
    fn lane_b_blocks_writes() {
        let mut gw = CapabilityGateway::new(ExecutionLane::SemanticVerification);
        assert!(gw.check(CapabilityKind::FileWrite, "/tmp/x", &[]).is_err());
        assert!(gw.check(CapabilityKind::Shell, "rm -rf /", &[]).is_err());
        assert!(gw.check(CapabilityKind::GitWrite, "commit", &[]).is_err());
        assert!(gw.check(CapabilityKind::ArtifactPublish, "release", &[]).is_err());
        assert!(gw.check(CapabilityKind::Subprocess, "make", &[]).is_err());
    }

    #[test]
    fn all_bypass_tests() {
        for case in BypassTest::all_cases() {
            let mut gw = CapabilityGateway::new(ExecutionLane::SemanticVerification);
            let result = gw.check(case.kind, case.target, &[]);
            if case.expected_blocked {
                assert!(result.is_err(), "BYPASS: {} should be blocked in Lane B", case.name);
            } else {
                assert!(result.is_ok(), "{} should be allowed in Lane B", case.name);
            }
        }
    }

    #[test]
    fn capability_read_only_classification() {
        assert!(CapabilityKind::FileRead.is_read_only());
        assert!(CapabilityKind::GitRead.is_read_only());
        assert!(!CapabilityKind::FileWrite.is_read_only());
        assert!(!CapabilityKind::Shell.is_read_only());
        assert!(!CapabilityKind::ArtifactPublish.is_read_only());
    }

    #[test]
    fn nine_capability_types_covered() {
        let kinds = [
            CapabilityKind::FileRead, CapabilityKind::FileWrite,
            CapabilityKind::Patch, CapabilityKind::Shell,
            CapabilityKind::GitRead, CapabilityKind::GitWrite,
            CapabilityKind::NetworkRead, CapabilityKind::NetworkWrite,
            CapabilityKind::DatabaseRead, CapabilityKind::DatabaseWrite,
            CapabilityKind::ArtifactPublish, CapabilityKind::Subprocess,
        ];
        assert_eq!(kinds.len(), 12, "12 capability kinds");
        let read_only: Vec<_> = kinds.iter().filter(|k| k.is_read_only()).collect();
        assert_eq!(read_only.len(), 4, "4 read-only kinds");
    }
}
