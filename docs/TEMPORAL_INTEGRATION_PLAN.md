# OntoFlow 集成方案（完整版）

> 基于 `/home/admin1/temporal` 真实代码库分析 + Temporal Task 分发模型深度对齐  
> 修正版：CHASM 原生组件路径 + Matching 分发 + Authority Projection 验证

---

## 〇、背景：OntoFlow 到底分发什么

OntoFlow 这个分布式系统分发两样东西：**Tasks（任务）** 和 **State（状态）**。

### 六种 Task 类型

```
┌─────────────────┬────────────────────────────────────┬────────────────────────┐
│    分发什么      │              从哪到哪              │         做什么         │
├─────────────────┼────────────────────────────────────┼────────────────────────┤
│ WorkflowTask    │ History → Matching → Worker        │ "请跑一段工作流代码"   │
├─────────────────┼────────────────────────────────────┼────────────────────────┤
│ ActivityTask    │ History → Matching → Worker        │ "请执行这个 Activity"  │
├─────────────────┼────────────────────────────────────┼────────────────────────┤
│ TransferTask    │ History Queue Processor → Matching │ "请给 Worker 发个任务" │
├─────────────────┼────────────────────────────────────┼────────────────────────┤
│ TimerTask       │ History 内部                       │ "时间到了，该处理了"   │
├─────────────────┼────────────────────────────────────┼────────────────────────┤
│ ReplicationTask │ 跨集群                             │ "同步此工作流的事件"  │
├─────────────────┼────────────────────────────────────┼────────────────────────┤
│ VisibilityTask  │ History → ES/SQL                   │ "更新搜索索引"         │
└─────────────────┴────────────────────────────────────┴────────────────────────┘
```

### 完整分发链路示例

```
用户发起 StartOntoFlow("build-service", payload)
                         │
                         ▼
                    Frontend ──(gRPC)──▶ History Shard (RPC Handler)
                         │
                         │ 1. 写入 Event[0]: OntoFlowExecutionStarted
                         │ 2. 创建 TransferTask
                         ▼
               History Shard (Queue Processor)
                         │
                         │ 3. 处理 TransferTask → 通过 gRPC 发送到 Matching
                         ▼
               Matching Service (Root Partition)
                         │
                         │ 4. CHASM Engine 的 PureTask 推进 OntoFlowComponent
                         │ 5. 计算 Ready WorkItems → 创建 ExecuteOntoLoop ActivityTask
                         ▼
               History Shard (RPC Handler)
                         │
                         │ 6. 写入 ActivityTaskScheduled 事件
                         │ 7. 创建 Activity TransferTask
                         ▼
               History Shard (Queue Processor)
                         │
                         │ 8. → Matching Service
                         ▼
               Matching Service (某个分区)
                         │
                         │ 9. ActivityTask 入队
                         ▼
               Rust OntoLoop Worker (长轮询拿到)
                         │
                         │ 10. 执行 run_or_resume_to_terminal()
                         │ 11. 返回 LoopTerminalEnvelope
                         ▼
               History Shard...
               (Outcome Resolver 验证权威事实 → WorkItem Committed → 解锁下游)
```

---

## 一、四层权力边界

```text
OntoFlow (Go)
负责：多个 OntoLoop 的组织、依赖、并发、通信、汇聚
        ↓
OntoLoop (Rust)
负责：一个子任务内部的 Attempt、修复、回滚、收敛
        ↓
OntoRuntime / OntoRuntime (Rust)
负责：一个 Attempt 的 Agent 执行与 Capability 调用
        ↓
OntoAssure (Rust)
负责：验证、证据、终态与副作用结算
```

一句话：

```
OntoFlow  = Many-Loop Orchestration (Go)
OntoLoop  = Single-Task Convergence   (Rust)
OntoRuntime  = Single-Attempt Execution  (Rust)
OntoAssure = Independent Authority    (Rust)
```

Go/Rust 完全解耦：

```
Go OntoFlow 只通过协议调用 Rust OntoLoop
Go 不理解：Attempt、Checkpoint、Verifier、Evidence、CommitPermit
Rust OntoLoop 不理解：DAG、其他 Loop 状态、并发调度、Barrier、Join
```

### 关键：不是外部 Workflow Worker 模式

OntoFlow 运行在 Temporal Server 内部，作为 **CHASM 原生一等持久化组件**，不是外部 Go Workflow Worker：

```
❌ 外部 Go Worker 轮询 WorkflowTask 执行 Go Workflow 代码
✅ CHASM Engine 内部 PureTaskHandler 推进 OntoFlowComponent 确定性状态转换
```

底层仍复用 Temporal History、TransferTask、TimerTask 和 Matching 机制。

### 真实链路

