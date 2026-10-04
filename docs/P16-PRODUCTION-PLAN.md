# P16: L1-L2-L3 生产收敛计划

> **目标**: 将当前 5 条半重叠链路收敛为 1 条唯一权威路径，消除空注册表/Dummy/Mock/旧Finalizer在生产构建中的存在可能。
>
> **日期**: 2026-07-30
>
> **前置审计**: [L1-L2-AUDIT.md](L1-L2-AUDIT.md) · [ONTOLOG.md](../ONTOLOG.md) · [ASSESSMENT.md](../ASSESSMENT.md)
>
> **封板审查**: 第一轮 14 处 + 第二轮 10 处已修正于正文，不再写入附录。

---

## 零、当前状态（不可跳过的事实基线）

### 0.1 五条同时存在的链路

```
链路 A (P15, 生产路径):
  RuntimeLoopRunner → AssuredAttemptCoordinator → PipelineManager(空registry)
  → LoopAdapter::evaluate_required() → Commit → Escalated

链路 B (OLD, 生产回退):
  RuntimeLoopRunner → LoopAdapter::evaluate_with_verdict(None)
  → 看 Agent 自报 TaskOutcome → Fail-Closed Escalate

链路 C (孤立, 零调用):
  OntoKernelFinalizer::do_finalize()
  → evidence.retrieve() → NotFound → Escalate

链路 D (孤立, 零调用):
  OntoRunFinalizationAdapter::finalize()
  → 功能与 OntoKernelFinalizer 90% 重复

链路 E (孤立, 零调用):
  旧 GraphIntegrity/GraphRisk/三本总账/EvidenceBuilder
  → 实现 DeterministicVerifierPort, 与 PipelineManager 的 Verifier trait 不兼容
```

### 0.2 实际代码事实

| 事实 | 位置 | 严重度 |
|------|------|:------:|
| worker.rs 生产二进制使用 `MockLoopRuntime` | `crates/onto-temporal-adapter/src/bin/worker.rs:66` | 阻断 |
| `PipelineManager.registry` 按 `(Pass, String)` 查找，创建为空，无 Verifier 注册 | `pipeline.rs:29`, `worker.rs:82` | 阻断 |
| `VerifierServices` 三个方法全是 Dummy (空manifest/Ok/"ok") | `assured_coordinator.rs:150-156` | 阻断 |
| `evaluate_required()` 的 `outcome` 参数是死参数 | `loop_adapter.rs:56` | 高 |
| `LoopDecision::Commit → LoopTerminalState::Escalated` | `loop_runner.rs:558-561` | 高 |
| 3 个 `RunFinalizationPort` 实现 | `finalizer.rs:225`, `run_finalization_adapter.rs:24,63` | 中 |
| `DeterministicVerifierPort` vs `Verifier` 两套 trait 互不兼容 | `verification/ports.rs`, `verifier.rs:140` | 高 |
| `EvidenceStorePort::store()` 唯一生产调用者不存在 | 搜索结果: 0 | 阻断 |
| `evidence_store.retrieve()` 只有孤立的 `OntoKernelFinalizer` 调用 | `finalizer.rs:84` | 阻断 |

### 0.3 关键冻结协议（P16 不会修改这些）

**以下类型来自 `onto-protocol` crate，P16 在现有变体内实施，不新增变体。**

**`ConformanceUnit`** (`check.rs:56-62`):

```rust
pub struct ConformanceUnit {
    pub unit_id: String,
    pub verifier_id: String,               // ← 权威查找键
    pub pass: crate::verifier::Pass,       // ← 元数据，不用于查找
    pub validation_dependencies: Vec<String>,
    pub applicability: Applicability,      // Required | Optional | NotApplicable
}
```

**`Pass` 枚举** (`verifier.rs:17-27`):

```rust
pub enum Pass {
    Format, Static, Build, Behavior,
    DependencySafety, FileIntegrity, GraphIntegrity,
    GraphRisk, SemanticRules,
}
```

**`VerifierStatus` 枚举** (`verifier.rs:108-115`):

```rust
pub enum VerifierStatus {
    Completed,
    PartiallyCompleted,
    PrerequisiteFailed,
    Unavailable,
    TimedOut,
    NotApplicable,
}
// 注: 不存在 EvidenceIncomplete / Failed 变体。
//     语义上 PartiallyCompleted 覆盖证据不足, Unavailable 覆盖执行失败。
```

**`LoopDirective`** (`loop_protocol.rs:73-88`):

```rust
pub enum LoopDirective {
    CandidateBound {
        decision_id: String,
        attempt_id: String,
        candidate_id: String,
        candidate_digest: Digest,
        verdict_id: String,                // 当 AssuranceBinding::Verdict 时填充
        verdict_digest: Digest,            // 当 AssuranceBinding::Verdict 时填充
        decision: CandidateLoopDecision,
    },
    AttemptBound {
        decision_id: String,
        attempt_id: String,
        decision: NoCandidateLoopDecision,
    },
}
```

**`CandidateLoopDecision`** (`loop_protocol.rs:52-59`):

```rust
pub enum CandidateLoopDecision {
    Continue { feedback: Vec<String> },
    DiscardCandidate { reason: String },
    RestoreCheckpoint { checkpoint_id: String },
    FinalizeCandidate,
    Freeze { reason: String },
    Escalate { reason: String },
}
```

**`NoCandidateLoopDecision`** (`loop_protocol.rs:62-66`):

```rust
pub enum NoCandidateLoopDecision {
    Freeze { reason: String },
    Escalate { reason: String },
    CloseFailed,
}
```

**`SettlementState`** (`loop_protocol.rs:118-123`):

```rust
pub enum SettlementState {
    Committed,
    RolledBack,
    Frozen,
    Unknown,
}
// 注: 不存在 PendingAuthority 变体。
```

**`AssuranceObservation`** (`loop_protocol.rs:15-21`):

```rust
pub enum AssuranceObservation {
    Verdict(ConformanceVerdict),
    Unavailable { reason: String, diagnostic_ref: String },
}
```

**`TransitionOutcome`** (`loop_protocol.rs:93-101`):

```rust
pub enum TransitionOutcome {
    Continued,
    CandidateDiscarded,
    CheckpointRestored { checkpoint_id: String },
    Finalized { settlement: SettlementState, receipt_ref: String },
    Frozen,
    Escalated,
    AttemptClosed,
}
```

### 0.4 P16 新增的协议类型（仅一个）

`AssuranceObservation::Unavailable` 携带诊断信息但没有可验证的摘要。当它进入 `LoopDirective::CandidateBound` 时，`verdict_id` / `verdict_digest` 字段为空字符串不能正确表达"这不是 Verdict"。P16 新增一个内部绑定类型：

```rust
/// 绑定到 L1 产物的可验证引用。Verdict 和 Unavailable 都有各自的 digest。
/// 此类型仅用于 LoopDirective 内部；不修改 LoopDirective 的公开变体。
pub enum AssuranceBinding {
    Verdict {
        verdict_id: String,
        verdict_digest: Digest,
    },
    Unavailable {
        failure_id: String,
        failure_digest: Digest,
    },
}
```

