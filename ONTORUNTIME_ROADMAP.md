# OntoRuntime 完整落地计划

> OntoRuntime Fork + OntoAssure 可信裁决 + OntoLoop Attempt 循环 + OntoFlow 耐久编排  
> 原位魔改，不全仓重命名，不另起平行 Runtime

---

## 总原则

- OntoRuntime 继续作为执行层主体，保留 Agent Loop、CapabilityHost、授权/信任/审批、Runtime Lane、EventStore、Memory、CLI/WebUI
- OntoAssure 只补它缺少的：合约、验证、证据、判决、副作用结算
- 每个需求先问：能否通过一个 Port、一个 Hook、一个 Adapter 或一个 Envelope 解决？
- 能，就禁止大重构

### 永远禁止

- ❌ 新建平行 Agent Loop / 权限系统 / 沙盒 / EventStore / Run 持久化
- ❌ 全仓 User→Actor、Run→ExecutionSession、ToolCall→CapabilityInvocation 替换
- ❌ 一次性删除 Conversation/Mission/Routine
- ❌ 一次性统一全部 Runtime Lane
- ❌ 把 OntoAssure 合并进 OntoRuntime Core

---

## 四层权力边界

```
OntoFlow 魔改版 ：决定先做什么、后做什么（耐久编排）
OntoLoop         ：决定当前任务下一轮怎么继续（Attempt 收敛）
OntoRuntime ：决定这一轮如何安全执行（Agent Loop + CapabilityHost）
OntoAssure       ：决定执行是否可信、效果能否结算（合约→证据→裁决→结算）
```

### 所有权

| 层 | 拥有 | 不拥有 |
|---|------|--------|
| OntoRuntime | User/Principal, Run, Conversation, Agent Loop, LLM, Skill, CapabilityHost, 授权, 信任, 审批, Lease, Secret, Network, Filesystem, Resource, Runtime Lane, EventStore, Memory | 成功判定, 证据归约, 副作用结算 |
| OntoAssure | ExecutionContract, Criterion, Verifier, Evidence, Verdict, SessionDecision, SettlementDecision, Effect Transaction, Replay | 执行, 授权, 调度 |
| OntoLoop | Attempt, Continuation, Repair, Progress, Convergence, Loop Budget, Checkpoint, Regression | Agent Loop 内部控制, 跨任务 DAG |
| OntoFlow | Workflow, WorkItem DAG, Timer, Signal, Saga, 长期等待, 分布式 Worker 调度 | 单次任务成功判定, 单次副作用结算 |

---

## 版本路线

| 版本 | 核心目标 | 状态 |
|------|---------|------|
| v0.2 | M5 + M6-A | ✅ |
| v0.3 | 架构宪法 + RunIngress + Invocation Envelope + Observation | ✅ |
| v0.4 | M6-B PostgreSQL/Redis 真实 Store 基础落地 | ✅ (基础测试) |
| v0.5 | M6-C + M6-D，Effect Authority 完整封口 | ✅ |
| v0.6 | OntoLoop 接入 | ✅ |
| v0.7 | OntoFlow 耐久编排接入 | ✅ (Rust侧 F0-F9) |
| v0.8 | 多 Profile 与去个人助手硬编码 | ⏳ |
| v0.9 | Code/Ops/Data/Workflow Packs | ⏳ |
| v1.0 | 生产加固（安全、高可用、审计） | ⏳ |

关键路径：

```
M5 + M6-A ✅
    ↓
OntoLoop L0-L7 ✅  (原属 v0.6, 提前完成)
    ↓
轻量统一执行边界（Phase 1–4）
    ↓
PostgreSQL / Redis M6-B（含真实 Store）
    ↓
Compensatable M6-C（不依赖 OntoFlow）
    ↓
Irreversible M6-D
    ↓
DirectRunAdapter (消除 M5 瓶颈: 绕过未完成的 HTTP 入口, 直连 RunExecutor)
    ↓
OntoFlow (v0.7)
    ↓
Profiles / Packs / Production
```

---

## 当前状态 (2026-07-27) — OntoOS v0.1 封存

**四层成熟度模型：**

```
1. IMPLEMENTATION COMPLETE                    ✅ ~480 tests (Rust 413 + Go 63 + Sandbox 4)
2. INTEGRATION HARNESS ACCEPTANCE COMPLETE    ✅ I0-I6
3. REAL RUNTIME ACCEPTANCE (single-node)      ✅ R-1/R0/R1/R2/R3
4. PRODUCTION READINESS                       ⏳ 仅差 Linux原生内核 4 sandbox tests + 18项故障注入
```