```
StartOntoFlow
        ↓
CHASM Engine.StartExecution
        ↓
OntoFlowExecutionComponent（确定性状态转换）
        ↓
PureTask：计算 Ready WorkItems
        ↓
通过 CHASM Activity 组件调度 ExecuteOntoLoop
        ↓
TransferTask → Matching
        ↓
Rust OntoLoop Worker（长轮询 PollActivityTaskQueue）
        ↓
run_or_resume_to_terminal()
        ↓
LoopTerminalEnvelope（Worker 报告，非权威事实）
        ↓
Outcome Resolver 调用 OntoAssure Authority Projection API 验证
        ↓
WorkItem Committed（Go 接受权威终态）
        ↓
PureTask 解锁下游 WorkItems
```

---

## 二、哪些 OntoFlow 部分不换

核心原则：**保留 OntoFlow 的 CHASM Engine、TransferTask、TimerTask、Matching 和 Event Sourcing，把 ActivityTask 原来分发的"普通函数调用"替换成"完整 Rust OntoLoop 自治工作单元"。**

### 1. CHASM PureTask（原理类比：WorkflowTask）

CHASM OntoFlowComponent 的确定性状态转换推进 OntoFlow。执行的是 Go 侧编排逻辑：

```text
读取 Flow 状态
→ 查找依赖已满足的 WorkItem
→ 创建 OntoLoop 执行任务（ExecuteOntoLoop Activity）
→ 处理已返回的 OntoLoop 结果
→ 解锁下游节点
→ 处理 Barrier、Timer、Signal、Cancel
```

CHASM PureTask 不执行 Rust OntoLoop，也不处理 Attempt。

正确边界：

```
OntoFlow 负责：
- 哪些 WorkItem 现在可以运行
- 同时允许多少个 WorkItem 运行
- 哪些节点需要等待
- 哪些节点已经满足依赖
- 整个 Flow 是否完成
- Flow 全局预算控制

OntoLoop 负责：
- 一个 WorkItem 内部有多少 Attempt
- 如何 Continuation
- 如何 Rollback
- 是否 Regression
- 是否 Loop 预算耗尽
- 最终是否 Committed
```

### 2. TransferTask 不改

TransferTask 仍然负责 History Service → Matching Service 的任务传递。它不需要知道任务是"普通 Activity"还是"ExecuteOntoLoop"——对 TransferTask 而言，都是一份需要发送到 Matching 的外部任务。

因此不修改：

```
Transfer Queue Processor
History Shard 任务队列
TransferTask Ack 机制
```

### 3. TimerTask 不改

TimerTask 继续用于：

```
WorkItem 执行超时
OntoLoop Worker heartbeat 超时
延迟重试
等待人工审批
讨论轮次截止
Barrier 等待超时
全局 Flow Deadline
```

它只是时间触发器，不需要感知 OntoLoop 内部状态。

### 4. ReplicationTask 不改

跨集群同步的仍然是：

```
OntoFlow 执行历史
WorkItem 调度状态
任务创建和终态事件
Timer、Signal、Cancel 等事件
```

OntoLoop 内部的 Checkpoint、Evidence、Decision 仍保存在 Rust 侧对应权威存储中。OntoFlow 只同步引用：

```
loop_id
decision_id
terminal_envelope_ref
artifact_ref
```

不应把完整 Evidence、工作区快照全部塞进 OntoFlow 历史。

### 5. VisibilityTask 基本不改

只增加 OntoFlow 搜索属性：

```
OntoFlowType
FlowState
TotalWorkItems
RunningWorkItems
CommittedWorkItems
EscalatedWorkItems
RiskClass
TenantID
ProjectID
OntoLoopWorkerClass
```

VisibilityTask 机制本身不变。

---

## 三、真正替换的部分：ActivityTask

### 替换对比

原来：

```
ActivityTask
→ 执行一个普通函数
→ 返回函数结果
```

现在：

```
ExecuteOntoLoop Activity
→ 执行一个完整 Rust OntoLoop
→ 返回可信终态报告
```

### v0.1 策略：复用 ActivityTask 传输管道

第一阶段不增加新的底层任务类型，而是**使用 OntoFlow 现有 ActivityTask 传输管道，承载新的 ExecuteOntoLoop 语义**。

关键：**不要 CHASM SideEffectHandler 直接 gRPC 调用 Rust**。正确路径是：

```
❌ CHASM SideEffectTaskHandler → gRPC 直连 Rust Worker（绕过 Matching）

✅ OntoFlowComponent
   → 创建标准 CHASM Activity 组件
   → ActivityTaskScheduled 事件
   → TransferTask → Matching
   → Rust Worker PollActivityTaskQueue（标准 OntoFlow Worker 协议）
```

Activity 类型固定为 `execute_onto_loop`，Payload 是 `LoopInvocationRequest`，Result 是 `LoopTerminalEnvelope`。

Rust 侧通过标准 OntoFlow Worker 协议：

```
PollActivityTaskQueue
RecordActivityTaskHeartbeat
RespondActivityTaskCompleted
RespondActivityTaskFailed
```

### 关键：ActivityTaskCompleted ≠ Agent WorkItem Committed

