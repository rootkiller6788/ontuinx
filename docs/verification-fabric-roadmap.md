# OntoAssure Verification Fabric — 生产路线图

**日期**: 2026-07-27
**版本路线**: v0.2a → v0.2b → v0.2c → v0.3a → v0.3b → v0.4a → v0.4b → v0.4c

---

## 成熟度分维度记录

| 维度 | 当前 |
|------|------|
| 架构完整度 | ★★★★★ 14步贯通, OCR吸收, Port/Adapter边界 |
| 验证语义完整度 | ★★☆☆☆ 缺 VerifierRunResult/Completeness/Ledger |
| 真实运行完整度 | ★☆☆☆☆ Mock为主, LaneGuard未接入, 缺真IronClaw闭环 |
| 分布式可靠性 | ★☆☆☆☆ 单节点测试, 多进程未验证 |
| 领域覆盖度 | ★★☆☆☆ 代码领域完整, 非代码stub |
| 生产运维成熟度 | ★☆☆☆☆ 内存存储, 无持久化, 无可观测性 |

---

## S0: 生产验证语义封口 (v0.2a — v0.2b)

**目标**: 系统能可靠回答"到底验证了什么、哪些没验证、Verifier 是否成功执行、为什么通过或不能通过"。

### S0.1 统一 VerifierRunResult

当前 `ScheduleResult = Result<Vec<FindingCandidate>, ScheduleError>` 无法区分"没有发现问题"和"工具没有运行"。

```rust
pub struct VerifierRunResult {
    pub run_id: VerificationRunId,
    pub unit_id: VerificationUnitId,

    pub verifier_id: VerifierId,
    pub verifier_version_hash: ContentHash,

    pub execution_status: VerifierExecutionStatus,
    pub verifier_verdict: Option<VerifierVerdict>,

    pub findings: Vec<FindingCandidate>,
    pub positive_observations: Vec<EvidenceCandidate>,

    pub checkpoint_hash: ContentHash,
    pub unit_fingerprint: ContentHash,
    pub configuration_hash: ContentHash,
    pub environment_hash: ContentHash,

    pub stdout_ref: Option<ArtifactRef>,
    pub stderr_ref: Option<ArtifactRef>,
    pub exit_code: Option<i32>,

    pub started_at: Timestamp,
    pub finished_at: Timestamp,
    pub resource_usage: VerifierResourceUsage,
}
```

执行状态:

```
Completed          — 正常完成
Unavailable        — 工具未安装或不可用
TimedOut           — 执行超时
ExecutionFailed    — 运行但非零退出码
InvalidOutput      — 输出无法解析
Cancelled          — 被取消
BudgetBlocked      — 预算不足未启动
Unsupported        — 语言/目标类型不支持
```

根不变量: `execution_status = Completed ≠ verifier_verdict = Passed`

### S0.2 三本不可变总账

#### Scope Ledger — 每个目标必须有处置

```
Verified
ExcludedByPolicy
Unsupported
ReadFailed
BudgetBlocked
RequiresChunking
EnvironmentFailed
```

不变量: `all_targets = Verified + Excluded + Unsupported + Failed + Blocked`

#### Rule Coverage Ledger — 每个目标的每条必需规则有执行记录

```
RequiredRuleExecuted
AdvisoryRuleExecuted
RuleNotApplicable
RuleExecutionFailed
RuleUnsupported
RuleSuppressed
```

#### Verifier Execution Ledger — 每个 Unit 的每个 Required Verifier 有终态

```
verification_unit_id | required_verifier_id | status | result_ref
```

#### 完整性归约

```
Scope完整 AND Required Rules完整 AND Required Verifiers完整 AND 必要Evidence有效
→ EligibleForPositiveReduction

否则:
→ VerificationIncomplete / EnvironmentError / BudgetExhausted
绝不为 PASS
```

### S0.3 位置解析四态

```rust
pub enum LocationResolution {
    Resolved(ResolvedAnchor),
    Ambiguous(Vec<AnchorCandidate>),
    Unresolved(UnresolvedReason),
    StaleSnapshot { expected: ContentHash, actual: ContentHash },
}
```

解析规则:

| 场景 | 结果 |
|------|------|
| Hunk 与 File Scan 指向同一位置 | Resolved + DualProof |
| 仅一个通道成功 | Resolved + SingleProof |
| 两个通道指向不同位置 | Ambiguous |
| 文件 Hash 变化 | StaleSnapshot |
| 重复代码片段出现多处 | Ambiguous |
| 定位到 old-side 删除行 | 不绑定当前 Checkpoint |

证据锚点: `repository_snapshot_hash | file_hash | byte_range | line_range | snippet_hash | context_hash | resolution_proofs`

测试语料: 函数移动、文件 rename、相同代码重复、CRLF/LF、Unicode 多字节、行号漂移、新增/删除侧混淆、双通道冲突

关键指标: **错误唯一定位率 → 0**。宁可 Ambiguous，不生产错误 Evidence。

### S0.4 规则双平面

#### 语义规则平面 (已有)

```
system_rules.json → rule_docs/*.md → {{system_rule}}
```

告诉 LLM 关注什么。

#### 机器验证平面 (新增)

```
verification_profiles/
├── rust.toml
├── go.toml
├── cargo_manifest.toml
├── github_workflow.toml
└── generic.toml
```

```toml
profile_id = "onto.code.rust.v1"

required_verifiers = [
  "cargo-check", "cargo-test", "cargo-clippy", "ironclaw-semantic"
]

[enforcement]
build = "required"
test = "required"
lint = "required"
semantic = "advisory"

[evidence]
require_snapshot_binding = true
require_location_resolution = true
allow_unresolved_semantic_findings = false
```

告诉系统必须跑什么、哪个失败阻塞。

### S0.5 规则 Bundle Hash

```rust
pub struct ResolvedSemanticRuleBundle {
    pub target_ref: CanonicalTargetRef,
    pub fragments: Vec<ResolvedRuleFragment>,  // System/Organization/Project/Invocation
    pub effective_rule_hash: ContentHash,
    pub effective_text_ref: ArtifactRef,
}
```

规则变化 → effective_rule_hash 变化 → 相关 VerificationUnit 失效 → 必须重跑。

### S0 封口标准

```
0 Findings + ToolUnavailable 永远 ≠ Pass
每个目标都有 Scope 处置记录
每条 Required Rule 都有执行记录
每个 Required Verifier 都有终态
规则变化使 Session 失效
Ambiguous/Stale 位置不能成为强 Evidence
```

---

## S1: 真实单机可信闭环 (v0.2c)

**目标**: Mock → 真实 IronClaw, 端到端可信闭环。

### S1.1 LaneGuard 接入 Capability Gateway

所有 Capability 经唯一入口:

```
Request → Resolve Lane → Authorization → Effect Classification
→ LaneGuard → Sandbox → Execute → Observation
```

覆盖: filesystem, patch, shell, git, MCP, network, database, artifact publish, subprocess

Lane B 固定: 只读 Snapshot, 禁止写/Shell 副作用/网络写/Git 修改/Publish/Decision/Settlement

绕过测试: shell > file, Python 写文件, symlink 逃逸, git apply, MCP write, 子进程写, rename, 硬链接

### S1.2 IronClaw 内部 Rust Port

```rust
pub trait AttemptRunPort {
    async fn start_attempt(&self, request: AttemptRunRequest) -> Result<AttemptRunHandle, AttemptRunError>;
    async fn await_terminal(&self, handle: &AttemptRunHandle) -> Result<AttemptRunReport, AttemptRunError>;
    async fn cancel_attempt(&self, handle: &AttemptRunHandle) -> Result<(), AttemptRunError>;
}
```

绑定: loop_id+attempt_id+ironclaw_run_id+execution_generation+contract_hash+candidate_checkpoint_hash

不变量: 1 Attempt = 1 IronClaw Run, 不允许跨 Attempt 复用 Run。

### S1.3 Generic LLM Fallback 降权

```rust
pub enum VerifierTrustClass {
    DeterministicTool,
    BoundedIronClaw,
    GenericSemanticFallback,
    Experimental,
}
```

高风险 Profile: IronClaw 不可用 → VerificationIncomplete, 不自动换 Generic LLM。
Generic LLM 只能产出 Advisory FindingCandidate / Weak Evidence Candidate。

### S1.4 真实黄金链