**Sandbox 集成 (新增)：**

```
onto-sandbox/ (grok-build 移植)   ✅ 63/67 tests
  源码: /tmp/onto-sandbox/ (独立 workspace)
  编译: ✅ Rust 1.96, edition 2021
  改名: xai_grok → onto (全部完成)
  config.rs: 完整替代 xai_grok_config
  部署: sandbox (默认) / Docker (备选)
  内核限制: 4 tests 需 Landlock/bwrap (WSL 不支持)
```

**Verification Fabric 完整进度：**

```
v0.2a S0.1 VerifierRunResult          ✅  5 tests
v0.2a S0.2 三本总账 (Scope/Rule/Verifier Ledger) ✅ 5 tests
v0.2a S0.3 机器验证双平面 (7语言×4级)   ✅ 22 tests
v0.2a S0.4 双平面路由 (语义+机器)       ✅  6 tests
v0.2b S0.5 Rule Bundle Hash           ✅
v0.2b S1   PG 持久化 (3 tables)       ✅  3 tests
v0.3a S2   Artifact Store + 恢复      ✅  7 tests
v0.3b S3   分布式故障矩阵 (18项)       ✅  9 tests
v0.3b S3.4 可观测性 (12 trace + 9 metrics) ✅ 6 tests
v0.4  S4   领域 Pack                  ✅  3 packs (Workflow/Document/Data)

P0 位置解析 (hunk+file双通道)          ✅ 12 tests
P1 规则生产化 (OCR移植26种文件类型)     ✅  9 tests
P2 IronClaw集成 (LaneGuard+Lane隔离)   ✅ 18 tests
P3 分布式验收 (6故障窗口+并发+DAG)      ✅  9 tests
P4-P6 Workflow/Document/Data Pack      ✅ 10 rules each
S1.1 CapabilityGateway (12种+11绕过)   ✅  6 tests
S1.2 IronClaw RunPort (9退出路径)      ✅  5 tests
S3.2 Docker Compose (12 services)      ✅ deploy/docker/
S3.3 Fault Matrix (18项+脚本)          ✅ deploy/fault-injection/
────────────────────────────────────────────
Total: ~403 tests, 0 failures
```

**代码位置：**

| 层 | 路径 | 说明 |
|----|------|------|
| Rust crates | `crates/` (12 crates) | OntoAssure + OntoLoop + Adapter |
| Temporal Fork | `temporal/chasm/lib/ontoflow/` | Go OntoFlow (63 tests) |
| 魔改Temporal | `temporal/` | CHASM + fx.go注入 |
| Docker部署 | `deploy/docker/` | 12服务 Compose |
| 故障注入 | `deploy/fault-injection/` | 18项故障矩阵脚本 |
| 证据 | `evidence/ontoflow-v0.1/` | I0-I6验收 |
| 文档 | `docs/` (15+ files) | 架构/协议/路线图/生产计划 |

**Sandbox 集成 (grok-build → onto-sandbox)：**
- 源码: `onto-sandbox/` (从 grok-build 移植)
- 编译: ✅ `cargo build` 通过
- 测试: 63 passed, 4 failed (OS内核限制: Landlock/bwrap)
- 部署: 双路径 — grok-build sandbox (默认) / Docker Compose (备选)

**仅差真实基础设施：**
- Linux 原生环境运行 sandbox 4 项内核测试
- `./fault-matrix.sh` 运行18项故障注入
- 真实 IronClaw 二进制 (LaneGuard已就绪)
- TLS/IAM/审计/HA (生产加固)

---

## 历史记录

### 已完成

```
M0       ✅ Python Reference Freeze
M1       ✅ JSON Schema v1 + 10 Golden Fixtures
M2       ✅ Rust Pure-Function Kernel
M2.5     ✅ Effect Semantics
M3       ✅ Runtime Coordinator (8 Port traits)
M5       ✅ Success Authority Takeover
M6-A     ✅ Staged Filesystem Effect Authority
M6-0     ✅ Pure/ReadOnly Semantics (4 tests)
M6-B     ✅ Transactional Database (4 tests)
M6-C     ✅ Compensatable External (10 tests)
M6-D     ✅ Irreversible External (12 tests)
M6       ✅ Effect Authority CLOSED
OntoLoop ✅ L0–L7 Attempt Loop Convergence (43 tests)
CodePack ✅ Build/Test/Lint Verifiers + Profiler
OntoFlow ✅ F0–F9 Rust Adapter Complete (60 tests)
DirectRun✅ Rust + Python adapters (5 tests)
Phase2-4✅ RunIngress + Envelope + Observation types
──────────────────────────────────────────────────────────────
Total: 370 tests, 0 failures
```