```
ActivityTaskCompleted 只代表：
  Rust Worker 已经成功返回了一份 LoopTerminalEnvelope

Go 侧的 Outcome Resolver 必须调用 OntoAssure Authority Projection API 验证：
  OutcomeReported → AuthorityVerifying → Committed / Escalated / ProtocolFailed
```

### 完整链路

```
OntoFlowComponent
→ 发现 WorkItem-A 依赖已满足
→ 创建 ExecuteOntoLoop Activity
→ Activity Payload = LoopInvocationRequest
→ TransferTask
→ Matching
→ Rust OntoLoop Worker (PollActivityTaskQueue)
→ OntoLoopWorker.run_or_resume_to_terminal()
    ├── Attempt 1
    ├── OntoRuntime 执行
    ├── OntoAssure 裁决
    ├── Continue
    ├── Attempt 2
    ├── OntoAssure Success
    └── M6 Commit
→ RespondActivityTaskCompleted(LoopTerminalEnvelope)
→ Go Outcome Resolver
→ 调用 AuthorityProjectionPort.resolve_loop_outcome()
→ 验证 decision_id / checkpoint / receipt 绑定
→ WorkItem-A = Committed
→ 解锁 WorkItem-B、C
```

---

## 四、OntoLoop 输入协议：LoopInvocationRequest

OntoLoop 输入就是 ActivityTask 的 Payload。

```rust
/// Go OntoFlow → Rust OntoLoop Worker 的请求
pub struct LoopInvocationRequest {
    pub schema_version: u32,

    // ── 身份 ──
    pub flow_id: String,
    pub work_item_id: String,
    pub loop_id: String,

    // ── 任务定义（Go 只给引用，不给内容） ──
    pub task_spec_ref: String,
    pub contract_ref: String,
    pub policy_ref: String,
    pub input_artifact_refs: Vec<String>,

    // ── 资源与风险 ──
    pub resource_class: String,
    pub risk_class: String,
    pub trust_requirement: String,

    // ── Flow 分配的预算 ──
    pub budget_grant_ref: String,
    pub budget_grant_hash: String,

    // ── 幂等与版本 ──
    pub execution_generation: u64,
    pub idempotency_key: String,

    // ── 可选约束 ──
    pub deadline: Option<String>,

    // ── 输入绑定 Hash ──
    pub request_binding_hash: String,
}
```

### Go 传什么

Go 只告诉 OntoLoop：

```
你是谁
你属于哪个 Flow
你要完成哪个 WorkItem
任务定义在哪里（引用）
输入产物在哪里（引用）
验收契约在哪里（引用）
安全策略在哪里（引用）
分配了多少预算（BudgetGrant）
允许使用什么资源
风险等级是什么
本次执行代数是什么
幂等身份是什么
```

### Go 绝对不能传什么

```
❌ AttemptNum
❌ RepairPrompt
❌ ContinuationDirective
❌ Checkpoint 选择
❌ ProgressSnapshot
❌ Regression 策略
❌ 当前 Verifier 失败列表
```

这些全部属于 Rust OntoLoop 内部。

**正确粒度：OntoFlow 分发一个完整 WorkItem，而不是分发一个 Attempt。**

---

## 五、OntoLoop 输出协议：LoopTerminalEnvelope

OntoLoop 输出作为 ActivityTask 完成结果返回。但**不返回完整产物和 Evidence**，而是返回不可变引用。

```rust
/// Rust OntoLoop Worker → Go OntoFlow 的终态报告（非权威，需验证）
pub struct LoopTerminalEnvelope {
    pub schema_version: u32,

    // ── 身份 ──
    pub flow_id: String,
    pub work_item_id: String,
    pub loop_id: String,
    pub execution_generation: u64,

    // ── 输入绑定 ──
    /// Hash of the original LoopInvocationRequest. Rust must store this
    /// at receipt and return it unchanged. Go verifies it matches.
    pub request_binding_hash: String,

    // ── Worker 报告的终态（Go 仍需验证） ──
    /// Named `reported_*` to emphasize: this is the Worker's claim,
    /// NOT yet accepted by Go OntoFlow.
    pub reported_terminal_state: LoopTerminalState,

    // ── 权威裁决引用（不返回完整 Evidence） ──
    pub decision_id: Option<String>,
    pub decision_hash: Option<String>,
    pub evidence_bundle_ref: Option<String>,
    pub settlement_receipt_ref: Option<String>,

    // ── 产物引用 ──
    pub output_artifact_refs: Vec<String>,
    pub output_checkpoint_hash: Option<String>,

    // ── 输出绑定 ──
    /// Hash binding request_binding_hash + all outcome fields.
    pub outcome_binding_hash: String,

    // ── 摘要信息（非权威，供展示） ──
    pub total_attempts: u32,
    pub terminal_reason: String,
}

/// Worker 报告的终态（Go 验证后才接受）
pub enum LoopTerminalState {
    Committed,               // OntoAssure 验证通过 + Settlement 已提交
    Escalated,               // 需要人工介入
    EnvironmentBlocked,      // 外部环境不可用（非 Agent 错误）
    LoopBudgetExhausted,     // Rust OntoLoop 内部预算耗尽（Attempt/NoProgress/Regression）
    Cancelled,               // 被外部信号取消
    ProtocolFailed,          // 协议本身出错（绑定不一致、版本不匹配等）
}
```

