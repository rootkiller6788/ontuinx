# OntoOS 生产就绪度评估

**评估日期**: 2026-07-25
**评估范围**: Onto Assurance Kernel + OntoRuntime 集成 + Code Pack

---

## 核心原则

**不要把四种完全不同的完成度混成一个百分比：**

- 代码迁移完成度
- 核心协议成熟度
- OntoRuntime 运行时接通度
- 完整多行业产品完成度

以下按维度分别评估。

---

## 一、分层完成度

| 评估对象 | 完成度 | 说明 |
|---------|--------|------|
| Onto Assurance Kernel 纯函数库 | **85%–90%** | Types/Core 已冻结，纯函数测试覆盖好，可独立使用 |
| Onto Assurance Runtime 协调层 | **75%–80%** | 8 个 Port trait 定义清晰，TransactionCoordinator 完整 |
| OntoRuntime 编译期接入 | **65%–70%** | BuiltinBridge/RunFinalizationPort/AfterLoopExit 已编译链接 |
| OntoRuntime 真实运行时接入 | **20%–30%** | 已编译的 Hook 尚未被真实 Agent Loop 触发 |
| Code Pack 最小产品 | **50%–60%** | Build/Test/Lint verifier + Profiler 有基础实现 |
| 完整多行业 OntoOS | **20%–30%** | 仅 Code Pack 存在，Ops/Data/Workflow/Robotics/Industrial 均为空 |
| **真正生产就绪度** | **25%–35%** | 缺持久化、恢复、幂等、并发隔离、故障注入、审计、密钥管理等 |

> 工程原型完成度约 **55%–60%**；生产就绪度约 **25%–35%**。两者不可混用。

---

## 二、各层详细评估

### 2.1 onto-assurance-types（1,090 行，95%）

- 14 个 Typed ID（UUIDv7 newtype）
- 18 个 Enum：TaskOutcome / BudgetOutcome / LifecycleState（三维独立）、EffectClass（6 级单调升级）、TransactionState（18 状态）、ExitReason / RiskLevel / FailureKind / CriterionStatus 等
- ExecutionContract / ExecutionIntent / EffectClassification / ResourceScope
- EvidenceRecord / EvidenceBundle / ChainedRecord / VerifierBinding
- SessionResult / SettlementDecision / RequirementVerdict
- Schema v1 已冻结，serde 兼容

**仍缺**：版本迁移辅助、更多 Schema 验证用例。

### 2.2 onto-assurance-core（2,168 行，88%）

9 个纯函数模块：

| 模块 | 完成度 | 说明 |
|------|--------|------|
| `canonical` | 90% | 跨语言确定性 JSON + SHA-256，域分离 |
| `evidence_chain` | 90% | 7 个不变量全覆盖，append/seal/verify |
| `reduction` | 85% | 证据→标准映射，阻塞/非阻塞分离 |
| `session_decision` | 90% | ExitReason + Verdict → 三维结果 |
| `settlement` | 85% | EffectClass × TaskOutcome → Publish/Discard/Freeze/Compensate/Escalate |
| `effect_classifier` | 85% | RulesBasedClassifier + trait，未知→Irreversible |
| `replay` | 75% | 确定性复现，基础实现 |
| `checkpoint` | 70% | 检查点绑定，持久化路径未验证 |
| `invalidation` | 70% | 验证器版本失效，基础实现 |

**仍缺**：EvidenceIntegrator 完善、更多故障注入、CheckpointBinding 真实持久化、版本迁移。

### 2.3 onto-assurance-runtime（991 行，80%）

8 个 Port trait：
- AuthorizationPort / ApprovalPort / RuntimePort / VerifierPort
- EvidenceStorePort / EventSinkPort / CheckpointPort / ClockPort

TransactionCoordinator 完整 16 步生命周期：
```
Classify → Authorize → Pre-exec Gate → Prepare → Stage → Execute
→ Capture → Verify → Build Evidence → Chain → Reduce → Session
→ Settlement → Apply → Checkpoint → Store
```