```
1. 单 Attempt 真实修复成功
2. Attempt 1 失败, Attempt 2 真实修复成功
3. 模型声称成功, 但 Verifier UNSAT
4. 工具退出码 0, 但 Criterion 不满足
5. Agent 异常退出仍进入 Finalization
6. Semantic Lane 尝试写文件被拒绝
7. Budget 耗尽不能产生 Success
8. M6 只 Publish 一次
9. Commit 后响应丢失, 重试不重复 Publish
10. 旧 Checkpoint Evidence 不能提交新 Attempt
```

### S1 封口标准

```
真实模型 + 真实工具 + 真实 Checkpoint + 真实 Verifier
+ 真实 Decision + 真实 M6 Publish + 真实两 Attempt 收敛
+ 0 Completed 旁路 + 0 Lane B 写入
```

---

## S2: 持久化与恢复 (v0.3a)

**目标**: 进程重启可恢复, 已完成不重复, 变化精确失效。

### S2.1 PostgreSQL 存储

```
verification_sessions
verification_units
verifier_runs
scope_ledger_entries
rule_coverage_entries
finding_candidates
resolved_findings
evidence_records
budget_usage
```

append-only events + current projection, 不依赖内存状态。

### S2.2 Artifact Store

大对象 (stdout/stderr/规则文本/文件内容/Diff/Evidence Bundle) → MinIO/本地
PG 只存 artifact_ref + content_hash + size + media_type
读取时重新校验 Hash。

### S2.3 Session 恢复

恢复绑定: checkpoint_hash, contract_hash, scope_manifest_hash, effective_rule_hash, verification_profile_hash, verifier_version_hash, planner_version_hash, unit_fingerprint

策略:
- 完全匹配 → 复用
- 规则变化 → 精准失效受影响 Unit
- Verifier 版本变化 → 只重跑依赖该 Verifier 的 Unit
- Checkpoint 变化 → 相关 Target 和 Evidence 全部失效

### S2.4 Budget 多维持久化

tokens, 模型费用, 墙钟时间, 并发 Unit, 语义调用次数, CPU, 内存, 子进程数, 单目标大小, Artifact 读取量。
全部写入持久化 Ledger。

### S2 封口标准

```
进程重启恢复 Session
已完成 Verifier 不重复调用
规则变化精确失效
Checkpoint 变化严格失效
Artifact 被篡改时拒绝
Budget 耗尽明确终态
```

---

## S3: 多进程分布式验收 (v0.3b)

### S3.1 多进程单机拓扑

```
Temporal Frontend / History / Matching / Worker Service
PostgreSQL | Authority Service | Artifact Store
Rust Worker A / B / C
Test Driver | Fault Injector
```

### S3.2 Docker Compose

temporal-frontend, temporal-history, temporal-matching, postgres, authority, artifact-store, worker-a/b/c, toxiproxy, evidence-collector

### S3.3 18 项故障矩阵

| # | 故障 | 验证不变量 |
|----|------|-----------|
| 1 | Worker 领取前崩溃 | 无丢失 WorkItem |
| 2 | 领取后首次 Heartbeat 前崩溃 | 超时重分配 |
| 3 | Heartbeat 后崩溃 | 恢复继续 |
| 4 | Attempt 完成后崩溃 | 不重复 Attempt |
| 5 | Decision 持久化后崩溃 | 不重复 Decision |
| 6 | M6 Commit 后 Envelope 前崩溃 | 不重复 Publish |
| 7 | Completion RPC 响应丢失 | 幂等重试 |
| 8 | Matching 重启 | WorkItem 重新分配 |
| 9 | History 重启 | 事件重放一致 |
| 10 | Temporal 整体重启 | 全部恢复 |
| 11 | PG 短暂宕机 | 写重试, 读降级 |
| 12 | Authority 断网 | 拒绝开放, 恢复后一致 |
| 13 | Artifact Store 不可读 | 明确失败, 不静默跳过 |
| 14 | Worker 网络分区 | 无脑裂 |
| 15 | 双 Worker 竞争同一 loop_id | Lease 生效, 无重复 |
| 16 | 旧 generation 晚到 | 被拒绝 |
| 17 | 磁盘满 | 明确失败 |
| 18 | 时钟偏移 | 时间比较使用逻辑时钟 |

根不变量:

```
0 duplicate effects
0 unauthorized Committed
0 stale-generation commit
0 lost WorkItem
0 duplicate loop / duplicate Attempt
0 silent VerificationIncomplete
0 永久卡死且无明确状态
```

### S3.4 可观测性

传播: trace_id, flow_id, work_item_id, loop_id, attempt_id, run_id, verification_session_id, verification_unit_id, verifier_run_id, decision_id, settlement_id, generation

指标: verification_scope_incomplete_total, required_verifier_failed_total, ambiguous_location_total, stale_finding_rejected_total, lane_violation_total, duplicate_effect_prevented_total, session_resume_total, stale_generation_rejected_total, authority_resolution_latency

### S3 封口标准

```
18 项故障窗口重复运行
多 Worker 并行 DAG 成功
0 重复现实副作用
0 非权威 Committed
0 旧 generation 污染
所有失败有明确状态和证据
```

---

## S4: 领域扩张与产品化 (v0.4a — v0.4c)

### S4.1 WorkflowPack

```
DAG 环检测 | 不可达节点 | 缺失错误出口 | 无限重试
缺失幂等键 | AuthorityGate 缺失 | Escalated 解锁下游
generation 污染 | 定义与 Temporal History 不一致
```

位置: workflow_type, workflow_id, run_id, history_event_id, node_id, transition_id, definition_hash

### S4.2 DocumentPack

```
Markdown | OpenAPI | ADR | 协议文档

确定性: 必要章节, 链接有效, 标题层级, Schema 合法性, Golden Vector 一致
语义: 术语一致性, 代码与文档冲突, 安全说明缺失
```

### S4.3 DataPack (只读)

```
PostgreSQL Schema: Table, Column, Index, Constraint, Migration

禁止: 任意 SQL, 生产数据写入, 原始 PII 进 Evidence, 自动改 Schema
```

### S4.4 跨领域交叉验证 (最后)

```
代码 API 变化 → 验证 OpenAPI
Migration 变化 → 验证 Schema + DataPolicy
Activity 变化 → 验证 Workflow Retry/Idempotency
协议文档变化 → 验证 Go/Rust 结构 + Golden Vector
```

前提: 三个 Pack 均为真实 MVP, 不在 stub 之间转发 Finding。

---

## 建议延后/取消

| 项目 | 决策 | 原因 |
|------|------|------|
| 独立 VerificationInvocation 协议 | 延后 | 内部主链 Rust 内完成; 审计任务复用 LoopInvocationRequest |
| 26 语言全部机器验证 | 聚焦 | 保留 26 类 Markdown 语义规则, 机器 Profile 先做 Rust/Go/Cargo/GitHub Workflow/通用配置 |

---

## 版本路线图

| 版本 | 内容 | 完成后代表 |
|------|------|-----------|
| v0.2a | VerifierRunResult + Completeness + 三本 Ledger + 四态位置 | 验证语义不再含糊 |
| v0.2b | 规则 Bundle Hash + 机器 Profile + LaneGuard 接入 Gateway | 验证过程不可绕过 |
| v0.2c | 真实 IronClaw 双 Attempt + M6 闭环 | 真实单机可信 Agent 成立 |
| v0.3a | PG + Artifact Store + Session 恢复 + Budget | 状态可持久化恢复 |
| v0.3b | 多进程 Temporal + 多 Worker + 18 项故障注入 | 分布式安全不变量成立 |
| v0.4a | WorkflowPack | 能验证编排系统自身 |
| v0.4b | DocumentPack | 能验证文档、协议与实现一致性 |
| v0.4c | DataPack + 跨领域验证 | 通用 Assurance 平台 |

---

## 执行顺序

```
1. VerifierRunResult + Completeness + 三本 Ledger
2. 规则 Bundle Hash + Verification Profile + 四态位置
3. LaneGuard 接入 Capability Gateway
4. 真实 IronClaw Agent Loop + 两 Attempt 黄金链
5. PG Session/Evidence + Artifact Store + 精确恢复
6. 多进程 + 多 Worker + 真实故障注入
7. WorkflowPack → DocumentPack → DataPack → Cross-domain
```

**核心原则**: 先保证"验证结果是什么意思"完全清晰，再让真实 Agent 接入；先证明单机可信闭环，再证明进程和网络故障下仍可信；最后才扩张验证领域。