### 关键命名区分

```
reported_terminal_state   = Worker 报告，Go 尚未接受
WorkItemPhase.OutcomeReported → AuthorityVerifying → Committed  = Go 接受
```

### Go 收到后的验证步骤（通过 AuthorityProjectionPort）

`LoopTerminalEnvelope` 是 Worker 报告，不是 OntoFlow 直接接受的最终事实。Go 必须调用 Rust 只读权威接口验证：

```
Go 调用 AuthorityProjectionPort::resolve_loop_outcome()
Rust 返回 VerifiedLoopOutcome
Go 验证：
  1. decision_id 真实存在
  2. decision 绑定当前 loop_id
  3. decision 绑定当前 output_checkpoint_hash
  4. request_binding_hash 与发送时一致
  5. outcome_binding_hash 正确绑定所有字段
  6. settlement 状态为 Committed 或 Confirmed
  7. receipt 绑定当前 decision
  8. execution_generation 匹配
```

验证全部通过后，Go 才将 WorkItem 标记为 `Committed`。

### Go 不应直接读取 OntoAssure 数据库

```
❌ OntoFlow Go Server 直接查询 onto_decisions、onto_receipts 表
   → Go 依赖 Rust 内部数据库 Schema，破坏解耦

✅ Go 通过 AuthorityProjectionPort (gRPC) 调用 Rust 只读权威接口
   → Rust 内部 Schema 变化不影响 Go
```

---

## 六、AuthorityProjectionPort（Rust 只读权威接口）

```rust
/// Go OntoFlow → Rust OntoAssure 的只读权威查询接口。
/// Go 不直接读取 OntoAssure 数据库表。
pub trait AuthorityProjectionPort: Send + Sync {
    /// Resolve the authoritative outcome for a loop.
    async fn resolve_loop_outcome(
        &self,
        request: ResolveLoopOutcomeRequest,
    ) -> Result<VerifiedLoopOutcome, AuthorityError>;
}

pub struct ResolveLoopOutcomeRequest {
    pub flow_id: String,
    pub work_item_id: String,
    pub loop_id: String,
    pub execution_generation: u64,
    pub terminal_envelope_hash: String,
}

pub struct VerifiedLoopOutcome {
    pub loop_id: String,
    pub outcome: VerifiedOutcome,

    pub decision_id: String,
    pub decision_hash: String,
    pub output_checkpoint_hash: String,
    pub settlement_receipt_ref: Option<String>,

    pub authority_binding_hash: String,
}

pub enum VerifiedOutcome {
    Committed,
    NotCommitted { reason: String },
    DecisionNotFound,
    BindingMismatch { detail: String },
}
```

---

## 七、Heartbeat 协议

完整 OntoLoop 可能运行很久（数分钟到数小时），因此 Activity Worker 需要 Heartbeat。

```rust
/// Rust OntoLoop Worker → OntoFlow 的心跳
pub struct OntoLoopHeartbeat {
    pub flow_id: String,
    pub work_item_id: String,
    pub loop_id: String,

    pub current_attempt_id: Option<String>,
    pub current_run_id: Option<String>,

    pub lifecycle_state: String,
    pub progress_snapshot_hash: Option<String>,

    pub last_decision_id: Option<String>,
    pub current_checkpoint_id: Option<String>,
}
```

Heartbeat 用途：

```
活性判断        — Worker 是否还活着
取消传播        — 外部 Cancel 信号通过 Heartbeat 响应传递
超时判断        — OntoFlow 根据 Heartbeat 判断是否超时
运行进度展示    — OntoFlow UI 展示当前 Attempt 数、进度
Worker 崩溃恢复 — Heartbeat 缺失触发重新派发
```

**Heartbeat 不参与成功判定。** 成功判定永远由 OntoAssure 独立完成。

OntoFlow 主要消费心跳的活性、超时和进度投影，不需要在 CHASM OntoFlow 库内单独实现完整的心跳管理——标准 Activity Heartbeat 机制已覆盖大部分。

---

## 八、重试与幂等

OntoFlow 可能因为 Worker 崩溃重新派发同一个 ActivityTask。

### 重试时必须保持不变的字段

```
flow_id              — 不变
work_item_id         — 不变
loop_id              — 不变
execution_generation — 不变
idempotency_key      — 不变
task_spec_ref        — 不变
contract_ref         — 不变
budget_grant_ref     — 不变
budget_grant_hash    — 不变
request_binding_hash — 不变
```

### Rust Worker 收到后的处理

```
查询 loop_id
├── 不存在
│   → 创建新 OntoLoop
├── Running
│   → 恢复原 OntoLoop（从最近 Checkpoint）
├── 已终态
│   → 返回原 TerminalEnvelope（幂等）
└── 请求绑定不一致（request_binding_hash 不匹配 / generation 不匹配）
    → ProtocolFailed
```

