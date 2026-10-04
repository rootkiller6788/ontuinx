# OntoOS 生产化路线图

> 现状：架构骨架 + 不变量 + 单节点语义正确性 ✅  
> 下一阶段：真实深度 + 真实环境 + 生产级证据

---

## 〇、当前状态 (2026-07-27) — ALL CODE COMPLETE

```
OntoOS Core                 ✅ ~403 tests
OntoFlow Integration        ✅ I0-I6 Harness
Runtime Acceptance          ✅ R-1/R0/R1/R2/R3
Verification Fabric v0.2    ✅ S0-S2 (P0-P3)
Productionization v0.3-0.4  ✅ S3-S4 (P4-P6)
Temporal Server             ✅ 197MB binary, CHASM registered
Docker + Fault Matrix       ✅ deploy/ ready
Go ontoflow                 ✅ 63 tests
Protocol v1                 ✅ Frozen
```

**四层成熟度：**

```
1. IMPLEMENTATION COMPLETE                    ✅
2. INTEGRATION HARNESS ACCEPTANCE COMPLETE    ✅
3. REAL RUNTIME ACCEPTANCE                    ✅ (single-node)
4. PRODUCTION READINESS                       ⏳
```

---

## 一、发行路线

| 版本 | 名称 | 目标 | 封口标准 |
|------|------|------|---------|
| v0.2 | Verification Depth | 位置解析 + Rust/Go 规则生产化 | Evidence 从"行号提示"升级为"可锚定证据" |
| v0.3 | Real Trusted Runtime | 真实 IronClaw + 真实 M6 + 多进程故障 | AI 执行→验证→裁决→副作用全部真实闭环 |
| v0.4 | Domain Assurance | WorkflowPack + DocumentPack + DataPack | Verification Fabric 扩展为通用验证基础设施 |

---

## 二、优先级总排序

| P | 缺口 | 原因 |
|----|------|------|
| P0 | 位置解析生产化 | 直接决定 Finding 能否成为可靠 Evidence |
| P1 | Rust/Go 规则生产化 | 决定验证实际能发现什么 |
| P2 | 真实 IronClaw Agent Loop | 系统从 Mock 变成真实 AI 执行 |
| P3 | 多进程分布式验收 | 真实故障下不变量是否成立 |
| P4 | WorkflowPack | 直接验证自身 OntoFlow 语义 |
| P5 | DocumentPack | 强化协议、文档与代码一致性 |
| P6 | DataPack | 高价值，安全复杂度最高 |

---

## 三、P0：位置解析生产化

### 目标

从 `line_offset` 升级为证据锚定系统。核心原则：

> **唯一确定时返回唯一位置；存在歧义时明确返回 Ambiguous；快照变化时返回 Stale，绝不猜测。**

### 数据模型

```rust
pub struct SourceLocationClaim {
    pub target_ref: String,           // 规范目标引用
    pub claimed_lines: Option<LineRange>,
    pub snippet: Option<String>,
    pub symbol_name: Option<String>,
    pub byte_range: Option<ByteRange>,
}

pub enum SourceLocationResolution {
    Resolved(ResolvedSourceAnchor),
    Ambiguous(Vec<AnchorCandidate>),
    Unresolved(UnresolvedReason),
    StaleSnapshot { expected_hash: String, actual_hash: String },
}
```

### 解析流水线

```
FindingCandidate
  → 目标路径规范化与边界校验
  → 快照与 file_hash 验证
  → 原始 byte/line 范围校验
  → Diff Hunk 新侧匹配         }  两个通道独立运行
  → 完整文件精确片段匹配        }  都成功=Cooroborated
  → Symbol 索引匹配             }  只有一个=Single-source
  → AST 节点匹配                }  都失败=Unresolved
  → Context Hash 确认           }  指向不同=Ambiguous
  → Resolved / Ambiguous / Unresolved / Stale
```

### 文件归属

```
onto-assurance-types/src/evidence_location.rs    — 数据模型
onto-assurance-core/src/location_resolution.rs   — 纯函数
onto-code-pack/src/location/
  ├── diff_index.rs         — Diff hunk 索引
  ├── source_index.rs       — 全文件索引
  ├── tree_sitter_index.rs  — AST 索引 (Rust/Go)
  ├── symbol_index.rs       — 符号索引
  └── source_location_adapter.rs
```

### 封口门槛

