# OntoOS v0.1 — 完整架构 ASCII 图

```
╔══════════════════════════════════════════════════════════════════════════════════╗
║                              OntoOS v0.1                                        ║
║              Trusted AI Agent Execution Control Plane                           ║
║                    ~413 tests │ 0 failures │ v0.4                               ║
╚══════════════════════════════════════════════════════════════════════════════════╝

┌─────────────────────────────────────────────────────────────────────────────────┐
│                         四层权力边界 (Top-Down)                                  │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌─────────────────────────────────────────────────────────────────────────┐    │
│  │ L4: OntoFlow (Go)         多 Loop 编排层                                 │    │
│  │     拥有: Workflow, DAG, Batch, Barrier, Timer, Signal, Saga            │    │
│  │     不拥有: Attempt 成功判定, 副作用结算                                   │    │
│  └────────────────────────────────┬────────────────────────────────────────┘    │
│                                   │ LoopInvocationRequest (JSON)                 │
│  ┌────────────────────────────────▼────────────────────────────────────────┐    │
│  │ L3: OntoLoop (Rust)        单任务收敛引擎                                 │    │
│  │     拥有: Attempt, Continuation, Checkpoint, Progress, Budget           │    │
│  │     不拥有: Agent Loop 内部控制                                            │    │
│  └────────────────────────────────┬────────────────────────────────────────┘    │
│                                   │ start_attempt()                               │
│  ┌────────────────────────────────▼────────────────────────────────────────┐    │
│  │ L2: OntoRuntime (Rust)     安全执行层 (IronClaw Fork)                     │    │
│  │     拥有: Agent Loop, CapabilityHost, 授权, 审批, Secret, FS, Network   │    │
│  │     不拥有: 成功判定, 证据归约                                             │    │
│  │                                                                          │    │
│  │  ┌──────────────────┐   ┌──────────────────┐                             │    │
│  │  │ Lane A: Execute   │   │ Lane B: Verify    │  CapabilityGateway        │    │
│  │  │ ✅ Write Staging  │   │ ✅ Read-Only      │  12 kinds, 11 bypass    │    │
│  │  │ ✅ Call Tools     │   │ ❌ Write/Shell    │  tests                  │    │
│  │  │ → Candidate      │   │ → Finding[]       │                          │    │
│  │  └──────────────────┘   └──────────────────┘                             │    │
│  └────────────────────────────────┬────────────────────────────────────────┘    │
│                                   │ Candidate Checkpoint + Observations          │
│  ┌────────────────────────────────▼────────────────────────────────────────┐    │
│  │ L1: OntoAssure (Rust)     可信裁决内核                                    │    │
│  │     拥有: Contract, Verifier, Evidence, Decision, Settlement, Replay    │    │
│  │     不拥有: 执行, 授权, 调度                                               │    │
│  │                                                                          │    │
│  │  ┌──────────────────────────────────────────────────────────────────┐   │    │
│  │  │ Verification Fabric (14-step pipeline)                            │   │    │
│  │  │                                                                   │   │    │
│  │  │ Discovery → Scope → Rules → Plan → Session → Schedule            │   │    │
│  │  │ → Location(Hunk║File) → Dedup → Criterion → Evidence              │   │    │
│  │  │ → Reduction → SessionDecision → Settlement                       │   │    │
│  │  │                                                                   │   │    │
│  │  │ 双平面: 语义(OCR 25 .md) + 机器(7语言×4级)                        │   │    │
│  │  └──────────────────────────────────────────────────────────────────┘   │    │
│  │                                                                          │    │
│  │  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐       │    │
│  │  │ M5 Success Auth  │  │ M6 Effect Auth   │  │ 三本总账          │       │    │
│  │  │ Agent≠Success    │  │ M6-A Staged FS   │  │ Scope Ledger      │       │    │
│  │  │ 21 tests         │  │ M6-B Transactional│  │ Rule Coverage     │       │    │
│  │  │                  │  │ M6-C Compensatable│  │ Verifier Exec     │       │    │
│  │  │                  │  │ M6-D Irreversible │  │                   │       │    │
│  │  └──────────────────┘  └──────────────────┘  └──────────────────┘       │    │
│  └──────────────────────────────────────────────────────────────────────────┘    │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                         Crate 拓扑 (Rust ~350 tests)                             │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  onto-integration-test (30+ tests)                                              │
│       │                                                                          │
│       ├── onto-temporal-adapter (60 tests) ── F0-F9 完整                        │
│       │    ├── protocol (LoopInvocationRequest/Envelope/Heartbeat)              │
│       │    ├── loop_runner (RuntimeLoopRunner)                                   │
│       │    ├── authority_projection (Go gRPC 验证)                               │
│       │    └── lease/idempotency/heartbeat                                       │
│       │                                                                          │
│       ├── onto-ironclaw-adapter (166 tests)                                     │
│       │    ├── finalizer (★ M5)  staged_filesystem (★ M6-A)                    │
│       │    ├── loop_adapter  attempt_run_port  ironclaw_run_port                │
│       │    ├── lane_isolation  capability_gateway  rollout_stage               │
│       │    └── semantic_verifier_adapter  direct_run  pg/redis                  │
│       │                                                                          │
│       ├── onto-assurance-runtime (16 tests)                                     │
│       │    ├── ports (14 traits)  coordinator  staged_settlement                │
│       │    └── verification/ (planner/scheduler/session/budget/...)            │
│       │                                                                          │
│       ├── onto-assurance-core (18 tests)                                        │
│       │    ├── canonical  evidence_chain  reduction  settlement                 │
│       │    └── verification/ (scope_coverage/rule_router/location/...)         │
│       │                                                                          │
│       ├── onto-assurance-types (57 tests)  ── leaf crate, 零外部依赖            │
│       │    ├── ids/enums/contract/decision/evidence/transaction/hash            │
│       │    ├── finding/verification_target/scope_manifest/rule_binding          │
│       │    ├── verification_plan/session/budget/ledger                          │
│       │    └── evidence_location/verifier_run_result/language_profile           │
│       │                                                                          │
│       ├── onto-loop (13 tests)  ── checkpoint/progress/budget                  │
│       ├── onto-code-pack (86 tests) ── VF + 机器验证 + Domain Pack              │
│       └── onto-pack-sdk (14 tests) ── Code/Ops/Data/Workflow Packs             │
│                                                                                  │
│  依赖方向 (单向，禁止反向):                                                       │
│    integration → adapter → runtime → core → types                                │
│    types ← core ← runtime ← adapter ← temporal                                   │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                         Go 侧: OntoFlow (63 tests)                              │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  temporal/chasm/lib/ontoflow/                                                    │
│  ├── types/transitions/work_item     — 状态机 (10 phases)                       │
│  ├── flow_component/library          — CHASM 组件 + 注册                        │
│  ├── activity_scheduler/scheduler    — ActivityTask 调度 → Matching             │
│  ├── outcome_handler/resolver        — Envelope 处理 + Authority 验证            │
│  ├── graph/batch                     — DAG 解锁 + 批量调度                      │
│  ├── recovery                        — 崩溃恢复 + 幂等守卫                      │
│  ├── planning/deliberation           — 动态规划 + 去中心化讨论                  │
│  └── fx.go                           — CHASM 注入点                             │
│                                                                                  │
│  Temporal Server: /tmp/temporal-server (197MB, v1.32.0)                          │
│  CHASM Registration: "OntoFlow library registered" × 5 services                 │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                     跨语言协议 v1 (Frozen)                                       │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  Go → Rust                    Rust → Go                  Go → Rust (gRPC)        │
│  ┌──────────────────┐       ┌──────────────────┐       ┌──────────────────┐     │
│  │LoopInvocation    │       │LoopTerminal      │       │Authority         │     │
│  │Request           │       │Envelope          │       │ProjectionPort    │     │
│  │                  │       │                  │       │                  │     │
│  │ flow_id          │       │ flow_id          │       │ resolve_loop_    │     │
│  │ work_item_id     │       │ work_item_id     │       │ outcome()        │     │
│  │ loop_id          │       │ loop_id          │       │                  │     │
│  │ task_spec_ref    │       │ reported_*_state │       │ → VerifiedLoop   │     │
│  │ contract_ref     │       │ decision_id      │       │   Outcome        │     │
│  │ execution_gen    │       │ decision_hash    │       │                  │     │
│  │ idempotency_key  │       │ checkpoint_hash  │       │ Go 不直接读      │     │
│  │ request_bind_hash│       │ outcome_bind_hash│       │ Rust PG 表       │     │
│  └──────────────────┘       └──────────────────┘       └──────────────────┘     │
│                                                                                  │
│  Hash: SHA-256, 16 hex chars │ Golden Vectors: 8 cases │ Go ≡ Rust              │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                        Verification Fabric (14-step)                            │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐      │
│  │1.Discover│──→│2.Scope   │──→│3.Rules   │──→│4.Plan    │──→│5.Session │      │
│  │ diff.rs  │   │coverage  │   │router.rs │   │planner   │   │session   │      │
│  │ repo_scan│   │SC-1/2/3  │   │26 types  │   │Target×   │   │resume    │      │
│  └──────────┘   └──────────┘   └──────────┘   │Rule→Unit │   └──────────┘      │
│                                               └──────────┘                      │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐      │
│  │6.Schedule│──→│7.Location│──→│8.Dedup   │──→│9.Criterion│──→│10.Evidence│    │
│  │7lang×4lvl│   │Hunk║File │   │finding_  │   │mapping   │   │builder   │      │
│  │28+ verif │   │dual chan │   │norm      │   │category  │   │Finding→  │      │
│  └──────────┘   └──────────┘   └──────────┘   │→kind     │   │Evidence  │      │
│                                               └──────────┘   └──────────┘      │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐                                     │
│  │11.Reduce│──→│12.Session│──→│13.Settle │  ★ 14-step贯通                       │
│  │(M5 core)│   │Decision  │   │(M6 core) │  ★ 双平面: 语义+机器                  │
│  └──────────┘   └──────────┘   └──────────┘  ★ OCR 25 .md 移植                  │
│                                                                                  │
│  Rule 双平面:                                                                    │
│  ┌─────────────────────────┐    ┌─────────────────────────────────────┐         │
│  │ 语义平面 (OCR 移植)     │    │ 机器平面 (7语言×4级)                │         │
│  │ system_rules.json      │    │ Rust: fmt/clippy/check/test/deny    │         │
│  │ → rule_docs/*.md (25)  │    │ Go:   fmt/vet/lint/test/race/vuln  │         │
│  │ → {{system_rule}} 注入 │    │ Python/TS/Java/Kotlin/C++           │         │
│  │ LLM 原生理解 Markdown   │    │ Enforcement: Required/Advisory       │         │
│  └─────────────────────────┘    └─────────────────────────────────────┘         │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                        位置解析 (P0: 双通道)                                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  FindingCandidate.ExistingCode                                                   │
│       │                                                                          │
│       ├── Channel 1: Diff Hunk Match ── 搜索新增/上下文行 → new_line numbers    │
│       │                                                                          │
│       └── Channel 2: Full File Scan ── 逐行扫描匹配 → 1-indexed line numbers    │
│                                                                                  │
│  两个通道独立运行:                                                                │
│  ┌─────────────────────────────────────────────────────────────────────────┐    │
│  │ 都指向同一位置      → Resolved + Corroborated (2 proofs)                  │    │
│  │ 仅一个成功          → Resolved + SingleSource (1 proof)                   │    │
│  │ 指向不同位置        → Ambiguous (不升级为 Evidence!)                      │    │
│  │ 都失败              → Unresolved                                          │    │
│  │ 快照变化            → StaleSnapshot                                       │    │
│  │ 代码<2行            → Unresolved (SnippetTooShort)                        │    │
│  └─────────────────────────────────────────────────────────────────────────┘    │
│                                                                                  │
│  7 种 LocationProof: ExactByteRange/LineRange, DiffNewSideMatch,                │
│                      FullFileSnippetMatch, SymbolMatch, AstNodeMatch,            │
│                      ContextWindowMatch                                          │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                        执行链路 (完整闭环)                                       │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  StartOntoFlow                                                                   │
│       │                                                                          │
│       ▼                                                                          │
│  CHASM OntoFlowComponent (Go 确定性状态转换)                                      │
│       │                                                                          │
│       ▼                                                                          │
│  WorkItem Ready → ExecuteOntoLoop ActivityTask                                   │
│       │                                                                          │
│       ▼                                                                          │
│  TransferTask → Temporal Matching (127.0.0.1:7235)                               │
│       │                                                                          │
│       ▼                                                                          │
│  Rust Worker Poll (onto-loop-v0)                                                 │
│       │                                                                          │
│       ▼                                                                          │
│  OntoLoop.run_or_resume_to_terminal()                                            │
│       │                                                                          │
│       ├── Attempt 1 ──────────────────────────────────────────┐                  │
│       │    │                                                   │                  │
│       │    ▼                                                   │                  │
│       │  IronClaw Agent Loop (Lane A)                          │                  │
│       │    │                                                   │                  │
│       │    ├── CapabilityGateway.check()                       │                  │
│       │    ├── CapabilityHost → Staged Workspace               │                  │
│       │    └── AfterLoopExit → Candidate Checkpoint            │                  │
│       │    │                                                   │                  │
│       │    ▼                                                   │                  │
│       │  Verification Fabric (14-step)                         │                  │
│       │    │                                                   │                  │
│       │    ▼                                                   │                  │
│       │  OntoAssure: Failed → Continue (structured gap)        │                  │
│       │                                                        │                  │
│       ├── Attempt 2 ──────────────────────────────────────────┤                  │
│       │    │                                                   │                  │
│       │    ▼                                                   │                  │
│       │  IronClaw → Fix → All Verifiers SAT                    │                  │
│       │    │                                                   │                  │
│       │    ▼                                                   │                  │
│       │  OntoAssure: Success → SessionDecision.Committed       │                  │
│       │    │                                                   │                  │
│       │    ▼                                                   │                  │
│       │  M6-A Settlement: CommitPermit → Publish               │                  │
│       │                                                        │                  │
│       └── LoopTerminalEnvelope (Worker 报告)                    │                  │
│                                                                                  │
│       ▼                                                                          │
│  Go Outcome Resolver                                                             │
│       │                                                                          │
│       ▼                                                                          │
│  AuthorityProjectionPort::resolve_loop_outcome() (gRPC)                          │
│       │                                                                          │
│       ├── Decision 不存在 → NOT Committed                                        │
│       ├── Decision 被拒绝  → Escalated                                           │
│       └── Decision 确认    → VerifiedLoopOutcome::Committed                      │
│       │                                                                          │
│       ▼                                                                          │
│  WorkItem Committed → PureTask 解锁下游 WorkItems                                │
│                                                                                  │
│  根不变量: ActivityTaskCompleted ≠ WorkItem Committed                            │
│           Worker reported Committed ≠ WorkItem Committed                          │
│           AuthorityVerified = WorkItem Committed                                  │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                    CapabilityGateway + Lane 隔离 (P2/S1.1)                      │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  所有 Capability 经唯一入口:                                                      │
│  Request → Resolve Lane → Gate.check() → Sandbox → Execute → Observation        │
│                                                                                  │
│  12 种 Capability:                                                               │
│  ┌──────────────┬────────┬────────┬────────────────────────────────┐           │
│  │   Kind       │ Lane A │ Lane B │ 备注                           │           │
│  ├──────────────┼────────┼────────┼────────────────────────────────┤           │
│  │ FileRead     │   ✅   │   ✅   │                                │           │
│  │ FileWrite    │   ✅   │   ❌   │                                │           │
│  │ FileDelete   │   ✅   │   ❌   │                                │           │
│  │ FileRename   │   ✅   │   ❌   │                                │           │
│  │ Patch        │   ✅   │   ❌   │                                │           │
│  │ Shell        │   ✅   │   ❌   │ shell>file, python -c, etc.    │           │
│  │ GitRead      │   ✅   │   ✅   │                                │           │
│  │ GitWrite     │   ✅   │   ❌   │                                │           │
│  │ McpRead      │   ✅   │   ✅   │                                │           │
│  │ McpWrite     │   ✅   │   ❌   │                                │           │
│  │ NetworkRead  │   ✅   │   ✅   │                                │           │
│  │ NetworkWrite │   ✅   │   ❌   │                                │           │
│  │ DatabaseRead │   ✅   │   ✅   │                                │           │
│  │ DatabaseWrite│   ✅   │   ❌   │                                │           │
│  │ ArtifactPub  │   ✅   │   ❌   │                                │           │
│  │ Subprocess   │   ✅   │   ❌   │ dd, make, etc.                 │           │
│  └──────────────┴────────┴────────┴────────────────────────────────┘           │
│                                                                                  │
│  11 项绕过测试: shell>file, python写, symlink, git apply,                       │
│                MCP write, 子进程, rename, 硬链接, file_read ✓, git_log ✓,        │
│                network_read ✓                                                    │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                        三本总账 (S0.2)                                           │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌─────────────────────┐  ┌─────────────────────┐  ┌─────────────────────┐      │
│  │ Scope Ledger        │  │ Rule Coverage       │  │ Verifier Execution  │      │
│  │                     │  │ Ledger              │  │ Ledger              │      │
│  │ 每个 Target 有处置:  │  │ 每个 Target×Rule    │  │ 每个 Unit×Verifier  │      │
│  │ Verified            │  │ 有执行记录:          │  │ 有终态:             │      │
│  │ ExcludedByPolicy    │  │ RequiredRuleExec    │  │ Completed           │      │
│  │ Unsupported         │  │ AdvisoryRuleExec    │  │ Unavailable         │      │
│  │ ReadFailed          │  │ RuleNotApplicable   │  │ TimedOut            │      │
│  │ BudgetBlocked       │  │ RuleExecutionFailed │  │ ExecutionFailed     │      │
│  │ RequiresChunking    │  │ RuleUnsupported     │  │ InvalidOutput       │      │
│  │ EnvironmentFailed   │  │ RuleSuppressed      │  │ Cancelled           │      │
│  └─────────────────────┘  └─────────────────────┘  └─────────────────────┘      │
│                                                                                  │
│  完整性归约:                                                                      │
│    Scope 完整 + Rules 完整 + Verifiers 完整 + Evidence 有效                       │
│    → EligibleForPositiveReduction                                                │
│    否则 → VerificationIncomplete (绝不 PASS)                                      │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                    Domain Pack (P4-P6)                                           │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌────────────────────┐  ┌────────────────────┐  ┌────────────────────┐         │
│  │ WorkflowPack (6)   │  │ DocumentPack (7)   │  │ DataPack (10)      │         │
│  │                    │  │                    │  │                    │         │
│  │ DAG 可达性         │  │ 必要章节           │  │ Migration 可逆     │         │
│  │ 无死节点           │  │ 内部链接           │  │ NOT NULL + DEFAULT │         │
│  │ DAG 无环           │  │ 标题层级           │  │ 列删除需审批       │         │
│  │ Barrier 检查       │  │ 代码块语言         │  │ FK 完整性          │         │
│  │ Authority Gate     │  │ OpenAPI 合法性     │  │ 索引覆盖           │         │
│  │ Generation 纯净    │  │ ADR 完整性         │  │ PII 分类           │         │
│  │                    │  │ Golden Vector 一致 │  │ Evidence 无原值    │         │
│  │                    │  │                    │  │ Migration 幂等     │         │
│  │                    │  │                    │  │ Schema Drift       │         │
│  │                    │  │                    │  │ DB Version 绑定    │         │
│  └────────────────────┘  └────────────────────┘  └────────────────────┘         │
│                                                                                  │
│  安全边界: 只读账号, statement_timeout, 禁止任意SQL, 敏感值不存Evidence           │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                    PG 持久化 (S1/S2) + 故障矩阵 (S3)                             │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌────────────────────┐  ┌────────────────────┐  ┌────────────────────┐         │
│  │ vf_sessions        │  │ vf_verifier_results │  │ vf_scope_ledger    │         │
│  │ session_id (PK)    │  │ run_id (PK)         │  │ target_id (PK)     │         │
│  │ plan_json          │  │ execution_status    │  │ disposition        │         │
│  │ state              │  │ verifier_verdict    │  └────────────────────┘         │
│  │ completed_units    │  │ findings_json       │                                 │
│  │ failed_units       │  │ checkpoint_hash     │  ┌────────────────────┐         │
│  └────────────────────┘  └────────────────────┘  │ Artifact Store     │         │
│                                                  │ SHA-256 校验       │         │
│  ┌────────────────────┐                          │ 篡改→拒绝          │         │
│  │ 18 项故障矩阵      │                          └────────────────────┘         │
│  │                    │                                                         │
│  │ Worker 进程: 6     │  Temporal 服务: 4                                       │
│  │ 网络竞争:   5      │  存储环境:   3                                           │
│  │                                                       │                      │
│  │ 7 根分布式不变量:   │  0 dup effects │ 0 unauth Commit │ 0 stale gen        │
│  │                     │  0 lost WorkItem│ 0 dup loop     │ 0 silent incomplete│
│  └────────────────────┘                                                         │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                        部署拓扑 (双路径)                                        │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  默认: onto-sandbox (grok-build移植) (nono crate)                                           │
│  备选: Docker Compose                                                            │
│                                                                                  │
│  ┌─────────────────────────────────────────────────────────────────────────┐    │
│  │ onto-sandbox (grok-build移植) Profiles:                                             │    │
│  │                                                                          │    │
│  │  onto-worker    (inherit workspace) — Rust Workers, 读写工作区            │    │
│  │  onto-authority (inherit read-only) — Authority, 最小写                   │    │
│  │  onto-verifier  (inherit strict)    — Lane B, 无网络, 最严格             │    │
│  │  onto-temporal  (inherit workspace) — Temporal Server                    │    │
│  │                                                                          │    │
│  │  OS 级隔离: Linux Landlock+bwrap+seccomp | macOS Seatbelt                │    │
│  └─────────────────────────────────────────────────────────────────────────┘    │
│                                                                                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐                        │
│  │ Postgres │  │ Temporal │  │ Temporal │  │ Temporal │                        │
│  │    :5432 │  │ Frontend │  │ History  │  │ Matching │                        │
│  └────┬─────┘  │   :7233  │  │   :7234  │  │   :7235  │                        │
│       │        └────┬─────┘  └────┬─────┘  └────┬─────┘                        │
│       │             └──────────────┼─────────────┘                              │
│       │                           │                                             │
│  ┌────┴─────┐              ┌──────┴──────┐                                      │
│  │Authority │              │  Temporal   │                                      │
│  │ (strict) │              │  Worker     │                                      │
│  └──────────┘              └─────────────┘                                      │
│                                                                                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐          │
│  │ Worker A │  │ Worker B │  │ Worker C │  │Toxiproxy │  │ Evidence │          │
│  │workspace │  │workspace │  │workspace │  │  :8474   │  │ Collector│          │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘  └──────────┘          │
│                                                                                  │
│  启动: deploy/sandbox/start-ontoos.sh [sandbox|docker]                           │
│  Sandbox: deploy/sandbox/sandbox.toml (4 profiles)                               │
│  Docker:  deploy/docker/docker-compose.yml (12 services)                         │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘


┌─────────────────────────────────────────────────────────────────────────────────┐
│                        项目成熟度                                                │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  IMPLEMENTATION COMPLETE                  ✅ ~413 tests, 0 failures              │
│  INTEGRATION HARNESS ACCEPTANCE            ✅ I0-I6                              │
│  REAL RUNTIME ACCEPTANCE (single-node)     ✅ R-1/R0/R1/R2/R3 (45 tests)        │
│  VERIFICATION DEPTH (v0.2)                 ✅ P0-P3 + S0-S2                      │
│  PRODUCTIONIZATION (v0.3-v0.4)             ✅ S3-S4 + P4-P6                      │
│  PRODUCTION RUNTIME                        ⏳ Linux原生环境 (sandbox 63 passed, 4 kernel-limited) │
│                                                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
```