**不能因为 Activity 重试就创建第二个 OntoLoop。**

### Activity 身份层次（不可混淆）

```
flow_id + work_item_id
  = 逻辑 WorkItem 身份

flow_id + work_item_id + execution_generation
  = 一次恢复代执行身份

loop_id
  = Rust OntoLoop 执行实例身份

activity_id
  = OntoFlow Activity 调度身份

task_token
  = 当前 Worker 领取凭证，不持久化为业务身份
```

---

## 九、预算模型：Loop 预算与 Flow 预算分离

### 关键认知

```
Loop 预算 → Rust OntoLoop 内部（Attempt 数、NoProgress 限制、Regression 限制）
Flow 预算 → Go OntoFlow（全局 Token、全局成本、全局时间）
```

一个 OntoLoop 不能知道其他 99 个 OntoLoop 已经花了多少全局预算。

### Rust 只报告

```rust
pub enum LoopTerminalState {
    Committed,
    Escalated,
    EnvironmentBlocked,
    LoopBudgetExhausted,  // ← 只表示 Rust 内部预算耗尽
    Cancelled,
    ProtocolFailed,
}
```

### Go 拥有

```go
type FlowBudget struct {
    TotalLimit    BudgetAmount
    Consumed      BudgetAmount
    Remaining     BudgetAmount
}
```

分派前：

```
OntoFlow
→ 为 WorkItem 分配 BudgetGrant
→ Rust OntoLoop 只消费自己的 Grant
```

请求中明确为：

```rust
pub budget_grant_ref: String,
pub budget_grant_hash: String,
```

结果处理：

```
LoopBudgetExhausted
→ Go 决定是否追加预算、重新分派或 Escalate

FlowBudgetExhausted
→ Go 停止创建新的 WorkItem
```

---

## 十、OntoFlow Workflow State 模型

### OntoFlow 状态

```go
type OntoFlowState struct {
    FlowID string
    Phase  FlowPhase

    FlowSpecRef  string   // 静态 DAG 定义存不可变 Artifact
    GraphHash    string

    ActiveWorkItems    map[string]WorkItemRuntimeState
    CompletedSummary   FlowCompletionSummary

    MaxConcurrency uint32
    RunningCount   uint32
}

// v0.1 边界
const MaxInlineWorkItems    = 256
const MaxInlineDependencies = 1024
```

### 每个 WorkItem 运行时状态

```go
type WorkItemRuntimeState struct {
    WorkItemID string
    LoopID     string

    Phase WorkItemPhase

    TaskSpecRef string
    ContractRef string
    PolicyRef   string

    InputArtifactRefs  []string
    OutputArtifactRefs []string

    ExecutionGeneration uint64

    // 调度身份（不是 task_token）
    CurrentActivityID            string
    CurrentActivityScheduleEventID int64

    TerminalEnvelopeRef string
    DecisionID           string

    LastError string
}
```

### 状态机

```
Blocked → Ready → Dispatched → Running
                                  ↓
                         OutcomeReported
                                  ↓
                        AuthorityVerifying
                              ↓          ↓
                        Committed     Escalated

（BudgetExhausted、Cancelled 可从多个阶段进入）
```

Go 侧完全不知道 `Running` 内部有多少个 Attempt。

### OntoFlow 只保存

```
调度状态
依赖状态
任务引用
权威终态引用
```

### OntoFlow 不保存

```
Attempt 状态
Checkpoint 内容
完整 Evidence
Agent 上下文
完整模型对话
```

### 大规模状态预留

v0.1 静态 DAG + 有限批量足够。但未来 1000+ WorkItem 时，完整 Map 参与每次 History 序列化会膨胀。预留：

```text
超过 MaxInlineWorkItems → 使用 PagedWorkItemState
超过 MaxInlineDependencies → 使用 GraphArtifactRef + 外部不可变定义
大型批量 → 考虑 Child Flow 分解
```

---

## 十一、三种核心场景

### 1. 大任务拆成多个子任务 (DAG)

```
需求分析 OntoLoop → 架构设计 OntoLoop
    → [后端 OntoLoop | 前端 OntoLoop | 测试 OntoLoop]
    → Integration OntoLoop → Release OntoLoop
```

OntoFlow 实际分发：

```
ActivityTask-A
  Payload = LoopInvocationRequest(A)
  → Rust OntoLoop-A
  → OutcomeReported → AuthorityVerifying → Committed

A Committed 后：

ActivityTask-B (依赖 A)
ActivityTask-C (依赖 A，与 B 并发)
→ B、C Committed 后 → ActivityTask-D
```

每个 ActivityTask = 一个完整 OntoLoop 调用。

### 2. 100 个同类任务批量调度

```
100 个 WorkItem
→ 生成 100 个 ExecuteOntoLoop ActivityTask
→ OntoFlow 控制 MaxConcurrency = 10
→ 先发 10 个
→ 任意一个 Committed 或终止 → 再补发 1 个
```

