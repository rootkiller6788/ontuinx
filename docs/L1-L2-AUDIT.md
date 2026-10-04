# L1 OntoAssure + L2 OntoRuntime —— P0–P6 架构审计

> 审计日期: 2026-07-28
> 范围: `crates/onto-assurance-runtime`, `crates/onto-assurance-core`, `crates/onto-assurance-types`, `crates/onto-graph-verifiers`, `crates/onto-ironclaw-adapter`
> 方法: 逐 trait / 逐 impl / 逐调用链 追踪，按 P0→P6 六级优先级审查

---

## 总览

```
生产路径 (实际在跑的):
  Go OntoFlow
    → RuntimeLoopRunner::execute()
      → LoopAdapter::execute_attempt()
        → RuntimeRunPort::start_run() + await_terminal()
          ↑ 只有 MockLoopRuntime 实现, 生产实现缺位
        → LoopAdapter::evaluate()  ← 简单 match TaskOutcome, 不看 OntoAssure 结果
        → 驱动 LoopBudget + compare_progress(&[], &[], ...)

旁路1 (DirectRunAdapter): CI 测试路径, 同样绕过所有验证器
旁路2 (TransactionCoordinator): 完整8步流水线, 但零生产调用

孤岛:
  VerificationCoordinator + GraphIntegrity + GraphRisk + 三本总账 + EvidenceBuilder
  → 全部实现但没有任何生产路径触发

结论: L1 和 L2 各自内部完整, 但之间存在三重断裂:
  1. Trait 签名互不兼容
  2. 协调器不在执行路径中
  3. Evidence 写了但永远读不到
```

---

## P0 — 架构拓扑完整性

### P0.1 核心断裂: `RuntimeRunPort` 没有生产实现

```
trait RuntimeRunPort {
    start_run() → RunId
    await_terminal(RunId) → RunFinalizationOutcome  ← Agent 跑完后的裁决
    cancel_run(RunId)
}

实现:
  MockLoopRuntime (mocks.rs:377)     ← CI 测试用
  ???                                 ← 生产环境: 无

生产路径 RuntimeLoopRunner 依赖 RuntimeRunPort,
但 IronClaw fork 的生产实现不在本仓库中。
await_terminal() 在生产中是否调用了 OntoAssure 验证? 不可知。
```

| 严重度 | 说明 |
|--------|------|
| **阻断** | 无法确认生产环境中 Agent 的 run 是否经过 OntoAssure 验证 |
| **影响** | 整个系统的安全假设悬空 |

### P0.2 双重断裂: 两个协调器都不在生产路径中

```
TransactionCoordinator (coordinator.rs)    VerificationCoordinator (verification/coordinator.rs)
══════════════════════════════             ═══════════════════════════════
流水线:                                    流水线:
  Classify → Authorize → Prepare            Plan → for each unit:
  → Execute → Verify → Reduce                Scheduler::schedule()
  → Settle                                   → GraphIntegrity / GraphRisk
                                             → 三本总账
                                             → EvidenceBuilder

依赖:                                      依赖:
  AuthorizationPort + ApprovalPort           PlanGenerator + UnitScheduler
  + RuntimePort + VerifierPort               + SessionManager + BudgetTracker
  + EvidenceStorePort + CheckpointPort
  + EventSinkPort + ClockPort

EvidenceStore.store() ✅                   EvidenceStore.store() ❌ (不依赖 EvidenceStorePort)
reduction + session_decision ✅            completeness reduction ✅
settlement ✅                              三本总账 ✅

调用位置:                                   调用位置:
  coordinator_integration.rs (测试)          不存在任何生产调用
  ⚠️ 零生产调用                              ⚠️ 零生产调用
```

**两个协调器各自完成验证流水线的一半，但都不在 `RuntimeLoopRunner::execute()` 的任何分支中。**

| 严重度 | 说明 |
|--------|------|
| **阻断** | GraphIntegrity / GraphRisk 写好了但没有任何代码路径触发它们 |

### P0.3 Trait 体系三重分裂