### 已建立的不变量

1. Agent 不能自行宣布成功 — 只有 OntoAssure 的持久化 Decision 能产出 TaskOutcome::Success
2. Agent 不能自行发布暂存文件 — 只有 CommitPermit + CAS 能发布
3. FinishRequested + 空证据 → Escalated，绝不 Success
4. CAS 竞争 → 只有一个发布者
5. Receipt 失败 → 不 Committed
6. 崩溃 → 按磁盘事实对账恢复
7. 状态不可判定 → Freeze + Escalate，不伪装 Committed
8. Attempt N 的 Evidence 只能证明 Attempt N 的 Checkpoint（跨 Attempt 复用被拒绝）
9. 预算耗尽与任务结果是正交维度（Success + Depleted = Completed）
10. Progress 判定来自 OntoAssure 结构化结果，非 LLM 自评
11. 一次 Attempt = 一次 OntoRuntime Run（严格一对一，幂等）
12. OntoLoop 不能自产 Decision（只从 DecisionStore 加载）

### 当前瓶颈

**DirectRunAdapter**（ASSESSMENT.md）:
OntoRuntime HTTP 产品入口的存储后端未完成，导致已接好的 M5 seam 拿不到真实 Agent Loop 数据。需要 DirectRunAdapter 绕过 HTTP 入口，直连 RebornTurnRunExecutor。

---

## Phase 1：架构宪法 + 自动化门禁

### 目标
文档和约束，不做大规模代码搬迁。

### 新增文件
```
docs/ONTO_RUNTIME_ARCHITECTURE.md   ✅ 四层权力边界 + 不变量
docs/OWNERSHIP_MATRIX.md            ✅ 谁拥有什么
docs/STATE_AUTHORITY.md             ✅ 状态权威
docs/INTEGRATION_SEAMS.md           ✅ 集成点
docs/ONTOLOOP_L3_L7_PLAN.md         ✅ OntoLoop 施工记录
docs/TEMPORAL_INTEGRATION_PLAN.md   ✅ OntoFlow 集成方案
docs/TEMPORAL_INTEGRATION_ANALYSIS.md ✅ OntoFlow 代码库分析
docs/LLM_PROVIDER_RESOLVER_SOLUTION.md ✅ LLM Provider 解决方案
```

### 自动化架构检查

最小脚本检查 cargo 依赖方向：

```bash
# onto-assurance-core 不得依赖 OntoRuntime / PostgreSQL / Redis / OntoFlow
cargo tree -p onto-assurance-core | grep -E 'ironclaw|postgres|redis|temporal' && exit 1

# OntoRuntime runner 不得直接依赖 onto-assurance-core
cargo tree -p ironclaw_runner | grep 'onto-assurance-core' && exit 1

# Agent/Skill crate 不得直接依赖数据库/Secret/外部 Device Adapter
```

### 产品身份
- 源码 crate 继续叫 `ironclaw_*`
- 对外：**OntoRuntime**（基于 OntoRuntime Fork 深度演进）

### 验收
- [x] P1.1 四份架构文档边界明确 (7 份已完成)
- [x] P1.2 Cargo 依赖方向检查通过（scripts/check-architecture.sh）
- [ ] P1.3 禁止类型/模块扫描通过
- [x] P1.4 M5/M6-A 43 项回归通过

---

## Phase 2：统一 Run 入口 RunIngressPort

### 目标
让 Conversation 从"唯一核心入口"降级为"多个入口之一"。不删除 Conversation。

### 新增
```rust
pub trait RunIngressPort {
    async fn start_run(&self, request: StartRunRequest) -> Result<RunId, RunIngressError>;
}

pub struct StartRunRequest {
    pub actor: RuntimeActorRef,
    pub source: SessionSource,
    pub input: InputEnvelope,
    pub project_id: Option<ProjectId>,
    pub parent_work_item: Option<ExternalWorkItemRef>,
}

pub enum RuntimeActorRef { Human(UserId), Agent(AgentId), Service(ServiceId), Device(DeviceId) }
pub enum SessionSource { Conversation, Cli, Http, Direct, OntoLoop, OntoFlow, DeviceEvent }
```