Matching 继续负责具体由哪个 Rust Worker 领取。

### 3. 去中心化讨论

```
Round 1:
  ActivityTask(Proposer-A/B/C)
  Barrier 等待全部终态

Round 2:
  ActivityTask(Critic-A/B)
  Barrier + Quorum

Round 3:
  ActivityTask(Synthesizer)
```

不同 OntoLoop 通过 `ArtifactRef` 交换成果，不共享内部 Checkpoint。

---

## 十二、OntoFlow 组件模型

```
OntoFlowExecutionComponent          ← 根组件，整个 Flow 的生命周期
├── FlowSpec                        ← DAG / Sequence / Batch 定义
├── GraphState                      ← 依赖解锁状态
├── ConcurrencyPolicy               ← 并发控制
├── FlowBudget                      ← Flow 全局预算
├── WorkItems[]                     ← 多个 WorkItem
│   ├── WorkItem-A → LoopInvocationRequest(A) → Rust OntoLoop-A
│   └── WorkItem-B → LoopInvocationRequest(B) → Rust OntoLoop-B
├── Barriers[]                      ← Fan-in / Round 同步
├── Signals[]                       ← 外部信号
└── AggregationState                ← 汇总状态
```

---

## 十三、是否需要新增原生 AgentWorkItemTask

分两个阶段。

### v0.1：不改底层任务协议

```
✅ 复用 ActivityTask
✅ Activity 类型固定为 "execute_onto_loop"
✅ Payload = LoopInvocationRequest
✅ Result = LoopTerminalEnvelope
✅ 通过标准 OntoFlow Worker 协议（Poll/Heartbeat/Respond）
```

几乎不用改：

```
History
Matching
TransferTask
Worker Poll 协议
Persistence
```

### v1.0：可选原生 AgentWorkItemTask

未来为了让 Agent 任务成为服务器一等类型，可以增加专用任务类型：

```
AgentWorkItemTaskScheduled
AgentWorkItemTaskStarted
AgentWorkItemOutcomeReported
AgentWorkItemOutcomeAccepted
```

但这会涉及 Proto / Frontend API / History 事件 / State Machine / Matching task type / SDK / Worker 协议。**第一版完全没有必要。** 本体改变不依赖专用 RPC，而依赖：

```
1. OntoFlow 所有外部 WorkItem 都必须由 OntoLoop 执行
2. 所有成功结果都必须由 OntoAssure 裁决
3. Go 通过 AuthorityProjectionPort 验证，不直接读数据库
```

---

## 十四、最终映射表

```
┌──────────────────────┬──────────────┬───────────────────────────────────────────┐
│ OntoFlow 部分        │ 是否替换     │ Onto 化后的职责                            │
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ CHASM PureTask       │ 不替换       │ 推进 OntoFlowComponent 确定性状态转换      │
│ (WorkflowTask 类比)  │ 修改上层逻辑 │ 计算 Ready WorkItems, 解锁下游             │
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ ActivityTask         │ **替换语义** │ 承载一个完整 OntoLoop 的输入和终态报告     │
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ TransferTask         │ 不替换       │ 把 ExecuteOntoLoop Activity 送到 Matching   │
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ TimerTask            │ 不替换       │ 超时、等待、重试、轮次截止、人工等待       │
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ ReplicationTask      │ 不替换       │ 复制 OntoFlow 调度历史                     │
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ VisibilityTask       │ 不替换       │ 索引 Flow、WorkItem、Loop 与风险状态       │
│                      │ 扩展字段     │                                            │
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ Workflow/Flow State  │ 替换领域模型 │ 保存多个 WorkItem、依赖、Barrier 和终态引用│
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ Activity Worker      │ 替换执行器   │ 从普通函数 Worker 变成 Rust OntoLoop Worker│
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ Activity Input       │ **替换**     │ LoopInvocationRequest                     │
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ Activity Output      │ **替换**     │ LoopTerminalEnvelope（Worker 报告）        │
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ Activity 成功定义    │ **替换**     │ ActivityTaskCompleted = OutcomeReported，  │
│                      │              │ AuthorityProjection 验证后才 Committed     │
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ SideEffectHandler    │ **删除此路径**│ 不走直接 gRPC，走 Matching + Poll          │
├──────────────────────┼──────────────┼───────────────────────────────────────────┤
│ Go↔Rust 验证接口     │ **新增**     │ AuthorityProjectionPort（Rust 只读权威）   │
└──────────────────────┴──────────────┴───────────────────────────────────────────┘
```

---

## 十五、关键不变量