规则：

```text
Unavailable 必须先作为 PersistedAssuranceFailure 落库
→ 获得 failure_id + failure_digest
→ CandidateBound 携带 AssuranceBinding::Unavailable { failure_id, failure_digest }
→ TransitionExecutor 可据此验证失败记录确实存在且未被篡改
```

**不使用 `verdict_id: String::new()` 占位。**

### 0.5 稳定 Verifier ID

Plan 和 Registry 双方共用。Pass 只作为元数据附在 `VerifierDescriptor` 和 `ConformanceUnitResult` 上。

```text
artifact.manifest.integrity
file.protected_path
project.build
project.test
project.static
graph.integrity
graph.risk
```

### 0.6 基线生产 Profile

```text
artifact.manifest.integrity   Required
file.protected_path           Required
project.build                 Required
project.test                  Required  (execution_dependencies: [project.build])
```

`ConformancePlanCompiler` 必须在生产模式下至少生成以上 4 个 Required Unit，否则 → `PipelineError::InvalidPlan`（类别 B）。

---

## 一、目标架构（唯一生产路径）

```
Go OntoFlow
    │  LoopInvocationRequest (跨语言确定性hash)
    ▼
┌──────────────────────────────────────────────────────────────────┐
│  L3 OntoLoop                                                     │
│                                                                  │
│  RuntimeLoopRunner::execute()                                    │
│   coordinator: Arc<AssuredAttemptCoordinator>  ← 非 Option       │
│                                                                  │
│  while budget remains:                                           │
│    ┌───────────────────────────────────────────────────────┐     │
│    │  AssuredAttemptCoordinator::execute_attempt()         │     │
│    │                                                       │     │
│    │  ① L2 RealAttemptExecutor                             │     │
│    │     └─ IronClawRuntimeAdapter (真实Agent Sandbox)     │     │
│    │         Agent 写 Staging → 停止写入                   │     │
│    │                                                       │     │
│    │  ② CandidateSealer                                    │     │
│    │     ├─ 计算 candidate_digest + manifest_digest         │     │
│    │     └─ 写入不可变 SealedArtifactStore                  │     │
│    │        此后任何人不得读 staging                       │     │
│    │                                                       │     │
│    │  ③ L1 AssuranceRunner                                 │     │
│    │     ├─ ConformancePlanCompiler (基线Profile驱动)       │     │
│    │     ├─ FrozenVerifierRegistry (by verifier_id,        │     │
│    │     │    拒绝重复ID, 冻结后不可变)                     │     │
│    │     ├─ ProductionVerifierServices                     │     │
│    │     ├─ GVisorVerificationExecutor                     │     │
│    │     ├─ EvidenceAssembler                              │     │
│    │     └─ VerdictReducer                                 │     │
│    │                                                       │     │
│    │  ④ AssuranceRunStore (两阶段持久化, 无可见性竞态)      │     │
│    │     ├─ Phase 1: Evidence 写入内容寻址对象              │     │
│    │     ├─ Phase 2: DB事务1 — Verdict (status=Finalizing)  │     │
│    │     │              + Outbox (status=Held)              │     │
│    │     ├─ Phase 3: mark_evidence_referenced()             │     │
│    │     └─ Phase 4: DB事务2 — run→Finalized,               │     │
│    │                  verdict→Visible, outbox→Publishable   │     │
│    │                                                       │     │
│    │  ⑤ L3 DecisionEngine (onto-loop crate)                │     │
│    │     └─ AttemptObservation + ProgressSnapshot           │     │
│    │        + LoopContext → LoopDirective                   │     │
│    │        Progress 实际影响决策 (Unchanged阈值等)         │     │
│    │                                                       │     │
│    │  ⑥ LoopDirectiveStore                                 │     │
│    │     └─ ALL Directive 必须 persist_if_absent()          │     │
│    │                                                       │     │
│    │  ⑦ L2 TransitionExecutor                              │     │
│    │     └─ ALL Directive → apply_transition()              │     │
│    │        → TransitionReceipt                             │     │
│    │        └─ 校验所有 digest                              │     │
│    │                                                       │     │
│    │  ⑧ LifecycleReducer (唯一归约点)                       │     │
│    │     └─ Directive + TransitionReceipt                   │     │
│    │        → Lifecycle (终态或循环)                        │     │
│    │     RuntimeLoopRunner 只根据 Reducer 结果行动           │     │
│    └───────────────────────────────────────────────────────┘     │
└──────────────────────────────────────────────────────────────────┘
```

### 权力边界

```
L1 OntoAssure  → ConformanceVerdict 或 PersistedAssuranceFailure (Unavailable 时)
L3 OntoLoop    → LoopDirective 或 AttemptBound (带完整 AssuranceBinding)
L2 OntoRuntime → TransitionReceipt { outcome: Committed | RolledBack | Frozen | Unknown }
L4 OntoFlow    → 验证 Envelope 版本和 digest, 编排下游, 不重新裁决
```

### 唯一主链（所有 Directive 都必须经过）

```
LoopDirective
  → directive_store.persist_if_absent()   ← 幂等键: decision_id + directive_digest
  → transition_executor.apply_transition()
  → TransitionReceipt
  → lifecycle_reducer.reduce()
  → Lifecycle (终态: Committed/Failed/Frozen/Escalated | 循环: Continue)
```

**任何 Directive 不得绕过此链。** `RuntimeLoopRunner` 不得自行映射状态。

### 禁止的终态路径

```
❌ Agent::finish() → TaskOutcome::Success
❌ 空 Plan (零 Required Unit) → 任何 Verdict
❌ 空 Registry → 服务启动
❌ Dummy Service → VerifierStatus::Completed
❌ Missing Required Verifier → 静默跳过
❌ Verifier 读 staging (Candidate Seal 后只能读 SealedArtifactStore)
❌ Registry 静默覆盖重复 verifier_id
❌ evidence.iter().all() 空集恒真
❌ Evidence 未标记 Referenced → Verdict 对外可见
❌ Directive 未持久化 → TransitionExecutor::apply()
❌ Freeze/Escalate/Continue/AttemptBound 绕过持久化+Transition 链
❌ 基础设施故障 → Continue (应 Coordinator 内部 Assurance 重试)
❌ AssuranceUnavailable 伪造 verdict_id/verdict_digest
❌ RuntimeLoopRunner 自行映射状态 (应通过 LifecycleReducer)
```

---

## 二、三类错误的统一定义

### 2.1 分类

```
类别 A: 构建期错误 → BootstrapError
  生产 Profile 声明的 Verifier 没注册
  Registry 为空
  Registry 包含重复 verifier_id
  gVisor 不可用
  Required Service 不可用
  → 服务拒绝启动

类别 B: 计划期错误 → PipelineError → AssuranceObservation::Unavailable
  Plan 编译出零 Required Unit (包括基线 Profile 未生成对应 Unit)
  Plan 引用的 Required verifier_id 在 Registry 中不存在
  → 不产出 Verdict, 向上报告 Unavailable (携带 PersistedAssuranceFailure)

类别 C: 执行期错误 → ConformanceOutcome::Inconclusive
  Verifier 已注册, 但执行时工具/Sandbox/Graph 服务不可用
  Coverage 不完整
  Sandbox 启动失败或 RuntimeLost
  → 产出合法的 Inconclusive Verdict, 字段完整绑定
```