```
L-1 旧 snapshot 位置 100% 拒绝
L-2 Ambiguous 绝不升级为唯一 Evidence
L-3 删除侧位置绝不绑定当前 Checkpoint
L-4 Evidence 包含 file_hash + context_hash
L-5 相同输入解析结果与 Hash 完全一致
L-6 Rust/Go Golden Corpus 中不存在"错误唯一定位"
```

---

## 四、P1：Rust/Go 规则生产化

### 目标

从每条语言 2 条骨架规则升级为 ~30 条高价值生产规则。

### 规则模型

```rust
pub struct AssuranceRule {
    pub rule_id: StableRuleId,
    pub version: RuleVersion,
    pub selector: RuleSelector,
    pub assertion: RuleAssertion,
    pub evidence_policy: EvidencePolicy,
    pub enforcement: EnforcementLevel,   // Enforced / Advisory / Experimental
    pub criterion_mapping: Vec<CriterionMapping>,
    pub remediation: Option<RemediationGuide>,
    pub provenance: RuleProvenance,
    pub lifecycle: RuleLifecycle,        // Draft→Shadow→Advisory→Enforced→Deprecated
}
```

### Rust 规则覆盖 (30 条)

| 类别 | 重点 |
|------|------|
| 错误处理 | unwrap/expect、错误吞噬、错误上下文 |
| Unsafe | SAFETY 说明、边界验证、unsafe trait 实现 |
| 并发 | 锁顺序、跨 await 持锁、Send/Sync |
| 资源 | 文件、连接、临时目录、进程清理 |
| API 兼容 | public enum、trait 修改、序列化兼容 |
| 供应链 | 依赖风险、feature 组合、build.rs |
| Onto 专项 | Decision 绕过、Evidence 污染、generation 绑定 |

### Go 规则覆盖 (30 条)

| 类别 | 重点 |
|------|------|
| 并发 | goroutine 退出、channel 关闭、锁顺序 |
| Context | 传播、超时、取消 |
| 错误 | 忽略、wrap、sentinel 判断 |
| Nil | typed nil、map/channel 零值 |
| 资源 | Body Close、Rows Close、Ticker 停止 |
| Temporal 专项 | 确定性、Activity 边界、重试、幂等 |
| OntoFlow 专项 | Committed 门禁、generation、DAG 解锁 |

### 每条规则必须带测试资产

```
rules/rust/RUST-ERR-001/
├── manifest.toml
├── positive/           (≥3 正样例)
├── negative/           (≥3 负样例)
├── edge/               (≥1 边界样例)
└── expected_findings.json
```

### 例外管理

```rust
pub struct RuleSuppression {
    pub rule_id: StableRuleId,
    pub justification: String,
    pub approved_by: String,
    pub scope_hash: String,
    pub expires_at: String,
}
```

必须：有理由、有范围、有审批、有过期时间、绑定具体快照。

### 封口门槛

```
RP-1 Rule ID 稳定，版本升级不改变旧证据含义
RP-2 每条 Enforced 规则有完整 fixture
RP-3 不支持的语言明确 Unsupported，不静默跳过
RP-4 规则执行失败与规则未命中严格区分
RP-5 RulePack Hash 参与 Session 和 Evidence 绑定
RP-6 Suppression 可审计、可过期、不能跨快照滥用
```

---

## 五、P2：真实 IronClaw Agent Loop

### 目标

把 Mock 执行主体替换为真实 IronClaw Agent Loop。

### 不走 HTTP 入口

```rust
pub trait RunIngressPort {
    async fn start_attempt(&self, request: AttemptRunRequest) -> Result<AttemptRunHandle, RunIngressError>;
    async fn await_exit(&self, handle: AttemptRunHandle) -> Result<AttemptExitReport, RunIngressError>;
    async fn cancel(&self, handle: AttemptRunHandle) -> Result<(), RunIngressError>;
}
```

由 `onto-ironclaw-adapter` 实现。HTTP 只是产品入口，不是内核依赖。

### 五个接入点

```
1. BeforeRun          — 注入 Contract、Attempt、预算、Scope
2. BeforeCapability   — 授权与 Effect 分类
3. AfterCapability    — 捕获 RuntimeObservation 和 SideEffectManifest
4. AfterLoopExit      — 封存 CandidateCheckpoint，进入 Verification
5. Settlement         — 只有 Decision 和 Permit 允许 Publish/Discard
```

所有退出路径必须经过 AfterLoopExit。不能存在"异常退出直接写 Completed"的旁路。

### 执行 Lane / 验证 Lane 隔离