```
 1. 一个 WorkItem = 一个稳定 loop_id
 2. OntoFlow 不能进入 OntoLoop 内部管理 Attempt
 3. 一个 OntoLoop 失败 ≠ Flow 立即失败
 4. 只有 AuthorityProjection 验证通过的 WorkItem 才能满足下游依赖
 5. ActivityTaskCompleted ≠ WorkItem Committed（OutcomeReported ≠ Committed）
 6. WorkItem 间只通过不可变 ArtifactRef 传递
 7. 多 OntoLoop 之间不共享可变 Checkpoint
 8. Flow 恢复不创建重复 loop_id（幂等）
 9. OntoFlow 只决定调度和组合，不夺取 OntoAssure 终态权威
10. Activity 重试保持所有输入字段不变（generation / idempotency_key / request_binding_hash）
11. Heartbeat 不参与成功判定
12. OntoFlow/Go 不自产任务 Success（最终权威永远在 OntoAssure）
13. Evidence 不进入 Temporal History
14. Go 不直接读取 OntoAssure 数据库 Schema（通过 AuthorityProjectionPort）
15. LoopBudgetExhausted ≠ FlowBudgetExhausted（Loop 预算由 Rust 控制，Flow 预算由 Go 控制）
16. Activity 分发必须走 Matching，不通过 SideEffectHandler 直连 Rust
```

---

## 十六、Rust Worker 统一接口

```rust
/// Go 只调这一个接口（通过 OntoFlow Activity Worker 协议）
#[async_trait]
pub trait OntoLoopWorker: Send + Sync {
    async fn run_or_resume_to_terminal(
        &self,
        request: LoopInvocationRequest,
    ) -> Result<LoopTerminalEnvelope, WorkerError>;
}
```

内部流程：

```
run_or_resume_to_terminal()
    ├── 持久化 request_binding_hash
    ├── 检查 loop_id 是否已存在
    │   ├── 不存在 → 创建新 OntoLoop
    │   ├── Running → 恢复（从最近 Checkpoint）
    │   └── 已终态 → 返回原 TerminalEnvelope（幂等）
    ├── OntoLoop.run_to_completion()
    │   ├── Attempt 1 → OntoRuntime 执行 → OntoAssure 裁决
    │   ├── Attempt 2 → Continuation → 执行 → 裁决
    │   └── Attempt N → Terminal
    ├── 计算 outcome_binding_hash（绑定 request_binding_hash + 所有 outcome 字段）
    └── 返回 LoopTerminalEnvelope
```

---

## 十七、文件结构

### Go 侧 (temporal/)

```
chasm/lib/ontoflow/
├── library.go              — OntoFlowLibrary 注册（fx/provider 注入）
├── flow_component.go       — OntoFlowExecutionComponent（根组件）
├── work_item.go            — WorkItem 运行时状态
├── graph.go                — DAG 依赖解锁
├── scheduler.go            — PureTask：计算 Ready WorkItems
├── activity_scheduler.go   — 通过 chasm/lib/activity 调度 ExecuteOntoLoop
├── outcome_resolver.go     — 调用 OntoAssure Authority Projection API
├── barrier.go              — Fan-in / Round 同步
├── transitions.go          — 状态转换定义
├── types.go                — FlowSpec, WorkItemSpec, DependencySpec
├── cancellation.go         — Cancel / Signal / 超时处理
├── visibility.go           — OntoFlow 搜索属性注册
└── recovery.go             — 崩溃恢复与幂等重放
```

**注意：** 没有 `dispatch_handler.go`（不走 SideEffectHandler 直连），没有独立的 `heartbeat.go`（标准 Activity Heartbeat 已覆盖）。

### Rust 侧 (OntoOS/)

```
crates/onto-temporal-adapter/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── protocol.rs          — LoopInvocationRequest / LoopTerminalEnvelope / Heartbeat
│   ├── worker.rs            — OntoLoopWorker trait + WorkerError
│   ├── loop_runner.rs       — RuntimeLoopRunner: run_or_resume_to_terminal()
│   ├── heartbeat.rs         — HeartbeatSender trait + 心跳发送
│   ├── idempotency.rs       — IdempotencyKey 验证 + InMemoryIdempotencyStore
│   └── authority_projection.rs  — AuthorityProjectionPort (Go 只读权威接口)
└── tests/
    ├── protocol_tests.rs
    ├── idempotency_tests.rs
    └── integration_tests.rs
```

---

## 十八、实施阶段（修正顺序）

故障恢复必须在 DAG 和批量之前完成——单 WorkItem 的崩溃恢复是 Tier 1。

```
F0  桥接验证
    CHASM ActivityTask ↔ Matching ↔ Rust Worker Poll
    证明 ExecuteOntoLoop 走完整标准 OntoFlow 分发链路

F1  单 WorkItem 完整 OntoLoop
    Attempt1 → Attempt2 → Committed
    Heartbeat + 重试幂等 + request_binding_hash

F2  Authority Resolution
    OutcomeReported → AuthorityProjectionPort → Committed
    Worker 报告成功但 Decision 缺失 → 不得 Committed
    Decision 绑定错误 loop_id → 不得 Committed

F3  故障恢复
    Worker 崩溃不重复已完成 Attempt
    Activity 重试复用同一 loop_id
    Commit 后响应丢失不重复副作用
    双 Worker 竞争只有一个持有 LoopExecutionLease
    重复 Activity Completion 只应用一次

F4  静态 DAG (A→B→C, A→B∥C→D)
    依赖解锁、并发、Fan-in
    A Committed 后解锁 B
    A 未 Committed 不解锁 B

F5  批量调度 (100 WorkItem, max=10)
    并发控制、失败隔离、逐个补发

F6  Cancel / Signal / Resume

F7  动态任务拆分 (Planning OntoLoop → Graph)

F8  去中心化讨论 (Proposer/Critic/Synthesis)

F9  原生 AgentWorkItemTask（可选，v1.0）
```