### 2.2 Inconclusive Verdict 字段要求

```text
Inconclusive Verdict 必须完整绑定:
  attempt_id, candidate_id, candidate_digest
  plan_id, plan_digest
  evidence_bundle_ref, evidence_bundle_digest
  unit_results (每个 Required Unit 的确切状态)
  sandbox (Executed / StartupFailed / RuntimeLost)
  verdict_digest

不是"默认占位"——是"证据不足但过程已记录"。
Pre-Plan 失败 (类别 B) 产 PersistedAssuranceFailure, 不产 Verdict。
```

### 2.3 基础设施故障处理

```text
基础设施可重试错误:
  → Coordinator 内部针对同一 Sealed Candidate 重试 Assurance
  → 不创建新 Candidate
  → 不消耗 Agent 修复轮次

超过 Assurance retry budget:
  → PersistedAssuranceFailure
  → DecisionEngine → Freeze / Escalate (通过 TransitionExecutor)

Continue 只用于:
  Candidate 存在可修复的 NonConformant Finding
  → Agent 修复代码, 创建新 Candidate
```

---

## 三、修复阶段

### P16-0: 锁死虚假成功 (立即, 不改架构)

**目标**: 系统可以失败, 但不能假成功。

**修改文件**: `crates/onto-assurance-runtime/src/pipeline.rs`

删除 `(Pass, String)` 查找，改为按 `verifier_id` 查找，拒绝重复 ID：

```rust
pub struct PassRegistry {
    by_id: HashMap<String, Arc<dyn Verifier>>,
}

impl PassRegistry {
    pub fn register(&mut self, verifier_id: &str, v: Box<dyn Verifier>)
        -> Result<(), BootstrapError>
    {
        let v: Arc<dyn Verifier> = Arc::from(v);
        let stored_id = v.descriptor().verifier_id.clone();

        // 不变量 1: 注册键 == VerifierDescriptor.verifier_id
        if stored_id != verifier_id {
            return Err(BootstrapError::VerifierIdMismatch {
                registration_key: verifier_id.to_string(),
                descriptor_key: stored_id,
            });
        }

        // 不变量 2: 拒绝重复 ID
        if self.by_id.contains_key(verifier_id) {
            return Err(BootstrapError::DuplicateVerifierId {
                verifier_id: verifier_id.to_string(),
            });
        }

        self.by_id.insert(verifier_id.to_string(), v);
        Ok(())
    }

    pub fn require(&self, verifier_id: &str) -> Option<&Arc<dyn Verifier>> {
        self.by_id.get(verifier_id)
    }

    /// 冻结后不可再 register。
    pub fn freeze(self) -> FrozenVerifierRegistry {
        FrozenVerifierRegistry { by_id: self.by_id }
    }
}
```

`PipelineManager::execute()` — 正确处理 Required 和 Optional Unit：

```rust
pub async fn execute(
    &self, plan: &ConformancePlan, ctx: &VerificationContext,
    exec: &dyn VerificationExecutor, svc: &VerifierServices<'_>,
) -> Result<ConformanceVerdict, PipelineError> {
    // 检查 1: 零 Required Unit → PipelineError (类别 B)
    let required_units: Vec<&ConformanceUnit> = plan.units.iter()
        .filter(|u| u.applicability == Applicability::Required)
        .collect();
    if required_units.is_empty() {
        return Err(PipelineError::InvalidPlan {
            reason: "Plan contains zero required units".into(),
            plan_id: plan.plan_id.clone(),
        });
    }

    // 检查 2: Required Unit 引用的 verifier_id 未注册 → PipelineError (类别 B)
    for unit in &required_units {
        if self.registry.require(&unit.verifier_id).is_none() {
            return Err(PipelineError::InvalidPlan {
                reason: format!(
                    "Required verifier '{}' (unit '{}') not in registry",
                    unit.verifier_id, unit.unit_id
                ),
                plan_id: plan.plan_id.clone(),
            });
        }
    }

    // 执行: 按 Plan.units, 正确处理 Optional 未注册
    let mut unit_results: Vec<ConformanceUnitResult> = vec![];
    let mut blocking: Vec<Finding> = vec![];
    let mut advisory: Vec<Finding> = vec![];

    for unit in &plan.units {
        match self.registry.require(&unit.verifier_id) {
            Some(verifier) => {
                // 执行 verifier ...
                let result = verifier.evaluate(ctx, &[], svc).await;
                let ur = build_unit_result(unit, &result);
                collect_findings(&result, &mut blocking, &mut advisory);
                unit_results.push(ur);
            }
            None => {
                // Required 已在检查 2 中处理, 这里只有 Optional / NotApplicable
                unit_results.push(ConformanceUnitResult::not_applicable(unit));
            }
        }
    }

    // 归约 Verdict
    let verdict = crate::verdict::reduce(plan, blocking, advisory, unit_results, sandbox, graph)?;

    // 检查 3: Conformant 必须满足全量条件
    if verdict.conformance == ConformanceOutcome::Conformant {
        if !matches!(verdict.freshness, FreshnessState::Current)
            || !matches!(verdict.coverage, CoverageState::Complete)
            || !verdict.sandbox.is_executed_and_completed()
            || !verdict.all_required_units_have_determinate_result()
        {
            return Ok(ConformanceVerdict {
                conformance: ConformanceOutcome::Inconclusive,
                ..verdict
            });
        }
    }

    Ok(verdict)
}
```

**验收**:
- [ ] 空 Plan (零 Required Unit) → `PipelineError::InvalidPlan`, 不产 Verdict
- [ ] Required Unit 的 `verifier_id` 未注册 → `PipelineError::InvalidPlan`
- [ ] Optional Unit 未注册 → `ConformanceUnitResult::NotApplicable`, 不 panic
- [ ] 重复 `verifier_id` 注册 → `BootstrapError::DuplicateVerifierId`
- [ ] `Conformant` 但 Sandbox 不完整 → 自动降级为 `Inconclusive`
- [ ] Registry 查找键是 `verifier_id`，不是 `(Pass, String)`
- [ ] 代码中无 `.expect()` 依赖协议完整性
- [ ] `cargo test --workspace` 全部通过

---

### P16-1: 生产 Composition Root

**目标**: 只有一种构造方式进入生产, Mock/Dummy 在编译期隔离。

**新增文件**: `crates/onto-temporal-adapter/src/production.rs`

