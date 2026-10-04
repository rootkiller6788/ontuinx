# OntoOS 完整架构

> **v1.0** — P0–P10 全周期完成  
> **日期**：2026-07-28  
> **测试**：~500 tests, 0 failures | `cargo test --workspace` 全绿

---

## 一、系统总架构

```
╔══════════════════════════════════════════════════════════════════════╗
║                              OntoOS                                  ║
║            AI Coding Agent 可信执行与验证控制系统                     ║
╚══════════════════════════════════════════════════════════════════════╝

Requirement / Issue / Contract
               │
               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ L4 OntoFlow · Go                                                    │
│ Workflow / DAG / Batch / Barrier / Timer / Signal / Saga           │
│ 负责：跨任务编排                                                     │
└──────────────────────────────┬──────────────────────────────────────┘
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ L3 OntoLoop · Rust                                                  │
│ Attempt / Continuation / Checkpoint / Progress / Budget            │
│ 负责：单任务收敛                                                     │
└──────────────────────────────┬──────────────────────────────────────┘
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│ L2 OntoRuntime · Rust · IronClaw Fork                               │
│ Agent Loop / CapabilityGateway / Lane A Execute / Lane B Verify    │
│ Staged Workspace / Permission / Secret / FS / Shell / Network      │
│ Candidate Checkpoint                                                │
│                                                                     │
│ P9 可选能力：RepositoryGraphReadCapability ──────────────┐          │
│ Agent可查询图谱，但安全性不依赖Agent是否调用              │          │
└──────────────────────────────┬───────────────────────│─────────────┘
                               │                       │
                               │ Candidate Checkpoint  │ P9 只读查询
                               ▼                       ▼
┌─────────────────────────────────────────────────────────────────────┐
│ L1 OntoAssure · Rust                                                │
│ Discovery → Scope → Rules → Plan → Session → Schedule              │
│ → Evidence → Reduction → SessionDecision → Settlement              │
│                                                                     │
│ Compiler Verifier / Test Verifier / Static Analysis Verifier       │
│ Semantic Review Verifier                                            │
│ P7 Graph Integrity Verifier ───────────────────────────┐           │
│ P7 Graph Risk Verifier ────────────────────────────────┤           │
└──────────────────────────────┬─────────────────────────│───────────┘
                               │                         │
                               │ Settlement Decision     │ 系统确定性调用
                               ▼                         ▼
                  ┌─────────────────────────────────────────────┐
                  │         OntoCodeGraph                       │
                  │                                             │
                  │  通用代码仓库事实系统 (CBM深度魔改)          │
                  │  无前端 / 无MCP / PostgreSQL持久化           │
                  │                                             │
                  │  ┌─────────────────────────────────────┐    │
                  │  │ ofg-service (ontofirmwaregraphd)     │    │
                  │  │ gRPC: SnapshotQuery + GraphRisk      │    │
                  │  │     + QueryService + Health          │    │
                  │  │ P8 Projector (EventSource → PG)      │    │
                  │  │ PG: onto_firmware_graph              │    │
                  │  ├─────────────────────────────────────┤    │
                  │  │ ofg-core (CBM-derived C kernel)      │    │
                  │  │ discover→parse→extract→resolve       │    │
                  │  │ 159 tree-sitter grammars             │    │
                  │  │ FFI via onto-graph-core crate        │    │
                  │  └─────────────────────────────────────┘    │
                  └─────────────────────────────────────────────┘
                               ▲
                               │ P10 MCP JSON-RPC
                  ┌────────────┴────────────────────────────────┐
                  │ onto-graph-mcp                               │
                  │ 6 tools: search / context / callers /        │
                  │ callees / references / history               │
                  │ stdio + HTTP transport                       │
                  │ 注册: ironclaw/registry/mcp-servers/         │
                  └─────────────────────────────────────────────┘

Decision / Settlement
        │
        ├── COMMIT ────▶ P8 OutboxPublisher → Redis Streams
        ├── CONTINUE ──▶ P8 Projector: INCOMPLETE episode
        ├── ROLLBACK ──▶ P8 Projector: Failed episode
        └── ESCALATE ──▶ P8 Projector: PendingAuthority
```

---

## 二、四层权力边界