```
意图层 (L1 设计):                    实际验证器实现层:

  VerifierPort (ports.rs:193)         DeterministicVerifierPort (verification/ports.rs)
  verify(txn, handle)                 verify(unit)
    → VerificationRunResult             → Vec<FindingCandidate>
    → VerifierError {2 变体}             → VerifierError {8 变体}
                                          
  ├── StubVerifierPort               ├── GraphIntegrityVerifier
  │   (verifier_adapter.rs)          │   (graph_integrity.rs)
  │                                  │
  └── MockVerifierPort               ├── GraphRiskVerifier
      (mocks.rs)                     │   (graph_risk.rs)
                                     │
                                     ├── SemanticVerifierPort (verification/ports.rs)
                                     │   analyze(unit, budget)
                                     │     → Vec<FindingCandidate>
                                     │     → VerifierError {8 变体}
                                     │
                                     └── OntoRuntimeSemanticVerifierAdapter
                                         (semantic_verifier_adapter.rs:9)
                                         → 返回 "analysis stub" (占位实现)
```

**三套 trait、三套错误类型、三套实现体系，互相之间没有 `From`/`Into` 转换。**

| Trait | 输入 | 输出 | 实现数 |
|-------|------|------|--------|
| `VerifierPort` (ports.rs) | `(Transaction, StageHandle)` | `VerificationRunResult` | 2 (Stub + Mock) |
| `DeterministicVerifierPort` (verification/ports.rs) | `(VerificationUnit)` | `Vec<FindingCandidate>` | 2 (GraphIntegrity + GraphRisk) |
| `SemanticVerifierPort` (verification/ports.rs) | `(VerificationUnit, BudgetGrant)` | `Vec<FindingCandidate>` | 2 (GenericLLM + OntoRuntimeSemantic) |

| 严重度 | 说明 |
|--------|------|
| **高** | GraphIntegrity/GraphRisk 无法注入 TransactionCoordinator (签名不兼容) |
| **高** | TransactionCoordinator 的生产 VerifierPort 实现缺失 |

### P0.4 `RuntimePort` vs `RuntimeRunPort` —— 两个不同的 Runtime 抽象

```
RuntimePort (ports.rs:121)              RuntimeRunPort (ports.rs:945)
══════════════════════                  ════════════════════
用于: TransactionCoordinator            用于: LoopAdapter + RuntimeLoopRunner
抽象层次: 单次 capability 执行           抽象层次: 单次 Agent Run

stage() → StageHandle                  start_run() → RunId
execute(handle, command)               await_terminal(RunId) → RunFinalizationOutcome
capture_effects(handle)               cancel_run(RunId)
publish / discard / freeze(handle)

实现:                                   实现:
  StubRuntimePort                        MockLoopRuntime
  Adapter (→ IronClaw HostRuntime)       无生产实现
```

**两个 trait 服务于不同层次的抽象（capability vs run），但它们之间没有桥接。Agent 的一次 Run 包含多次 capability 调用，但目前没有任何代码在 capability 调用时触发验证。**

| 严重度 | 说明 |
|--------|------|
| **中** | 两个抽象各司其职是合理的，但 capability 级验证缺失 |

---

## P1 — 权力边界与状态机

### P1.1 权威决策者不唯一

目前有三个地方声称自己是"权威决策者"：

```
1. LoopAdapter::evaluate()         // 注释: "Decision must come from OntoAssure"
   实际: 只 match TaskOutcome, 不看 OntoAssure 数据

2. OntoKernelFinalizer::do_finalize()  // 注释: "pure Onto — zero OntoRuntime dependencies"
   实际: 确实调用 reduction → session_decision → settlement
   但: 读到的 evidence 为空 → 永远 Escalate

3. TransactionCoordinator::execute_attempt()
   实际: 完整的 Classify → Authorize → Reduce → Settle
   但: 零生产调用
```

**不存在唯一的权威裁决点。LoopAdapter 在运行时裁决，KernelFinalizer 在事后裁决，TransactionCoordinator 在设计文档里裁决。三个各判各的。**

| 严重度 | 说明 |
|--------|------|
| **阻断** | LoopAdapter 是实际在跑的裁决者，但它的裁决基于 Agent 自报的 TaskOutcome，而非 OntoAssure 验证结果 |

### P1.2 EffectClass 降级保护存在但未被调用

```rust
// capability.rs:114-120 — 正确实现
impl CapabilityInvocationEnvelope {
    pub fn upgrade_effect_class(&mut self, new_class: EffectClass) {
        // 只能升级, 不能降级
        if new_rank > current_rank { self.effect_class = new_class; }
    }
}
```

设计正确: Agent 不能降级 EffectClass。但 `CapabilityInvocationEnvelope` 的构造路径在生产中未被调用——没有代码在 Agent 调用 capability 时创建这个信封。

| 严重度 | 说明 |
|--------|------|
| **高** | 保护机制正确实现但未接入生产路径 |