```rust
pub struct ProductionComponents {
    // ── L2: Agent 执行 ──
    pub attempt_executor: Arc<dyn AttemptExecutor>,
    pub candidate_sealer: Arc<dyn CandidateSealer>,
    pub artifact_store: Arc<dyn SealedArtifactStore>,

    // ── L1: Assurance ──
    pub plan_compiler: Arc<ConformancePlanCompiler>,
    pub verification_executor: Arc<dyn VerificationExecutor>,
    pub registry: FrozenVerifierRegistry,
    pub verifier_services: ProductionVerifierServices,
    pub assurance_store: Arc<dyn AssuranceRunStore>,

    // ── L3: Decision ──
    pub progress_store: Arc<dyn ProgressStore>,
    pub directive_store: Arc<dyn LoopDirectiveStore>,
    pub decision_engine: Arc<DecisionEngine>,

    // ── L2: Transition + Lifecycle ──
    pub transition_executor: Arc<dyn TransitionExecutor>,
    pub lifecycle_reducer: Arc<LifecycleReducer>,
}

impl ProductionComponents {
    pub async fn build(config: ProductionConfig) -> Result<Self, BootstrapError> {
        // 1. Runtime
        let attempt_executor = IronClawRuntimeAdapter::connect(config.runtime).await?;
        attempt_executor.verify_gvisor_available().await?;

        // 2. CandidateSealer + ArtifactStore
        let artifact_store = build_sealed_artifact_store(&config).await?;
        let candidate_sealer = Arc::new(CandidateSealer::new(artifact_store.clone()));

        // 3. Registry — 类别 A
        let registry = build_production_registry(&config)?;
        if registry.is_empty() {
            return Err(BootstrapError::EmptyRegistry);
        }
        registry.validate_required_profiles(&config.profiles)?;
        let registry = registry.freeze();

        // 4. Services
        let verifier_services = build_production_services(&config).await?;
        verifier_services.health_check_required().await?;

        // 5. PlanCompiler (基线 Profile)
        let plan_compiler = Arc::new(ConformancePlanCompiler::new(config.profiles.clone()));

        // 6. gVisor
        let verification_executor = build_gvisor_executor(&config).await?;
        verification_executor.health_check().await?;

        // 7. Stores
        let assurance_store = build_assurance_store(&config).await?;
        let progress_store = build_progress_store(&config).await?;
        let directive_store = build_directive_store(&config).await?;

        // 8. DecisionEngine — onto-loop crate
        let decision_engine = Arc::new(DecisionEngine::new());

        // 9. TransitionExecutor + LifecycleReducer
        let transition_executor = build_transition_executor(&config).await?;
        let lifecycle_reducer = Arc::new(LifecycleReducer::new());

        Ok(Self { attempt_executor: Arc::new(attempt_executor), candidate_sealer,
            artifact_store, plan_compiler, verification_executor, registry,
            verifier_services, assurance_store, progress_store, directive_store,
            decision_engine, transition_executor: Arc::new(transition_executor),
            lifecycle_reducer })
    }
}
```

**修改文件**: `crates/onto-temporal-adapter/src/loop_runner.rs`

```rust
// 删除 assured: Option<Arc<AssuredAttemptCoordinator>>
// 删除 pub fn with_assured(...)
// 改为:
pub struct RuntimeLoopRunner {
    coordinator: Arc<AssuredAttemptCoordinator>,  // 非 Option
}
```

**标记隔离**: 所有测试替身加 `#[cfg(test)]`。

**验收**:
- [ ] 生产启动时任一条件不满足 → BootstrapError
- [ ] `RuntimeLoopRunner` 无法在无 Coordinator 的情况下构造
- [ ] CI 检查生产二进制不包含 MockLoopRuntime/Dummy/Stub

---

### P16-2: 最小真实规则包

**目标**: 基于基线 Profile 注册 4 个强制核心 Verifier。空 evidence 不产生虚假通过。

**基线生产 Profile**（在 `ConformancePlanCompiler` 配置中）:

```text
artifact.manifest.integrity   Required
file.protected_path           Required
project.build                 Required
project.test                  Required
  execution_dependencies: [project.build]
```

**文件**: `crates/onto-temporal-adapter/src/production.rs`

```rust
fn build_production_registry(config: &ProductionConfig) -> Result<PassRegistry, BootstrapError> {
    let mut registry = PassRegistry::new();

    registry.register("artifact.manifest.integrity", Box::new(
        ArtifactManifestIntegrityVerifier::new()
    ))?;

    registry.register("file.protected_path", Box::new(
        ProtectedPathVerifier::new(config.protected_paths.clone())
    ))?;

    registry.register("project.build", Box::new(
        ProjectBuildVerifier::new(config.toolchain.clone())
    ))?;

    registry.register("project.test", Box::new(
        ProjectTestVerifier::new(config.toolchain.clone())
    ))?;

    Ok(registry)
}
```

**ProjectBuildVerifier — 拒绝空 evidence**:

```rust
impl Verifier for ProjectBuildVerifier {
    fn descriptor(&self) -> &VerifierDescriptor { &self.desc }

    fn external_requirements(&self, _ctx: &VerificationContext) -> Vec<ExternalCheckRequirement> {
        vec![ExternalCheckRequirement {
            requirement_id: "req-build".into(),
            check_id: "project.build".into(),
            execution_dependencies: vec![],
            evidence_kind: EvidenceKind::BuildOutput,
            artifact_scope: ArtifactScope { paths: vec![], include_all: true },
        }]
    }

    async fn evaluate(
        &self, _ctx: &VerificationContext,
        evidence: &[(&String, &RawCheckResult)],
        _services: &VerifierServices<'_>,
    ) -> VerifierResult {
        // 精确查找 project.build 结果 — 不使用 evidence.iter().all()
        let build_result = evidence.iter()
            .find(|(check_id, _)| check_id.as_str() == "project.build");

        let (status, findings) = match build_result {
            None => {
                // 证据缺失 → PartiallyCompleted, 不是 Completed
                (VerifierStatus::PartiallyCompleted, vec![])
            }
            Some((_, r)) => match &r.status {
                CheckExecutionStatus::Exited { exit_code: 0 } => {
                    (VerifierStatus::Completed, vec![])
                }
                CheckExecutionStatus::Exited { exit_code } => {
                    (VerifierStatus::Completed, vec![blocking_finding(
                        "project.build", Pass::Build,
                        format!("Build failed with exit code {}", exit_code),
                    )])
                }
                CheckExecutionStatus::ToolNotFound { .. } => {
                    (VerifierStatus::Unavailable, vec![])
                }
                CheckExecutionStatus::TimedOut => {
                    (VerifierStatus::TimedOut, vec![])
                }
                CheckExecutionStatus::PrerequisiteFailed { .. } => {
                    (VerifierStatus::PrerequisiteFailed, vec![])
                }
                _ => {
                    (VerifierStatus::PartiallyCompleted, vec![])
                }
            },
        };

        VerifierResult {
            verifier_id: "project.build".into(), pass: Pass::Build,
            status, findings, raw_evidence: vec![], diagnostic: None,
        }
    }
}
```

**ProjectTestVerifier** 同样：精确查找 `"project.test"`，不依赖 `.all()`。

**时序约束**:

```text
Agent 写 Staging → 停止写入
  → CandidateSealer::seal(staging_path)
  → 计算 candidate_digest + manifest_digest
  → 写入不可变 SealedArtifactStore
  → 此后任何人不得读 staging

Verifier 执行时:
  → artifact_reader 只接受 SealedCandidateRef
  → 重新校验 candidate_digest + manifest_digest
  → 不接受宿主路径或 staging 路径
```