```
Layer 4: OntoFlow (Go)       — 多个 OntoLoop 的 DAG/批量/耐久编排
Layer 3: OntoLoop (Rust)     — 单个 WorkItem 的多 Attempt 收敛
Layer 2: OntoRuntime (Rust)  — 一次 Attempt 的安全执行
Layer 1: OntoAssure (Rust)   — 验证、证据、裁决与副作用结算
```

**核心不变量：**
- Agent 不能自声明成功
- STALE / PARTIAL / UNSUPPORTED 不能进入 Positive Evidence
- 图谱不可用 → 显式 EnvironmentError，绝不静默 PASS
- 1 Attempt = 1 Run | 1 WorkItem = 1 loop_id | 1 Generation = 1 idempotency_key
- `ActivityTaskCompleted ≠ WorkItem Committed`
- `Worker reported Committed ≠ WorkItem Committed`
- `AuthorityVerified = WorkItem Committed`

---

## 三、Crate 拓扑（14 个 workspace member）

```
crates/
├── onto-assurance-types/     74 tests  纯数据类型（零外部依赖）
│   ├── ids, enums, contract, decision, evidence, transaction, hash
│   ├── ingress, capability, observation, ontoloop, external_effects
│   ├── profile, language_profile, verification_target
│   ├── scope_manifest, rule_binding, verification_plan, verification_session
│   ├── finding, verification_budget, evidence_location, verifier_run_result
│   └── verification_ledger (三本总账: ScopeLedger / RuleCoverageLedger / VerifierExecutionLedger)
│
├── onto-assurance-core/      13 tests  纯函数内核（零 IO）
│   ├── canonical, evidence_chain, reduction, session_decision, settlement
│   ├── replay, invalidation, effect_classifier, checkpoint
│   └── verification/ (scope_coverage, rule_router, location_binding,
│                      finding_normalization, criterion_mapping)
│
├── onto-assurance-runtime/   42 tests  Port traits + Coordinator
│   ├── ports (14 traits), coordinator, staged_settlement
│   ├── database_settlement, compensation_coordinator, irreversible_dispatcher
│   └── verification/ (ports, planner, scheduler, session, budget,
│                      locator, evidence_builder, coordinator)
│
├── onto-ironclaw-adapter/   86 tests  M5/M6/DB/Redis/DirectRun
│   ├── finalizer (★ M5 seam), staged_filesystem (★ M6-A seam)
│   ├── loop_adapter, attempt_run_port, ironclaw_run_port
│   ├── lane_isolation (Lane A/B), capability_gateway (12 kinds + 11 bypass)
│   ├── rollout_stage (Shadow→Gated→Enforced)
│   ├── direct_run, semantic_verifier_adapter, redis_adapter, pg_adapter
│   ├── outbox_publisher (★ P8: PG→Redis Streams)
│   ├── graph_read_capability (★ P9: RepositoryGraphReadCapability)
│   └── graph_context_injector (★ P9: 确定性策略注入)
│
├── onto-temporal-adapter/    60 tests  OntoFlow 桥接 (F0-F9)
│   ├── protocol (LoopInvocationRequest/Envelope/Heartbeat)
│   ├── loop_runner (RuntimeLoopRunner), worker (OntoLoopWorker trait)
│   ├── heartbeat, idempotency, lease, authority_projection
│   └── agent_task (v1.0 原生 AgentTask 类型)
│
├── onto-loop/                13 tests  Attempt 收敛引擎
│   └── checkpoint, progress, budget
│
├── onto-code-pack/           88 tests  验证工厂 (VF + 机器验证 + Domain Pack)
│   ├── build_verifier, test_verifier, lint_verifier, file_verifiers
│   ├── machine_verifiers (7 languages × 4 levels = 28+ verifiers)
│   ├── rules/ (system_rules.json + 26 rule_docs/*.md + router)
│   ├── scope/ (diff, repository_scan)
│   ├── verifiers/ (ironclaw_semantic, generic_llm)
│   ├── location/ (diff_index, source_index)
│   ├── verification_profiles/ (rust.toml, go.toml, generic.toml)
│   ├── pg_stores (3 tables), artifact_store, observability
│   ├── workflow_pack, document_pack, data_pack, domain_pack
│   └── project_profiler, artifact_collector
│
├── onto-pack-sdk/            14 tests  行业 Pack SDK
│   └── packs (Code/Ops/Data/Workflow)
│
├── onto-pack/                —         Domain Plugin SPI + DomainRegistry
│
├── onto-conformance/          3 tests  一致性测试 (Golden fixture diff)
│
├── onto-integration-test/    30+ tests  跨 Crate E2E
│   ├── cross_crate_pipeline, golden_vectors
│   ├── r0_worker_lifecycle, r1_real_pipeline, r2_pg_authority, r3_dag_fault
│   ├── vf_pipeline, p3_distributed, s3_fault_matrix, ocr_absorption
│   └── g3_cross_language_cycle, g4_authority_resolution
│
├── onto-graph-core/           2 tests  ★ P2: C 内核 FFI 绑定
│   ├── build.rs (cc crate 编译 ofg-core C 源码)
│   └── src/lib.rs (ofg_core_t / ofg_graph_sink_t / extern "C" API)
│
├── onto-graph-service/        4+2 tests ★ P3-P9: gRPC 图谱服务 (ontofirmwaregraphd)
│   ├── proto/ (health.proto, snapshot_query.proto, graph_risk.proto, query.proto)
│   ├── src/
│   │   ├── main.rs (memfd + ring + gRPC server + PG pool)
│   │   ├── merkle.rs (3-layer Merkle: FileFact→EntityVersion→SnapshotRoot)
│   │   ├── pg_store.rs (10 表 CRUD + ensure/snapshot/seal/insert)
│   │   ├── snapshot.rs (BUILDING→SEALED→PROMOTED/DISCARDED/INVALID)
│   │   ├── ring.rs (SPSC RingConsumer, 8 frame types)
│   │   ├── snapshot_query_service.rs (★ P7: CheckIntegrity RPC)
│   │   ├── graph_risk_service.rs (★ P7: CallChain/Dependency/Tests/SharedState/Language RPCs)
│   │   ├── query_service.rs (★ P9: Search/Context/Callers/Callees/Refs/History RPCs)
│   │   └── projector.rs (★ P8: EventSource trait → ofg_execution_projection)
│   ├── migrations/01_initial.sql (10 tables + dead_letter + projection_offset)
│   ├── baselines/ (P0 golden + P1 findings + P6 capability-matrix)
│   ├── fixtures/ (C/Java/Python/TypeScript 4语言 Golden)
│   └── plan/ (OntoFirmwareGraph-PLAN.md)
│
└── onto-graph-verifiers/     24+9 tests ★ P7: Graph Verifiers
    ├── src/
    │   ├── client.rs (SnapshotQueryClient + GraphRiskClient traits + Mock impls)
    │   ├── schedule_rules.rs (ChangeType分类 + integrity/risk policy 纯函数)
    │   ├── graph_integrity.rs (GraphIntegrityVerifier: DeterminsticVerifierPort)
    │   ├── graph_risk.rs (GraphRiskVerifier + RiskDimensions)
    │   └── scheduler.rs (GraphVerifierScheduler: UnitScheduler + 三本总账写入)
    └── tests/e2e_pipeline.rs (9 E2E: auth/doc/stale/graph-down/unsafe/ledger/mixed/config)
```