**仍缺**：错误恢复路径、重试策略、超时处理。

### 2.4 onto-ironclaw-adapter（1,266 行，35%）

#### 已完成
- **BuiltinBridge**（507 行）：ShadowObserver + BuiltinGate 完整实现，包含单调收紧逻辑和测试
- **RunFinalizationPort**（244 行）：OntoRunFinalizationAdapter 完整实现，包含 DecisionStore + EvidenceStore 集成，fault injection 测试通过

#### 存根（M6 或后期需要）
- **checkpoint_adapter**（15 行）：`Ok("stub".into())`
- **event_adapter**（15 行）：空实现
- **verifier_adapter**（16 行）：空实现

#### 部分实现
- **auth_adapter**（147 行）：有 feature-gated OntoRuntime 版本和独立存根
- **runtime_adapter**（104 行）：同上
- **approval_adapter**（43 行）：存根实现
- **finalization_adapter**（75 行）：存根实现

#### 应按阶段分类评估

**M5 关键 Adapter（不允许存根）**：
| Adapter | 当前状态 |
|---------|---------|
| DecisionStoreAdapter | 已实现 |
| EvidenceStoreAdapter | 部分 |
| RequirementStoreAdapter | 存根 |
| ArtifactStoreAdapter | 存根 |
| RunStateAdapter | 存根 |
| FinalizationAdapter | 存根 |

**M6 Adapter（可以存根）**：RuntimeLaneAdapter、EffectPublishAdapter、SettlementAdapter、CompensationAdapter

**可选 Adapter（不阻塞 M5）**：ApprovalAdapter、OntoFlowAdapter、Industry Pack Adapter

### 2.5 onto-code-pack（755 行，55%）

- ProjectProfiler：语言/构建系统/测试框架/包管理器自动检测
- BuildVerifier / TestVerifier / LintVerifier：基础实现
- ArtifactCollector：源文件/二进制/日志哈希清单
- E2E 测试存在

**仍缺**：更多语言支持（当前以 C/make 为主）、复杂构建系统、性能基准验证。

### 2.6 测试基础设施（70%）

- 10 个 Golden Fixture：success / verification_failed / budget_depleted_success / incomplete / environment_error / evidence_tampered / checkpoint_mismatch / commit_failed / cross_attempt_pollution / replay_mismatch
- 单元测试覆盖核心模块
- onto-integration-test：ShadowObserver → BuiltinGate → CodePack → Pipeline 全链路
- onto-conformance：6 个 golden fixture 对比
- ontotest/runner.py：OntoRuntime 启动→Agent 任务→Hook 验证→Evidence 完整性
- test_onto_assurance.sh：集成对比测试脚本

---

## 三、M4/M5/M6 真实状态

### M4 Shadow Mode
- [x] 基础 Hook 注册和 Bridge 代码
- [ ] Shadow Decision 链真实运行
- [ ] 与真实 Agent 任务差分验证

### M5 Success Authority
- [x] AfterLoopExit hook 编译进 OntoRuntime 二进制
- [x] RunFinalizationPort 实现和单元测试
- [x] Executor/Composition 接线编译通过
- [ ] **真实 Agent → Finalization 链路（当前瓶颈）**

### M6 Effect Settlement
- [x] EffectClass 模型
- [x] Pre/Settlement 语义定义
- [x] TransactionCoordinator 基础实现
- [ ] Runtime Lane 发布时间线证明
- [ ] Publish 边界和 Crash Reconciliation
- [ ] Irreversible 协议 E2E

---

## 四、真正断裂的位置

**不是** "OntoRuntime 没有给 Onto 提供 seam"。

**而是**：OntoRuntime 当前 HTTP 产品入口的存储后端未完成，导致无法启动真实 Agent Session，已经接好的 seam 拿不到真实运行数据。