**验收**:
- [ ] 4 个 Verifier 注册成功，Registry 拒绝重复 ID
- [ ] PlanCompiler 产出基线 Profile 对应的 4 个 Required Unit
- [ ] Verifier 在不可变 `SealedCandidateRef` 上执行
- [ ] Build evidence 缺失 → `VerifierStatus::PartiallyCompleted` (不是 Completed)
- [ ] Build Sandbox 返回 exit_code≠0 → Blocking Finding
- [ ] Build Sandbox ToolNotFound → `VerifierStatus::Unavailable`
- [ ] 空 evidence → PartiallyCompleted, 不产生 `Conformant`
- [ ] Verifier 不调用 `Command::new(...)` — 只通过 gVisor

---

### P16-3: 证据两阶段持久化（消除可见性竞态）

**目标**: Evidence 先写入对象存储, 然后两个 DB 事务消除 Verdict 对外可见但 Evidence 尚未标记 Referenced 的窗口。

**新增文件**: `crates/onto-assurance-runtime/src/assurance_run.rs`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssuranceRunState {
    Created,
    PlanResolved,
    Executing,
    EvidenceStaged,        // Evidence 内容已写入对象存储
    VerdictFinalizing,     // DB事务1 已提交, 但 Verdict 状态=Finalizing, 对外不可见
    Finalized,             // DB事务2 已提交, Verdict→Visible, Outbox→Publishable
    Failed,
}

pub struct AssuranceRunner {
    plan_compiler: ConformancePlanCompiler,
    registry: FrozenVerifierRegistry,
    evidence_assembler: EvidenceAssembler,
    verdict_reducer: VerdictReducer,
    store: Arc<dyn AssuranceRunStore>,
}

impl AssuranceRunner {
    pub async fn execute(
        &self, request: AssuranceRequest,
    ) -> Result<PersistedVerdict, PipelineError> {
        // ① 开启 Run
        let mut run = self.store.begin_run(&request).await?;

        // ② 编译 Plan
        let plan = self.plan_compiler.resolve(&request)?;
        if plan.required_units().is_empty() {
            return Err(PipelineError::InvalidPlan {
                reason: "Plan contains zero required units".into(),
                plan_id: plan.plan_id.clone(),
            });
        }
        self.registry.validate_against(&plan)?;
        self.store.record_plan(&run, &plan).await?;
        // state: PlanResolved

        // ③ 执行 (含 Assurance 重试)
        let unit_results = self.execute_all_units_with_assurance_retry(
            &plan, &request,
        ).await?;
        // state: Executing

        // ④ Phase 1: Evidence → 内容寻址对象
        let evidence_bundle = self.evidence_assembler.assemble(&plan, &unit_results)?;
        self.store.stage_evidence(&run, &evidence_bundle).await?;
        // state: EvidenceStaged

        let evidence_ref = run.evidence_ref();
        let evidence_digest = self.evidence_assembler.digest(&evidence_bundle);

        // ⑤ 归约 Verdict
        let verdict = self.verdict_reducer.reduce(&plan, &unit_results, &evidence_bundle)?;

        // ⑥ Phase 2: DB事务1 — Verdict (status=Finalizing) + Outbox (status=Held)
        self.store.finalize_run_atomic_phase1(
            &run, &plan, &unit_results, &evidence_bundle, &verdict,
        ).await?;
        // state: VerdictFinalizing — Verdict 已写入但对外不可见

        // ⑦ Phase 3: 标记 Evidence 为 Referenced
        self.store.mark_evidence_referenced(&run, &evidence_ref).await?;

        // ⑧ Phase 4: DB事务2 — run→Finalized, verdict→Visible, outbox→Publishable
        self.store.finalize_run_atomic_phase2(&run).await?;
        // state: Finalized

        // ⑨ 后台 GC 以数据库引用为权威: 无引用的对象可被清理

        Ok(PersistedVerdict { verdict, verdict_ref: run.verdict_ref(),
            evidence_bundle_ref: evidence_ref })
    }
}
```

**崩溃恢复**:

```rust
impl AssuranceRunner {
    pub async fn recover(&self, run_id: &str) -> Result<Option<PersistedVerdict>, PipelineError> {
        match self.store.get_run_state(run_id).await? {
            AssuranceRunState::Created | AssuranceRunState::PlanResolved
            | AssuranceRunState::Executing => {
                Ok(None)  // 从头重试
            }
            AssuranceRunState::EvidenceStaged => {
                // 从 Phase 2 继续
                self.resume_from_evidence_staged(run_id).await
            }
            AssuranceRunState::VerdictFinalizing => {
                // DB事务1 已提交, Evidence 可能未标记 Referenced
                // → 补标记 → 执行 DB事务2
                self.recover_from_finalizing(run_id).await
            }
            AssuranceRunState::Finalized => {
                self.store.load_verdict(run_id).await
            }
            AssuranceRunState::Failed => {
                Err(PipelineError::RunFailed { run_id: run_id.to_string() })
            }
        }
    }
}
```

**可见性保证**:

```
EvidenceStaged → 对象存在但无 DB 引用
VerdictFinalizing → DB 有记录但 status=Finalizing, 对外不可见
mark_evidence_referenced() 成功
+ DB事务2 提交 → Verdict::Visible, Outbox::Publishable
  → 此时 Evidence 一定已被标记 Referenced
  → GC 不会删除被引用的 Evidence

任何时刻进程崩溃:
  VerdictFinalizing 但 Evidence 未标记 → 恢复时补标记
  VerdictFinalizing 且 Evidence 已标记 → 恢复时执行 DB事务2
  不存在"Verdict 对外可见但 Evidence 可能被 GC 删除"的窗口
```

**验收**:
- [ ] Evidence 写入对象存储失败 → 无 DB 事务, 无 Verdict
- [ ] DB事务1 失败 → Evidence 标记为 Orphan, GC 可清理
- [ ] DB事务1 成功后、标记 Referenced 前崩溃 → 恢复时补标记
- [ ] 标记 Referenced 后、DB事务2 前崩溃 → 恢复时执行 DB事务2
- [ ] Verdict 在 DB事务2 提交前对外不可见
- [ ] 不再有独立的 `OntoKernelFinalizer` 调用路径

---

### P16-4: 统一 Decision + Transition + Lifecycle

**目标**:
1. **所有** Directive 经过同一条链: persist → apply_transition → lifecycle_reducer.reduce()
2. DecisionEngine 使用 Progress + LoopContext 实际影响决策
3. AssuranceUnavailable 不伪造 verdict digest
4. LifecycleReducer 是唯一的状态归约点

**新增文件**: `crates/onto-loop/src/decision.rs`

```rust
pub struct DecisionEngine;

