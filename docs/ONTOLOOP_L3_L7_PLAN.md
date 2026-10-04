# OntoLoop L3–L7 施工记录（已完成）

> 原计划：2 个核心机制 + 1 个补齐 + 1 个薄适配器 + 1 轮验收，新增 13–16 个测试  
> **状态：全部完成。OntoLoop v0.1 CLOSED。**

---

## 最终状态

```
L0 Attempt 数据模型            ✅  (6 tests)   — onto-assurance-types
L1 Attempt 生命周期            ✅               — onto-assurance-types
L2 结构化 Continuation         ✅               — onto-assurance-types
L3 Checkpoint / Evidence 绑定  ✅  (4 tests)   — onto-loop/src/checkpoint.rs
L4 Progress / Regression       ✅  (5 tests)   — onto-loop/src/progress.rs
L5 Budget / Stuck 控制         ✅  (4 tests)   — onto-loop/src/budget.rs
L6 OntoRuntime Adapter            ✅               — onto-ironclaw-adapter/src/loop_adapter.rs
L7 真实 E2E                    ✅               — loop_adapter.rs tests
─────────────────────────────────────────────
OntoLoop v0.1 封口: 13 tests in onto-loop, 24 tests in adapter
```

已证明：

```
Attempt 1 → OntoRuntime 执行 → OntoAssure 裁决 Failed
→ 生成结构化缺口 → Attempt 2 → Success + Commit → Loop Completed
```

---

## L3：Checkpoint 与 Evidence 绑定

### 源码

`crates/onto-loop/src/checkpoint.rs` (144 行)

### 新增类型

- `AttemptCheckpoint` — 每个 Attempt 的输入/输出快照
- `AttemptAuthorityBinding` — Attempt ↔ Evidence ↔ Decision 完整绑定

### 核心函数

- `validate_evidence_matches_checkpoint()` — Evidence hash 必须匹配 Checkpoint
- `validate_decision_matches_binding()` — Decision 必须绑定当前 output
- `select_rollback_checkpoint()` — 选择正确父 Checkpoint
- `validate_no_cross_branch_evidence()` — 旧分支 Evidence 不得提交新分支

### L3 测试（4 项，全部通过）

| # | 测试 | 预期 |
|---|------|------|
| L3-1 | Evidence 跨 Attempt 复用被拒绝 | hash-A 的 Evidence 提交 hash-B → 拒绝 |
| L3-2 | Decision 绑定当前 Checkpoint | Decision 绑定 C1，当前输出 C2 → 不得 Completed |
| L3-3 | Rollback 选择正确父 Checkpoint | C0→C1→C2 退化 → 下一 Attempt 输入必须是 C1 |
| L3-4 | 旧分支 Evidence 失效 | C1→C2a→回滚→C2b，C2a 的 Evidence 不能提交 C2b |

---

## L4：Progress 与 Regression

### 源码

`crates/onto-loop/src/progress.rs` (123 行)

### 新增类型

- `ProgressSnapshot` — 来自 OntoAssure 结构化结果的进展快照
- `ProgressComparison` — Improved / Unchanged / Regressed / Incomparable

### 核心函数

- `compare_progress()` — 纯函数，确定性比较两次 Attempt 进展

### 策略

```
Improved    → Continue (当前策略有效)
Unchanged   → 增加 no_progress_count，超阈值 Escalate
Regressed   → Rollback 到上一个 Checkpoint
Incomparable → Escalate
```

### L4 测试（5 项，全部通过）

| # | 测试 | 预期 |
|---|------|------|
| L4-1 | 新增 SAT → Improved | C1 SAT, C2 UNSAT → C2: C1 SAT, C2 SAT → Improved |
| L4-2 | 文件变化但 Criterion 不变 → Unchanged | Hash 变，SAT/UNSAT 集合不变 → Unchanged |
| L4-3 | 原 SAT 变 UNSAT → Regressed | C1 SAT → C2 UNSAT → Regressed → Rollback |
| L4-4 | 有得有失 → Incomparable | C1 UNSAT→SAT, C2 SAT→UNSAT → Incomparable → Escalate |
| L4-5 | violations 增加 → Regressed | 相同 criteria 但 protected_scope_violations 增加 |

---

## L5：Budget / Stuck 控制

### 源码

`crates/onto-loop/src/budget.rs` (110 行)

### 新增类型

- `LoopBudget` — 多维预算（max_attempts / max_no_progress / max_regressions / max_env_errors）
- `LoopOutcome` — 正交结果（TaskOutcome × BudgetOutcome × LoopStatus）

### 三维状态正交

```
最后一个 Attempt 成功 + 同时消耗完预算
→ TaskOutcome = Success
→ BudgetOutcome = Depleted
→ LifecycleState = Completed

预算耗尽不能把成功改成失败
```