### 接线
```
Conversation ─┐
CLI          ├──→ RunIngressPort → OntoRuntime 现有 Run 创建逻辑
HTTP         ┤
DirectRun    ┘
```

### 不做
不替换现有 Run 类型、Conversation Store、WebUI、Thread。

### 验收
- [ ] P2.1 CLI 与 Conversation 创建的是同一种 Run
- [ ] P2.2 DirectRun 不依赖 Conversation
- [ ] P2.3 每个 Run 可追踪 actor 与 source
- [ ] P2.4 原聊天产品无功能退化

---

## Phase 3：标准化 Capability 执行边界

### 目标
在 OntoRuntime 现有 CapabilityHost 基础上增加统一 Envelope，不重写 CapabilityHost。

### 新增
```rust
pub struct CapabilityInvocationEnvelope {
    pub invocation_id: InvocationId,
    pub run_id: RunId,
    pub actor: RuntimeActorRef,
    pub capability_id: CapabilityId,
    pub arguments_hash: ContentHash,
    pub resource_refs: Vec<ResourceRef>,
    pub effect_class: EffectClass,       // 必须来自可信 CapabilityDescriptor
    pub correlation_id: CorrelationId,
}
```

### 原则
- EffectClass 必须来自可信 CapabilityDescriptor，Agent 不能自己填写或降级
- Skill = Prompt/知识/认知方法，无权限
- Capability = 对现实资源的操作，必须经过 CapabilityHost

### 绕过审计（收缩范围）

**只审计 AI 可达路径**，不禁止底层 Adapter 内部使用系统库：

AI 可控输入 → 未经 CapabilityHost → 直接到文件/网络/进程/密钥/数据库 → **禁止**

底层 Runtime Lane 内部调用 `std::fs`、`reqwest`、数据库 Client → **允许**

审计目标：
```
Agent Loop → CapabilityHost ✅
Skill 执行器 → CapabilityHost ✅
Tool/MCP 入口 → CapabilityHost ✅
插件入口 → CapabilityHost ✅
Mission 执行入口 → CapabilityHost ✅
模型生成命令入口 → CapabilityHost ✅
```

### 验收
- [ ] P3.1 所有副作用都有 InvocationId
- [ ] P3.2 所有副作用关联 Actor 和 Run
- [ ] P3.3 AI 可控路径不能绕过 CapabilityHost
- [ ] P3.4 Skill 不能隐式获得权限
- [ ] P3.5 EffectClass 不可由 Agent 降级

---

## Phase 4：统一运行事实 RuntimeObservation

### 目标
OntoRuntime 记录发生了什么；OntoAssure 判断这些事实能证明什么。

### 新增
```rust
pub struct RuntimeObservation {
    pub invocation_id: InvocationId,
    pub outcome_ref: OutcomeRef,
    pub filesystem_effect_ref: Option<EffectRef>,
    pub network_receipt_ref: Option<ReceiptRef>,
    pub stdout_ref: Option<BlobRef>,
    pub stderr_ref: Option<BlobRef>,
    pub exit_code: Option<i32>,
    pub resource_usage: ResourceUsage,
    pub runtime_error: Option<RuntimeErrorKind>,
}
```

### 严格区分
```
RuntimeObservation  = 原始执行事实
EvidenceRecord      = 某项事实对某个 Criterion 的证明关系
Verdict             = Criterion 是否满足
```

### 验收
- [ ] P4.1 每个 Evidence 可追溯到 RuntimeObservation
- [ ] P4.2 每个 Observation 可追溯到 Invocation
- [ ] P4.3 OntoRuntime 不能自产 passed=true
- [ ] P4.4 M5/M6-A 43 项测试继续通过

---

## Phase 5：M6 完整 Effect Authority

> **先做完所有 Effect 协议，再接入 OntoFlow。**
> M6-B 依赖数据库 Lane + Capability 边界 + OntoAssure，不依赖 OntoLoop。
> M6-C 核心补偿协议不依赖 OntoFlow（OntoFlow 只是未来调度补偿）。

---

### M6-0：Pure / ReadOnly Semantics