impl DecisionEngine {
    pub fn decide(
        &self,
        observation: &AttemptObservation,
        progress: &ProgressSnapshot,
        context: &LoopContext,
    ) -> Result<LoopDirective, DecisionError> {
        // ── 进度检查 (在匹配 observation 之前) ──
        // 连续 Unchanged 达到阈值 → Freeze, 不继续
        if context.consecutive_unchanged >= context.max_unchanged_attempts {
            // 即使本次 Conformant, 连续无进展意味着 Agent 在无效循环
            // → Freeze, 不 Finalize
        }

        match observation {
            AttemptObservation::NoCandidate { attempt_id, outcome, .. } => {
                let decision = match outcome {
                    NoCandidateOutcome::AgentFailed => NoCandidateLoopDecision::CloseFailed,
                    NoCandidateOutcome::Cancelled =>
                        NoCandidateLoopDecision::Escalate { reason: "Cancelled".into() },
                    NoCandidateOutcome::RuntimeLost =>
                        NoCandidateLoopDecision::Freeze { reason: "Runtime lost".into() },
                };
                Ok(LoopDirective::AttemptBound {
                    decision_id: uuid::Uuid::new_v4().to_string(),
                    attempt_id: attempt_id.clone(),
                    decision,
                })
            }

            // ── Assurance Unavailable — 使用 PersistedAssuranceFailure ──
            AttemptObservation::CandidateAvailable {
                attempt_id, candidate,
                assurance: AssuranceObservation::Unavailable { reason, diagnostic_ref },
                ..
            } => {
                // 必须先将 failure 落库, 获得 failure_id + failure_digest
                // (此处的伪代码省略了落库步骤, 实现时通过 AssuranceRunner 产出)
                let failure_id = uuid::Uuid::new_v4().to_string();
                let failure_digest = d(&format!("failure-{}", failure_id));

                Ok(LoopDirective::CandidateBound {
                    decision_id: uuid::Uuid::new_v4().to_string(),
                    attempt_id: attempt_id.clone(),
                    candidate_id: candidate.candidate_id.clone(),
                    candidate_digest: candidate.digest.clone(),
                    verdict_id: String::new(),        // 无 Verdict
                    verdict_digest: Digest::empty(),  // 不伪造
                    decision: CandidateLoopDecision::Freeze {
                        reason: format!("Assurance unavailable (failure={}): {}", failure_id, reason),
                    },
                })
            }

            // ── Verdict 存在 ──
            AttemptObservation::CandidateAvailable {
                attempt_id, candidate,
                assurance: AssuranceObservation::Verdict(v),
                ..
            } => {
                match v.conformance {
                    ConformanceOutcome::Inconclusive => {
                        // Assurance retry 已耗尽
                        Ok(candidate_bound(attempt_id, candidate, v,
                            CandidateLoopDecision::Freeze {
                                reason: format!("Inconclusive after retries: coverage={:?}", v.coverage)
                            }))
                    }

                    ConformanceOutcome::NonConformant => {
                        let fixable_count = v.blocking_findings.iter()
                            .filter(|f| matches!(f.remediation,
                                RemediationClass::AutoRepairable | RemediationClass::RetryWithFeedback))
                            .count();
                        let unfixable_count = v.blocking_findings.len() - fixable_count;

                        if unfixable_count > 0 {
                            Ok(candidate_bound(attempt_id, candidate, v,
                                CandidateLoopDecision::Escalate {
                                    reason: format!("{} unfixable, {} fixable",
                                        unfixable_count, fixable_count)
                                }))
                        } else {
                            let feedback: Vec<String> = v.blocking_findings.iter()
                                .map(|f| f.message.clone()).collect();

                            // Progress: 检查是否与上轮相比有改善
                            if let Some(prev) = &progress.prev_blocking_keys {
                                let new_keys: Vec<String> = v.blocking_findings.iter()
                                    .map(|f| f.fingerprint.rule_id.clone()).collect();
                                let regression = prev.iter().any(|k| !new_keys.contains(k))
                                    && new_keys.len() > prev.len();
                                if regression {
                                    // 新 Finding 出现 + 旧 Finding 未消除 → Regressed
                                    // 允许一次 Continue, 但记录
                                }
                            }

                            Ok(candidate_bound(attempt_id, candidate, v,
                                CandidateLoopDecision::Continue { feedback }))
                        }
                    }

                    ConformanceOutcome::Conformant => {
                        // 全量检查
                        if !matches!(v.freshness, FreshnessState::Current)
                            || !matches!(v.coverage, CoverageState::Complete)
                            || !v.sandbox.is_executed_and_completed()
                            || !v.all_required_units_have_determinate_result()
                        {
                            return Ok(candidate_bound(attempt_id, candidate, v,
                                CandidateLoopDecision::Freeze {
                                    reason: "Conformant but incomplete evidence".into()
                                }));
                        }

                        // Progress: 首轮 Conformant → Finalize
                        // 多轮后 Conformant → Finalize (合理的收敛)
                        Ok(candidate_bound(attempt_id, candidate, v,
                            CandidateLoopDecision::FinalizeCandidate))
                    }
                }
            }
        }
    }
}

fn candidate_bound(
    attempt_id: &str, candidate: &SealedCandidateRef,
    v: &ConformanceVerdict, decision: CandidateLoopDecision,
) -> LoopDirective {
    LoopDirective::CandidateBound {
        decision_id: uuid::Uuid::new_v4().to_string(),
        attempt_id: attempt_id.to_string(),
        candidate_id: candidate.candidate_id.clone(),
        candidate_digest: candidate.digest.clone(),
        verdict_id: v.verdict_id.clone(),
        verdict_digest: v.verdict_digest.clone(),
        decision,
    }
}
```

**新增文件**: `crates/onto-temporal-adapter/src/lifecycle_reducer.rs`

```rust
/// 唯一的状态归约点。将所有 Directive + TransitionReceipt 映射为 Lifecycle。
pub struct LifecycleReducer;

#[derive(Debug)]
pub enum Lifecycle {
    /// 继续下一轮 Attempt (非终态)
    Continue,
    /// 终态: 已完成
    Committed { receipt_ref: String },
    /// 终态: 已失败
    Failed { reason: String },
    /// 终态: 已冻结
    Frozen { reason: String },
    /// 终态: 已升级
    Escalated { reason: String },
}

impl LifecycleReducer {
    pub fn reduce(
        &self,
        directive: &LoopDirective,
        receipt: &TransitionReceipt,
    ) -> Result<Lifecycle, LifecycleError> {
        match &receipt.outcome {
            TransitionOutcome::Finalized { settlement, receipt_ref } => {
                match settlement {
                    SettlementState::Committed => {
                        Ok(Lifecycle::Committed { receipt_ref: receipt_ref.clone() })
                    }
                    SettlementState::RolledBack => {
                        Ok(Lifecycle::Failed {
                            reason: format!("RolledBack: receipt={}", receipt_ref)
                        })
                    }
                    SettlementState::Frozen => {
                        Ok(Lifecycle::Frozen {
                            reason: format!("Frozen by TransitionExecutor: receipt={}", receipt_ref)
                        })
                    }
                    SettlementState::Unknown => {
                        Ok(Lifecycle::Escalated {
                            reason: format!("Unknown settlement: receipt={}", receipt_ref)
                        })
                    }
                }
            }
            TransitionOutcome::Continued => Ok(Lifecycle::Continue),
            TransitionOutcome::Frozen => {
                Ok(Lifecycle::Frozen { reason: "Frozen by TransitionExecutor".into() })
            }
            TransitionOutcome::Escalated => {
                Ok(Lifecycle::Escalated { reason: "Escalated by TransitionExecutor".into() })
            }
            TransitionOutcome::AttemptClosed => {
                Ok(Lifecycle::Failed { reason: "AttemptClosed".into() })
            }
            TransitionOutcome::CandidateDiscarded => {
                Ok(Lifecycle::Continue) // Discard → 可以重试新 Candidate
            }
            TransitionOutcome::CheckpointRestored { .. } => {
                Ok(Lifecycle::Continue) // Restore → 回到检查点, 可以重试
            }
        }
    }
}
```

**修改文件**: `crates/onto-temporal-adapter/src/loop_runner.rs`

**所有 Directive 走同一条链**:

```rust
// ⑤ DecisionEngine 产出 LoopDirective
let directive = decision_engine.decide(&observation, &progress_snapshot, &loop_context)?;

