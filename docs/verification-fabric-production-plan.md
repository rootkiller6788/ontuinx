# OntoAssure Verification Fabric — 生产就绪计划

**日期**: 2026-07-27
**状态**: 架构完成 (82%)，生产语义待封口

---

## 1. 当前准确口径

```
Verification Fabric Architecture       ✅ COMPLETE
14-Step Functional Pipeline            ✅ COMPLETE
OCR Semantic Rule System               ✅ INTEGRATED
Mock/Single-Node Validation            ✅ COMPLETE

Production Verification Semantics      ⏳
Real IronClaw Trusted Loop             ⏳
Multi-Process Runtime Acceptance       ⏳
Non-Code Domain Verification           ⏳ (stubs only)
```

**可以宣布：**

```text
Verification Fabric 14步控制流          ✅ 已贯通
OCR式多语言语义规则路由                 ✅ 已吸收
Rust类型/Port/Adapter边界               ✅ 已建立
Mock与单节点功能测试                    ✅ 已完成
```

**不能宣布：**

```text
生产级 Verification Fabric              ❌
真实可信 Agent 验证闭环                 ❌
通用非代码领域验证                     ❌
分布式故障下安全不变量                  ❌
```

---

## 2. 代码现状

| 层 | 代码量 | 关键文件 |
|----|--------|---------|
| onto-assurance-types | 271行 | verification_target, scope_manifest, finding, language_profile, evidence_location (158行) |
| onto-assurance-core | 250行 | location_binding (双通道), criterion_mapping, scope_coverage, rule_router |
| onto-assurance-runtime | 111行 | coordinator (30行), planner, scheduler (4行 trait), session, budget, ports |
| onto-code-pack | 1,567行 | router.rs (221行, OCR移植), diff_index, source_index, build_verifier, lint_verifier, ironclaw_semantic, generic_llm, data_pack, document_pack, workflow_pack |
| onto-ironclaw-adapter (P2) | 413行 | attempt_run_port (234行), lane_isolation (114行), rollout_stage (49行), semantic_verifier_adapter (19行) |
| 规则 Markdown | 946行 | 26个 rule_docs/*.md (25个移植OCR + 1个自建go.md) |
| 测试 | 2,302行 | vf_pipeline, e2e_verification_chain, coordinator_integration, cross_language_cycle, phase34_acceptance, m6_effect_authority, m6_cd_external_effects 等14个测试文件 |

---

## 3. 14步流程状态

| 步骤 | 名称 | 性质 | 文件 | 状态 |
|------|------|------|------|------|
| 1 | Discovery | 领域相关 | diff.rs, repository_scan.rs | ✅ |
| 2 | Scope Resolution | 纯函数 | scope_coverage.rs | ✅ |
| 3 | Rule Routing | 纯函数 | router.rs (221行, 26语言) | ✅ |
| 4 | Planning | 纯函数+策略 | planner.rs | ✅ |
| 5 | Session Check | 状态相关 | session.rs | ✅ |
| 6 | Verifier Scheduling | 异步编排 | scheduler.rs + coordinator.rs | ✅ |
| 7 | Location Resolution | 纯函数 | location_binding.rs + diff_index.rs + source_index.rs | ✅ 双通道 |
| 8 | Deduplication | 纯函数 | finding_normalization.rs | ✅ |
| 9 | Cross-Verification | 纯函数 | finding_normalization.rs | ✅ |
| 10 | Criterion Mapping | 纯函数 | criterion_mapping.rs | ✅ |
| 11 | Evidence Building | 状态相关 | evidence_builder.rs | ✅ |
| 12 | Reduction | 已有纯函数 | onto-assurance-core | ✅ 不变 |
| 13 | Session Decision | 已有纯函数 | onto-assurance-core | ✅ 不变 |
| 14 | Settlement | 已有纯函数 | onto-assurance-core | ✅ 不变 |

**14/14 全部贯通。**

---

## 4. OCR 吸收状态

| OCR 设计 | 吸收方式 | 状态 |
|----------|---------|------|
| system_rules.json + glob 路由 | router.rs (221行) + system_rules.json (27种类型) | ✅ |
| expandBraces 大括号展开 | router.rs expand_braces() | ✅ |
| 26个 Markdown 规则文件 | rule_docs/*.md, include_str! 编译时嵌入 | ✅ |
| 新增 go.md (OCR 没有) | 含 Temporal/OntoFlow 专用规则 | ✅ |
| {{system_rule}} 模板注入 | 语义 Verifier prompt 构建 | ✅ |
| 双通道位置解析 | core/location_binding + diff_index + source_index | ✅ |
| 4层优先级 Resolver | 未移植 (当前单层) | 🔶 |
| merge_system_rule | 未移植 | 🔶 |
| FileFilter include/exclude | ScopeManifest.excluded_targets | ✅ |
| Session 指纹恢复 | VerificationSession | ✅ |
| --preview 模式 | 未实现 | 🔶 |

---

## 5. P2 交付物质量

| 文件 | 行数 | 测试 | 评估 |
|------|------|------|------|
| attempt_run_port.rs | 234 | 3 | ✅ 1 Attempt = 1 Run, AlreadyExecuted 正确 |
| lane_isolation.rs | 114 | 6 | ✅ Lane A/B 权限矩阵清晰, LaneViolation 消息好 |
| rollout_stage.rs | 49 | 4 | ✅ Shadow→Gated→Enforced, 默认 Shadow |

**架构质量**: ★★★★☆ 清晰、可测试、不变量明确
**生产就绪**: ★★★☆☆ LaneGuard 未接入 tool dispatch, 真实 Adapter 未实现

---

## 6. 剩余工作分级

### P0: 封口前必须做 (8项)

```
1. 确认编排控制权仍归 onto-assurance-runtime
   现状: coordinator 仍拥有循环, scheduler 是 trait
   风险: coordinator 30行, 需确认 "逻辑内聚" 不等于 "控制权下沉"
   验证: coordinator.execute() 拥有 plan→session→loop→save 全流程

2. 完整 VerifierRunResult 与 Completeness 模型
   缺口: scheduler 返回 Vec<FindingCandidate>, 无法区分 Pass vs ToolUnavailable
   需要: VerifierRunResult { execution_status, verdict, findings, environment_hash... }
   需要: VerificationCompleteness { scope_coverage, rule_coverage, verifier_coverage... }
   根不变量: 0 Findings + Completed ≠ 0 Findings + ToolUnavailable

3. 零静默遗漏的 Scope Ledger
   缺口: 循环结束直接标记 Completed, 无覆盖率检查
   需要: 每个 target 的处置记录 (verified / excluded_with_reason / failed / skipped)
   不变量: verified + excluded + failed + skipped = all_targets

4. LaneGuard 接入所有真实 tool dispatch
   缺口: LaneGuard 是值对象, 未被任何 dispatch 路径调用
   需要: IronClaw 每个写操作前调用 guard_write()
   需要: 绕过测试 (shell, MCP, git, filesystem API, symlink)

5. 真实 IronClaw Agent Loop
   缺口: StubAttemptRunPort 用于测试, 真实 HTTP/进程入口未完成
   需要: Attempt → IronClaw Run → Agent Loop → Candidate Checkpoint
   黄金链: Attempt1 失败 → Structured Continuation → Attempt2 成功 → Publish 仅一次

6. Session/预算真实持久化和恢复
   缺口: InMemorySessionManager, BudgetTracker 简单计数器
   需要: PG 持久化, 恢复时绑定验证 (contract/rule/verifier hash)
   需要: 多维度预算 (token/cost/time/concurrency/semantic_calls)

7. 规则 Bundle Hash 与机器 Verification Profile
   缺口: 26个.md 嵌入但没有 content hash, 规则版本变化不触发失效
   需要: RulePack.content_hash, 双平面 (Semantic Rule Plane + Machine Assurance Profile)
   需要: 每种语言声明 required_verifiers + enforcement + evidence_policy

8. 位置解析 Ambiguous/Stale 语义
   缺口: 双通道算法存在, 但定位结果只有 Resolved/Failed
   需要: Resolved / Ambiguous / Unresolved / StaleSnapshot 四种终态
   需要: Evidence 锚点绑定 repository_snapshot_hash + file_hash + context_hash
```

### P1: 真实系统验收 (5项)

```
9. Polyglot 项目级分片
   需要: 按 project/build root → crate/module → dependency → language → budget
   需要: unit_id = hash(checkpoint + targets + rules + verifier + planner_version)

10. 真实 PG/Artifact/Evidence 存储
    缺口: InMemorySessionManager, EvidenceBuilder 未持久化
    需要: PG 存储 session/evidence/budget, Artifact Store (MinIO/local)

11. 多进程 Temporal + 多 Worker
    缺口: R0-R3 测试单节点通过, 多进程未跑
    需要: Temporal Frontend/History/Matching + PG + Worker×3 + Artifact Store

12. 真实网络和进程故障注入
    需要: kill Worker, 停止 Heartbeat, PG 宕机, Authority 断网, 双 Worker 竞争,
          旧 generation 晚到, Artifact Store 不可读
    根不变量: 0 duplicate effects, 0 unauthorized Committed, 0 lost WorkItem

13. Rust/Go 真实缺陷基准
    需要: 真实缺陷集, 规则 fixture, 位置漂移语料, 无缺陷对照集, 多语言混合仓库
    指标: Precision, Recall, F1, 文件覆盖率, 位置唯一解析准确率, Token 消耗
    关键指标: 错误唯一定位率 → 趋近于零
```

### P2: 后续扩张 (5项)

```
14. WorkflowPack 真实实现 (当前 29行 stub)
15. DocumentPack 真实实现 (当前 30行 stub)
16. DataPack 真实实现 (当前 44行 stub)
    每个 Pack 需要: 真实 Target 发现, Snapshot, 规则, ≥1个确定性 Verifier,
    Location Resolver, Evidence 映射, 正负 fixture, E2E

17. 跨领域交叉验证
   前提: WorkflowPack/DocumentPack/DataPack 从 stub 升级为 MVP
   否则: 在三个 stub 之间转发 Finding 无意义

18. 独立 VerificationInvocation 协议
   当前: Verification Fabric 在 Rust 内部调用，不需要新 Go↔Rust 协议
   需要时: 可先复用 LoopInvocationRequest + task_spec_ref = verification_task
   降级: 从"核心缺口"降至"独立审计模式的可选扩展"
```

---

## 7. 架构防倒退检查清单

| 检查项 | 状态 | 风险 |
|--------|------|------|
| SemanticVerifierPort 定义在 runtime | ✅ ports.rs | — |
| SemanticVerifierPort 实现在 adapter | ✅ semantic_verifier_adapter.rs | — |
| Lane B 只输出 FindingCandidate | ✅ analyze() 返回类型 | — |
| Coordinator 拥有循环控制权 | ✅ execute() 30行但完整 | 逻辑内聚 ≠ 控制权下沉 |
| 纯函数层独立性 | ✅ scope_coverage/rule_binding | — |
| DomainPack trait 存在 | ✅ domain_pack.rs (3行) | — |
| Generic LLM fallback 不应等于 IronClaw | ⚠️ generic_llm.rs 无 trust_class | 需降权或 Fail Closed |
| LaneGuard 已接入 dispatch | ❌ | 值对象, 无调用者 |
| 0 findings 不隐式等于 PASS | ❌ | ScheduleResult 无 execution_status |
| 规则变化导致 Session 失效 | ❌ | .md 无 content hash |

---

## 8. 最终目标架构

```
┌──────────────────────────────────────────────────────────────┐
│ OntoFlow (Temporal Go)                                       │
│ 多 WorkItem、DAG、并发、持久化编排                           │
└────────────────────┬─────────────────────────────────────────┘
                     ▼
┌──────────────────────────────────────────────────────────────┐
│ OntoLoop (Rust)                                               │
│ 单任务多 Attempt、结构化反馈、进展跟踪、预算、恢复           │
└────────────────────┬─────────────────────────────────────────┘
                     ▼
┌──────────────────────────────────────────────────────────────┐
│ IronClaw / OntoRuntime (Rust)                                 │
│                                                               │
│ Lane A: Agent Execution                                       │
│   被 OntoLoop 调用, 完成开放式执行                            │
│   输入: objective → 输出: Candidate Checkpoint                │
│                                                               │
│ Lane B: Semantic Verification                                 │
│   被 OntoAssure Verification Fabric 调用                      │
│   输入: VerificationUnit + Rules + Budget                     │
│   输出: FindingCandidate[] (仅此)                             │
│   禁止: write, publish, decide                                │
└────────────────────┬─────────────────────────────────────────┘
                     ▼ Candidate Checkpoint / FindingCandidate[]
┌──────────────────────────────────────────────────────────────┐
│ OntoAssure Verification Fabric (Rust)                         │
│                                                               │
│ 1. Discovery      2. Scope       3. Rule Routing              │
│ 4. Planning       5. Session     6. Verifier Scheduling       │
│ 7. Location       8. Dedup       9. Cross-Verify              │
│ 10. Criterion     11. Evidence   12. Reduction                │
│ 13. Decision      14. Settlement                              │
│                                                               │
│ → VerifierRunResult (per verifier, per unit)                  │
│ → VerificationCompleteness (scope + rule + verifier coverage) │
│ → Scope Ledger (every target accounted for)                   │
└──────────────────────────────────────────────────────────────┘
```

---

## 9. 一句话总结

```
OpenCodeReview 不是被吸收进 IronClaw，
而是把它"如何把非确定性的 Verifier 装进确定性的验证流水线"这一工程方法，
抽象进了 OntoAssure Verification Fabric。

IronClaw 是其中最高深度的语义 Verifier，
但验证是否完整、是否可信、在故障下是否仍然成立，
由 OntoAssure 的确定性控制面保证。

82% 的架构 → 100% 的生产可用，
差的不是"再加模块"，
而是"VerifierRunResult + Completeness + Scope Ledger + LaneGuard 集成
+ 真实 IronClaw Loop + 持久化恢复 + 分布式故障验证"。
```