### L5 测试（4 项，全部通过）

| # | 测试 | 预期 |
|---|------|------|
| L5-1 | 成功与预算耗尽正交 | Success + Depleted + Completed |
| L5-2 | 连续无进展停止 | 连续 2 轮 Unchanged → no_progress_exhausted |
| L5-3 | 回归次数超限 | 2 次回归 → regressions_exhausted |
| L5-4 | 改善后重置无进展计数 | Improvement → consecutive_no_progress 归零 |

---

## L6：OntoRuntime Adapter

### 源码

`crates/onto-ironclaw-adapter/src/loop_adapter.rs`

### 核心约束

```
一次 Attempt = 一次 OntoRuntime Run (严格一对一)
OntoLoop 不能直接调用 Capability
OntoLoop 不能自产 Decision (只能从 DecisionStore 加载)
```

### L6 关键实现

- `LoopAdapter::execute_attempt()` — 通过 RuntimeRunPort 执行 Attempt，含幂等检查
- `LoopAdapter::evaluate()` — 从 OntoAssure Decision 判定 LoopDecision（Commit / Continue / Escalate / Cancel）
- `LoopAdapter::cancel()` — 终止未完成的 Run
- L6-1: Attempt 元数据正确传入
- L6-2: 相同 attempt_id 不创建第二个 Run（幂等）
- L6-3: 无 Decision 不得完成 Attempt
- L6-4: Loop 终止 → 未完成 Run 被取消

---

## L7：真实 E2E 验收

### 源码

`crates/onto-ironclaw-adapter/src/loop_adapter.rs` tests section（L6 + L7 合并）

### E2E-1：两轮修复后成功

```
Contract: C1(file exists), C2(content), C3(tests pass), C4(protected)

Attempt 1: C1 SAT, C2 SAT, C3 UNSAT, C4 SAT → Failed + Continue
Attempt 2: C1 SAT, C2 SAT, C3 SAT, C4 SAT → Success + Commit
→ Loop Completed
```

### E2E-2：第二轮退化后回滚

```
Attempt 1: C1/C2 SAT, C3 UNSAT
Attempt 2: C3 SAT, C1 UNSAT → Regressed
→ Rollback 到 Attempt 1 Checkpoint
```

### E2E-3：连续无进展升级

```
Attempt 1: C3 UNSAT
Attempt 2: C3 UNSAT
Attempt 3: C3 UNSAT
→ NoProgress 阈值达到 → Escalated
```

### 两种测试模式

| 模式 | 用途 | CI |
|------|------|-----|
| 确定性 CI | Scripted Worker，经过真实 RunIngress/CapabilityHost/Hook/Verifier/OntoAssure/M6-A | ✅ |
| Live LLM Smoke | DeepSeek + 真实 tool calling | 手动/夜间 |

---

## 测试预算（实际 vs 计划）

| 阶段 | 计划 | 实际 | 来源 |
|------|------|------|------|
| L0-L2 | 6 | 6 | onto-assurance-types |
| L3 Checkpoint | 4 | 4 | onto-loop/src/checkpoint.rs |
| L4 Progress | 4 | 5 | onto-loop/src/progress.rs |
| L5 Budget | 3 | 4 | onto-loop/src/budget.rs |
| L6 Adapter | 4 | — | (合并在 adapter) |
| L7 E2E | 3 | — | (合并在 adapter) |
| L6+L7 | 7 | 24 | onto-ironclaw-adapter tests |
| **合计** | **24** | **≥43** | **0 failures** |

---

## 源码落点（实际）

```
OntoOS/crates/
├── onto-loop/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              — 模块导出
│       ├── checkpoint.rs       — L3 (144 lines, 4 tests)
│       ├── progress.rs         — L4 (123 lines, 5 tests)
│       └── budget.rs           — L5 (110 lines, 4 tests)
│
├── onto-ironclaw-adapter/
│   └── src/
│       └── loop_adapter.rs     — L6 实现 + L6+L7 tests
│
└── onto-assurance-types/src/
    └── ontoloop.rs             — L0-L2 类型定义 (6 tests)
```

---

## 封口标准（已满足）

```
OntoLoop v0.1 CLOSED ✅

范围（全部完成）:
- [x] 单任务、串行 Attempt
- [x] 一次 Attempt = 一次 OntoRuntime Run
- [x] OntoAssure 唯一终态权威
- [x] 结构化 Continuation (非模糊 prompt)
- [x] Checkpoint/Evidence/Decision 绑定
- [x] 确定性 Progress 判断
- [x] 有限预算与 Stuck 控制

不做（属于后续里程碑）:
- 多 Agent 并行
- 搜索树 / 分支合并
- 自动策略学习
- 跨任务 DAG → OntoFlow
- 分布式调度 → OntoFlow
- OntoFlow Workflow → v0.7
```