```
HTTP / Product 入口          ❌ 存储后端阻断
        ↓
真实 Agent Loop              未启动
        ↓
AfterLoopExit                未真实触发
        ↓
RunFinalizationPort          未获得真实上下文
        ↓
真实 Evidence 加载           未验证
        ↓
Onto Kernel 归约             仅在单元测试中验证
        ↓
SessionDecision 持久化       未真实产生
        ↓
OntoRuntime RunState 更新       未验证
```

---

## 五、当前里程碑范围

**M5 Code Assurance E2E**：

| 包含 | 不包含 |
|------|--------|
| Assurance Kernel | Ops/Data/Workflow/Robotics/Industrial Pack |
| OntoRuntime Finalization | M6 副作用事务接管 |
| Code Pack | 分布式状态机 |
| Evidence → Decision → Replay | HTTP 产品入口 |

未来 Pack 权重在当前里程碑中应为 0。

---

## 六、下一步：DirectRunAdapter

不继续等 HTTP 入口。增加 `DirectRunAdapter`，绕过未完成的产品入口，直连 `RebornTurnRunExecutor`：

```
ontotest
  → DirectRunAdapter
  → RebornTurnRunExecutor
  → Agent Loop (真实 LLM + 真实工具调用)
  → AfterLoopExit
  → RunFinalizationPort
  → Onto Kernel (Reduction → SessionDecision → Settlement)
```

必须走真实 Executor，不能直接 Mock LoopExit。

### 最小接口

```rust
pub trait DirectRunPort {
    async fn create_run(&self, request: DirectRunRequest) -> Result<RunId, DirectRunError>;
    async fn await_terminal(&self, run_id: RunId) -> Result<RunFinalizationOutcome, DirectRunError>;
}
```

---

## 七、M5 验收标准

### 5 个必须场景

| # | 场景 | 预期 |
|---|------|------|
| 1 | Agent 主动 finish | SUCCESS + COMMITTED |
| 2 | Agent 自然停止，不调用 finish | SUCCESS + COMMITTED |
| 3 | 预算耗尽但任务完成 | SUCCESS + DEPLETED + COMMITTED |
| 4 | 需求缺失 | INCOMPLETE + CONTINUING |
| 5 | Evidence 持久化失败 | 不得 SUCCESS，ESCALATED |

### 每条必须验证

- [ ] 真实 LLM 调用（非 Mock）
- [ ] 真实 Agent Loop（走 RebornTurnRunExecutor）
- [ ] AfterLoopExit 恰好触发一次
- [ ] Finalization 幂等（重复调用不产生副作用）
- [ ] Evidence 来自真实 Artifact
- [ ] Decision 只持久化一次
- [ ] OntoRuntime 不能自行 Complete（Onto 是唯一的 Success 生产者）
- [ ] Replay 结果一致

### 封口条件

```
Direct E2E                    5/5
错误 SUCCESS                  0
错误 COMMITTED                0
错误 CONTINUING               0
重复 Finalization             0
Decision 持久化失败误完成      0
真实 Replay 不一致            0
```

---

## 八、M5 之后才需要的生产特性

以下特性即使 M5 全部通过也不能缺，否则不能称为生产就绪：

- 关键 Adapter 全部非存根
- 真实持久化（非内存 Mock）
- 进程重启恢复
- 幂等 Finalization
- 并发 Run 隔离
- 故障注入和恢复测试
- 版本迁移
- 指标和审计日志
- 配置 fail-fast
- 密钥管理
- 权限审计
- 依赖供应链扫描
- 性能与资源上限
- M6 Runtime Lane 证明（副作用门禁）

---

## 九、不应该现在做的事

- ❌ 原 Python 15 步 ToolRuntime 逐步翻译
- ❌ 分布式状态机
- ❌ 全部未来 Industry Pack（Ops/Data/Workflow/Robotics/Industrial）
- ❌ CompileRepair 进入通用 Kernel
- ❌ 大规模 WebUI 和产品后台
- ❌ 为提高代码量而扩 Adapter

**当前唯一主线：DirectRunAdapter → M5 真实闭环 → 5 场景验收。**