**补充二进制 crate（workspace member 但不在此目录）：**

```
crates/onto-graph-mcp/        4+8 tests  ★ P10: MCP JSON-RPC Server
├── src/main.rs (JSON-RPC 2.0 over stdio + HTTP/axum)
├── tests/e2e_mcp.rs (8 tests: handshake/tools/error/registry/protocol/batch)
└── 注册: ironclaw/registry/mcp-servers/onto-code-graph.json
```

---

## 四、Go 侧：OntoFlow

```
temporal/chasm/lib/ontoflow/    63 tests
├── types, transitions, work_item, flow_component, library
├── activity_scheduler, scheduler, batch
├── outcome_handler, outcome_resolver
├── graph (DAG), recovery, planning, deliberation
├── fx.go (CHASM 注册), system_rules.json
└── g1-g9 tests
```

**Temporal Server:** 197MB binary v1.32.0, OntoFlow 注册到 CHASM 5 服务

---

## 五、跨语言协议 v1 (Frozen)

```
Go → Rust: LoopInvocationRequest (JSON)
Rust → Go: LoopTerminalEnvelope (JSON, 非权威事实)
Go → Rust (gRPC): AuthorityProjectionPort::resolve_loop_outcome()
Rust → Go: VerifiedLoopOutcome (权威终态)

Hash: SHA-256, 16 hex chars, Go=Rust 逐字节一致
Golden Vectors: canonical_cases.json (8 cases)
```