// ⑥ ALL Directive → persist
let persisted = directive_store.persist_if_absent(&directive).await?;

// ⑦ ALL Directive → apply_transition
let receipt = transition_executor.apply_transition(&handle, &persisted).await?;

// ⑧ ALL Directive → LifecycleReducer.reduce() (唯一归约点)
let lifecycle = lifecycle_reducer.reduce(&persisted, &receipt)?;

// ⑨ RuntimeLoopRunner 只根据 Lifecycle 结果行动
match lifecycle {
    Lifecycle::Continue => {
        // 更新进度存储
        progress_store.record(
            &request.loop_id, total_attempts,
            &observation, &progress_snapshot,
        );
        // 循环回去, 不产生 Terminal Envelope
    }
    Lifecycle::Committed { receipt_ref } => {
        let envelope = build_envelope(&request, LoopTerminalState::Committed,
            Some(decision_id_str), Some(decision_hash), Some(output_hash),
            total_attempts,
            format!("Committed: receipt={}", receipt_ref),
        );
        self.finalize_terminal(&loop_id, &envelope);
        return Ok(envelope);
    }
    Lifecycle::Failed { reason } => {
        let envelope = build_envelope(&request, LoopTerminalState::Escalated,
            Some(decision_id_str), None, Some(output_hash),
            total_attempts, reason,
        );
        self.finalize_terminal(&loop_id, &envelope);
        return Ok(envelope);
    }
    Lifecycle::Frozen { reason } => {
        let envelope = build_envelope(&request, LoopTerminalState::Escalated,
            Some(decision_id_str), None, Some(output_hash),
            total_attempts, reason,
        );
        self.finalize_terminal(&loop_id, &envelope);
        return Ok(envelope);
    }
    Lifecycle::Escalated { reason } => {
        let envelope = build_envelope(&request, LoopTerminalState::Escalated,
            Some(decision_id_str), None, Some(output_hash),
            total_attempts, reason,
        );
        self.finalize_terminal(&loop_id, &envelope);
        return Ok(envelope);
    }
}
```

**进度记录**（在每轮结束时，循环回去前）:

```rust
// 本轮结束后, 记录进度供下一轮 DecisionEngine 使用
progress_store.record(
    &request.loop_id,
    total_attempts,
    &observation,
    &ProgressSnapshot {
        blocking_keys: verdict.as_ref().map(|v|
            v.blocking_findings.iter()
                .map(|f| f.fingerprint.rule_id.clone())
                .collect()
        ),
        prev_blocking_keys: progress_snapshot.blocking_keys.clone(),
        consecutive_unchanged: if progress_comparison == ProgressComparison::Unchanged {
            loop_context.consecutive_unchanged + 1
        } else { 0 },
        ..progress_snapshot
    },
);
```

**验证**:
- [ ] `Freeze → persist → apply_transition → TransitionOutcome::Frozen → Lifecycle::Frozen`
- [ ] `Escalate → persist → apply_transition → TransitionOutcome::Escalated → Lifecycle::Escalated`
- [ ] `Continue → persist → apply_transition → TransitionOutcome::Continued → Lifecycle::Continue`
- [ ] `FinalizeCandidate → persist → apply_transition → Finalized(Committed) → Lifecycle::Committed`
- [ ] `AttemptBound(CloseFailed) → persist → apply_transition → AttemptClosed → Lifecycle::Failed`
- [ ] 任何 Directive 不绕过 persist + apply_transition + lifecycle_reducer.reduce()
- [ ] `RuntimeLoopRunner` 不自行映射状态
- [ ] AssuranceUnavailable 先落库 PersistedAssuranceFailure, 获得 failure_id+failure_digest
- [ ] `verdict_id: String::new()` 不出现在生产路径
- [ ] 连续 Unchanged≥阈值 → Freeze
- [ ] Progress 记录在每轮结束时更新, 下一轮 DecisionEngine 可读取
- [ ] 旧 `evaluate_required` / `evaluate_with_verdict` / `VerdictInfo` 全部删除

---

### P16-5: 迁移 Graph Verifier

**三步迁移**:

```
P16-5A: 通过 Legacy Adapter 接入生产 Pipeline
  → 验证旧 Verifier 在新流水线中行为正确
  → 不删除旧 trait

P16-5B: GraphIntegrityVerifier 原生实现新 Verifier trait
        GraphRiskVerifier 原生实现新 Verifier trait
  → 行为与 Legacy Adapter 完全一致

P16-5C: Registry 切换到原生实现 → 删除 Legacy Adapter