### P1.3 `RunFinalizationPort` 重复实现，权威性不明

```
OntoKernelFinalizer (finalizer.rs)     OntoRunFinalizationAdapter (run_finalization_adapter.rs)
══════════════════════════             ══════════════════════════════
impl RunFinalizationPort               impl RunFinalizationPort

相同的流水线:                           相同的流水线:
  幂等检查 → 读证据 → reduction         幂等检查 → 读证据 → reduction
  → session_decision → settlement       → session_decision → settlement
  → persist → emit event                → persist → emit event

额外功能:                               额外功能:
  - 内存幂等缓存                          - emit 开始事件
  - 空 bundle → Escalate                 - exit_reason 映射
  - 空 records → Escalate
  - fail_next_persist 测试注入
  - InMemory* 测试实现内嵌
```

**两个实现干同一件事。哪个是生产用的？调用方不知道。**

| 严重度 | 说明 |
|--------|------|
| **中** | 功能重复，增加维护负担和选择歧义 |

### P1.4 状态机权威性: WorkItemPhase 有 Go/Rust 两套

```
Go (temporal/chasm/lib/ontoflow/types.go):
  blocked → ready → dispatched → running → outcome_reported
  → authority_verifying → committed / escalated / budget_exhausted / cancelled

Rust (没有对等的状态机):
  Loop 侧: LoopStatus (Running/Completed/Failed/BudgetExhausted/Escalated)
  Verification 侧: SessionState (Created/Running/Completed)
  VerifierExecutionLedger: VerifierExecutionStatus (Completed/Unavailable/TimedOut/ExecutionFailed)

Go 的 authority_verifying 状态在 Rust 侧没有对应实现。
Go 期望 Rust Worker 在返回 envelope 之前做 authority verification, 
但 Rust 侧的 LoopAdapter::evaluate() 不调用 OntoAssure。
```

| 严重度 | 说明 |
|--------|------|
| **高** | Go 侧状态机预留了 authority_verifying 阶段，Rust 侧没有对等实现 |

---

## P2 — 证据、状态与持久化完整性

### P2.1 Evidence 写入路径在生产中永远不触发

```
EvidenceStorePort::store() 的唯一生产级调用:
  → TransactionCoordinator::execute_attempt() line 308
    → coordinator.rs (只在其集成测试中实例化)

RuntimeLoopRunner 路径:
  → LoopAdapter::execute_attempt()
    → RuntimeRunPort::await_terminal()
    → 返回 RunFinalizationOutcome  ← evidence 从何而来? 不可知

OntoKernelFinalizer 路径:
  → EvidenceStorePort::retrieve("evidence/{attempt_id}")
  → NotFound → Escalate("No evidence found for attempt X. Finalization blocked.")
```

**证据链在 L1 内部闭环（EvidenceBuilder 构建 → EvidenceChain 封装），但从不写入 EvidenceStore。L2 的 finalizer 从 EvidenceStore 读取，永远读到空。**

| 严重度 | 说明 |
|--------|------|
| **阻断** | 证据链的读出端和写入端存在，中间缺失持久化步骤 |

### P2.2 三本总账与 EvidenceStore 断开

```
VerificationCoordinator 产出:
  ScopeLedger         → 存在内存中, 不持久化
  RuleCoverageLedger  → 存在内存中, 不持久化
  VerifierExecutionLedger → 存在内存中, 不持久化
  EvidenceBuilder::build() → Vec<EvidenceRecord> → 不写 EvidenceStore

TransactionCoordinator 产出:
  EvidenceChain::seal() → EvidenceBundle → EvidenceStore::store() ✅
  但 TransactionCoordinator 不产出三本总账

两个协调器的产物互不覆盖。需要合并。
```

| 严重度 | 说明 |
|--------|------|
| **高** | 三本总账和 EvidenceBundle 是两个协调器各自的产物，合起来才完整 |

### P2.3 `compare_progress()` 语义失明

```rust
// loop_runner.rs:349-353
let curr_progress = to_progress_snapshot(
    &[],  // "F1: satisfied criteria from verifier output"  ← 永远是空
    &[],  // "F1: unsatisfied from verifier"                ← 永远是空
    &current_output_hash,
);
```

注释明确说明数据来源应该是 "verifier output"，但实际传的是空数组。后果：

```
compare_progress(prev, curr) → 永远 Unchanged
  → record_no_progress() 每次重试都触发
  → 连续2次 Unchanged → no_progress_exhausted() → Escalate
```