---

## 六、gRPC 服务拓扑（ontofirmwaregraphd :50051）

```
ontofirmwaregraphd
├── Health                  (P3)
├── SnapshotQueryService    (P7) — CheckIntegrity RPC
│     检查: exists / SEALED / checkpoint / generation / files / coverage
├── GraphRiskService        (P7) — 5 RPCs
│     AnalyzeCallChain / GetDependencyClosure / GetAffectedTests
│     / GetSharedStateAccess / GetLanguageRisks
├── QueryService            (P9) — 6 RPCs
│     SearchSymbols / GetSymbolContext / GetCallers
│     / GetCallees / GetReferences / GetPreviousChanges
└── [P8 Projector]          (P8) — EventSource trait → ofg_execution_projection
```

---

## 七、PostgreSQL 数据库

```
PostgreSQL Cluster
├── ontoos                     ← OntoOS 核心
│   ├── onto_outbox            ← P8 权威事件表（Settlement 同事务写入）
│   ├── vf_sessions / vf_verifier_results / vf_scope_ledger
│   └── Flow / Loop / Runtime / Assurance / Evidence / Settlement
│
└── onto_firmware_graph        ← OntoCodeGraph (独立数据库)
    ├── ofg_repository
    ├── ofg_snapshot            (BUILDING→SEALED→PROMOTED/DISCARDED/INVALID)
    ├── ofg_entity / ofg_entity_version   (稳定实体 × 快照版本)
    ├── ofg_file / ofg_file_version
    ├── ofg_edge               (stable_key = hash(source,target,kind,callsite,slot) 去重)
    ├── ofg_candidate_delta    (added_files / changed_files / deleted_files / renamed)
    ├── ofg_pass_coverage / ofg_diagnostic
    ├── ofg_execution_projection   (P8 投影目标，event_id UNIQUE 幂等)
    ├── ofg_dead_letter            (P8 死信队列)
    └── ofg_projection_offset      (P8 Exactly-Once 消费追踪)

硬约束：
  不同数据库账号（ontofirmwaregraph vs ontoos）
  不同 Migration、不同连接池
  无跨库外键、无跨数据库事务
```

---

## 八、执行链路

```
StartOntoFlow
  → CHASM OntoFlowComponent (确定性状态转换)
  → WorkItem Ready → ExecuteOntoLoop ActivityTask
  → TransferTask → Matching → Rust Worker Poll
  → OntoLoop.run_or_resume_to_terminal()
      ├── Attempt 1: IronClaw Agent Loop → CapabilityHost
      │   → CapabilityGateway (LaneGuard)
      │   → P9 GraphContextInjector (确定性注入, ≤500 tokens)
      │   → Agent 可选 P9 RepositoryGraphReadCapability (Lane B 只读)
      │   → AfterLoopExit → CandidateCheckpoint
      │   → Verification Fabric:
      │       ├── Scope → Rules → Plan → Session
      │       ├── Compiler / Test / Lint / Machine Verifiers
      │       ├── P7 GraphIntegrityVerifier (所有代码变更强制)
      │       └── P7 GraphRiskVerifier (按策略调度)
      │   → EvidenceBuilder → Reduction → SessionDecision
      │   → P8 Settlement → onto_outbox (同事务)
      │   → Failed → Continue (structured gap)
      ├── Attempt 2: Fix → Success → OntoAssure Decision
      └── M6 Settlement → CommitPermit → Publish
  → LoopTerminalEnvelope (Worker 报告)
  → Outcome Resolver → AuthorityProjectionPort (gRPC)
  → VerifiedLoopOutcome → WorkItem Committed
  → P8 OutboxPublisher → Redis Streams → Projector → Canonical Graph
```

---

## 九、P7 Graph Verifier 流水线