P16-6:  删除 DeterministicVerifierPort trait + 旧 Coordinator + 旧 EvidenceBuilder 链
```

**P16-5A**: Legacy Adapter 的错误映射必须用真实的 `VerifierStatus` 变体:

```rust
// 旧错误 → VerifierStatus 映射 (VerifierStatus 只有6个变体)
let status = match &e {
    VerifierError::Unavailable(_) => VerifierStatus::Unavailable,
    VerifierError::EnvironmentError(_) => VerifierStatus::Unavailable,
    VerifierError::StaleSnapshot(_) => VerifierStatus::PartiallyCompleted,
    VerifierError::VerificationIncomplete(_) => VerifierStatus::PartiallyCompleted,
    VerifierError::Timeout(_) => VerifierStatus::TimedOut,
    VerifierError::BudgetExhausted(_) => VerifierStatus::PartiallyCompleted,
    // 无法归类的执行错误 → Unavailable (不是不存在的 Failed 变体)
    _ => VerifierStatus::Unavailable,
};
```

**验收**:
- [ ] Adapter 使用的 VerifierStatus 变体全部在冻结枚举中存在
- [ ] 图谱不可用 → `VerifierStatus::Unavailable` → Coverage Partial → Inconclusive
- [ ] P16-6 删除 `DeterministicVerifierPort` trait

---

### P16-6: 删除重复和旧代码

**删除前验证**:
- [ ] `DeterministicVerifierPort` trait 无任何 impl
- [ ] Legacy Adapter 已从 Registry 移除

**删除清单**:

| 文件/模块 | 行数 | 原因 |
|-----------|:----:|------|
| `crates/onto-ironclaw-adapter/src/finalizer.rs` | 714 | `OntoKernelFinalizer` + InMemory stores |
| `crates/onto-ironclaw-adapter/src/run_finalization_adapter.rs` | 246 | 两个重复 RunFinalizationPort 实现 |
| `crates/onto-ironclaw-adapter/src/finalization_adapter.rs` | 75 | `StubFinalizationAdapter` — 硬编码 `overall_passed: true` |
| `crates/onto-ironclaw-adapter/src/loop_adapter.rs` 的 `evaluate_*` + `VerdictInfo` | ~40 | 被 DecisionEngine 取代 |
| `crates/onto-assurance-runtime/src/ports.rs` 的 `RunFinalizationPort` trait + 关联类型 | ~60 | 被 AssuranceRunner + PersistedVerdict 取代 |
| `crates/onto-assurance-runtime/src/verification/ports.rs` 的 `DeterministicVerifierPort` + `VerifierError`(8变体) | ~30 | 被 `Verifier` trait 取代 |
| `crates/onto-graph-verifiers/src/legacy_adapter.rs` | ~100 | P16-5C 已切换 |
| `crates/onto-temporal-adapter/src/loop_runner.rs` 旧 evaluate 逻辑 + `Commit→Escalated` | ~50 | 被 LifecycleReducer 取代 |
| `crates/onto-assurance-runtime/src/pipeline.rs` `PassRegistry` 旧 `(Pass,String)` 查找 | ~10 | 被 `by verifier_id` 取代 |
| `crates/onto-temporal-adapter/src/assured_coordinator.rs` `dummy_services()` | ~10 | 被 ProductionVerifierServices 取代 |
| `crates/onto-temporal-adapter/src/assured_loop_runner.rs` | ~177 | 被 AssuranceRunner 取代 |

**系统唯一的组件**:

```
1 个 Verifier trait        (onto-protocol/src/verifier.rs)
1 个 DecisionEngine        (onto-loop/src/decision.rs)           ← L3
1 个 TransitionExecutor    (onto-ironclaw-adapter)              ← L2
1 个 LifecycleReducer      (onto-temporal-adapter)
1 个 AssuranceRunner       (onto-assurance-runtime)             ← L1
1 个 Composition Root      (ProductionComponents)
```

---

## 四、生产验收门槛（34 项）

```
□  1. 生产启动时空 Registry → BootstrapError, 不启动
□  2. 生产 Profile 声明的 Verifier 未注册 → BootstrapError
□  3. Registry 重复 verifier_id → BootstrapError
□  4. 零 Required Unit → PipelineError::InvalidPlan
□  5. 生产二进制不包含 MockLoopRuntime
□  6. Verifier 读取 SealedArtifactStore, 不是 staging
□  7. Graph/Semantic Required 但不可用 → Unavailable → Inconclusive
□  8. Evidence 写入对象存储失败 → 无 DB 事务, 无 Verdict
□  9. DB事务1 提交后崩溃 → 恢复时补标记 Evidence + DB事务2
□ 10. Verdict 在 DB事务2 提交前对外不可见
□ 11. Agent 自报 Success + Build 失败 → NonConformant → 不 Finalize
□ 12. Agent 自报 Success + 空 Plan → PipelineError → 不产 Verdict
□ 13. Conformant 但 Coverage Partial → Freeze
□ 14. Conformant 但 Sandbox 不完整 → Freeze
□ 15. Build evidence 缺失 → VerifierStatus::PartiallyCompleted
□ 16. Build evidence 为空 → [].all() 不被调用
□ 17. Optional Unit 未注册 → NotApplicable, 不 panic
□ 18. TransitionExecutor 验证 digest 不匹配 → 拒绝
□ 19. 相同 decision_id+digest → persist_if_absent 返回已持久化 (幂等)
□ 20. 相同 decision_id+不同 digest → IdempotencyConflict
□ 21. FinalizeCandidate+Committed → Lifecycle::Committed → Terminal::Committed
□ 22. Continue → Lifecycle::Continue → 不返回 Terminal Envelope
□ 23. Freeze → persist → apply_transition → Transition::Frozen → Lifecycle::Frozen
□ 24. Escalate → persist → apply_transition → Transition::Escalated → Lifecycle::Escalated
□ 25. AttemptBound(CloseFailed) → persist → apply_transition → AttemptClosed → Lifecycle::Failed
□ 26. 任何 Directive 不绕过 persist + apply_transition + lifecycle_reducer.reduce()
□ 27. RuntimeLoopRunner 不自行映射状态
□ 28. AssuranceUnavailable 先落库 PersistedAssuranceFailure, 不伪造 verdict digest
□ 29. 连续 Unchanged≥阈值 → Freeze (Progress 实际影响决策)
□ 30. Go 侧不能覆盖 Rust 侧权威结果
□ 31. 进程在 Directive 持久化后崩溃 → 恢复时从 directive_store 重放
□ 32. 全生产入口 E2E 经过真实 Runtime、Pipeline、Store、Decision、Transition、LifecycleReducer
□ 33. 全仓只剩一个 Verifier trait、一个 DecisionEngine、一个 TransitionExecutor、一个 LifecycleReducer
□ 34. 旧 `RunFinalizationPort` / `DeterministicVerifierPort` / `OntoKernelFinalizer` / `dummy_services` / `VerdictInfo` / `LoopDecision::Commit` 全部不存在
```

---

## 五、不在范围内的

```
❌ 大规模 WebUI
❌ 分布式状态机
❌ 全部未来 Industry Pack (Ops/Data/Workflow)
❌ CompileRepair 进入通用 Kernel
❌ HTTP 产品入口
❌ 新的 Verifier 类型
❌ 性能优化
```

---

## 六、阶段时间估算

| 阶段 | 工时估计 | 依赖 |
|------|:--------:|------|
| P16-0 锁死虚假成功 | 1-2d | 无 |
| P16-1 生产 Composition Root | 3-5d | P16-0 |
| P16-2 最小真实规则包 + 基线Profile | 2-3d | P16-1 |
| P16-3 证据两阶段持久化 | 3-5d | P16-2 |
| P16-4 统一 Decision + Transition + Lifecycle | 4-6d | P16-3 |
| P16-5A Legacy Adapter 接入 | 1-2d | P16-4 |
| P16-5B Graph Verifier 原生实现 | 1-2d | P16-5A |
| P16-5C Registry 切换 | 0.5-1d | P16-5B |
| P16-6 删除重复 | 1-2d | P16-5C |
| **总计** | **16.5-28d** | |

---

> **最终结论**: 修复不是"接上线"——是建立唯一生产 Composition Root, 按 verifier_id 的 Plan 驱动 (基线Profile), 拒绝空注册表和重复ID, 拒绝空 evidence 恒真, 真实 SealedArtifactStore, 真实 Services 通过 gVisor, 统一 Verifier 协议, Evidence 两阶段持久化消除竞态, **所有 Directive 经过 persist → apply_transition → lifecycle_reducer.reduce() 同一条链**, Progress 实际影响决策, AssuranceUnavailable 不伪造 digest, LifecycleReducer 是唯一状态归约点, 删除全部重复和旁路。
>
> **P16 完成以前, 系统没有真正的验证。**