**花了很大力气实现的进度比较基础设施，输入是盲的。任何失败都会在2次重试后被 Escalate，不管 Agent 实际上有没有进步。**

| 严重度 | 说明 |
|--------|------|
| **高** | 进度跟踪有完整的代码框架但数据源为空，导致语义上完全失明 |

### P2.4 持久化存储: PgEffectStore 实现了但没有接入

```rust
// pg_effect_store.rs
impl DecisionStorePort for PgEffectStore { ... }     // ✅ 生产实现
impl TransactionStorePort for PgEffectStore { ... }   // ✅ 生产实现
impl PublishReceiptStorePort for PgEffectStore { ... } // ✅ 生产实现
```

PostgreSQL 存储适配器已实现，但 `RuntimeLoopRunner` 不依赖这些 Port——它用 `IdempotencyStore` (内存实现) 而非 `DecisionStorePort`。

| 严重度 | 说明 |
|--------|------|
| **中** | PostgreSQL 存储已实现但 Loop 路径用另一套存储抽象 |

---

## P3 — 接口契约正确性

### P3.1 VerifierError 同名异体

```rust
// crates/onto-assurance-runtime/src/ports.rs:214-219
pub enum VerifierError {
    Failed(String),         // ← 2 变体, 和 TransactionCoordinator 配套
    Unavailable(String),
}

// crates/onto-assurance-runtime/src/verification/ports.rs:8
pub enum VerifierError {    // ← 同名! 8 变体, 和 DeterministicVerifierPort 配套
    Unavailable(String),
    BudgetExhausted(String),
    Failed(String),
    Timeout(u64),
    EnvironmentError(String),
    VerificationIncomplete(String),
    StaleSnapshot(String),
    UnsupportedCoverage(String),
}
```

同一个 crate 的两个子模块中定义了同名类型，编译时依赖 `use` 路径区分。两者之间没有 `From` 转换。P7 精心设计的 8 种错误分类无法流入 `TransactionCoordinator` 的错误处理。

| 严重度 | 说明 |
|--------|------|
| **高** | 同名异体，无互转，P7 错误分类被隔离在 DeterministicVerifierPort 体系内 |

### P3.2 `VerificationRunResult` vs `FindingCandidate` —— 信息密度降级

```
VerifierPort::verify()           DeterministicVerifierPort::verify()
  → VerificationRunResult          → Vec<FindingCandidate>
    {                                [{
      passed: bool,                    finding_id, target_id,
      total_checks: u32,              rule_id, severity,
      passed_checks: u32,             category, location,
      output: String                  message, suggestion_code,
    }                                  confidence: 1.0
                                     }]

信息密度: 低 (4字段)               信息密度: 高 (11字段)
面向: 布尔判定                      面向: 结构化审计
```

`TransactionCoordinator` 使用 `VerificationRunResult` 作为 VerifierPort 的返回值。如果 GraphIntegrity 发现 5 个未覆盖文件、3 个缺失 pass，这些信息在 `VerificationRunResult { passed: false, output: "..." }` 中全部丢失。下游只能知道"没过"，不知道"为什么没过"。

| 严重度 | 说明 |
|--------|------|
| **中** | 信息降级导致审计能力和自动化修复能力丧失 |

### P3.3 `RuntimePort` 的 `execute()` 接受裸 `&str command`

```rust
// ports.rs:128
async fn execute(&self, handle: &StageHandle, command: &str)
    -> Result<ExecutionResult, RuntimeError>;
```

command 是裸字符串，没有结构化参数、没有 CapabilityInvocationEnvelope、没有 arguments_hash。Agent 可以传任何字符串进去。对比 `CapabilityDescriptorPort::create_envelope()` 的设计——有 capability_id、arguments_hash、effect_class——`RuntimePort::execute()` 完全没有这些安全元数据。

| 严重度 | 说明 |
|--------|------|
| **中** | capability 执行入口缺少安全元数据，CapabilityInvocationEnvelope 体系无法接入 |

---

## P4 — 并发、资源与故障恢复

### P4.1 TransactionCoordinator 缺少超时和重试

```rust
// coordinator.rs:205
let verifier_result = self.verifier.verify(&txn, &handle).await
    .map_err(|e| CoordinatorError::Verification(e.to_string()))?;
```

