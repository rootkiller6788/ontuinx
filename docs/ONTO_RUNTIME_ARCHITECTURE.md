# OntoRuntime Architecture

## 系统定位

OntoRuntime = OntoRuntime Fork + OntoAssure 可信裁决 + OntoLoop Attempt 循环 + OntoFlow 耐久编排

不是另起一套 Runtime，而是在 OntoRuntime 主体上通过 Port / Hook / Adapter / Envelope 原位魔改。

## 四层权力边界

```
┌──────────────────────────────────────────┐
│ OntoFlow 魔改版                           │
│ 决定：先做什么、后做什么                     │
│ 拥有：Workflow, WorkItem DAG, Timer,      │
│       Signal, Saga, 长期等待, Worker 调度   │
│ 不拥有：单次任务成功判定, 单次副作用结算       │
├──────────────────────────────────────────┤
│ OntoLoop                                 │
│ 决定：当前任务下一轮怎么继续                 │
│ 拥有：Attempt, Continuation, Repair,      │
│       Progress, Convergence, Loop Budget  │
│ 不拥有：Agent Loop 内部控制, Evidence 归约   │
├──────────────────────────────────────────┤
│ OntoRuntime (OntoRuntime Fork)              │
│ 决定：这一轮如何安全执行                     │
│ 拥有：Run, Agent Loop, LLM, CapabilityHost,│
│       授权, 信任, 审批, Lease, Secret,      │
│       Filesystem, Network, Resource,       │
│       Runtime Lane, EventStore, Memory     │
│ 不拥有：成功判定, 证据归约, 副作用结算        │
├──────────────────────────────────────────┤
│ OntoAssure                               │
│ 决定：执行是否可信、效果能否结算              │
│ 拥有：ExecutionContract, Criterion,       │
│       Verifier, Evidence, Verdict,         │
│       SessionDecision, SettlementDecision, │
│       Effect Transaction, Replay           │
│ 不拥有：执行, 授权, 调度                    │
└──────────────────────────────────────────┘
```

## 核心依赖方向

```
OntoFlow Adapter
    ↓
OntoLoop
    ↓
onto-ironclaw-adapter → OntoRuntime composition
    ↓                        ↓
onto-assurance-runtime   OntoRuntime 内部 crates
    ↓
onto-assurance-core
    ↓
onto-assurance-types
```

禁止的依赖方向：
- `onto-assurance-core` → OntoRuntime / PostgreSQL / Redis / OntoFlow
- `onto-assurance-runtime` → OntoRuntime 具体类型
- OntoRuntime Agent Loop → `onto-assurance-core`

## 不变量

### M5 不变量
1. Agent 不能自行宣布成功
2. FinishRequested ≠ Success
3. Agent FinishRequested + Onto Escalated ≠ OntoRuntime Completed
4. 空证据 + FinishRequested → Escalated
5. Persist failure → 不得 Committed
6. 重复 finalize → 幂等（相同 DecisionId，单次 persist，单次事件）

### M6-A 不变量
1. Agent 不能直接修改正式 Workspace
2. 无持久化 Commit → Workspace 不变
3. CAS 失败 → publish_calls = 0
4. Receipt 失败 → 不 Committed
5. 重复 reconcile → 无新增副作用
6. 状态不可判定 → Frozen + Escalate

## 施工纪律

### 永久禁止
- 新建平行 Agent Loop / 权限系统 / 沙盒 / EventStore / Run 持久化
- 全仓 User→Actor、Run→ExecutionSession、ToolCall→CapabilityInvocation 替换
- 一次性删除 Conversation/Mission/Routine
- 一次性统一全部 Runtime Lane
- 把 OntoAssure 合并进 OntoRuntime Core

### 允许的魔改
- 现有类型外层包装
- 现有服务前增加 Port
- 现有执行路径增加 Hook
- Composition 注入外部权威
- 对高风险 Lane 增加 Settlement Adapter
- 逐个消除硬编码个人助手假设

## 源码形态

```
OntoOS/                              ← OntoAssure 主体
├── crates/
│   ├── onto-assurance-types         ← 纯类型，零外部依赖
│   ├── onto-assurance-core          ← 纯函数内核
│   ├── onto-assurance-runtime       ← Port traits + Coordinator
│   ├── onto-ironclaw-adapter        ← OntoRuntime 适配器
│   ├── onto-pack-sdk                ← 行业验证器 trait
│   └── onto-code-pack               ← 代码验证器

ironclaw-main/                       ← OntoRuntime 执行层主体
├── crates/
│   ├── onto-assurance-* → symlink → OntoOS
│   ├── ironclaw_runner/              ← RebornTurnRunExecutor
│   ├── ironclaw_reborn_composition/  ← build_reborn_runtime
│   ├── ironclaw_turns/              ← RunFinalizationPort
│   └── ...
```