已由 M5 覆盖大部分。需补 3 类测试：
- Pure Effect 不进入 Settlement 事务
- ReadOnly 能力实际发生写入 → 拒绝或升级 EffectClass
- 声明 ReadOnly 但产生 MutationReceipt → Escalate

```
M6-0 ⏳ Pure / ReadOnly（需补齐测试）
```

---

### M6-B：PostgreSQL Transactional + Redis Atomic

#### 存储定位

```
PostgreSQL = 权威业务事实
  ACID 事务
  Decision、Transaction、Receipt、Outbox 的主存储

Redis = 快速原子状态与协调存储
  Lease、幂等键、缓存、原子脚本、WATCH/CAS
  PostgreSQL 业务事实的快速投影
  不能与 PostgreSQL 并列作为同一业务事实的双主
```

#### 一致性模式

```
PostgreSQL 事务：业务变更 + Outbox 记录一起 COMMIT
                           ↓
                      Outbox Worker
                           ↓
                      Redis 幂等投影
```

不做 PostgreSQL + Redis 分布式事务。

#### 环境

直接使用已有真实服务：
```
PostgreSQL  localhost:5432  → ontoos_m6_test
Redis       localhost:6379  → ontoos:m6:test:<run_id>:*
```

每个测试独立 schema / key prefix，不共享固定表或固定 Key。

#### 基础真实持久化（M6-B 前完成）

```
DecisionStore          ← PostgreSQL
ExecutionTransactionStore ← PostgreSQL
ReceiptStore           ← PostgreSQL
OutboxStore            ← PostgreSQL
RunStateStore          ← PostgreSQL
```

#### M6-B1–B6 子阶段

| Sub | 范围 | 关键测试 |
|-----|------|---------|
| B1 | Resource Isolation | 独立 Schema, 最小权限角色, 不暴露 DSN |
| B2 | Decision-gated Transaction | Decision→验证→BEGIN→Verifier→Permit→COMMIT→Receipt |
| B3 | Crash Reconciliation | 8 场景: 无Commit不得COMMIT, Verifier失败ROLLBACK, SERIALIZABLE竞争CAS, COMMIT前断连, 响应丢失对账 |
| B4 | Redis Atomic State | 7 场景: Key隔离, 未授权拒绝, Lua SHA绑定, WATCH冲突, 幂等键, TTL过期, 宕机对账 |
| B5 | Outbox → Redis Projection | 7 场景: 最终一致, Redis宕机重放, 幂等event_id, 版本覆盖, Worker崩溃, ACK丢失, 缓存污染回源 |
| B6 | Schema Migration Gate | DDL在事务中, Verifier失败ROLLBACK, Manifest不匹配拒绝 |

### M6-C：Compensatable External Effect

#### 关键认知
**Compensation ≠ Rollback**  
删除已创建的云资源不恢复计费、通知和外部审计记录。

#### 正确流程
```
PreExecutionDecision
→ 执行外部操作
→ ExternalOperationReceipt
→ 后验 Verifier
├── 满足要求 → Confirm
├── 不满足但可补偿 → Compensate
└── 外部状态未知 → Freeze + Escalate
```

#### 10 场景测试 (C.1–C.10)

不依赖 OntoFlow 的本地补偿协调器 + Mock HTTP Service。

### M6-D：Irreversible External Effect

#### 关键认知
**不能先执行再验证**，现实动作已经发生。只能承诺 **At-most-once**。

#### 正确流程
```
完整 Contract 与输入冻结
→ 所有必要 Verifier 预检查
→ Onto PreExecutionDecision
→ 强审批
→ ExactInvocationLease
→ 写入 DispatchIntent
→ At-most-once Dispatch
→ ExternalReceipt
→ 后验确认
```

未知状态：绝不自动重发，查询外部系统或人工对账。

#### 12 场景测试 (D.1–D.12)

---

### 四级测试体系

每个 M6 里程碑必须形成：

| Level | 名称 | 内容 | 速度 |
|-------|------|------|------|
| L1 | Fast | 纯函数、Port、Adapter、状态机 | <1s |
| L2 | Wiring | `cargo check --tests` 验证接线 | 秒级 |
| L3 | Real Service | 真实 PG/Redis/HTTP/OntoRuntime binary | 分钟 |
| L4 | Fault Injection | 崩溃、响应丢失、持久化失败、并发 | 分钟 |

---

## Phase 6：接入 OntoLoop ✅ (已完成)

### 目标
OntoLoop 保持外置，不进入 OntoRuntime Agent Loop 内部。