`VerifierPort::verify()` 没有超时参数。如果 verifier 是 gRPC 调用（GraphIntegrity），网络超时会挂起整个 attempt。对比 L3 的 LoopBudget 中 `timeout_per_iteration_secs` 和心跳超时检测——L1 没有任何超时保护。

| 严重度 | 说明 |
|--------|------|
| **中** | 验证器调用缺少超时，与 L3 的超时体系不一致 |

### P4.2 `VerificationCoordinator` 的 `for` 循环没有并发

```rust
// verification/coordinator.rs:12
for unit in &session.plan.units {
    // 顺序执行, 没有并行
    match self.scheduler.schedule(unit).await { ... }
}
```

`VerificationPlan` 有 `max_parallel_units` 字段，但循环是顺序的。如果 100 个 unit 每个需要 5 秒的 GraphIntegrity gRPC 调用，总耗时 500 秒。

| 严重度 | 说明 |
|--------|------|
| **低** | 性能问题，不影响正确性 |

### P4.3 并发安全性: 三本总账用 `Arc<Mutex<>>`

```rust
// graph-verifiers/scheduler.rs:26-28
scope_ledger: Arc<Mutex<ScopeLedger>>,
rule_ledger: Arc<Mutex<RuleCoverageLedger>>,
execution_ledger: Arc<Mutex<VerifierExecutionLedger>>,
```

三个独立的 `Mutex`，在 `schedule()` 中依次 lock。目前是顺序执行所以没有死锁风险。如果并行化（P4.2），三个锁可能在不同顺序被获取。

| 严重度 | 说明 |
|--------|------|
| **低** | 当前无风险，并行化时需注意锁顺序 |

---

## P5 — 可观测性与进度真实性

### P5.1 心跳不携带验证状态

```rust
// loop_runner.rs:290-296
let _ = self.send_heartbeat(
    &request,
    Some(attempt_id.to_string()),
    None,    // run_id = None (attempt 启动前)
    None,    // decision_id = None
    checkpoints.last().map(|c| c.checkpoint_id.to_string()),
).await;
```

心跳上报了 `attempt_id` 和 `checkpoint_id`，但没有携带：
- 验证器执行状态 (GraphIntegrity 通过了吗?)
- Finding 数量
- Evidence 是否入库
- 当前 LoopBudget 各项计数器

Go 侧只能知道 "attempt 还在跑"，不知道 "attempt 的验证质量如何"。

| 严重度 | 说明 |
|--------|------|
| **中** | 心跳丢失可观测性关键维度，运维无法判断 Loop 健康度 |

### P5.2 `VerificationCoordinator` 没有 tracing span

```rust
// P7 代码有 tracing:
info!(%target_id, %unit_id, ?ct, ?i_policy, ?r_policy, "GraphVerifierScheduler");

// 但 VerificationCoordinator 没有对应的 span:
for unit in &session.plan.units {
    match self.scheduler.schedule(unit).await { ... }  // 无 tracing
}
```

L3 的心跳 + L1 的 tracing 各做各的，没有关联。无法从一次 Loop 的执行追踪到具体哪个 verifier 在哪个 unit 上产生了什么结果。

| 严重度 | 说明 |
|--------|------|
| **低** | 运维体验问题 |

---

## P6 — 局部代码质量

### P6.1 死代码

| 位置 | 内容 | 大小 |
|------|------|------|
| `ontoloop_agent.rs` | 完整模块: `OntoLoopAgent`, `execute_attempt()`, `evaluate_outcome()`, `run_to_completion()`, `build_prompt()` | ~260 行 |
| `ontoloop.rs:76-92` | `VerificationResult` 类型，定义了但零构造 | ~17 行 |
| `attempt_decision.rs:25-111` | `decide_attempt()` 函数，零生产调用 | ~86 行 |

### P6.2 重复代码

| 位置1 | 位置2 | 重复度 |
|-------|-------|--------|
| `finalizer.rs:41-231` | `run_finalization_adapter.rs:46-152` | ~90% (相同 pipeline) |
| `loop_adapter.rs:46-53` | `ontoloop_agent.rs:107-119` | ~90% (相同 evaluate 逻辑) |
| `coordinator.rs` (TransactionCoordinator) | `verification/coordinator.rs` (VerificationCoordinator) | ~40% (都做 verify → reduce, 但不同 trait) |

### P6.3 注释与实现不一致