```
VerificationUnit.metadata
  { repository_name, base_commit_sha, candidate_checkpoint_hash,
    execution_generation, changed_files, required_passes,
    snapshot_id, changed_entity_keys, language }
        │
        ▼
schedule_rules::classify_change(changed_files)
  → ChangeType: Documentation | Config | TestCode | NormalCode
              | PublicApi | SecurityCritical | Unknown
        │
        ├── integrity_policy(ct) → EnforcementLevel::Skip / Advisory / Required / StrengthenedRequired
        └── risk_policy(ct)      → EnforcementLevel::Skip / Advisory / Required / StrengthenedRequired
        │
        ▼
GraphVerifierScheduler (impl UnitScheduler)
  ├── GraphIntegrityVerifier ──gRPC──▶ SnapshotQueryService.CheckIntegrity
  │    检查: exists / SEALED / checkpoint / generation / files / coverage
  │    失败映射:
  │      NOT_SEALED / checkpoint_mismatch / generation_mismatch → StaleSnapshot
  │      unprocessed_files / missing_passes → VerificationIncomplete
  │      gRPC unreachable → EnvironmentError
  │    绝不自动降级为 PASS
  │
  └── GraphRiskVerifier ──gRPC──▶ GraphRiskService.*
       维度: call_chain / dependency_closure / affected_tests
             / shared_state / language_risks
       模式: Advisory (失败不阻断) / Required (失败→EnvironmentError)
             / StrengthenedRequired (全维度启用)
        │
        ▼
  三本总账写入:
    ScopeLedger          → TargetDisposition (Verified/Excluded/ReadFailed/EnvironmentFailed)
    RuleCoverageLedger   → RuleCoverageStatus (RequiredRuleExecuted/AdvisoryRuleExecuted/...)
    VerifierExecutionLedger → VerifierExecutionEntry (Completed/Unavailable/...)
```

**调度规则映射表：**

| 变更类型 | Graph Integrity | Graph Risk |
|---------|----------------|------------|
| README / 文档 | Skip | Skip |
| 普通配置 | Advisory | Advisory |
| 测试代码 | Required | Advisory |
| 普通业务代码 | Required | Required |
| 公共 API | Required | StrengthenedRequired |
| 权限/支付/认证 | StrengthenedRequired | StrengthenedRequired |
| 未知 | Required | Required |

---

## 十、P8 Outbox + 留存流水线

```
Settlement (OntoAssure)
    │ 同事务
    ▼
onto_outbox (PG)  ← 已有 (pg_effect_store.rs)
    │
    ▼
OutboxPublisher (P8 新增, onto-ironclaw-adapter)
    │ poll onto_outbox WHERE published_at IS NULL
    │ XADD ontoos.authoritative-events.v1
    │ mark_published
    ▼
Redis Streams: ontoos.authoritative-events.v1
    │
    ▼
Projector (P8 新增, ofg-service)
    │ EventSource trait (支持 PG polling / Redis Streams 双后端)
    │ Consumer Group: ontofirmwaregraph-projector
    ▼
┌─────────────────────────────────────┐
│ ofg_execution_projection (幂等)     │
│                                     │
│ 投影语义:                            │
│   CandidateCommitted                │
│     → ofg_snapshot.state = PROMOTED │
│   AttemptContinued                  │
│     → INCOMPLETE episode            │
│   CandidateRejected / RolledBack    │
│     → DISCARDED                     │
│   AttemptEscalated                  │
│     → PENDING_AUTHORITY             │
│                                     │
│ ofg_dead_letter (失败事件记录)       │
│ ofg_projection_offset (消费追踪)    │
└─────────────────────────────────────┘
```

---

## 十一、P9 Runtime GraphRead

```
Agent (可选查询)
    │ CapabilityGateway Lane B (只读)
    ▼
RepositoryGraphReadCapability (onto-ironclaw-adapter)
    │ tonic QueryServiceClient → ontofirmwaregraphd :50051
    ▼
QueryService → PostgreSQL (onto_firmware_graph)

GraphContextInjector (确定性注入，非Agent决策)
  触发条件:
    1. 首次修改陌生模块    → target symbol + 1层 callers/callees (≤5)
    2. 修改公共 API        → 所有调用者 (≤10)
    3. 修改高风险文件       → 影响摘要 (≤500 tokens)
    4. 连续验证失败         → 相关测试 (≤5) + 历史失败 (≤3)

关键约束:
  图谱不可用 → Agent 继续执行（不阻断）
  查询结果不构成安全证明（那是 P7 GraphIntegrity 的职责）
```