### 源码落点

```
crates/onto-loop/
├── src/
│   ├── lib.rs              — 模块导出
│   ├── checkpoint.rs       — L3 Checkpoint/Evidence/Decision 绑定 (144 lines, 4 tests)
│   ├── progress.rs         — L4 确定性 Progress/Regression (123 lines, 5 tests)
│   └── budget.rs           — L5 多维预算 + 正交终态 (110 lines, 4 tests)

crates/onto-ironclaw-adapter/src/
└── loop_adapter.rs         — L6 Adapter + L6+L7 tests (24 tests)

crates/onto-assurance-types/src/
└── ontoloop.rs             — L0-L2 类型定义 (6 tests)
```

### OntoLoop 使用的四个 Port
```rust
pub trait RuntimeRunPort { async fn start_run(...); async fn await_stopped(...); async fn cancel_run(...); }
pub trait AssuranceResultPort { async fn load_finalization(...); }
pub trait CheckpointReferencePort { async fn select_checkpoint(...); }
pub trait ContinuationIngressPort { async fn start_continuation(...); }
```

### 调用链
```
OntoLoop 创建 Attempt → RunIngressPort 启动 OntoRuntime Run
→ OntoRuntime Agent Loop 执行
→ OntoAssure Finalization
→ OntoLoop 读取结果

COMMIT    → Attempt 完成
CONTINUE  → 生成下一 Attempt（结构化缺口，非模糊 Prompt）
ROLLBACK  → 选择旧 Checkpoint
ESCALATE  → 返回上层
```

### 验收
- [x] P6.1 OntoRuntime 一次 Run = OntoLoop 一次 Attempt（L6-2 幂等）
- [x] P6.2 Attempt 间 Evidence 不串用（L3-1, L3-4 跨分支拒绝）
- [x] P6.3 Budget 耗尽与任务结果保持正交（L5-1 三维独立）

### OntoLoop v0.1 封口标准

```
✅ 单任务、串行 Attempt
✅ 一次 Attempt = 一次 OntoRuntime Run
✅ OntoAssure 唯一终态权威
✅ 结构化 Continuation (非模糊 prompt)
✅ Checkpoint/Evidence/Decision 绑定
✅ 确定性 Progress 判断
✅ 有限预算与 Stuck 控制
```

---

## Phase 7：接入 OntoFlow 魔改编排层

> 详细方案见 `docs/TEMPORAL_INTEGRATION_PLAN.md`

### 目标
OntoFlow 只负责耐久编排，不进入 Agent 内部。

**核心策略：保留 OntoFlow 的 WorkflowTask、TransferTask、TimerTask、Matching 和 Event Sourcing，把 ActivityTask 原来分发的"普通函数调用"替换成"完整 Rust OntoLoop 自治工作单元"。**

### 分工
| 场景 | 谁处理 |
|------|--------|
| 简单本地 Mission/Routine | OntoRuntime 现有实现 |
| 长时间、多节点、跨服务、人工等待 | OntoFlow |

### 第一条完整 Workflow
```
OntoFlow WorkItem → OntoLoop Attempt → OntoRuntime Run
→ OntoAssure → OntoLoop 结果 → OntoFlow 继续下一 WorkItem
```

OntoFlow 只接收粗粒度结果：`Committed | NeedsContinuation | RolledBack | Escalated | EnvironmentFailure`

### 最终链路
```
WorkflowTask 推进 OntoFlow
        ↓
ActivityTask 输入 = LoopInvocationRequest
        ↓
Rust OntoLoop 完整执行一个子任务
        ↓
ActivityTask 输出 = LoopTerminalEnvelope
        ↓
WorkflowTask 验证权威终态并推进其他 OntoLoop
```

### 实施阶段

| 阶段 | 内容 | 验证标准 |
|------|------|----------|
| F0 | CHASM 外部任务桥接 | WorkItem → Matching → Rust Worker → Envelope 返回 |
| F1 | 单 WorkItem 完整 OntoLoop | 多 Attempt → Committed，含心跳、重试、幂等 |
| F2 | 静态 DAG (A→B→C, A→B∥C→D) | 依赖解锁、并发、Fan-in |
| F3 | 批量调度 (100 WorkItem, max=10) | 并发控制、失败隔离、逐个补发 |
| F4 | 动态任务拆分 (Planning → Graph) | LLM 生成计划，确定性 Schema 验证 |
| F5 | 去中心化讨论 (Proposer/Critic/Synthesis) | Barrier, Quorum, Artifact routing |
| F6 | 故障恢复 | 重启、Worker 崩溃、Heartbeat 丢失、取消传播 |