| 位置 | 注释 | 实际 |
|------|------|------|
| `loop_runner.rs:350` | `"F1: satisfied criteria from verifier output"` | `&[]` — 空数组 |
| `loop_runner.rs:351` | `"F1: unsatisfied from verifier"` | `&[]` — 空数组 |
| `loop_adapter.rs:4` | `"Decision must come from OntoAssure, not from Agent text output"` | `match task_outcome` — 不看 OntoAssure |
| `ontoloop_agent.rs:54` | `"Evaluate via OntoAssure result"` | `match outcome.task_outcome` — 不看 OntoAssure |

---

## P0–P6 问题汇总

| 优先级 | # | 问题 | 严重度 |
|--------|---|------|--------|
| **P0** | 1 | `RuntimeRunPort` 无生产实现，Agent Run 是否经过验证不可知 | 阻断 |
| **P0** | 2 | `TransactionCoordinator` 和 `VerificationCoordinator` 都不在生产路径中 | 阻断 |
| **P0** | 3 | `VerifierPort` vs `DeterministicVerifierPort` vs `SemanticVerifierPort` 三套 trait 互不兼容 | 高 |
| **P1** | 4 | 权威决策分散在 LoopAdapter / KernelFinalizer / TransactionCoordinator 三处 | 阻断 |
| **P1** | 5 | LoopAdapter 是实际裁决者，但只看 Agent 自报的 TaskOutcome | 阻断 |
| **P1** | 6 | Go 侧 `authority_verifying` 状态在 Rust 侧无对等实现 | 高 |
| **P1** | 7 | `RunFinalizationPort` 两个重复实现 | 中 |
| **P1** | 8 | EffectClass 保护机制正确但未被调用 | 高 |
| **P2** | 9 | Evidence 写入路径在生产中永不触发 → finalizer 永远读到空 | 阻断 |
| **P2** | 10 | 三本总账与 EvidenceStore 断开，两套协调器产物互不覆盖 | 高 |
| **P2** | 11 | `compare_progress()` 输入空数组，进度跟踪完全失明 | 高 |
| **P2** | 12 | PgEffectStore 实现了但 Loop 路径用另一套存储抽象 | 中 |
| **P3** | 13 | `VerifierError` 同名异体，无互转 | 高 |
| **P3** | 14 | `VerificationRunResult` → `FindingCandidate` 信息降级 | 中 |
| **P3** | 15 | `RuntimePort::execute()` 接受裸字符串，缺少安全元数据 | 中 |
| **P4** | 16 | `TransactionCoordinator` 缺少超时和重试 | 中 |
| **P4** | 17 | `VerificationCoordinator` 顺序执行无并发 | 低 |
| **P4** | 18 | 三本总账用独立 Mutex，并行化时需注意锁顺序 | 低 |
| **P5** | 19 | 心跳不携带验证状态 | 中 |
| **P5** | 20 | L3 心跳 + L1 tracing 无关联 | 低 |
| **P6** | 21 | `OntoLoopAgent` 死模块 (~260行) | 低 |
| **P6** | 22 | `decide_attempt()` 孤立函数 (~86行) | 低 |
| **P6** | 23 | `VerificationResult` 死类型 (~17行) | 低 |
| **P6** | 24 | 4 处注释与实现不一致 | 低 |
| **P6** | 25 | finalizer 重复实现 (~350行重复) | 中 |

---

## 修复路线图

### 第一步 (解除阻断): 统一 Trait → 接入生产路径

```
1. 将 DeterministicVerifierPort 的签名适配到 VerifierPort, 或反之
   → GraphIntegrity/GraphRisk 可以注入 TransactionCoordinator
   
2. 在 RuntimeLoopRunner::execute() 中, 
   LoopAdapter::evaluate() 之前,
   插入 VerificationCoordinator::execute() 调用
   → 产出 FindingCandidate → EvidenceBuilder → EvidenceStore::store()

3. compare_progress() 的数据源改为 ReasonCode 中的 satisfied/unsatisfied criteria
```

### 第二步 (清理): 删除死代码 + 合并重复

```
1. 删除 OntoLoopAgent 整个模块
2. 删除 VerificationResult 类型 (或接入)
3. 合并两个 RunFinalizationPort 实现
4. 将 decide_attempt() 逻辑融入 LoopAdapter::evaluate()
5. 统一两个 VerifierError 为一个
```

### 第三步 (加固): 可观测性 + 持久化

```
1. 心跳携带验证状态 (verifier 结果 + finding 数量)
2. PgEffectStore 接入 Loop 路径
3. 三本总账持久化到 EvidenceStore 或独立存储
4. TransactionCoordinator 加超时
```