---

## 十二、P10 MCP 集成

```
AI Tool (Claude/Cursor/IronClaw)
    │ MCP JSON-RPC 2.0
    │ stdio:  onto-graph-mcp --repo my-project
    │ HTTP:   onto-graph-mcp --repo my-project --http-port 50052
    ▼
onto-graph-mcp (独立二进制)
    │ 6 MCP Tools:
    │   search_symbols      — 按名称搜索代码符号
    │   get_symbol_context  — 符号上下文 (callers+callees+refs)
    │   get_callers         — 递归调用者链
    │   get_callees         — 递归被调用链
    │   get_references      — 引用关系
    │   get_change_history  — 历史变更记录
    │ tonic gRPC
    ▼
ontofirmwaregraphd QueryService (:50051) → PostgreSQL

IronClaw 注册表:
  ironclaw/registry/mcp-servers/onto-code-graph.json
  {
    "name": "onto-code-graph",
    "kind": "mcp_server",
    "url": "http://localhost:50052/mcp",
    "auth": "none"
  }
```

---

## 十三、Verification Fabric 流水线

```
1.  Discovery        diff.rs / repository_scan.rs     (文件/仓库扫描)
2.  Scope Resolution scope_coverage.rs                (覆盖检查 + SC-1/2/3)
3.  Rule Routing     system_rules.json + router.rs    (26 种文件类型)
4.  Planning         planner.rs                       (Scope × Rules → Units)
5.  Session          session.rs                       (可恢复会话)
6.  Verifier Schedule machine_verifiers.rs            (7语言 × 4级)
    + P7 GraphIntegrityVerifier (所有代码变更强制)
    + P7 GraphRiskVerifier (按策略调度)
7.  Location         location_binding.rs              (Hunk+File 双通道)
8.  Dedup            finding_normalization.rs         (去重 + 交叉验证)
9.  Criterion Map    criterion_mapping.rs             (Finding → Criterion)
10. Evidence Build  evidence_builder.rs              (Finding → Evidence)
11. Reduction       onto-assurance-core              (已有 M5)
12. Session Decision onto-assurance-core              (已有 M5)
13. Settlement      onto-assurance-core              (已有 M6)
```

---

## 十四、Rule 双平面

```
语义平面 (OCR 移植):
  system_rules.json → rule_docs/*.md (25 文件) → {{system_rule}} 注入
  LLM 原生理解 Markdown — "Do not report" 防误报

机器平面 (新增):
  MachineVerifier × LanguageVerifierSet (7语言 × 4级)
  Rust: cargo fmt/clippy/check/test/deny/audit
  Go:   gofmt/vet/golangci-lint/test/test-race/vulncheck/gosec
  Python/TS/Java/Kotlin/C++: 各自工具链
```

---

## 十五、Domain Pack

| Pack | 规则数 | 验证内容 |
|------|--------|---------|
| WorkflowPack | 6 | DAG 可达性/死节点/环/Barrier/AuthorityGate/Generation |
| DocumentPack | 7 | 章节/链接/标题/代码块/OpenAPI Schema/ADR/协议字段 |
| DataPack | 10 | 迁移可逆/NOT NULL/FK/索引/PII/漂移（只读安全边界） |

---

## 十六、Lane 隔离 (P2)

```
Lane A (Agent Execution):
  ✅ 可写 staging → 生成 Candidate
  ❌ 不能给自己修改生成 Evidence

Lane B (Semantic Verification):
  ✅ 只读 Candidate Snapshot
  ❌ 12 种 Capability 中仅 4 种 Read 被允许
  ❌ FileWrite/Shell/GitWrite/McpWrite/NetworkWrite/
     DatabaseWrite/ArtifactPublish/Subprocess 全部被 CapabilityGateway 阻止

P9 RepositoryGraphReadCapability: Lane B (只读)
  图谱不可用 → Agent 继续执行（不阻断 Lane A）
```

---

## 十七、PG 持久化