```
Agent Execution Lane:
  ✅ 可写 staging → 生成 Candidate
  ❌ 不能给自己的修改生成强 Evidence

Semantic Verification Lane:
  ✅ 只读 Candidate Snapshot
  ❌ 无 Publish 权限、无 Decision 权限
  只返回 FindingCandidate[]
```

### 三阶段上线

| 阶段 | 执行 | 验证 | Commit |
|------|------|------|--------|
| Shadow | 真实 IronClaw | Mock 比对 | Mock 控制 |
| Gated | 真实 IronClaw | 真实 VF | 人工批准 |
| Enforced | 真实 IronClaw | 真实 VF | OntoAssure Decision |

### 黄金验收 (10 项)

```
IA-1  单文件一次 Attempt 成功
IA-2  第一次写错、第二次根据 CriterionGap 修复
IA-3  非法文件写入被 CapabilityHost 拒绝
IA-4  工具输出成功但 Verifier UNSAT
IA-5  模型声称成功但无 Evidence
IA-6  Provider 中断后恢复同一 Attempt
IA-7  Budget 耗尽不产生 Success
IA-8  Commit 后响应丢失不重复 Publish
IA-9  Semantic Lane 尝试写文件被拒绝
IA-10 所有退出路径均经过 FinalizationGateway
```

---

## 六、P3：多进程分布式验收

### 目标

单节点已证明算法和状态机。生产化证明：**进程、网络和存储可以失败，但权威不变量仍然成立。**

### 拓扑

```
Temporal Frontend + History + Matching + Internal Worker
PostgreSQL + Authority Projection
Rust Onto Worker A / B / C
Artifact Store + Fault Injector
```

### 故障矩阵

| 故障 | 必须证明 |
|------|---------|
| Worker 取得任务前崩溃 | 任务重新分发 |
| Heartbeat 后崩溃 | 新 Worker 恢复稳定 loop_id |
| M6 Commit 后崩溃 | 不重复副作用 |
| Completion RPC 响应丢失 | 幂等 re-Respond |
| Temporal 整体重启 | CHASM 组件恢复 |
| PG 短暂不可用 | 不降级信任 Worker |
| Authority 不可用 | 保持 OutcomeReported |
| 两个 Worker 竞争 | 单一 LoopExecutionLease |
| 旧 generation 晚到 | 拒绝 |
| 磁盘满 | 冻结或 EnvironmentError |

### 观测

所有服务传播：`trace_id → flow_id → work_item_id → loop_id → attempt_id → run_id → decision_id`

### 封口

```
0 重复现实副作用
0 无 Authority 的 Committed
0 旧 generation 覆盖
0 WorkItem 永久丢失
所有恢复路径有完整 Evidence
```

---

## 七、P4-P6：WorkflowPack → DocumentPack → DataPack

### WorkflowPack (P4)

验证 OntoFlow 自身：可达性、死节点、循环、缺失幂等键、Barrier 条件、DAG 环、Authority gate 缺失、generation 污染。

### DocumentPack (P5)

Markdown / OpenAPI / ADR：章节存在、链接有效、Schema 合法、协议字段与 Golden Vector 一致。

### DataPack (P6)

PostgreSQL Schema 只读验证：迁移差异、NOT NULL 风险、索引缺失、外键完整性、PII 字段分类。

安全边界：只读账号、statement_timeout、禁止任意 SQL、敏感值不存 Evidence。

---

## 八、不变边界（永久冻结）

```
IronClaw 产生行为和候选结果
Verification Fabric 组织验证
Verifier 产生运行结果和 Finding
Evidence Authority 决定哪些结果可信
Decision Authority 决定是否成功
M6 决定现实副作用是否允许结算
Temporal/OntoFlow 决定多个可信任务如何持久化推进

Verifier 没有成功宣告权
Finding 不等于 Evidence
Worker reported Committed ≠ WorkItem Committed
AuthorityVerified = WorkItem Committed
```

---

## 九、版本记录

| 版本 | 日期 | 里程碑 |
|------|------|--------|
| v0.1 | 2026-07-27 | Implementation + Harness + Single-node Runtime |
| v0.2 | 2026-07-27 | Verification Depth (P0-P3 + S0-S2) ✅ |
| v0.3 | 2026-07-27 | Real Trusted Runtime (S1.1-S1.2 + S3.2-S3.3) ✅ |
| v0.4 | 2026-07-27 | Domain Assurance (P4-P6 + S4) ✅ |
| v1.0 | TBD | Production: docker compose up + fault-matrix.sh + TLS/IAM |