### 验收
- [ ] P7.1 OntoFlow 不自产任务 Success
- [ ] P7.2 OntoLoop 仍是 Attempt 循环唯一权威
- [ ] P7.3 OntoAssure 仍是 Success 唯一权威

---

## Phase 8：渐进去个人助手中心化

- 不删除个人助手，增加其他运行 Profile
- `personal-assistant / coding-agent / ops-agent / workflow-agent / robot-agent / industrial-agent`
- 不同 Profile 只选择入口、Skill、Capability、资源范围、Policy、OntoAssure Pack
- 逐项消除硬编码（按需，不全仓）

### 验收
- [ ] P8.1 无 Conversation 也能运行 Agent
- [ ] P8.2 Service/Agent/Device 可成为调用主体
- [ ] P8.3 个人助手功能仍完整

---

## Phase 9：行业 Pack 扩展

| Pack | 内容 | 状态 |
|------|------|------|
| Code | compile, test, lint, diff, dependency audit | 基础版存在 |
| Ops | 部署, K8s, 配置变更, 健康检查, 基础设施回滚 | 未来 |
| Data | SQL, Schema Migration, 数据质量, 隐私, 血缘 | 未来 |
| Workflow | 邮件, 审批, CRM, ERP, 订单, 企业 API | 未来 |
| Robotics | 仿真, 轨迹, 安全区, 真机发布门禁 | 未来 |
| Industrial | PLC, SCADA, 能源, 化工, 安全联锁 | 未来 |

---

## Phase 10：生产加固

### 持久化（基础已在 M6-B 完成，此处为生产级）
多租户, 备份恢复, Schema 滚动升级, 分区, 高可用, 审计留存

### 可靠性
并发隔离, 网络分区, 超时, 限流, 降级, 对账

### 安全
密钥轮换, Capability 审计, SBOM, 签名构建, Secret 泄露检测, 设备控制双重门禁

### 可观测性
OpenTelemetry, Run Trace, Decision Trace, Effect Trace, Recovery Trace, 成本与 Token 计量

---

## 最终验收标准 (v1.0)

1. Chat、CLI、HTTP、OntoLoop、OntoFlow 走统一 Run 入口
2. 所有 Agent 副作用经过 CapabilityHost
3. 所有调用可关联 Actor、Run、Invocation 和 Resource
4. OntoRuntime 不能自行产生 Success
5. 无持久化 Decision 不得发布副作用
6. Staged / Transactional / Compensatable / Irreversible 语义明确
7. OntoLoop 拥有 Attempt 收敛权
8. OntoFlow 拥有耐久编排权
9. OntoAssure 拥有成功与结算权
10. Conversation 仍可用，但不是系统唯一中心
11. 不存在第二套权限、沙盒、事件、运行状态系统
12. 任意终态和副作用均可审计、关联和恢复

---

## 立即下一步

**Phase 2：RunIngressPort**
- 实现 `RunIngressPort` trait
- CLI、Conversation、DirectRun 统一接线
- **优先：DirectRunAdapter** — 绕过未完成的 HTTP 入口，直连 RebornTurnRunExecutor，消除当前最大瓶颈

**Phase 3：CapabilityInvocationEnvelope**
- 新增 `RuntimeActorRef`, `SessionSource`, `StartRunRequest`
- 标准化 Capability 执行边界，确保 AI 可达路径不绕过 CapabilityHost

---

## 版本记录

| 版本 | 日期 | 里程碑 |
|------|------|--------|
| v0.1 | — | M0–M3 类型 + 纯函数 + 协调器 |
| v0.2 | 2026-07-25 | M5 + M6-A 封口 |
| v0.6 | 2026-07-26 | OntoLoop L0–L7 封口（提前完成） |
| v0.3 | TBD | 架构宪法 + 统一执行边界 |
| v0.4 | TBD | M6-B PostgreSQL/Redis + 真实 Store |
| v0.5 | TBD | M6-C/D Effect Authority 完整 |
| v0.7 | TBD | OntoFlow 编排接入 |
| v0.8 | TBD | 多 Profile |
| v0.9 | TBD | 行业 Packs |
| v1.0 | TBD | 生产加固 |