| 表 | 所属数据库 | 内容 |
|----|-----------|------|
| `vf_sessions` | ontoos | Session 状态 + Plan + Units |
| `vf_verifier_results` | ontoos | Verifier 执行记录 (status/verdict/findings) |
| `vf_scope_ledger` | ontoos | 每个 Target 的处置记录 |
| `onto_outbox` | ontoos | P8 权威事件 (Settlement 同事务) |
| `ofg_repository` | onto_firmware_graph | 仓库注册 |
| `ofg_snapshot` | onto_firmware_graph | 版本化快照 (BUILDING→SEALED→...) |
| `ofg_entity` + `ofg_entity_version` | onto_firmware_graph | 稳定实体 × 快照版本事实 |
| `ofg_file` + `ofg_file_version` | onto_firmware_graph | 文件注册 × 快照版本 |
| `ofg_edge` | onto_firmware_graph | 边 (stable_key UNIQUE 去重) |
| `ofg_candidate_delta` | onto_firmware_graph | Candidate 增量文件清单 |
| `ofg_pass_coverage` | onto_firmware_graph | 每个 Pass 的覆盖状态 |
| `ofg_diagnostic` | onto_firmware_graph | 结构化诊断信息 |
| `ofg_execution_projection` | onto_firmware_graph | P8 事件投影 (幂等) |
| `ofg_dead_letter` | onto_firmware_graph | P8 死信队列 |
| `ofg_projection_offset` | onto_firmware_graph | P8 消费追踪 |

---

## 十八、故障矩阵 (18 项)

| 类别 | 项数 | 覆盖 |
|------|------|------|
| Worker 进程 | 6 | 领取前/Heartbeat后/Attempt后/Decision后/M6后/RPC丢失 |
| Temporal 服务 | 4 | Matching/History/全重启/PG不可用 |
| 网络竞争 | 5 | Authority分区/Artifact不可读/Worker分区/双Worker/旧gen |
| 存储环境 | 3 | 磁盘满/Artifact篡改/时钟偏移 |

**7 根不变量：** 0 duplicate effects | 0 unauthorized Committed | 0 stale-gen commit | 0 lost WorkItem | 0 duplicate loop | 0 silent incomplete | 0 permanent stall

---

## 十九、Docker 拓扑

```
12 服务: PG + Temporal×4 + Authority + 3 Workers + Toxiproxy + Evidence + TestDriver
```

---

## 二十、语言与领域覆盖

### 语言等级（P6）

| Tier | 语言 | 状态 |
|------|------|------|
| Tier 0 | C, C++, Java, Python, JS/TS, Go, Rust | Golden fixture |
| Tier 1 | C#, Kotlin, Swift, Ruby, PHP, SQL, Shell | 实测通过 |
| Tier 2 | 其余 137 tree-sitter 语言 | 编译进二进制 |

### 机器验证矩阵（onto-code-pack, 7×4=28）

| 级别 | Rust | Go | Python | TS/JS | Java | Kotlin | C/C++ |
|------|------|----|--------|-------|------|--------|-------|
| Fmt (0) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Lint (1) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Test (2) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Audit (3) | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

---

## 二十一、P0-P10 完成度

| 阶段 | 内容 | 工期 | 状态 |
|------|------|------|------|
| P0 | Fork + Golden 基线 | 1-2周 | ✅ |
| P1 | 功能开关隔离 | 2-3周 | ✅ |
| P2 | ofg_core_t + GraphSink 抽象 | 3-5周 | ✅ |
| P2.5 | 物理删除产品壳 | 1-2周 | ✅ |
| P3 | 共享内存 + ring consumer + Rust Service | 3-5周 | ✅ |
| P4 | PG 替换 SQLite | 5-8周 | ✅ |
| P5 | Merkle + Snapshot 状态机 | 4-6周 | ✅ |
| P6 | 22 语言实测 + 159 grammar 编译 | 4-8周 | ✅ |
| **第一周期合计** | **22-37周** | ✅ |
| P7 | GraphIntegrity + GraphRisk Verifier | 2-3周 | ✅ |
| P8 | Outbox Publisher + Projector + Dead Letter | 2-3周 | ✅ |
| P9 | Runtime GraphRead + ContextInjector | 1-2周 | ✅ |
| P10 | MCP Server + IronClaw 注册 | 2-4周 | ✅ |
| **总计** | **29-49周** | ✅ |

---

> **第一周期 (P0-P6)**: OntoCodeGraph 独立完成  
> **第二周期 (P7-P10)**: 拼接 OntoOS — 全部完成  
> **cargo test --workspace: ~500 tests, 0 failures**