封口边界：

```
OntoFlow v0.1 = F0–F6   ✅ Rust 侧完成 (60 tests)
OntoFlow v0.2 = F7–F8   ✅ Rust 侧完成 (ongo-temporal-adapter)
OntoFlow v1.0 = F9      ✅ Rust 类型定义完成

Go 侧 chasm/lib/ontoflow/ (~800行) ⏳ 下一阶段
```

### v0.1 封口测试（约 20 项）

**桥接 (F0) — 5 项**

| # | 测试 |
|---|------|
| F0-1 | OntoFlowLibrary 注册成功 |
| F0-2 | WorkItem 创建真实 ActivityTask |
| F0-3 | ActivityTask 进入 Matching |
| F0-4 | Rust Worker 可以 Poll 到 |
| F0-5 | Completion 返回正确 Component |

**单 Loop 与权威 (F1–F2) — 8 项**

| # | 测试 |
|---|------|
| F1-1 | 单 WorkItem 两 Attempt 后 Committed |
| F1-2 | Heartbeat 在每次 Attempt 间发送 |
| F1-3 | Resume 从 Running 状态恢复 |
| F2-1 | Worker 返回成功但 Decision 缺失 → 不得 Committed |
| F2-2 | Decision 绑定错误 loop_id → 不得 Committed |
| F2-3 | AuthorityProjectionPort 验证通过 → Committed |
| F2-4 | Receipt 与 Decision 不匹配 → 不得 Committed |
| F2-5 | request_binding_hash 不匹配 → ProtocolFailed |

**恢复 (F3) — 5 项**

| # | 测试 |
|---|------|
| F3-1 | Activity 重试复用同一 loop_id |
| F3-2 | Worker 崩溃不重复已完成 Attempt |
| F3-3 | Commit 后响应丢失不重复副作用 |
| F3-4 | 双 Worker 竞争只有一个持有租约 |
| F3-5 | 重复 Activity Completion 只应用一次 |

**DAG + 控制 (F4–F6) — 5 项**

| # | 测试 |
|---|------|
| F4-1 | A Committed 后解锁 B |
| F4-2 | A 未 Committed 不解锁 B |
| F4-3 | B、C 均 Committed 后解锁 D |
| F5-1 | MaxConcurrency 严格限制 |
| F6-1 | Cancel 停止所有未完成 WorkItem |

---

## 十九、与 CHASM 核心的关系

```
CHASM 通用实现（完全不修改）:
  component.go / task.go / engine.go / context.go / registry.go
  Matching Service / History Service / Frontend
  TransferTask / TimerTask / ReplicationTask

服务器启动/注册（少量修改，约 5–30 行）:
  增加 ontoflow.NewLibrary()
  增加必要的 fx/provider 注入

OntoFlow 新增（主要工作量）:
  chasm/lib/ontoflow/  (~800 行 Go)

复用:
  Temporal gRPC 协议栈
  CHASM 状态锁与持久化
  OntoFlow 任务队列与 Worker 分配
  OntoFlow Event Sourcing
  OntoFlow 标准 Activity Heartbeat
```

---

## 二十、总结

> **保留 OntoFlow 的 CHASM Engine、TransferTask、TimerTask、Matching 和 Event Sourcing，把 ActivityTask 原来分发的"普通函数调用"替换成"完整 Rust OntoLoop 自治工作单元"；同时把 Flow 状态改造成多个 OntoLoop 的 DAG、批量和协作编排状态。**

最终正确的根链路：

```
CHASM OntoFlowComponent
        ↓
确定性计算 Ready WorkItems
        ↓
通过 CHASM Activity 库创建 ExecuteOntoLoop ActivityTask
        ↓
TransferTask → Matching
        ↓
Rust OntoLoop Worker（PollActivityTaskQueue）
        ↓
run_or_resume_to_terminal()
        ↓
LoopTerminalEnvelope（Worker 报告，非权威）
        ↓
Outcome Resolver → AuthorityProjectionPort（Rust 只读权威接口）
        ↓
WorkItem Committed（Go 接受）
        ↓
解锁下游 WorkItems
```

**最大的架构修正：不要 "CHASM SideEffectHandler 直接 gRPC 调用 Rust"，而要 "CHASM 调度真正的 OntoFlow ActivityTask，由 Rust Worker 通过 Matching 领取"。** 这是施工前必须先验证的 F0。
